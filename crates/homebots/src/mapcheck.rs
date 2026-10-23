//! HB_MAPCHECK=<map folder>[:<mode>] (e.g. Crossroads:FL, Overpass:TDM; without a mode every mode
//! the folder has data for): headless check of a map's match data, no window. Runs the same
//! functions as a match start: mapmode::match_maps (the streamed sublevels the menu loads),
//! objectives::load_pkgs / load_frontlines / minimap_bounds / line_spawns, the PlayerStart groups
//! main.rs puts into the CollisionWorld, terrain actors, and whether the buyable vehicles'
//! skeletal meshes (GCGameContent Default__<class>.SVehicleMesh) are in the loaded packages
//! (what vehload::load searches). Packages are opened one at a time (RAM).
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

pub fn run(game: &Path, spec: &str) {
    let (name, mode) = spec.split_once(':').map(|(a, b)| (a, Some(b))).unwrap_or((spec, None));
    let mp = game.join("GCGame").join("CookedPC").join("Maps").join("MP");
    let Some(folder) = std::fs::read_dir(&mp).ok().and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)))) else {
        println!("mapcheck: no map folder {name} in {}", mp.display());
        return;
    };
    let (fl, tdm) = (crate::mapmode::has_data(&folder, "FL"), crate::mapmode::has_data(&folder, "TDM"));
    // TDM rules from the Coalesced ini (what objectives.rs / creategame.rs read)
    let cooked = game.join("GCGame").join("CookedPC");
    if let Some(loc) = upk::ui::Loc::load(&cooked.join(format!("Coalesced_{}.bin", crate::menu::language(game)))).or_else(|| upk::ui::Loc::load(&cooked.join("Coalesced_int.bin"))) {
        let bp = loc.ini.get("gcbattlepoints.tdm.infantry.rewards").and_then(|v| crate::tdm::reward_points(v, "PSR_KillAsInfantry"));
        let range = loc.ini.get("gcgame.gcgame.gconlineserveroptions.m_tdmscorelimit").and_then(|v| v.first()).and_then(|v| crate::tdm::range_value(v));
        println!("TDM kill reward {bp:?}, TDM score limit (value, min, max, step) {range:?}");
    }
    println!("mapcheck {}: Bodenkontrolle data {fl}, TDM data {tdm}", folder.display());
    let modes: Vec<&str> = match mode {
        Some(m) => vec![m],
        None => [("FL", fl), ("TDM", tdm)].into_iter().filter(|x| x.1).map(|x| x.0).collect(),
    };
    for m in modes {
        check(game, &folder, m);
    }
    // the level build (main.rs load_maps) is the same for every mode but the gameplay
    // sublevel: checked once with the first mode
    if std::env::var_os("HB_MAPCHECK_NOWORLD").is_none() {
        let m = mode.unwrap_or(if fl { "FL" } else { "TDM" });
        world_check(game, &folder, m);
    }
}

/// Actor classes whose mesh components main.rs load_maps places (upk level.rs
/// static_mesh_placements / skeletal_mesh_placements / speedtree_placements, road splines,
/// emitters, fluid surfaces, terrain); the census reports other classes that own a mesh
/// component in TheWorld.PersistentLevel.
const PLACED_STATIC: [&str; 7] = ["StaticMeshActor", "InterpActor", "GCStaticMeshActor", "GCSkyboxActor", "GCDestructibleStaticObject", "GCNudgeableCollectionActor", "GCRotatingSMActor"];

