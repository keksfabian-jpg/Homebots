//! Dev: why skeletal placements are (not) produced: per PersistentLevel actor with a skeletal component.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let pl = p.skeletal_mesh_placements();
    println!("{} placements", pl.len());
    for x in &pl { println!("  mesh {} at {:?} rot {:?}", p.object_path(x.mesh), x.location, x.rotation); }
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != a[2] || !p.object_path(i as i32 + 1).starts_with("TheWorld.PersistentLevel.") { continue; }
        let ap = p.export_props(i).unwrap();
        let comp = ap.object("SkeletalMeshComponent");
        let cp = comp.filter(|&c| c > 0).and_then(|c| p.export_props(c as usize - 1));
        println!("{} hidden {:?} comp {:?} mesh {:?} hiddengame {:?}", p.object_path(i as i32 + 1), ap.get("bHidden"), comp, cp.as_ref().and_then(|c| c.object("SkeletalMesh")).map(|m| p.object_path(m)), cp.as_ref().and_then(|c| c.get("HiddenGame").cloned()));
    }
}
