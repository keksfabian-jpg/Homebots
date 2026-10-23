//! Dev: brush volume 3D bounds (min/max x, y, z) of a class.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for v in p.brush_volumes(&a[2]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for q in v.polys.iter().flatten() {
            for k in 0..3 { lo[k] = lo[k].min(q[k]); hi[k] = hi[k].max(q[k]); }
        }
        println!("{} loc {:?} min {:?} max {:?}", v.name, v.location, lo, hi);
    }
}
