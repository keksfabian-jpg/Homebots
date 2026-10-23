//! Dev: RoadSpline actors of a package (path, whether placed in the level or a template such as
//! a Prefab archetype), their spline mesh components' mesh and material overrides.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "RoadSpline" {
            continue;
        }
        let path = p.object_path(i as i32 + 1);
        let Some(ap) = p.export_props(i) else { continue };
        let comps = ap.get("SplineMeshComps").map(|v| p.array_objects(v)).unwrap_or_default();
        let flags = e.object_flags;
        println!("#{} {path} flags {flags:#x} loc {:?} HiddenGame {:?} bCollideActors {:?} bBlockActors {:?}", i + 1, ap.vector("Location"), ap.bool("HiddenGame"), ap.bool("bCollideActors"), ap.bool("bBlockActors"));
        for c in comps.into_iter().filter(|&c| c > 0) {
            let Some(cp) = p.export_props(c as usize - 1) else { continue };
            let mesh = cp.object("StaticMesh").unwrap_or(0);
            let mats: Vec<String> = p.component_materials(&cp).iter().map(|&m| if m != 0 { p.object_path(m) } else { "-".into() }).collect();
            println!("   {} ({}) HiddenGame {:?} mesh {} mats {mats:?}", p.object_path(c), p.export_class(&p.exports[c as usize - 1]), cp.bool("HiddenGame"), if mesh != 0 { p.object_path(mesh) } else { "-".into() });
            // collision flags set on the component (unset = class defaults)
            for k in ["CollideActors", "BlockActors", "BlockZeroExtent", "BlockNonZeroExtent", "BlockRigidBody", "bDisableAllRigidBody"] {
                if let Some(v) = cp.bool(k) {
                    println!("      {k} = {v}");
                }
            }
        }
    }
}
