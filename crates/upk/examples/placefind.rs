//! Dev: locations of static mesh placements whose mesh path contains a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for pl in p.static_mesh_placements() {
            if pl.mesh > 0 && p.object_path(pl.mesh).contains(&a[1]) {
                println!("{} {} {:?}", f.rsplit('/').next().unwrap(), p.object_path(pl.mesh), pl.location);
            }
        }
    }
}
