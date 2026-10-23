//! Dev: static mesh placements within a radius of a UE point (path, scale, rotation).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c: Vec<f32> = a[1].split(',').map(|x| x.parse().unwrap()).collect();
    let rad: f32 = a[2].parse().unwrap();
    for f in &a[3..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for pl in p.static_mesh_placements() {
            let d = ((pl.location[0]-c[0]).powi(2)+(pl.location[1]-c[1]).powi(2)+(pl.location[2]-c[2]).powi(2)).sqrt();
            if d < rad {
                println!("{:6.0} {} [{}] loc {:?} rot {:?} scale {:?} col {}", d, p.object_path(pl.mesh), f.rsplit('/').next().unwrap(), pl.location.map(|x| x.round()), pl.rotation, pl.scale, pl.collide);
            }
        }
    }
}
