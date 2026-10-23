//! Dev: static mesh placements per package, with mesh path histogram filter.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pat = a[1].to_lowercase();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        let pl = p.static_mesh_placements();
        let mut m = std::collections::BTreeMap::new();
        for x in &pl { let n = p.object_path(x.mesh); if n.to_lowercase().contains(&pat) { *m.entry(n).or_insert(0) += 1; } }
        println!("{} {} placements", f.rsplit('/').next().unwrap(), pl.len());
        for (k, v) in m { println!("  {v:4} {k}"); }
    }
}