/// The level build without a window: the packages main.rs load_maps opens (match sublevels,
/// *_FX, the install packages of not-exported Texture2D / Material / MIC imports), import
/// resolution of the placed meshes / trees / skeletal meshes (unresolved ones grouped by their
/// package = missing package or object), mesh reads, the placed materials' generated WGSL
/// (world.rs build_material: graph compiler, naga validation), the terrain's game shaders
/// (spawn_terrain_emu), and a census of actor classes with mesh components that no placement
/// path reads. Each step runs under catch_unwind: a panic is reported instead of ending the check.
fn world_check(game: &Path, folder: &Path, mode: &str) {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    println!("== world build ({mode} sublevels, as main.rs load_maps)");
    let Ok(lib) = upk::library::Library::scan(game) else {
        println!("library scan failed");
        return;
    };
    if let Ok(mut g) = crate::gamemat::SHADOW_WGSL.write() {
        if g.is_empty() {
            *g = crate::sunshadow::wgsl(&Default::default(), 1.0, 1.0);
        }
    }
    let maps = crate::mapmode::match_maps(folder, mode);
    let mut pkgs: Vec<upk::Package> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for m in &maps {
        match catch_unwind(|| upk::Package::open(m)) {
            Ok(Ok(p)) => {
                pkgs.push(p);
                names.push(m.file_stem().unwrap_or_default().to_string_lossy().to_ascii_lowercase());
            }
            Ok(Err(e)) => println!("  {}: does not open: {e}", m.display()),
            Err(_) => println!("  {}: PANIC while opening", m.display()),
        }
    }
    let n_world = pkgs.len();
    if let Ok(rd) = std::fs::read_dir(folder) {
        let mut fx: Vec<_> = rd.flatten().map(|e| e.path()).filter(|f| f.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().ends_with("_fx.map"))).collect();
        fx.sort();
        for f in fx {
            if let Ok(p) = upk::Package::open(&f) {
                pkgs.push(p);
                names.push(f.file_stem().unwrap_or_default().to_string_lossy().to_ascii_lowercase());
            }
        }
    }
    // import packages (main.rs: Texture2D / Material / MIC imports no map exports)
    let mut exported: HashSet<String> = HashSet::new();
    for p in &pkgs {
        for i in 0..p.exports.len() {
            exported.insert(p.object_path(i as i32 + 1).to_ascii_lowercase());
        }
    }
    let mut need: std::collections::BTreeSet<String> = Default::default();
    // imports nobody exports and whose package the install lacks: (package -> class -> count)
    let mut missing: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for p in &pkgs {
        for (i, im) in p.imports.iter().enumerate() {
            let class = p.name(im.class_name);
            if !matches!(class.as_str(), "Texture2D" | "Material" | "MaterialInstanceConstant" | "StaticMesh" | "SkeletalMesh" | "SpeedTree" | "ParticleSystem") {
                continue;
            }
            let path = p.object_path(-(i as i32) - 1);
            let pk = path.split('.').next().unwrap_or("").to_ascii_lowercase();
            if exported.contains(&path.to_ascii_lowercase()) || names.contains(&pk) {
                continue;
            }
            if lib.path_of(&pk).is_some() {
                if matches!(class.as_str(), "Texture2D" | "Material" | "MaterialInstanceConstant") {
                    need.insert(pk);
                }
            } else {
                *missing.entry(pk).or_default().entry(class).or_default() += 1;
            }
        }
    }
    let mut alias: Vec<(usize, String)> = Vec::new();
    for pk in &need {
        if let Some(p) = lib.open(pk) {
            alias.push((pkgs.len(), pk.clone()));
            pkgs.push(p);
            names.push(pk.clone());
        }
    }
    println!("packages: {n_world} sublevels, {} FX, {} import packages {:?}", pkgs.len() - n_world - alias.len(), alias.len(), need);
    // object path -> (package, export), aliases as world.rs alias_package
    let mut by_path: std::collections::HashMap<String, (usize, usize)> = Default::default();
    for (pi, p) in pkgs.iter().enumerate() {
        for i in 0..p.exports.len() {
            by_path.entry(p.object_path(i as i32 + 1).to_ascii_lowercase()).or_insert((pi, i));
        }
    }
    for (pi, name) in &alias {
        let p = &pkgs[*pi];
        for i in 0..p.exports.len() {
            by_path.entry(format!("{}.{}", name, p.object_path(i as i32 + 1)).to_ascii_lowercase()).or_insert((*pi, i));
        }
    }
    let resolve = |pi: usize, obj: i32| -> Option<(usize, usize)> {
        if obj > 0 {
            return Some((pi, obj as usize - 1));
        }
        if obj == 0 {
            return None;
        }
        by_path.get(&pkgs[pi].object_path(obj).to_ascii_lowercase()).copied()
    };
    let top = |pi: usize, obj: i32| pkgs[pi].object_path(obj).split('.').next().unwrap_or("").to_string();
    // placements
    let mut unresolved: BTreeMap<String, usize> = BTreeMap::new();
    let (mut sm_ok, mut sm_bad, mut st_ok, mut st_bad, mut sk_ok, mut sk_bad, mut panics) = (0, 0, 0, 0, 0, 0, 0);
    let mut sm_read: std::collections::HashMap<(usize, usize), bool> = Default::default();
    // StaticMeshes that resolve but do not read (Spawner::static_mesh returns None)
    let mut unread: BTreeMap<String, usize> = BTreeMap::new();
    let mut mats: std::collections::BTreeSet<(usize, usize)> = Default::default();
    let mut census: BTreeMap<String, usize> = BTreeMap::new();
    let (mut emitters, mut fluids, mut roads) = (0, 0, 0);
    for pi in 0..n_world {
        let p = &pkgs[pi];
        let pl = match catch_unwind(AssertUnwindSafe(|| p.static_mesh_placements())) {
            Ok(v) => v,
            Err(_) => {
                println!("  {}: PANIC in static_mesh_placements", names[pi]);
                panics += 1;
                Vec::new()
            }
        };
        for x in &pl {
            let Some((mp, mi)) = resolve(pi, x.mesh) else {
                *unresolved.entry(format!("StaticMesh {}", top(pi, x.mesh))).or_default() += 1;
                sm_bad += 1;
                continue;
            };
            let ok = *sm_read.entry((mp, mi)).or_insert_with(|| {
                let r = catch_unwind(AssertUnwindSafe(|| pkgs[mp].read_static_mesh_full(mi, &lib)));
                match r {
                    Ok(Some(sm)) => {
                        if let Some(l) = sm.lods.first() {
                            for s in &l.sections {
                                if let Some(m) = resolve(mp, s.material) {
                                    mats.insert(m);
                                }
                            }
                        }
                        true
                    }
                    Ok(None) => false,
                    Err(_) => {
                        println!("  PANIC reading StaticMesh {}", pkgs[mp].object_path(mi as i32 + 1));
                        false
                    }
                }
            });
            if ok {
                sm_ok += 1;
            } else {
                sm_bad += 1;
                *unread.entry(pkgs[mp].object_path(mi as i32 + 1)).or_default() += 1;
            }
            for &m in x.materials.iter().chain(x.lod_materials.iter().flatten()).filter(|&&m| m != 0) {
                match resolve(pi, m) {
                    Some(r) => {
                        mats.insert(r);
                    }
                    None => *unresolved.entry(format!("material {}", top(pi, m))).or_default() += 1,
                }
            }
        }
        match catch_unwind(AssertUnwindSafe(|| p.speedtree_placements())) {
            Ok(v) => {
                for t in v {
                    match resolve(pi, t.tree) {
                        Some((tp, ti)) if catch_unwind(AssertUnwindSafe(|| pkgs[tp].read_speedtree(ti))).ok().flatten().is_some() => st_ok += 1,
                        Some(_) => st_bad += 1,
                        None => {
                            st_bad += 1;
                            *unresolved.entry(format!("SpeedTree {}", top(pi, t.tree))).or_default() += 1;
                        }
                    }
                }
            }
            Err(_) => {
                println!("  {}: PANIC in speedtree_placements", names[pi]);
                panics += 1;
            }
        }
        match catch_unwind(AssertUnwindSafe(|| p.skeletal_mesh_placements())) {
            Ok(v) => {
                for s in v {
                    // mesh 0: from the component archetype in another package (main.rs load_maps)
                    let r = if s.mesh == 0 {
                        resolve(pi, s.comp_import).and_then(|(ap, ai)| pkgs[ap].export_props(ai).and_then(|acp| acp.object("SkeletalMesh")).filter(|&m| m != 0).and_then(|m| resolve(ap, m)))
                    } else {
                        resolve(pi, s.mesh)
                    };
                    match r {
                        Some((kp, ki)) if catch_unwind(AssertUnwindSafe(|| pkgs[kp].read_skeletal_mesh(ki))).ok().flatten().is_some() => sk_ok += 1,
                        Some(_) => sk_bad += 1,
                        None => {
                            sk_bad += 1;
                            *unresolved.entry(format!("SkeletalMesh {}", top(pi, if s.mesh == 0 { s.comp_import } else { s.mesh }))).or_default() += 1;
                        }
                    }
                }
            }
            Err(_) => {
                println!("  {}: PANIC in skeletal_mesh_placements", names[pi]);
                panics += 1;
            }
        }
        emitters += catch_unwind(AssertUnwindSafe(|| p.emitter_placements().len())).unwrap_or_else(|_| {
            panics += 1;
            0
        });
        fluids += catch_unwind(AssertUnwindSafe(|| p.fluid_surfaces().len())).unwrap_or_else(|_| {
            panics += 1;
            0
        });
        roads += catch_unwind(AssertUnwindSafe(|| p.road_spline_meshes().len())).unwrap_or_else(|_| {
            panics += 1;
            0
        });
        // census: actors in the persistent level owning a mesh component no path places
        for (i, e) in p.exports.iter().enumerate() {
            let cls = p.export_class(e);
            if !matches!(cls.as_str(), "StaticMeshComponent" | "SkeletalMeshComponent") || e.outer_index <= 0 {
                continue;
            }
            let outer = &p.exports[e.outer_index as usize - 1];
            if !p.object_path(e.outer_index).starts_with("TheWorld.PersistentLevel.") || outer.outer_index <= 0 {
                continue;
            }
            let oc = p.export_class(outer);
            let handled = if cls == "StaticMeshComponent" { PLACED_STATIC.contains(&oc.as_str()) } else { oc.contains("Actor") };
            if !handled && p.export_props(i).is_some_and(|cp| cp.object(if cls == "StaticMeshComponent" { "StaticMesh" } else { "SkeletalMesh" }).is_some_and(|m| m != 0)) {
                *census.entry(format!("{oc} ({cls})")).or_default() += 1;
            }
        }
    }
    println!("static mesh placements: {sm_ok} ok, {sm_bad} not drawn; SpeedTrees {st_ok} ok, {st_bad} not; skeletal meshes {sk_ok} ok, {sk_bad} not; emitters {emitters}, fluid surfaces {fluids}, road spline meshes {roads}; panics {panics}");
    println!("unresolved references (by package): {unresolved:?}");
    println!("StaticMeshes that do not read: {unread:?}");
    println!("imports from packages missing in the install: {missing:?}");
    println!("mesh-owning actor classes not placed by load_maps: {census:?}");
    // materials of the placed meshes: graph compiler + naga (world.rs build_material)
    let (mut ok, mut bad, mut nocomp, mut mpanic) = (0, 0, 0, 0);
    let mut stub: BTreeMap<String, usize> = BTreeMap::new();
    let mut bad_names: Vec<String> = Vec::new();
    for &(mp, mi) in &mats {
        let p = &pkgs[mp];
        let r = catch_unwind(AssertUnwindSafe(|| {
            let c = p.compile_material(mi as i32 + 1).or_else(|| {
                let (chain, parent) = p.mic_chain(mi as i32 + 1);
                if parent >= 0 {
                    return None;
                }
                let (bp, bi) = resolve(mp, parent)?;
                pkgs[bp].compile_material_ext(bi as i32 + 1, &[], &chain)
            })?;
            let src = crate::gamemat::shader_source(&c);
            Some((crate::gamemat::validate_wgsl(&src).is_ok(), c.stubbed))
        }));
        match r {
            Ok(Some((v, st))) => {
                for s in st {
                    *stub.entry(s).or_default() += 1;
                }
                if v {
                    ok += 1;
                } else {
                    bad += 1;
                    bad_names.push(p.object_path(mi as i32 + 1));
                }
            }
            Ok(None) => nocomp += 1,
            Err(_) => {
                mpanic += 1;
                bad_names.push(format!("PANIC {}", p.object_path(mi as i32 + 1)));
            }
        }
    }
    println!("placed materials: {} total, WGSL ok {ok}, INVALID {bad}, not compiled {nocomp}, panics {mpanic}", mats.len());
    if !bad_names.is_empty() {
        println!("  invalid: {:?}", &bad_names[..bad_names.len().min(20)]);
    }
    println!("  stubbed material nodes: {stub:?}");
    // terrain: game shaders per palette as spawn_terrain_emu
    let sc = lib.path_of("RefShaderCache-PC-D3D-SM3").cloned().and_then(|p| upk::shadercache::ShaderCache::load(&p).ok());
    if let Some(sc) = &sc {
        for pi in 0..n_world {
            let p = &pkgs[pi];
            if !p.exports.iter().any(|e| p.export_class(e) == "Terrain") {
                continue;
            }
            let Some(t) = catch_unwind(AssertUnwindSafe(|| p.terrain_render(&|id| sc.is_record(id)))).ok().flatten() else {
                println!("  {}: terrain_render failed", names[pi]);
                continue;
            };
            let (mut tok, mut probs) = (0, Vec::new());
            for pal in &t.palette_names {
                let find = |mt: u32| {
                    let want = format!("TerrainMaterialResource:{pal} (MatType:{mt})");
                    t.resources.iter().find(|r| sc.record(&r.id).is_some_and(|rec| rec.friendly_name == want))
                };
                let (Some(rl), Some(rc)) = (find(1), find(0)) else {
                    probs.push(format!("{pal}: resources not found"));
                    continue;
                };
                let (Some(recl), Some(recc)) = (sc.record(&rl.id), sc.record(&rc.id)) else { continue };
                let vs = recl.shader("FTerrainLightingPassVertexFactory", "TBasePassVertexShaderFDirectionalLightLightMapPolicyFNoDensityPolicy").and_then(|g| sc.code(&g));
                let psl = recl.shader("FTerrainLightingPassVertexFactory", "TBasePassPixelShaderFDirectionalLightLightMapPolicySkyLight").and_then(|g| sc.code(&g));
                let psc = recc.shader("FTerrainVertexFactory", "FTerrainColorPixelShader").and_then(|g| sc.code(&g));
                let (Some(vs), Some(psl), Some(psc)) = (vs, psl, psc) else {
                    probs.push(format!("{pal}: shaders missing"));
                    continue;
                };
                let addr = |o: i32| crate::tex_address(p, o);
                match catch_unwind(AssertUnwindSafe(|| crate::terrainemu::generate(vs, psl, psc, &rl.uniforms, &rc.uniforms, &t.weight_maps, &addr))) {
                    Ok(Some(code)) => {
                        if crate::gamemat::validate_wgsl(&code.wgsl).is_ok() {
                            tok += 1;
                        } else {
                            probs.push(format!("{pal}: WGSL INVALID"));
                        }
                    }
                    Ok(None) => probs.push(format!("{pal}: generation failed")),
                    Err(_) => probs.push(format!("{pal}: PANIC")),
                }
            }
            println!("  {}: terrain {}x{} vertices, palettes {tok}/{} ok {probs:?}", names[pi], t.num_vertices[0], t.num_vertices[1], t.palette_names.len());
        }
    }
}

