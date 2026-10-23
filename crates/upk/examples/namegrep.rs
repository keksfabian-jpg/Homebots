//! Dev: names containing a substring (case-insensitive) in packages.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let q = a[1].to_lowercase();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for n in &p.names { if n.name.to_lowercase().contains(&q) { println!("{f}: {}", n.name); } }
    }
}
