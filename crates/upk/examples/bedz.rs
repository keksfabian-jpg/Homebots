//! Dev: histogram of terrain height below a water level (UE units).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let wl: f32 = a[2].parse().unwrap();
    let mut h = std::collections::BTreeMap::new();
    let mut near = Vec::new();
    for t in p.terrains() {
        for g in &t.grids {
            for q in &g.positions {
                if q[2] < wl {
                    *h.entry(((wl - q[2]) / 25.0) as i32).or_insert(0) += 1;
                }
                if (q[0] + 6045.0).abs() < 600.0 && (q[1] - 5400.0).abs() < 600.0 {
                    near.push(q[2]);
                }
            }
        }
    }
    for (k, n) in h { println!("depth {}..{}: {n}", k * 25, k * 25 + 25); }
    near.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("near camera: n={} min {:?} max {:?}", near.len(), near.first(), near.last());
}