/// Blend mode, lighting model, two-sidedness of a road material, and whether its WGSL (game
/// shader on FLocalVertexFactory, graph compiler) validates; translucent ones are fogged in the
/// shader (weather.rs TRANSLUCENT_FOG_WGSL).
fn road_material_check(p: &upk::Package, sc: Option<&upk::shadercache::ShaderCache>, m: i32) -> String {
    if m <= 0 {
        return "import (not checked)".into();
    }
    // the lit path calls the sun shadow (main.rs sets this in game)
    if let Ok(mut g) = crate::gamemat::SHADOW_WGSL.write() {
        if g.is_empty() {
            *g = crate::sunshadow::wgsl(&Default::default(), 1.0, 1.0);
        }
    }
    let info = p.material_info(m);
    let mut out = format!("blend {:?}, two-sided {}", info.blend, info.two_sided);
    if let Some(c) = p.compile_material(m) {
        out += &format!(", lighting {}", c.lighting);
        let src = crate::gamemat::shader_source(&c);
        let ok = crate::gamemat::validate_wgsl(&src).is_ok();
        out += &format!(", graph WGSL {}", if ok { "ok" } else { "INVALID" });
    }
    let gs = sc.and_then(|sc| {
        let (id, uniforms) = p.material_resource(m, &|g| sc.is_record(g))?;
        let rec = p.static_parameter_set(m).and_then(|(b, sw)| sc.record_for_static_set(&b, &sw)).or_else(|| sc.record(&id))?;
        let vf = "FLocalVertexFactory";
        let vs = sc.code(&rec.shader(vf, "TBasePassVertexShaderFNoLightMapPolicyFNoDensityPolicy")?)?;
        let ps = ["TBasePassPixelShaderFNoLightMapPolicySkyLight", "TBasePassPixelShaderFNoLightMapPolicyNoSkyLight"].iter().find_map(|t| sc.code(&rec.shader(vf, t)?))?;
        let params = p.material_params(m);
        let addr = |o: i32| crate::tex_address(p, o);
        let shadow = crate::sunshadow::wgsl(&Default::default(), 1.0, 1.0);
        crate::gameshader::generate(vs, ps, &uniforms, &params, &addr, info.blend, vf, None, &shadow)
    });
    match gs {
        Some(g) => {
            // main pass only, as world.rs validates (the prepass variant is not validated in game)
            let ok = match crate::gamemat::validate_wgsl(&g.wgsl) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("{e}");
                    false
                }
            };
            let fog = g.wgsl.contains("= hf_translucent_fog(");
            out += &format!(", game-shader WGSL {}, fogged in shader: {}", if ok { "ok" } else { "INVALID" }, if fog { "yes" } else { "no (opaque: full-screen fog pass)" });
        }
        None => out += ", no game shader",
    }
    out
}

