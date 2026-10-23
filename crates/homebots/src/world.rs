//! Spawning level objects across all loaded map packages.
//!
//! Cooked UE3 sublevels import objects that are exported by the persistent level (e.g.
//! Overpass_* sublevels import `STA_AngelIsland.rocks.Boulder8`, which FL-Overpass.map
//! exports), so object references are resolved across every loaded package by path, then
//! through the library for standalone packages.

use crate::gamemat;
use bevy::pbr::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::render_resource::Face;
use bevy::render::storage::ShaderStorageBuffer;
use std::collections::HashMap;
use upk::library::Library;
use upk::matcompile::Address;
use upk::Package;

pub struct Spawner<'a> {
    pub pkgs: &'a [Package],
    /// Shader cache for the game-shader path (gameshader.rs); None = graph compiler only.
    pub sc: Option<&'a upk::shadercache::ShaderCache>,
    pub gs_globals: Handle<ShaderStorageBuffer>,
    pub gs_objects: Handle<ShaderStorageBuffer>,
    /// Per-object constant blocks for the game-shader path, indexed by MeshTag.
    pub gs_blocks: Vec<[[f32; 4]; crate::gameshader::OBJ_STRIDE]>,
    pub gs_problems: HashMap<String, usize>,
    gs_cache: HashMap<(usize, i32, bool, &'static str), Option<Handle<crate::terrainemu::TerrainEmu>>>,
    smp_cache: HashMap<[Address; 2], Handle<Image>>,
    white: Option<Handle<Image>>,
    white_cube: Option<Handle<Image>>,
    black: Option<Handle<Image>>,
    pub upper_sky: [f32; 4],
    pub lower_sky: [f32; 4],
    /// SceneColorTexture (scenecopy.rs)
    pub scene: Option<Handle<Image>>,
    pub lib: &'a Library,
    pub globals: Handle<ShaderStorageBuffer>,
    pub grass: Option<Handle<Image>>,
    pub max_tex: u32,
    pub no_cull: bool,
    /// Per-object constants (TerrainHeight), indexed by MeshTag.
    pub objects: Vec<[f32; 4]>,
    pub stub_log: HashMap<String, usize>,
    by_path: HashMap<String, (usize, usize)>,
    mesh_cache: HashMap<(usize, usize), Option<(Vec<(Handle<Mesh>, i32)>, usize)>>,
    /// SpeedTree: per tree export, LOD-0 parts (element group, mesh, material ref in the tree's package)
    st_cache: HashMap<(usize, usize), Option<Vec<(usize, Handle<Mesh>, i32)>>>,
    skel_cache: HashMap<(usize, usize), Option<(Vec<(Handle<Mesh>, i32)>, Transform, [[f32; 3]; 4])>>,
    mat_cache: HashMap<(usize, i32, bool), Option<Handle<gamemat::GameMat>>>,
    tex_cache: HashMap<(usize, usize, [Address; 2]), Option<Handle<Image>>>,
    shader_cache: HashMap<String, Handle<Shader>>,
    /// sunshadow::wgsl() for the game shaders' LightAttenuationTexture
    pub shadow_wgsl: String,
    /// spawn_mesh: let the meshes cast sun shadows (set per placement by the caller)
    pub cast_shadows: bool,
    /// materials (package, export) with a 2D texture that could not be loaded
    pub incomplete: std::collections::HashSet<(usize, usize)>,
    /// destruction.rs: textures that are not in the data (imports of packages missing from the
    /// install) and what to bind instead: (missing object path, lower case; replacement object
    /// path, "" = black). Empty = the white fallback as before. NOT from data (see destruction.rs).
    pub tex_substitute: Vec<(String, String)>,
    /// Light map of the next spawn_mesh call (package of the component, its LOD's LightMap2D;
    /// main.rs static mesh placements), taken by spawn_mesh.
    pub lightmap: Option<(usize, upk::level::LightMap2D)>,
    /// build_material: compile the light-mapped shader variant (gamemat::shader_source_lightmap)
    lm_variant: bool,
    /// light-mapped GameMats: (package, material, two-sided, light-map textures) -> material
    lm_cache: HashMap<(usize, i32, bool, Option<Handle<Image>>, Option<Handle<Image>>), Option<Handle<gamemat::GameMat>>>,
    /// light-mapped StaticMesh placements spawned / with light map but not usable (texture or
    /// variant missing) (HB_MAPCHECK, log)
    pub lm_stats: [usize; 2],
}

pub struct Assets3<'a> {
    pub terrain_mats: &'a mut Assets<crate::terrainemu::TerrainEmu>,
    pub meshes: &'a mut Assets<Mesh>,
    pub images: &'a mut Assets<Image>,
    pub game_mats: &'a mut Assets<gamemat::GameMat>,
    pub shaders: &'a mut Assets<Shader>,
}

