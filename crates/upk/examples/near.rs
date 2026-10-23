//! Dev: static mesh placements near x,y (UE) with z range, mesh and override materials.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let (x, y, r): (f32, f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
    for pl in p.static_mesh_placements() {
        let d = (pl.location[0] - x).hypot(pl.location[1] - y);
        if d > r { continue; }
        let mats: Vec<String> = pl.materials.iter().map(|&m| if m != 0 { p.object_path(m) } else { "-".into() }).collect();
        println!("{:.0} {} {:?} scale {:?} mats {:?}", d, p.object_path(pl.mesh), pl.location, pl.scale, mats);
    }
}
