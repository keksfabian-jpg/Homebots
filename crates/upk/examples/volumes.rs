//! Dev: brush volumes of a class and whether points lie inside.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for v in p.brush_volumes(&a[2]) {
        let pts: Vec<[f32; 3]> = v.polys.iter().flatten().copied().collect();
        let mn = pts.iter().fold([f32::MAX; 3], |m, q| [m[0].min(q[0]), m[1].min(q[1]), m[2].min(q[2])]);
        let mx = pts.iter().fold([f32::MIN; 3], |m, q| [m[0].max(q[0]), m[1].max(q[1]), m[2].max(q[2])]);
        println!("{} loc {:?} polys {} bounds {:?} {:?} contains-loc {}", v.name, v.location, v.polys.len(), mn.map(|x| x.round()), mx.map(|x| x.round()), v.contains(v.location));
    }
}
