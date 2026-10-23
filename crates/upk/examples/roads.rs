//! Dev: road spline meshes and their mesh bounds.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for r in p.road_spline_meshes() {
        let b = if r.mesh > 0 { p.read_static_mesh(r.mesh as usize - 1).map(|m| {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &m.lods[0].positions { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
            format!("origin {:?} extent {:?} vtx lo {lo:?} hi {hi:?}", m.origin, m.extent)
        }) } else { None };
        println!("{} {:?}\n   {b:?}", p.object_path(r.mesh), r);
    }
}
