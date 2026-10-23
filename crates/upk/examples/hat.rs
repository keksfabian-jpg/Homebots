//! Dev: nearest terrain vertex heights around an x,y (UE units).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let (x, y): (f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let mut v = Vec::new();
    for t in p.terrains() { for g in &t.grids { for q in &g.positions { let d = (q[0] - x).hypot(q[1] - y); if d < 300.0 { v.push((d, q[2])); } } } }
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (d, z) in v.iter().take(8) { println!("d {d:.0} z {z:.1}"); }
}
