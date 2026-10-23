//! Dev: print placements of meshes whose name contains a substring (x y) for all maps given.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for m in &a[2..] {
        let Ok(p) = upk::Package::open(m) else { continue };
        for pl in p.static_mesh_placements() {
            let n = p.object_name(pl.mesh).to_lowercase();
            if n.contains(&a[1]) { println!("{} {} {}", pl.location[0], pl.location[1], n); }
        }
    }
}