impl<'a> Spawner<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pkgs: &'a [Package],
        lib: &'a Library,
        sc: Option<&'a upk::shadercache::ShaderCache>,
        globals: Handle<ShaderStorageBuffer>,
        gs_globals: Handle<ShaderStorageBuffer>,
        gs_objects: Handle<ShaderStorageBuffer>,
        grass: Option<Handle<Image>>,
        max_tex: u32,
        no_cull: bool,
    ) -> Self {
        let mut by_path = HashMap::new();
        for (pi, p) in pkgs.iter().enumerate() {
            for i in 0..p.exports.len() {
                by_path.entry(p.object_path(i as i32 + 1).to_ascii_lowercase()).or_insert((pi, i));
            }
        }
        Spawner {
            pkgs,
            sc,
            gs_globals,
            gs_objects,
            gs_blocks: vec![[[0.0; 4]; crate::gameshader::OBJ_STRIDE]],
            gs_problems: HashMap::new(),
            gs_cache: HashMap::new(),
            smp_cache: HashMap::new(),
            white: None,
            white_cube: None,
            black: None,
            upper_sky: [0.0; 4],
            lower_sky: [0.0; 4],
            scene: None,
            lib,
            globals,
            grass,
            max_tex,
            no_cull,
            objects: vec![[0.0; 4]],
            stub_log: HashMap::new(),
            by_path,
            mesh_cache: HashMap::new(),
            st_cache: HashMap::new(),
            skel_cache: HashMap::new(),
            mat_cache: HashMap::new(),
            tex_cache: HashMap::new(),
            shader_cache: HashMap::new(),
            shadow_wgsl: String::new(),
            cast_shadows: false,
            incomplete: Default::default(),
            tex_substitute: Vec::new(),
            lightmap: None,
            lm_variant: false,
            lm_cache: HashMap::new(),
            lm_stats: [0; 2],
        }
    }

    pub fn shader_count(&self) -> usize {
        self.shader_cache.len()
    }

    /// (package, export index) of an object reference made from package `pi`.
    /// Register the exports of a standalone package (e.g. Effects_FL2/KFX_Weapons.upk) under
    /// "<package>.<path>": inside its own file the package is the root, but map packages import
    /// its objects as "KFX_Weapons.Textures.MF_Smoke".
    pub fn alias_package(&mut self, pi: usize, name: &str) {
        let p = &self.pkgs[pi];
        for i in 0..p.exports.len() {
            let k = format!("{}.{}", name, p.object_path(i as i32 + 1)).to_ascii_lowercase();
            self.by_path.entry(k).or_insert((pi, i));
        }
    }

    pub fn resolve(&self, pi: usize, obj: i32) -> Option<(usize, usize)> {
        if obj > 0 {
            return Some((pi, obj as usize - 1));
        }
        if obj == 0 {
            return None;
        }
        let path = self.pkgs[pi].object_path(obj).to_ascii_lowercase();
        self.by_path.get(&path).copied()
    }

    /// Mesh sections of a StaticMesh export (game-shader vertex layout) with their materials.
    /// Sections of a StaticMesh (most detailed LOD available, loading LOD 0 from the source
    /// package when the map has it as separate bulk data) and the LOD index rendered.
    pub fn static_mesh(&mut self, a: &mut Assets3, pi: usize, idx: usize) -> Option<(Vec<(Handle<Mesh>, i32)>, usize)> {
        let _perf = crate::perf::scope("ld_static_mesh");
        let (pkgs, lib) = (self.pkgs, self.lib);
        self.mesh_cache
            .entry((pi, idx))
            .or_insert_with(|| {
                let sm = if std::env::var_os("HOMEBOTS_NO_FULL_LOD").is_some() { pkgs[pi].read_static_mesh(idx)? } else { pkgs[pi].read_static_mesh_full(idx, lib)? };
                let lod = sm.lods.first()?;
                // light-map UV channel: StaticMesh.LightMapCoordinateIndex (not serialized = 0,
                // Default__StaticMesh has none); used by the light-mapped GameMat variant only
                let lmci = pkgs[pi].export_props(idx).and_then(|p| p.int("LightMapCoordinateIndex")).unwrap_or(0).max(0) as usize;
                let lm_uv = lod.uv_sets.get(lmci).filter(|u| u.len() == lod.positions.len()).cloned().unwrap_or_else(|| vec![[0.0; 2]; lod.positions.len()]);
                let mut parts = Vec::new();
                for s in &lod.sections {
                    let Some(mut m) = crate::build_game_mesh(lod, s) else { continue };
                    m.insert_attribute(gamemat::ATTR_LM_UV, lm_uv.clone());
                    parts.push((a.meshes.add(m), s.material));
                }
                Some((parts, sm.lod_index))
            })
            .clone()
    }

    /// A StaticMesh as a one-bone skeletal mesh for CPU skinning (vehicle mirror attachments,
    /// vehload.rs): vertices through `lin` / `t` into the driver mesh's space, all on `bone`
    /// (its component-space reference as the bone's own); a mirroring `lin` (det < 0) reverses
    /// the winding and the binormal sign.
    pub fn static_as_skinned(&mut self, a: &mut Assets3, pi: usize, idx: usize, lin: bevy::math::Mat3, t: Vec3, bone: upk::skeletal::Bone) -> Option<(Vec<(Handle<Mesh>, i32)>, upk::skeletal::SkeletalMesh)> {
        let sm = if std::env::var_os("HOMEBOTS_NO_FULL_LOD").is_some() { self.pkgs[pi].read_static_mesh(idx)? } else { self.pkgs[pi].read_static_mesh_full(idx, self.lib)? };
        let mut lod = sm.lods.first()?.clone();
        let mirror = lin.determinant() < 0.0;
        let nrm = lin.inverse().transpose();
        for p in lod.positions.iter_mut() {
            *p = (lin * Vec3::from(*p) + t).to_array();
        }
        for n in lod.normals.iter_mut() {
            *n = (nrm * Vec3::from(*n)).normalize_or_zero().to_array();
        }
        let unpack = |v: [u8; 4]| Vec3::new(v[0] as f32 / 127.5 - 1.0, v[1] as f32 / 127.5 - 1.0, v[2] as f32 / 127.5 - 1.0);
        let pack = |v: Vec3, w: u8| {
            let c = |x: f32| ((x.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8;
            [c(v.x), c(v.y), c(v.z), w]
        };
        for tx in lod.tangent_x.iter_mut() {
            *tx = pack((lin * unpack(*tx)).normalize_or_zero(), tx[3]);
        }
        for tz in lod.tangent_z.iter_mut() {
            *tz = pack((nrm * unpack(*tz)).normalize_or_zero(), if mirror { 255 - tz[3] } else { tz[3] });
        }
        if mirror {
            for tri in lod.indices.chunks_mut(3) {
                tri.swap(1, 2);
            }
        }
        let mut materials: Vec<i32> = Vec::new();
        for s in lod.sections.iter_mut() {
            let k = materials.iter().position(|&m| m == s.material).unwrap_or_else(|| {
                materials.push(s.material);
                materials.len() - 1
            });
            s.material = k as i32;
        }
        let n = lod.positions.len();
        let mesh = upk::skeletal::SkeletalMesh { props: Default::default(), materials, origin: [0.0; 3], rot_origin: [0; 3], lod, bones: vec![bone], influences: vec![([0; 4], [255, 0, 0, 0]); n], lod0_material_map: Vec::new() };
        let mut parts = Vec::new();
        for s in &mesh.lod.sections {
            let Some(map) = crate::section_vertex_map(&mesh.lod, s) else { continue };
            let Some(m) = crate::build_skinned_section(&mesh.lod, s, &map) else { continue };
            parts.push((a.meshes.add(m), mesh.section_material(s.material)));
        }
        Some((parts, mesh))
    }

    /// Sections of a SkeletalMesh for CPU skinning (viewmodel.rs): one Mesh per section (all
    /// vertices, positions rewritten every frame) plus the decoded mesh (bones, influences).
    pub fn skinned_mesh(&mut self, a: &mut Assets3, pi: usize, idx: usize) -> Option<(Vec<(Handle<Mesh>, i32)>, upk::skeletal::SkeletalMesh)> {
        let sm = self.pkgs[pi].read_skeletal_mesh(idx)?;
        let mut parts = Vec::new();
        // sections in the order SkinPart::new rebuilds their vertex maps
        for s in &sm.lod.sections {
            let Some(map) = crate::section_vertex_map(&sm.lod, s) else { continue };
            let Some(m) = crate::build_skinned_section(&sm.lod, s, &map) else { continue };
            let mat = sm.section_material(s.material);
            parts.push((a.meshes.add(m), mat));
        }
        Some((parts, sm))
    }

    /// Reference-pose sections of a SkeletalMesh export with their material references, and
    /// the mesh's own transform (RotOrigin, Origin), applied before the component's.
    pub fn skeletal_mesh(&mut self, a: &mut Assets3, pi: usize, idx: usize) -> Option<(Vec<(Handle<Mesh>, i32)>, Transform, [[f32; 3]; 4])> {
        let pkgs = self.pkgs;
        self.skel_cache
            .entry((pi, idx))
            .or_insert_with(|| {
                let sk = pkgs[pi].read_skeletal_mesh(idx)?;
                let mut parts = Vec::new();
                for s in &sk.lod.sections {
                    let Some(m) = crate::build_game_mesh(&sk.lod, s) else { continue };
                    let mat = sk.section_material(s.material);
                    parts.push((a.meshes.add(m), mat));
                }
                let local = Transform {
                    translation: crate::ue_to_bevy(sk.origin),
                    rotation: crate::rotator_to_quat(sk.rot_origin),
                    scale: Vec3::ONE,
                };
                let l2w = crate::gameshader::ue_l2w(sk.origin, sk.rot_origin, [1.0; 3]);
                Some((parts, local, l2w))
            })
            .clone()
    }

    /// A static mesh deformed along a road spline segment, transcribed from the game's
    /// FSplineMeshVertexFactory vertex shader (RefShaderCache, e.g. M_Road_Corner_Integrated):
    /// a = (y - MinY) / LengthY; P(a) = Hermite(0, StartTangent, EndPos, EndTangent);
    /// D = normalize(P'(a)); X = normalize(D x Z); U = normalize(X x D); roll and scale lerped;
    /// pos = P + x * sx * X' + z * sy * U'; tangent basis mapped (x, y, z) -> (X', D, U');
    /// V texture coordinate scaled by the segment's MaterialScale (SplineEndPos.w).
    pub fn road_mesh(&mut self, a: &mut Assets3, pi: usize, idx: usize, r: &upk::level::RoadSplineMesh) -> Option<Vec<(Handle<Mesh>, i32)>> {
        let sm = self.pkgs[pi].read_static_mesh(idx)?;
        let mut lod = sm.lods.first()?.clone();
        let min_y = sm.origin[1] - sm.extent[1];
        let inv_len = 1.0 / (2.0 * sm.extent[1]).max(1e-6);
        let v3 = |x: [f32; 3]| Vec3::from_array(x);
        let (t0, p1, t1) = (v3(r.start_tangent), v3(r.end_pos), v3(r.end_tangent));
        let unpack = |b: [u8; 4]| Vec3::new(b[0] as f32 / 127.5 - 1.0, b[1] as f32 / 127.5 - 1.0, b[2] as f32 / 127.5 - 1.0);
        let pack = |v: Vec3, w: u8| [0, 1, 2].map(|k| ((v[k].clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8).into_iter().chain([w]).collect::<Vec<u8>>().try_into().unwrap();
        for k in 0..lod.positions.len() {
            let p = lod.positions[k];
            let t = (p[1] - min_y) * inv_len;
            let (t2, t3) = (t * t, t * t * t);
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            let pos = h10 * t0 + h01 * p1 + h11 * t1;
            let d = (t0 + t * (6.0 * p1 - 4.0 * t0 - 2.0 * t1) + t2 * (3.0 * t0 + 3.0 * t1 - 6.0 * p1)).normalize_or_zero();
            let x = d.cross(Vec3::Z).normalize_or_zero();
            let u = x.cross(d).normalize_or_zero();
            let roll = r.start_roll + (r.end_roll - r.start_roll) * t;
            let (sn, cs) = roll.sin_cos();
            let xr = cs * x - sn * u;
            let ur = cs * u + sn * x;
            let sx = r.start_scale[0] + (r.end_scale[0] - r.start_scale[0]) * t;
            let sy = r.start_scale[1] + (r.end_scale[1] - r.start_scale[1]) * t;
            let np = pos + p[0] * sx * xr + p[2] * sy * ur;
            lod.positions[k] = np.to_array();
            let map = |v: Vec3| v.x * xr + v.y * d + v.z * ur;
            let tx = lod.tangent_x[k];
            let tz = lod.tangent_z[k];
            lod.tangent_x[k] = pack(map(unpack(tx)), tx[3]);
            lod.tangent_z[k] = pack(map(unpack(tz)), tz[3]);
            lod.uvs[k][1] *= r.material_scale;
        }
        if let Some(s0) = lod.uv_sets.first_mut() {
            *s0 = lod.uvs.clone();
        }
        let mut parts = Vec::new();
        for s in &lod.sections {
            let Some(m) = crate::build_game_mesh(&lod, s) else { continue };
            parts.push((a.meshes.add(m), s.material));
        }
        Some(parts)
    }

    /// Collision triangles of a road spline segment (actor space, UE units): the deformation of
    /// `road_mesh` with the width (mesh X) scaled by the segment's collision scale (upk
    /// RoadSplineMesh.collision_scale, lerped along the spline; NOT verified in the exe that it
    /// multiplies the drawn width). Sections without EnableCollision are left out. Empty when the
    /// RoadSpline has DisableCollision.
    pub fn road_collision(&self, pi: usize, idx: usize, r: &upk::level::RoadSplineMesh) -> Vec<[[f32; 3]; 3]> {
        if !r.collision {
            return Vec::new();
        }
        let Some(sm) = self.pkgs[pi].read_static_mesh(idx) else { return Vec::new() };
        let Some(lod) = sm.lods.first() else { return Vec::new() };
        let flags = self.pkgs[pi].static_mesh_section_collision(idx).and_then(|f| f.get(sm.lod_index).cloned()).unwrap_or_default();
        let min_y = sm.origin[1] - sm.extent[1];
        let inv_len = 1.0 / (2.0 * sm.extent[1]).max(1e-6);
        let v3 = |x: [f32; 3]| Vec3::from_array(x);
        let (t0, p1, t1) = (v3(r.start_tangent), v3(r.end_pos), v3(r.end_tangent));
        let pos: Vec<[f32; 3]> = lod
            .positions
            .iter()
            .map(|p| {
                let t = (p[1] - min_y) * inv_len;
                let (t2, t3) = (t * t, t * t * t);
                let pos = (t3 - 2.0 * t2 + t) * t0 + (-2.0 * t3 + 3.0 * t2) * p1 + (t3 - t2) * t1;
                let d = (t0 + t * (6.0 * p1 - 4.0 * t0 - 2.0 * t1) + t2 * (3.0 * t0 + 3.0 * t1 - 6.0 * p1)).normalize_or_zero();
                let x = d.cross(Vec3::Z).normalize_or_zero();
                let u = x.cross(d).normalize_or_zero();
                let roll = r.start_roll + (r.end_roll - r.start_roll) * t;
                let (sn, cs) = roll.sin_cos();
                let (xr, ur) = (cs * x - sn * u, cs * u + sn * x);
                let cscale = r.collision_scale[0] + (r.collision_scale[1] - r.collision_scale[0]) * t;
                let sx = (r.start_scale[0] + (r.end_scale[0] - r.start_scale[0]) * t) * cscale;
                let sy = r.start_scale[1] + (r.end_scale[1] - r.start_scale[1]) * t;
                (pos + p[0] * sx * xr + p[2] * sy * ur).to_array()
            })
            .collect();
        let mut out = Vec::new();
        for (k, s) in lod.sections.iter().enumerate() {
            if !flags.get(k).copied().unwrap_or(true) {
                continue;
            }
            let first = s.first_index as usize;
            for t in 0..s.num_triangles as usize {
                let i = |j: usize| lod.indices.get(first + 3 * t + j).map(|&v| v as usize);
                let (Some(a), Some(b), Some(c)) = (i(0), i(1), i(2)) else { continue };
                if let (Some(a), Some(b), Some(c)) = (pos.get(a), pos.get(b), pos.get(c)) {
                    out.push([*a, *b, *c]);
                }
            }
        }
        out
    }

    /// AddressX / AddressY of a texture reference made from package `pi` (imports resolved).
    pub fn tex_addr(&self, pi: usize, obj: i32) -> [Address; 2] {
        match self.resolve(pi, obj) {
            Some((tp, ti)) => crate::tex_address(&self.pkgs[tp], ti as i32 + 1),
            None => crate::tex_address(&self.pkgs[pi], obj),
        }
    }

    fn texture(&mut self, a: &mut Assets3, pi: usize, obj: i32, address: [Address; 2]) -> Option<Handle<Image>> {
        let _perf = crate::perf::scope("ld_texture");
        let (tp, ti) = self.resolve(pi, obj)?;
        // an imported texture's address modes are those of its export (the referencing package
        // only has the import: KFX_LensFX Bloom01 TA_Clamp was sampled wrapped -> dot grid in the
        // holographic sight)
        let address = if obj < 0 { crate::tex_address(&self.pkgs[tp], ti as i32 + 1) } else { address };
        let (lib, pkgs, max_tex) = (self.lib, self.pkgs, self.max_tex);
        self.tex_cache
            .entry((tp, ti, address))
            .or_insert_with(|| gamemat::load_texture(&pkgs[tp], lib, ti as i32 + 1, address, max_tex).map(|i| a.images.add(i)))
            .clone()
    }

    /// Game material (compiled graph + Homefront base pass) for a material reference made
    /// from package `pi`.
    pub fn material(&mut self, a: &mut Assets3, pi: usize, mat_ref: i32, two_sided_override: bool) -> Option<Handle<gamemat::GameMat>> {
        let (mp, mi) = self.resolve(pi, mat_ref)?;
        // dev: HB_SKIPMAT=<path substrings> leaves sections with those materials out
        if let Ok(v) = std::env::var("HB_SKIPMAT") {
            let path = self.pkgs[mp].object_path(mi as i32 + 1);
            if v.split(',').any(|x| !x.is_empty() && path.contains(x)) {
                return None;
            }
        }
        let key = (mp, mi as i32 + 1, two_sided_override);
        if let Some(h) = self.mat_cache.get(&key) {
            return h.clone();
        }
        let h = self.build_material(a, mp, mi as i32 + 1, two_sided_override);
        self.mat_cache.insert(key, h.clone());
        h
    }

    /// Light-mapped variant of `material` with the light-map textures bound (lm1 = lm0 for an
    /// LQ map, unused by the shader there). None if the material has no lit variant.
    pub fn material_lightmapped(&mut self, a: &mut Assets3, pi: usize, mat_ref: i32, two_sided_override: bool, lm0: Handle<Image>, lm1: Handle<Image>) -> Option<Handle<gamemat::GameMat>> {
        let (mp, mi) = self.resolve(pi, mat_ref)?;
        let key = (mp, mi as i32 + 1, two_sided_override, Some(lm0.clone()), Some(lm1.clone()));
        if let Some(h) = self.lm_cache.get(&key) {
            return h.clone();
        }
        let base_key = (mp, mi as i32 + 1, two_sided_override, None, None);
        let base = match self.lm_cache.get(&base_key) {
            Some(h) => h.clone(),
            None => {
                self.lm_variant = true;
                let h = self.build_material(a, mp, mi as i32 + 1, two_sided_override);
                self.lm_variant = false;
                self.lm_cache.insert(base_key, h.clone());
                h
            }
        };
        let h = base.and_then(|b| a.game_mats.get(&b).cloned()).map(|mut m| {
            m.lm0 = Some(lm0);
            m.lm1 = Some(lm1);
            a.game_mats.add(m)
        });
        self.lm_cache.insert(key, h.clone());
        h
    }

    /// Light-map textures of a component's LightMap2D made from package `pi`: (coefficient 0
    /// or the LQ map, coefficient 1 or the same) and the per-object constants
    /// (objects[tag + 1..=3], gamemat::shader_source_lightmap).
    fn lightmap_inputs(&mut self, a: &mut Assets3, pi: usize, lm: &upk::level::LightMap2D) -> Option<(Handle<Image>, Handle<Image>, [[f32; 4]; 3])> {
        // sampler address modes: the texture's own (AddressX/Y not serialized = Texture2D
        // default, as for every other texture)
        let addr = |sp: &Self, o: i32| sp.resolve(pi, o).map(|(tp, ti)| crate::tex_address(&sp.pkgs[tp], ti as i32 + 1));
        let hq = lm.directional();
        let (i0, i1) = if hq { (0, 1) } else { (2, 2) };
        let a0 = addr(self, lm.textures[i0])?;
        let t0 = self.texture(a, pi, lm.textures[i0], a0)?;
        let t1 = if i1 == i0 {
            t0.clone()
        } else {
            let a1 = addr(self, lm.textures[i1])?;
            self.texture(a, pi, lm.textures[i1], a1)?
        };
        let s0 = lm.scales[i0];
        let s1 = lm.scales[i1];
        Some((t0, t1, [
            [lm.coord_scale[0], lm.coord_scale[1], lm.coord_bias[0], lm.coord_bias[1]],
            [s0[0], s0[1], s0[2], if hq { 2.0 } else { 1.0 }],
            [s1[0], s1[1], s1[2], 0.0],
        ]))
    }

    /// The material resolved and all its 2D textures loaded (call after `material`).
    pub fn material_complete(&self, pi: usize, mat_ref: i32) -> bool {
        self.resolve(pi, mat_ref).is_some_and(|k| !self.incomplete.contains(&k))
    }

    fn build_material(&mut self, a: &mut Assets3, pi: usize, mat_id: i32, two_sided_override: bool) -> Option<Handle<gamemat::GameMat>> {
        let _perf = crate::perf::scope("ld_material");
        let pkg = &self.pkgs[pi];
        // MICs whose parent material lives in another package (weapon/character content:
        // parent in MAT_LIB): compile the parent there with this package's instance overrides;
        // texture parameters become placeholders mapped back to this package below.
        const PLACEHOLDER: i32 = -1_000_000_000;
        let mut foreign: Option<(usize, Vec<i32>)> = None;
        let c = match pkg.compile_material(mat_id) {
            Some(c) => c,
            None => {
                let (mut chain, parent) = pkg.mic_chain(mat_id);
                if parent >= 0 {
                    return None;
                }
                let (bp, bi) = self.resolve(pi, parent)?;
                let objs: Vec<i32> = chain.tex.iter().map(|t| t.1).collect();
                for (k, t) in chain.tex.iter_mut().enumerate() {
                    t.1 = PLACEHOLDER - k as i32;
                }
                let c = self.pkgs[bp].compile_material_ext(bi as i32 + 1, &[], &chain)?;
                foreign = Some((bp, objs));
                c
            }
        };
        let pkg = &self.pkgs[pi];
        for st in &c.stubbed {
            *self.stub_log.entry(st.clone()).or_default() += 1;
        }
        let src = if self.lm_variant { gamemat::shader_source_lightmap(&c)? } else { gamemat::shader_source(&c) };
        if let Err(e) = gamemat::validate_wgsl(&src) {
            warn!("material {} skipped, generated shader invalid:\n{e}", pkg.object_path(mat_id));
            return None;
        }
        let name = pkg.object_name(mat_id);
        let sh = self
            .shader_cache
            .entry(src.clone())
            .or_insert_with(|| {
                if let Some(dir) = std::env::var_os("HOMEBOTS_DUMP_WGSL") {
                    let _ = std::fs::write(std::path::Path::new(&dir).join(format!("mat_{name}.wgsl")), &src);
                }
                a.shaders.add(Shader::from_wgsl(src, format!("hf_mat_{name}.wgsl")))
            })
            .clone();
        use upk::level::Blend;
        let alpha = gamemat::alpha_mode(&c);
        let cull = if two_sided_override || self.no_cull || c.info.two_sided { None } else { Some(Face::Back) };
        let prepass_shader = matches!(c.info.blend, Blend::Opaque | Blend::Masked).then(|| {
            let src = gamemat::prepass_source(&c);
            self.shader_cache.entry(src.clone()).or_insert_with(|| a.shaders.add(Shader::from_wgsl(src, format!("hf_mat_{name}_depth.wgsl")))).clone()
        });
        let mut gm = gamemat::GameMat {
            globals: self.globals.clone(),
            grass: self.grass.clone(),
            t0: None,
            t1: None,
            t2: None,
            t3: None,
            t4: None,
            t5: None,
            t6: None,
            t7: None,
            cube: None,
            scene: self.scene.clone(),
            lm0: None,
            lm1: None,
            lightmap: self.lm_variant,
            shader: sh,
            prepass_shader,
            alpha,
            cull,
        };
        let mut slot = 0;
        for t in c.textures.iter().filter(|t| !t.cube) {
            if slot >= gamemat::MAX_TEX {
                warn!("material {} uses more than {} textures", self.pkgs[pi].object_path(mat_id), gamemat::MAX_TEX);
                break;
            }
            // texture owner: this package, or the parent's package for a foreign-parent MIC
            let (tp, to) = match &foreign {
                Some((bp, objs)) => {
                    if t.object <= PLACEHOLDER && t.object > PLACEHOLDER - objs.len() as i32 - 1 {
                        (pi, objs[(PLACEHOLDER - t.object) as usize])
                    } else {
                        (*bp, t.object)
                    }
                }
                None => (pi, t.object),
            };
            if let Some(h) = self.texture(a, tp, to, t.address) {
                gm.set_tex(slot, h);
            } else {
                self.incomplete.insert((pi, mat_id as usize - 1));
                // destruction.rs: a texture missing from the data with a substitute
                if !self.tex_substitute.is_empty() && to < 0 {
                    let path = self.pkgs[tp].object_path(to).to_ascii_lowercase();
                    if let Some(rep) = self.tex_substitute.iter().find(|s| s.0 == path).map(|s| s.1.clone()) {
                        let h = if rep.is_empty() { Some(self.black(a)) } else { self.by_path.get(&rep.to_ascii_lowercase()).copied().and_then(|(rp, ri)| self.texture(a, rp, ri as i32 + 1, t.address)) };
                        if let Some(h) = h {
                            gm.set_tex(slot, h);
                        }
                    }
                }
            }
            slot += 1;
        }
        Some(a.game_mats.add(gm))
    }

    /// Spawn every section of a mesh with the given materials (per-section override from
    /// package `over_pi`, else the mesh's own material from its package). Materials go
    /// through the game's own shaders when the shader cache has them (gameshader.rs), else
    /// through the graph compiler (gamemat.rs).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_mesh(
        &mut self,
        commands: &mut Commands,
        a: &mut Assets3,
        mesh_pi: usize,
        parts: Vec<(Handle<Mesh>, i32)>,
        over_pi: usize,
        overrides: &[i32],
        transform: Transform,
        l2w: [[f32; 3]; 4],
        terrain_height: [f32; 4],
        two_sided_override: bool,
        game_shaders: bool,
    ) -> usize {
        let _perf = crate::perf::scope("ld_spawn_mesh");
        // light-mapped placement: its constants follow the object's block (3 extra tags)
        let lm = self.lightmap.take().and_then(|(lpi, l)| {
            let r = self.lightmap_inputs(a, lpi, &l);
            self.lm_stats[if r.is_some() { 0 } else { 1 }] += 1;
            r
        });
        let tag = self.push_object(l2w, terrain_height);
        // first-person mesh (objects[].w = 1, gamemat.rs depth squeeze): drawn after all world
        // translucency (viewmodel.rs FirstPersonDraw / fp_sort_last)
        let fg = terrain_height[3] > 0.5;
        if let Some((_, _, k)) = &lm {
            for v in k {
                self.objects.push(*v);
                self.gs_blocks.push([[0.0; 4]; crate::gameshader::OBJ_STRIDE]);
            }
        }
        let mut n = 0;
        for (si, (m, mesh_mat)) in parts.into_iter().enumerate() {
            let (mpi, mref) = match overrides.get(si).copied().filter(|&o| o != 0) {
                Some(o) if self.resolve(over_pi, o).is_some() => (over_pi, o),
                _ => (mesh_pi, mesh_mat),
            };
            // game-shader path: opt-in per call (roads) or for everything with HOMEBOTS_GAME_SHADERS
            if self.sc.is_some() && (game_shaders || std::env::var_os("HOMEBOTS_GAME_SHADERS").is_some()) {
                if let Some(mat) = self.game_shader_material(a, mpi, mref, "FLocalVertexFactory", two_sided_override) {
                    let cast = self.cast_shadows && a.terrain_mats.get(&mat).is_some_and(|m| m.prepass_shader.is_some());
                    let mut e = commands.spawn((crate::LevelEntity, Mesh3d(m), MeshMaterial3d(mat), transform, bevy::render::mesh::MeshTag(tag), NotShadowReceiver));
                    if !cast {
                        e.insert(NotShadowCaster);
                    }
                    if fg {
                        e.insert(crate::viewmodel::FirstPersonDraw(0.0));
                    }
                    n += 1;
                    continue;
                }
            }
            let Some(mut mat) = self.material(a, mpi, mref, two_sided_override) else { continue };
            if let Some((t0, t1, _)) = &lm {
                if let Some(m) = self.material_lightmapped(a, mpi, mref, two_sided_override, t0.clone(), t1.clone()) {
                    mat = m;
                }
            }
            let cast = self.cast_shadows && a.game_mats.get(&mat).is_some_and(|m| m.prepass_shader.is_some());
            let mut e = commands.spawn((crate::LevelEntity, Mesh3d(m), MeshMaterial3d(mat), transform, bevy::render::mesh::MeshTag(tag), NotShadowReceiver));
            if !cast {
                e.insert(NotShadowCaster);
            }
            if fg {
                e.insert(crate::viewmodel::FirstPersonDraw(0.0));
            }
            n += 1;
        }
        n
    }

    /// LOD-0 geometry of a SpeedTree (element groups with a known vertex declaration).
    pub fn speedtree_parts(&mut self, a: &mut Assets3, pi: usize, tree_ref: i32) -> Option<(usize, Vec<(usize, Handle<Mesh>, i32)>)> {
        let (tp, ti) = self.resolve(pi, tree_ref)?;
        if let Some(c) = self.st_cache.get(&(tp, ti)) {
            return c.clone().map(|v| (tp, v));
        }
        let built = self.build_speedtree(a, tp, ti);
        self.st_cache.insert((tp, ti), built.clone());
        built.map(|v| (tp, v))
    }

    fn build_speedtree(&mut self, a: &mut Assets3, tp: usize, ti: usize) -> Option<Vec<(usize, Handle<Mesh>, i32)>> {
        use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
        let st = self.pkgs[tp].read_speedtree(ti)?;
        let mut out = Vec::new();
        for g in 0..upk::speedtree::GROUPS {
            let Some(el) = st.elements[g].first() else { continue };
            let Some(layout) = crate::speedtree_layout(g) else { continue };
            let s = upk::speedtree::GROUP_STREAM[g];
            let n = st.positions[s].len();
            if el.num_primitives == 0 || n == 0 {
                continue;
            }
            let data = &st.data[s];
            let stride = st.strides[s];
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
            // bounds only (NoFrustumCulling): UE local positions
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, st.positions[s].clone());
            for (k, e) in layout.iter().enumerate() {
                let vals: Vec<[f32; 4]> = (0..n).map(|v| if e.stream == 0 { let p = st.positions[s][v]; [p[0], p[1], p[2], 1.0] } else { crate::d3d_element(&data[v * stride + e.offset..], e.ty) }).collect();
                mesh.insert_attribute(crate::gameshader::ATTR_RAW[k].clone(), VertexAttributeValues::Float32x4(vals));
            }
            let first = el.first_index as usize;
            let cnt = el.num_primitives as usize * 3;
            let idx: Vec<u32> = st.indices.get(first..first + cnt)?.iter().map(|&i| i as u32).collect();
            mesh.insert_indices(Indices::U32(idx));
            out.push((g, a.meshes.add(mesh), st.materials[g]));
        }
        Some(out)
    }

    /// New per-object constant block; returns its MeshTag.
    pub fn push_object(&mut self, l2w: [[f32; 3]; 4], terrain_height: [f32; 4]) -> u32 {
        let tag = self.objects.len() as u32;
        self.objects.push(terrain_height);
        // the game shaders read this block as the TerrainHeight constant: z stays 0 as the game
        // has it (weather.rs puts its sky / fog dome flag into z for the GameMat path only; the
        // flag in the game-shader block turned the sky and the fog dome white, test 2026-10-21)
        let gs_th = [terrain_height[0], terrain_height[1], 0.0, terrain_height[3]];
        self.gs_blocks.push(crate::gameshader::object_block(l2w, gs_th, self.upper_sky, self.lower_sky));
        tag
    }

    fn black(&mut self, a: &mut Assets3) -> Handle<Image> {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        self.black
            .get_or_insert_with(|| {
                a.images.add(Image::new_fill(Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Rgba8Unorm, bevy::asset::RenderAssetUsages::default()))
            })
            .clone()
    }

    fn white(&mut self, a: &mut Assets3, cube: bool) -> Handle<Image> {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
        let slot = if cube { &mut self.white_cube } else { &mut self.white };
        slot.get_or_insert_with(|| {
            let layers = if cube { 6 } else { 1 };
            let mut img = Image::new_fill(
                Extent3d { width: 1, height: 1, depth_or_array_layers: layers },
                TextureDimension::D2,
                &[255, 255, 255, 255],
                TextureFormat::Rgba8Unorm,
                bevy::asset::RenderAssetUsages::default(),
            );
            if cube {
                img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
            }
            a.images.add(img)
        })
        .clone()
    }

    /// The game's own vertex/pixel shader pair for a material on a vertex factory.
    pub fn game_shader_material(&mut self, a: &mut Assets3, pi: usize, mat_ref: i32, vf: &'static str, two_sided_override: bool) -> Option<Handle<crate::terrainemu::TerrainEmu>> {
        self.game_shader_material_raw(a, pi, mat_ref, vf, two_sided_override, None)
    }

    /// As game_shader_material, with the vertex streams given as raw D3D elements.
    pub fn game_shader_material_raw(&mut self, a: &mut Assets3, pi: usize, mat_ref: i32, vf: &'static str, two_sided_override: bool, raw: Option<&crate::gameshader::RawLayout>) -> Option<Handle<crate::terrainemu::TerrainEmu>> {
        let (mp, mi) = self.resolve(pi, mat_ref)?;
        let key = (mp, mi as i32 + 1, two_sided_override, vf);
        if let Some(h) = self.gs_cache.get(&key) {
            return h.clone();
        }
        let h = self.build_game_shader(a, mp, mi as i32 + 1, vf, two_sided_override, raw);
        self.gs_cache.insert(key, h.clone());
        h
    }

    fn build_game_shader(&mut self, a: &mut Assets3, pi: usize, mat: i32, vf: &'static str, two_sided_override: bool, raw: Option<&crate::gameshader::RawLayout>) -> Option<Handle<crate::terrainemu::TerrainEmu>> {
        let _perf = crate::perf::scope("ld_game_shader");
        use crate::terrainemu::TexSrc;
        let sc = self.sc?;
        let pkg = &self.pkgs[pi];
        // Fluid surfaces render through a runtime MaterialInstance the FluidSurfaceComponent
        // creates; the game then draws with the base Material's shader map and uniform set
        // (trace 2026-10-15, Overpass water: PS = M_water_translucent's own record, uniform
        // indices match the base set), parameter values still come from the instance chain.
        let (id, uniforms) = pkg.material_resource(mat, &|g| sc.is_record(g))?;
        // Static-permutation instances: the game finds the shader map by the instance's
        // FStaticParameterSet (base material id + switch values), not by the resource id in the
        // native data (verified with the Overpass water trace 2026-10-15: PS identical, uniform
        // indices = the instance's full cooked uniform set).
        let by_set = pkg.static_parameter_set(mat).and_then(|(base, sw)| sc.record_for_static_set(&base, &sw));
        let exact = by_set.is_some();
        let rec = match by_set {
            Some(r) => r,
            None => sc.record(&id)?,
        };
        let pairs = [
            ("TBasePassVertexShaderFDirectionalLightLightMapPolicyFNoDensityPolicy", "TBasePassPixelShaderFDirectionalLightLightMapPolicySkyLight"),
            ("TBasePassVertexShaderFNoLightMapPolicyFNoDensityPolicy", "TBasePassPixelShaderFNoLightMapPolicySkyLight"),
            ("TBasePassVertexShaderFNoLightMapPolicyFNoDensityPolicy", "TBasePassPixelShaderFNoLightMapPolicyNoSkyLight"),
        ];
        let (vs, ps) = pairs.iter().find_map(|(v, p)| Some((sc.code(&rec.shader(vf, v)?)?, sc.code(&rec.shader(vf, p)?)?)))?;
        let params = pkg.material_params(mat);
        let info = pkg.material_info(mat);
        // Static-permutation instances: the cooked uniform set follows the parent's default
        // switches; keep only expressions reachable under the instance's switches.
        let static_perm = !exact && {
            let mut cur = mat;
            let mut found = false;
            for _ in 0..8 {
                if cur <= 0 || pkg.export_class(&pkg.exports[cur as usize - 1]) == "Material" {
                    break;
                }
                let Some(p) = pkg.export_own_props(cur as usize - 1) else { break };
                if p.bool("bHasStaticPermutationResource") == Some(true) {
                    found = true;
                    break;
                }
                cur = p.object("Parent").unwrap_or(0);
            }
            found
        };
        let dump = std::env::var("HOMEBOTS_GS_UNIFORMS").ok().is_some_and(|n| pkg.object_path(mat).contains(&n));
        if dump {
            eprintln!("uniforms {} (static_perm {static_perm}) blend {:?} two_sided {}", pkg.object_path(mat), info.blend, info.two_sided);
            if let Some(d) = std::env::var_os("HOMEBOTS_GS_DUMPCODE") {
                let _ = std::fs::write(std::path::Path::new(&d).join("vs.bin"), vs);
                let _ = std::fs::write(std::path::Path::new(&d).join("ps.bin"), ps);
            }
            for (st, c) in [("vs", vs), ("ps", ps)] {
                if let Some(s) = upk::sm3::Shader::parse(c) {
                    let names: Vec<String> = s.ctab.iter().filter(|_| true).map(|((set, r), n)| format!("{set}:{r}={n}")).collect();
                    eprintln!("  {st} ctab {}", names.join(" "));
                }
            }
            for (k, l) in [("V", &uniforms.vectors), ("S", &uniforms.scalars), ("T", &uniforms.tex2d), ("C", &uniforms.cubes)] {
                for (i, e) in l.iter().enumerate() {
                    let mut n = Vec::new();
                    e.param_names(&mut n);
                    let w = e.to_wgsl(&params, "t");
                    eprintln!("  {k}{i} {n:?} {}", &w[..w.len().min(160)]);
                }
            }
        }
        let uniforms = if static_perm {
            // the switch values the cached shaders were compiled with
            let sw: Vec<([u8; 16], bool)> = rec.switches.iter().map(|s| (s.3, s.1)).collect();
            let c = pkg.compile_material_with(mat, &sw)?;
            if dump {
                eprintln!("lighting {}", c.lighting);
                for e in &uniforms.tex2d {
                    if let Some((n, o)) = e.texture() {
                        eprintln!("  tex2d {n:?} default {} -> {}", pkg.object_path(o), n.and_then(|n| params.texture(n)).map(|o| pkg.object_path(o)).unwrap_or_default());
                    }
                }
                for (n, o) in params.textures.iter() {
                    eprintln!("  tex param {n} = {}", pkg.object_path(*o));
                }
                eprintln!("record switches {:?}", rec.switches.iter().map(|s| (&s.0, s.1, s.2)).collect::<Vec<_>>());
                for (n, r) in &c.reachable_by_input {
                    eprintln!("  input {n}: {r:?}");
                }
            }
            uniforms.filtered(&c.reachable_params)
        } else {
            uniforms
        };
        use upk::level::Blend;
        let translucent = matches!(info.blend, Blend::Translucent | Blend::Additive | Blend::Modulate);
        // sampler address modes of the texture's own export (an import's are in the package that
        // exports it: KFX_LensFX Bloom01 / Bloom02 TA_Clamp were sampled wrapped here -> the
        // crosshair materials of all sights repeated along the axes, the grey cross)
        let addr = |o: i32| self.tex_addr(pi, o);
        let code = crate::gameshader::generate(vs, ps, &uniforms, &params, &addr, info.blend, vf, raw, &self.shadow_wgsl)?;
        // any uniform index outside the set means the uniform layout is not reproduced:
        // fall back to the graph compiler for this material
        if code.problems.iter().any(|p| p.contains("missing")) {
            *self.gs_problems.entry(format!("uniform layout mismatch -> graph compiler [{}]", pkg.object_path(mat))).or_default() += 1;
            return None;
        }
        let verbose = std::env::var_os("HOMEBOTS_GS_VERBOSE").is_some();
        for p in &code.problems {
            let k = if verbose { format!("{} [{}]", p, pkg.object_path(mat)) } else { p.clone() };
            *self.gs_problems.entry(k).or_default() += 1;
        }
        if let Err(e) = gamemat::validate_wgsl(&code.wgsl) {
            warn!("material {}: game shader invalid:\n{e}", pkg.object_path(mat));
            return None;
        }
        let name = pkg.object_name(mat);
        let sh = self
            .shader_cache
            .entry(code.wgsl.clone())
            .or_insert_with(|| {
                if let Some(dir) = std::env::var_os("HOMEBOTS_DUMP_WGSL") {
                    let _ = std::fs::write(std::path::Path::new(&dir).join(format!("gs_{name}.wgsl")), &code.wgsl);
                }
                a.shaders.add(Shader::from_wgsl(code.wgsl.clone(), format!("hf_gs_{name}.wgsl")))
            })
            .clone();
        if std::env::var("HOMEBOTS_GS_UNIFORMS").ok().is_some_and(|n| pkg.object_path(mat).contains(&n)) {
            for (k, src) in code.textures.iter().enumerate() {
                let what = match *src {
                    TexSrc::Object(o) => self.resolve(pi, o).map(|(tp, ti)| self.pkgs[tp].object_path(ti as i32 + 1)).unwrap_or_default(),
                    other => format!("{other:?}"),
                };
                eprintln!("  bound t{k} = {what}");
            }
        }
        // shadow depth pass variant (opaque / masked only cast shadows)
        let prepass = (!translucent).then(|| {
            self.shader_cache
                .entry(code.prepass_wgsl.clone())
                .or_insert_with(|| a.shaders.add(Shader::from_wgsl(code.prepass_wgsl.clone(), format!("hf_gs_{name}_depth.wgsl"))))
                .clone()
        });
        let mut m = crate::terrainemu::TerrainEmu::new(self.gs_globals.clone(), self.gs_objects.clone(), sh);
        m.prepass_shader = prepass;
        m.mesh_layout = match raw {
            Some(r) => 2 + r.0.len() as u8,
            None => 1,
        };
        m.alpha = match info.blend {
            Blend::Opaque | Blend::Masked => AlphaMode::Opaque,
            Blend::Translucent => AlphaMode::Blend,
            Blend::Additive => AlphaMode::Add,
            Blend::Modulate => AlphaMode::Multiply,
        };
        m.cull = if two_sided_override || self.no_cull || info.two_sided { None } else { Some(Face::Back) };
        for (k, src) in code.textures.iter().enumerate() {
            let h = match *src {
                TexSrc::White => Some(self.white(a, false)),
                TexSrc::Black => Some(self.black(a)),
                TexSrc::Scene => self.scene.clone(),
                TexSrc::Grass => self.grass.clone().or_else(|| Some(self.white(a, false))),
                TexSrc::Object(o) => {
                    let ad = self.tex_addr(pi, o);
                    self.texture(a, pi, o, ad).or_else(|| Some(self.white(a, false)))
                }
            };
            if let Some(h) = h {
                m.set_tex(k, h);
            }
        }
        for (k, src) in code.cubes.iter().enumerate() {
            let h = match *src {
                TexSrc::Object(o) => self.cube_texture(a, pi, o),
                _ => None,
            }
            .unwrap_or_else(|| self.white(a, true));
            if k == 0 {
                m.c0 = Some(h);
            } else {
                m.c1 = Some(h);
            }
        }
        for (k, addr) in code.samplers.iter().enumerate() {
            let h = self.sampler_carrier(a, *addr);
            m.set_smp(k, h);
        }
        Some(a.terrain_mats.add(m))
    }

    fn sampler_carrier(&mut self, a: &mut Assets3, addr: [Address; 2]) -> Handle<Image> {
        self.smp_cache
            .entry(addr)
            .or_insert_with(|| {
                let mut img = Image::new_fill(
                    bevy::render::render_resource::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    bevy::render::render_resource::TextureDimension::D2,
                    &[255, 255, 255, 255],
                    bevy::render::render_resource::TextureFormat::Rgba8Unorm,
                    bevy::asset::RenderAssetUsages::default(),
                );
                if let Some(smp) = gamemat::load_texture_sampler(addr) {
                    img.sampler = smp;
                }
                a.images.add(img)
            })
            .clone()
    }

    /// Decoded Texture2D for an object reference: loaded maps first, then the library
    /// (standalone packages such as ENV_Reflections when installed).
    fn decode_ref(&self, pi: usize, obj: i32) -> Option<(u32, u32, Vec<u8>, bool)> {
        if let Some((tp, ti)) = self.resolve(pi, obj) {
            return gamemat::decode_texture(&self.pkgs[tp], self.lib, ti, self.max_tex);
        }
        let path = self.pkgs[pi].object_path(obj);
        let pkg_name = path.split('.').next()?;
        let other = self.lib.open(pkg_name)?;
        let idx = (0..other.exports.len()).find(|&i| other.object_path(i as i32 + 1).eq_ignore_ascii_case(&path))?;
        gamemat::decode_texture(&other, self.lib, idx, self.max_tex)
    }

    fn cube_texture(&mut self, a: &mut Assets3, pi: usize, obj: i32) -> Option<Handle<Image>> {
        let (tp, ti) = self.resolve(pi, obj)?;
        let pkg = &self.pkgs[tp];
        if pkg.export_class(&pkg.exports[ti]) != "TextureCube" {
            return None;
        }
        let p = pkg.export_props(ti)?;
        let mut faces = Vec::new();
        for f in ["FacePosX", "FaceNegX", "FacePosY", "FaceNegY", "FacePosZ", "FaceNegZ"] {
            let o = p.object(f).filter(|&o| o != 0)?;
            faces.push(self.decode_ref(tp, o)?);
        }
        let img = gamemat::cube_image(faces)?;
        Some(a.images.add(img))
    }

}

