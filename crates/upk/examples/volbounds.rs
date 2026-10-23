//! Dev: brush volume bounds of a class.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for v in p.brush_volumes(&a[2]) {
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for q in v.polys.iter().flatten() { b[0] = b[0].min(q[0]); b[1] = b[1].min(q[1]); b[2] = b[2].max(q[0]); b[3] = b[3].max(q[1]); }
        println!("{} loc {:?} polys {} bounds {:?}", v.name, v.location, v.polys.len(), b);
    }
}
