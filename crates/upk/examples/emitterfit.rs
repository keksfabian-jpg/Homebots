//! Dev: placed Emitter actors (template substring filter) with their rotation and the terrain
//! height under them (nearest grid sample). Args: <terrain map> <filter> <maps...>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = upk::Package::open(&a[1]).unwrap();
    let ter = &t.terrains()[0];
    let pts: Vec<[f32; 3]> = ter.grids.iter().flat_map(|g| g.positions.clone()).collect();
    let filt = a[2].to_lowercase();
    for m in &a[3..] {
        let p = upk::Package::open(m).unwrap();
        for pl in p.emitter_placements() {
            let name = if pl.template > 0 { p.object_path(pl.template) } else { "-".into() };
            if !name.to_lowercase().contains(&filt) {
                continue;
            }
            let (x, y, z) = (pl.location[0], pl.location[1], pl.location[2]);
            let best = pts.iter().min_by(|a, b| ((a[0] - x).powi(2) + (a[1] - y).powi(2)).total_cmp(&((b[0] - x).powi(2) + (b[1] - y).powi(2)))).unwrap();
            let d = ((best[0] - x).powi(2) + (best[1] - y).powi(2)).sqrt();
            let near: Vec<f32> = pts.iter().filter(|q| ((q[0] - x).powi(2) + (q[1] - y).powi(2)).sqrt() < 600.0).map(|q| q[2]).collect();
            let (lo, hi) = near.iter().fold((f32::MAX, f32::MIN), |(a, b), &z| (a.min(z), b.max(z)));
            println!("  terrain within 6 m: {} samples, z {lo:.0}..{hi:.0}", near.len());
            println!("{name} loc {:?} rot {:?} scale {} {:?} terrain z {:.0} (sample {d:.0} away) dz {:.0}", pl.location, pl.rotation, pl.scale, pl.scale3d, best[2], z - best[2]);
        }
    }
}
