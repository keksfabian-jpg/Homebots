//! Dev: per placed static mesh of a map folder: component collision flags, the StaticMesh's
//! collision properties (UseSimple*Collision, ...), BodySetup and its AggGeom element counts.
//! Usage: meshcolsurvey <map folder> [mesh substring]
//! MASK = upk static_mesh_collision_mask (bit 0 pawns, bit 1 shots).
use upk::props::Value;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let filt = a.get(2).cloned().unwrap_or_default();
    let mut files: Vec<_> = std::fs::read_dir(&a[1]).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "map")).collect();
    files.sort();
    let pkgs: Vec<upk::Package> = files.iter().filter_map(|f| upk::Package::open(f).ok()).collect();
    let mut by_path = std::collections::HashMap::new();
    for (pi, p) in pkgs.iter().enumerate() {
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) == "StaticMesh" {
                by_path.entry(p.object_path(i as i32 + 1).to_ascii_lowercase()).or_insert((pi, i));
            }
        }
    }
    let mut rows: std::collections::BTreeMap<String, (u32, u32, String)> = Default::default();
    for (pi, p) in pkgs.iter().enumerate() {
        for pl in p.static_mesh_placements() {
            if pl.sky || pl.nudgeable {
                continue;
            }
            let path = p.object_path(pl.mesh);
            if !path.to_ascii_lowercase().contains(&filt.to_ascii_lowercase()) {
                continue;
            }
            let key = path.clone();
            let e = rows.entry(key).or_insert((0, 0, String::new()));
            if pl.collide & 1 != 0 { e.0 += 1 } else { e.1 += 1 }
            if !e.2.is_empty() {
                continue;
            }
            let res = if pl.mesh > 0 { Some((pi, pl.mesh as usize - 1)) } else { by_path.get(&path.to_ascii_lowercase()).copied() };
            let Some((mp, mi)) = res else { e.2 = "unresolved".into(); continue };
            let mpk = &pkgs[mp];
            let mut r = mpk.export_reader(mi);
            let Some(props) = mpk.read_props(&mut r) else { e.2 = "noprops".into(); continue };
            let u0 = r.u32().unwrap_or(0);
            r.bytes(28);
            let bs = r.i32().unwrap_or(0);
            // InternalVersion, TArray<FName>, LOD count, LOD 0: 2 dwords, sections (material,
            // EnableCollision, OldEnableCollision, bEnableShadowCasting, ...)
            let mut secs = String::new();
            (|| -> Option<()> {
                r.i32()?;
                let nn = r.i32()?;
                r.bytes(nn.max(0) as usize * 8)?;
                let nl = r.i32()?;
                for li in 0..nl {
                    r.u32()?;
                    r.u32()?;
                    let ns = r.i32()?;
                    secs += &format!("L{li}[");
                    let mut tris = 0u32;
                    for _ in 0..ns {
                        let _m = r.i32()?;
                        let c = r.u32()?;
                        let oc = r.u32()?;
                        r.u32()?;
                        r.u32()?;
                        tris += r.u32()?;
                        r.bytes(12)?;
                        let nf = r.i32()?;
                        r.bytes(nf.max(0) as usize * 8)?;
                        secs += &format!("{c}{oc} ");
                    }
                    secs += &format!("{tris}t] ");
                    if li == 0 {
                        // kDOP triangle array candidates: bulk TArray (element size 8, count = LOD 0 triangles)
                        let b = mpk.export_bytes(&mpk.exports[mi]);
                        let mut hits = Vec::new();
                        let mut o = 0;
                        while o + 8 <= b.len() {
                            let a0 = u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
                            let a1 = u32::from_le_bytes(b[o + 4..o + 8].try_into().unwrap());
                            if a0 == 8 && a1 == tris {
                                hits.push(o);
                            }
                            o += 4;
                        }
                        secs += &format!("kdop8@{:?}/{} ", hits, b.len());
                    }
                    r.u32()?;
                    r.bytes(28)?;
                }
                // per LOD: bulk header (flags, count, size, offset) + inline data
                for _ in 0..nl {
                    let fl = r.u32()?;
                    r.u32()?;
                    let sz = r.u32()? as usize;
                    r.u32()?;
                    if fl & 1 == 0 {
                        r.bytes(sz)?;
                    }
                }
                let mut after = Vec::new();
                let left = r.left();
                for _ in 0..(left / 4).min(48) {
                    after.push(r.u32()?);
                }
                secs += &format!("after(left {left}) {:?} ", after);
                Some(())
            })();
            let mask = mpk.static_mesh_collision_mask(mi);
            // section materials' PhysicalMaterial (MIC -> Parent chain inside the package, as
            // homebots impacts::material_phys)
            let mut pms = Vec::new();
            if let Some(sm) = mpk.read_static_mesh(mi) {
                for sec in &sm.lods[0].sections {
                    let mut m = sec.material;
                    let mut found = String::from("?");
                    for _ in 0..8 {
                        if m <= 0 {
                            break;
                        }
                        let Some(pr) = mpk.export_props(m as usize - 1) else { break };
                        if let Some(pm) = pr.object("PhysMaterial").filter(|&o| o != 0) {
                            found = mpk.object_path(pm);
                            break;
                        }
                        m = pr.object("Parent").unwrap_or(0);
                    }
                    pms.push(found);
                }
            }
            let veg = !pms.is_empty() && pms.iter().all(|p| p.ends_with("PM_Vegetation") || p.ends_with("PM_LeavesDeciduous"));
            let mut s = format!("MASK {mask} VEG {} PM {:?} u0 {u0} sections(collide,old) {secs}| ", veg as u8, pms);
            for pr in &props.0 {
                match &pr.value {
                    Value::Bool(b) => s += &format!("{}={} ", pr.name, *b as u8),
                    Value::Float(f) => s += &format!("{}={} ", pr.name, f),
                    Value::Int(f) => s += &format!("{}={} ", pr.name, f),
                    Value::Enum(f) | Value::Name(f) => s += &format!("{}={} ", pr.name, f),
                    _ => s += &format!("{}(..) ", pr.name),
                }
            }
            s += &format!("| BodySetup {}", if bs == 0 { "none".to_string() } else { mpk.object_path(bs) });
            if bs > 0 {
                if let Some(bp) = mpk.export_props(bs as usize - 1) {
                    let names: Vec<String> = bp.0.iter().map(|p| p.name.clone()).collect();
                    s += &format!(" props {:?}", names);
                    if let Some(ag) = mpk.struct_prop(&bp, "AggGeom") {
                        for k in ["SphereElems", "BoxElems", "SphylElems", "ConvexElems"] {
                            let n = mpk.struct_array(&ag, k).len();
                            if n > 0 {
                                s += &format!(" {k}={n}");
                            }
                        }
                    }
                }
            }
            e.2 = s;
        }
    }
    for (k, v) in rows {
        println!("{:4} block {:4} noblock {k}\n      {}", v.0 + v.1, v.0, v.2);
    }
}
