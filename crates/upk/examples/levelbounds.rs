//! Dev: bounds of placed static meshes (location + mesh bounding sphere) across maps.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
    let (mut mn2, mut mx2) = ([f32::MAX; 3], [f32::MIN; 3]);
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for pl in p.static_mesh_placements() {
            if pl.sky { continue; }
            for k in 0..3 { mn[k] = mn[k].min(pl.location[k]); mx[k] = mx[k].max(pl.location[k]); }
            if pl.mesh > 0 {
                if let Some(m) = p.read_static_mesh(pl.mesh as usize - 1) {
                    let s = pl.scale.iter().fold(0f32, |a, b| a.max(b.abs()));
                    for k in 0..3 { mn2[k] = mn2[k].min(pl.location[k] - m.radius * s); mx2[k] = mx2[k].max(pl.location[k] + m.radius * s); }
                }
            }
        }
    }
    let c = |a: [f32; 3], b: [f32; 3]| [0, 1, 2].map(|k| (a[k] + b[k]) / 2.0);
    let r = |a: [f32; 3], b: [f32; 3]| ((0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f32>()).sqrt() / 2.0;
    println!("locations {:?} {:?} center {:?} r {}", mn, mx, c(mn, mx), r(mn, mx));
    println!("with radius {:?} {:?} center {:?} r {}", mn2, mx2, c(mn2, mx2), r(mn2, mx2));
}
