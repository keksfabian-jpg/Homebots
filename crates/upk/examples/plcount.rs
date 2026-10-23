//! Dev: count static mesh placements per map.
fn main() {
    for m in std::env::args().skip(1) {
        let p = upk::Package::open(&m).unwrap();
        println!("{} {}", m.rsplit('/').next().unwrap(), p.static_mesh_placements().len());
    }
}