/// Baked light maps of a sublevel's StaticMesh placements (upk level.rs LightMap2D, world.rs /
/// gamemat.rs shader_source_lightmap): placements with one on LOD 0, HQ / LQ, textures that
/// decode, light-mapped WGSL of the materials (component overrides + the mesh's own, exports
/// of this package) that validates. None without light maps.
fn lightmap_check(p: &upk::Package) -> Option<String> {
    let pl = p.static_mesh_placements();
    let lm: Vec<_> = pl.iter().filter_map(|x| x.lightmaps.first().copied().flatten().map(|l| (x, l))).collect();
    let any_lod = pl.iter().filter(|x| x.lightmaps.iter().any(|l| l.is_some())).count();
    if any_lod == 0 {
        return None;
    }
    if let Ok(mut g) = crate::gamemat::SHADOW_WGSL.write() {
        if g.is_empty() {
            *g = crate::sunshadow::wgsl(&Default::default(), 1.0, 1.0);
        }
    }
    let hq = lm.iter().filter(|(_, l)| l.directional()).count();
    let mut texs: HashSet<i32> = HashSet::new();
    let mut mats: HashSet<i32> = HashSet::new();
    for (x, l) in &lm {
        texs.extend(l.textures.iter().copied().filter(|&t| t != 0));
        mats.extend(x.materials.iter().copied().filter(|&m| m > 0));
        if x.mesh > 0 {
            if let Some(sm) = p.read_static_mesh(x.mesh as usize - 1) {
                mats.extend(sm.lods.first().map(|l| l.sections.iter().map(|s| s.material).filter(|&m| m > 0).collect::<Vec<_>>()).unwrap_or_default());
            }
        }
    }
    let tex_ok = texs.iter().filter(|&&t| t > 0 && p.read_texture2d(t as usize - 1).and_then(|t| t.decode_rgba()).is_some()).count();
    let (mut ok, mut unlit, mut bad) = (0, 0, 0);
    for &m in &mats {
        let Some(c) = p.compile_material(m) else { continue };
        match crate::gamemat::shader_source_lightmap(&c) {
            None => unlit += 1,
            Some(src) if crate::gamemat::validate_wgsl(&src).is_ok() => ok += 1,
            Some(_) => bad += 1,
        }
    }
    Some(format!(
        "light maps: {} of {} placements on LOD 0 ({any_lod} on any LOD), {hq} HQ / {} LQ; textures {tex_ok}/{} decode; light-mapped WGSL ok {ok}, INVALID {bad}, no lit variant {unlit} (of {} export materials)",
        lm.len(),
        pl.len(),
        lm.len() - hq,
        texs.len(),
        mats.len()
    ))
}

