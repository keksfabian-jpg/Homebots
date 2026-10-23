//! Dev: terrain samples below a water height (river locations).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let wl: f32 = a[2].parse().unwrap();
    for t in p.terrains() {
        for g in &t.grids {
            for (k, q) in g.positions.iter().enumerate() {
                if q[2] < wl - 50.0 && q[0].abs() < 16000.0 && q[1].abs() < 16000.0 && k % 7 == 0 {
                    println!("{:.0} {:.0} {:.0}", q[0], q[1], q[2]);
                }
            }
        }
    }
}
