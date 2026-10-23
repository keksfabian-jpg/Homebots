//! Dev: sockets of a skeletal mesh.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e).eq_ignore_ascii_case(&a[2]) && p.export_class(e) == "SkeletalMesh").unwrap();
    for s in p.mesh_sockets(i) { println!("{:28} bone {:16} loc {:?} rot {:?}", s.name, s.bone, s.loc, s.rot); }
}