fn check(game: &Path, folder: &Path, mode: &str) {
    println!("== mode {mode} (playable in the create-game menu: {})", crate::mapmode::has_data(folder, mode));
    let maps = crate::mapmode::match_maps(folder, mode);
    println!("sublevels ({}): {:?}", maps.len(), maps.iter().map(|m| m.file_name().unwrap_or_default().to_string_lossy().to_string()).collect::<Vec<_>>());
    let mut objectives = Vec::new();
    let mut frontlines = Vec::new();
    let mut groups: BTreeMap<String, usize> = BTreeMap::new();
    let mut skel: HashSet<String> = HashSet::new();
    let mut terrains = 0;
    let mut gameplay: Vec<upk::Package> = Vec::new();
    // road splines as main.rs draws them (game-shader path, world.rs build_game_shader): the
    // textures each section material binds; EngineMaterials.Default* = the magenta checker
    let sc = upk::library::Library::scan(game).ok().and_then(|l| l.path_of("RefShaderCache-PC-D3D-SM3").cloned()).and_then(|p| upk::shadercache::ShaderCache::load(&p).ok());
    let mut road_mats: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut roads_drawn = 0;
    for m in &maps {
        let Ok(p) = upk::Package::open(m) else {
            println!("  {}: does not open", m.display());
            continue;
        };
        let o = crate::objectives::load_pkgs(std::iter::once(&p));
        let f = crate::objectives::load_frontlines(std::iter::once(&p));
        let mut n_start = 0;
        for (i, e) in p.exports.iter().enumerate() {
            match p.export_class(e).as_str() {
                "GameMode_PlayerStart" => {
                    let Some(pr) = p.export_props(i) else { continue };
                    let mut g = pr.enum_name("Group").unwrap_or("").to_string();
                    if pr.enum_name("CameraTransition").is_some_and(|c| c.contains("AirVehicle")) {
                        g.push_str("#air");
                    }
                    *groups.entry(g).or_default() += 1;
                    n_start += 1;
                }
                "SkeletalMesh" => {
                    skel.insert(p.object_path(i as i32 + 1).to_ascii_lowercase());
                }
                _ => {}
            }
        }
        let t = p.terrains().len();
        terrains += t;
        let stem = m.file_name().unwrap_or_default().to_string_lossy().to_string();
        if let Some(l) = lightmap_check(&p) {
            println!("  {stem}: {l}");
        }
        if !o.is_empty() || !f.is_empty() || n_start > 0 || t > 0 {
            println!("  {stem}: {} objectives, {} front lines, {n_start} PlayerStarts, {t} terrains", o.len(), f.len());
        }
        for r in p.road_spline_meshes() {
            roads_drawn += 1;
            for &mref in r.materials.iter().filter(|&&m| m != 0) {
                let e = road_mats.entry(p.object_path(mref)).or_insert_with(|| {
                    let tex = match (&sc, mref > 0) {
                        (Some(sc), true) => p
                            .material_resource(mref, &|g| sc.is_record(g))
                            .map(|(_, u)| {
                                let params = p.material_params(mref);
                                let t: Vec<String> = u.tex2d.iter().filter_map(|x| x.texture()).map(|(n, o)| p.object_path(n.and_then(|n| params.texture(n)).unwrap_or(o))).collect();
                                t.join(", ")
                            })
                            .unwrap_or_else(|| "no cooked resource".into()),
                        _ => "import / no shader cache".into(),
                    };
                    (0, format!("{tex}\n    {}", road_material_check(&p, sc.as_ref(), mref)))
                });
                e.0 += 1;
            }
        }
        objectives.extend(o);
        frontlines.extend(f);
        let keep = p.exports.iter().any(|e| matches!(p.export_class(e).as_str(), "GameMode_CombatVolume" | "MinimapVolume"));
        if keep {
            gameplay.push(p);
        }
    }
    // per material also: blend / lighting / two-sided, and whether the generated shaders
    // (game-shader path as world.rs build_game_shader, graph compiler as gamemat) validate
    println!("road spline meshes drawn: {roads_drawn}");
    for (m, (n, tex)) in &road_mats {
        let flag = if tex.contains("EngineMaterials.Default") { "  <- DEFAULT TEXTURE (checker)" } else { "" };
        println!("  road material {m} ({n}x): {tex}{flag}");
    }
    drop(sc);
    println!("PlayerStart groups: {groups:?}");
    println!("terrain actors: {terrains}");
    println!("full-map bounds (minimap): {:?}", crate::objectives::minimap_bounds(gameplay.iter()));
    drop(gameplay);
    // objectives and lines as update_objectives sets them up
    let mut obj = crate::objectives::Objectives { list: objectives.clone(), tdm: crate::mapmode::is_tdm(mode), ..Default::default() };
    let lines = obj.list.iter().map(|o| o.line).max().map(|m| m + 1).unwrap_or(0);
    obj.active_line = lines / 2;
    println!("objectives: {} in {lines} lines, active line {} (TDM {})", obj.list.len(), obj.active_line, obj.tdm);
    for o in &obj.list {
        println!("  {} line {} at {:?} cap {} s", ["A", "B", "C", "D"][o.letter.min(3)], o.line, o.location.map(|v| v.round()), o.cap_time);
    }
    for (k, f) in frontlines.iter().enumerate() {
        let per_team: Vec<Vec<(String, usize)>> = f.spawns.iter().map(|t| t.iter().map(|s| (s.0.trim_start_matches("GMST_").to_string(), s.1.len())).collect()).collect();
        println!("front line {k}: {} objectives, agnostic {}, USA {:?}, KVA {:?}, Group/Team mismatches {}", f.objectives.len(), f.agnostic, per_team[0], per_team[1], f.group_mismatch);
    }
    // every front line has infantry starts for both teams (TDM: the agnostic set has some);
    // mapmode.rs INCOMPLETE lists the gameplay sublevels where this is false
    let inf = |f: &crate::objectives::Frontline, t: usize| f.spawns[t].iter().filter(|s| s.0 == "GMST_Infantry").map(|s| s.1.len()).sum::<usize>();
    let complete = !frontlines.is_empty() && frontlines.iter().all(|f| inf(f, 0) > 0 && inf(f, 1) > 0);
    println!("spawn data complete: {complete}");
    let level = crate::objectives::LevelData { maps: maps.clone(), objectives, minimap_bounds: None, frontlines };
    for ty in ["GMST_Infantry", "GMST_Jeep", "GMST_TreadedVehicle", "GMST_AmphibiousVehicle", "GMST_LightHelicopter", "GMST_AttackHelicopter"] {
        let n: Vec<usize> = [0u8, 1].iter().map(|&t| crate::objectives::line_spawns(Some(&level), &obj, t, ty).len()).collect();
        println!("spawns at match start {ty}: USA {} KVA {}", n[0], n[1]);
    }
    if obj.tdm {
        // influence: one KVA soldier at the first USA start-half point
        let usa = crate::objectives::line_spawns(Some(&level), &obj, 0, "GMST_Infantry");
        if let Some((p, _)) = usa.first() {
            obj.people = vec![([p.x * 100.0, p.z * 100.0, p.y * 100.0], 1)];
            let a = crate::objectives::line_spawns(Some(&level), &obj, 0, "GMST_Infantry");
            let b = crate::objectives::line_spawns(Some(&level), &obj, 1, "GMST_Infantry");
            println!("TDM influence, one KVA soldier at {:?}: USA best {} points (nearest {:.0} m), KVA best {} points (nearest {:.0} m)", (*p * 100.0).round(), a.len(), a.iter().map(|q| q.0.distance(*p)).fold(f32::MAX, f32::min), b.len(), b.iter().map(|q| q.0.distance(*p)).fold(f32::MAX, f32::min));
        }
    }
    // vehicles: the buyable classes' meshes in the match packages
    let lib = upk::library::Library::scan(game).ok();
    let content = lib.as_ref().and_then(|l| l.open("GCGameContent"));
    let Some(content) = content else {
        println!("vehicles: GCGameContent not found");
        return;
    };
    for c in crate::vehload::CLASSES {
        let mut cls = content.exports.iter().position(|e| content.object_name_of(e) == c && content.export_class(e) == "Class").map(|x| x as i32 + 1).unwrap_or(0);
        let mut mesh = None;
        for _ in 0..16 {
            if cls <= 0 {
                break;
            }
            let n = content.object_name(cls);
            let sub = format!("Default__{n}.SVehicleMesh");
            if let Some(i) = content.exports.iter().enumerate().position(|(i, _)| content.object_path(i as i32 + 1) == sub) {
                if let Some(o) = content.export_props(i).and_then(|pr| pr.object("SkeletalMesh")).filter(|&o| o != 0) {
                    mesh = Some(content.object_path(o));
                    break;
                }
            }
            cls = content.exports[cls as usize - 1].super_index;
        }
        let found = mesh.as_ref().is_some_and(|m| {
            let l = m.to_ascii_lowercase();
            skel.contains(&l) || l.split_once('.').is_some_and(|(_, rest)| skel.contains(rest))
        });
        println!("vehicle {c}: mesh {:?} in the match packages: {found}", mesh);
    }
}
