//! Dev: skeletal mesh vertices (UE mesh space) with influences as text.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(i).unwrap();
    for (v, (b, w)) in m.lod.positions.iter().zip(&m.influences) { println!("{} {} {} {} {} {} {} {} {} {} {}", v[0], v[1], v[2], b[0], b[1], b[2], b[3], w[0], w[1], w[2], w[3]); }
}
