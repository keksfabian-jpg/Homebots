//! Dev: PhysicalMaterial impact effects (PhysicalMaterialPropertyEx DamageTypeInstigators).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "PhysicalMaterial" { continue; }
        if a.len() > 2 && !p.object_name_of(e).to_lowercase().contains(&a[2].to_lowercase()) { continue; }
        let Some(pr) = p.export_props(i) else { continue };
        println!("== {} parent {:?}", p.object_path(i as i32 + 1), pr.object("Parent").map(|o| p.object_path(o)));
        let Some(pp) = pr.object("PhysicalMaterialProperty").filter(|&o| o > 0).and_then(|o| p.export_props(o as usize - 1)) else { continue };
        for st in p.struct_array(&pp, "DamageTypeInstigators") {
            let dts: Vec<String> = st.get("DamageTypes").map(|v| p.array_objects(v)).unwrap_or_default().into_iter().map(|o| p.object_name(o)).collect();
            let ce = p.struct_prop(&st, "CollisionEffect");
            let ps: Vec<String> = ce.as_ref().and_then(|c| c.get("ParticleSystems")).map(|v| p.array_objects(v)).unwrap_or_default().into_iter().map(|o| p.object_path(o)).collect();
            let sc: Vec<String> = ce.as_ref().and_then(|c| c.get("SoundCues")).map(|v| p.array_objects(v)).unwrap_or_default().into_iter().map(|o| p.object_path(o)).collect();
            println!("  {:?} -> {:?} {:?}", dts, ps, sc);
        }
    }
}
