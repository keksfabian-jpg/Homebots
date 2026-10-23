//! Dev: placements near an UE point (x, y, radius) with collide flags and z.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (x, y, r): (f32, f32, f32) = (a[1].parse().unwrap(), a[2].parse().unwrap(), a[3].parse().unwrap());
    for f in &a[4..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for pl in p.static_mesh_placements() {
            let d = ((pl.location[0] - x).powi(2) + (pl.location[1] - y).powi(2)).sqrt();
            if d < r { println!("{} {} collide {} loc {:?} scale {:?}", f.rsplit('/').next().unwrap(), p.object_path(pl.mesh), pl.collide, pl.location.map(|v| v.round()), pl.scale); }
        }
    }
}
