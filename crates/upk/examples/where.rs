//! Dev: print locations of placements whose mesh name contains a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for pl in p.static_mesh_placements() {
        let n = p.object_name(pl.mesh);
        if n.to_lowercase().contains(&a[2].to_lowercase()) {
            println!("{n} {:?} rot {:?}", pl.location, pl.rotation);
        }
    }
}
