//! Dev: read a SkeletalMesh export by index.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse().unwrap();
    match p.read_skeletal_mesh(i - 1) {
        Some(m) => println!("ok sections {} verts {} mats {:?}", m.lod.sections.len(), m.lod.positions.len(), m.materials),
        None => println!("read failed"),
    }
}
