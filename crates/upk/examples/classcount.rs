//! Dev: export class histogram, optionally filtered by substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pat = a[1].to_lowercase();
    let mut m = std::collections::BTreeMap::new();
    for f in &a[2..] { let Ok(p) = upk::Package::open(f) else { continue }; for e in &p.exports { let c = p.export_class(e); if c.to_lowercase().contains(&pat) { *m.entry(format!("{} {}", f.rsplit('/').next().unwrap(), c)).or_insert(0) += 1; } } }
    for (k, v) in m { println!("{v:5} {k}"); }
}
