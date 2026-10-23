//! Dev: list exports (index, class, path, size) whose class or path contains a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let f = a.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
    for (i, e) in p.exports.iter().enumerate() {
        let c = p.export_class(e);
        let path = p.object_path(i as i32 + 1);
        if c.to_lowercase().contains(&f) || path.to_lowercase().contains(&f) {
            println!("#{} {c} {path} {}", i + 1, e.serial_size);
        }
    }
}
