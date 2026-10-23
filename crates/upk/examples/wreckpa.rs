//! Dev: bodies and constraints of the PhysicsAssets whose path contains a substring
//! (destruction.rs asset_pieces: which wreck bodies tear off).
//! Usage: wreckpa <package> <substring>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let pat = a[2].to_lowercase();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "PhysicsAsset" || !p.object_path(i as i32 + 1).to_lowercase().contains(&pat) { continue; }
        let pr = p.export_props(i).unwrap();
        println!("== {}", p.object_path(i as i32 + 1));
        for o in pr.get("BodySetup").map(|v| p.array_objects(v)).unwrap_or_default() {
            if o <= 0 { continue; }
            let b = p.export_props(o as usize - 1).unwrap();
            println!("  body {:?}", b.enum_name("BoneName"));
        }
        for o in pr.get("ConstraintSetup").map(|v| p.array_objects(v)).unwrap_or_default() {
            if o <= 0 { continue; }
            let c = p.export_props(o as usize - 1).unwrap();
            println!("  joint {:?} -> {:?} linbrk {:?} angbrk {:?} thr {:?}", c.enum_name("ConstraintBone1"), c.enum_name("ConstraintBone2"), c.bool("bLinearBreakable"), c.bool("bAngularBreakable"), c.float("LinearBreakThreshold"));
        }
    }
}
