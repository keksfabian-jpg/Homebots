//! Dev: bone names / parents of a SkeletalMesh (object name).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let _ = 0;
    let m = p.read_skeletal_mesh(i).unwrap();
    for (k, b) in m.bones.iter().enumerate() {
        println!("{k} {} parent {} pos {:?} rot {:?}", b.name, b.parent, b.pos, b.rot);
    }
}
