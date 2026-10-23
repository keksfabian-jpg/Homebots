//! Dev: which packages export an object path (case-insensitive substring).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let q = a[1].to_lowercase();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for i in 0..p.exports.len() {
            let path = p.object_path(i as i32 + 1);
            if path.to_lowercase().contains(&q) {
                println!("{f}: {} {path} [{}]", i + 1, p.export_class(&p.exports[i]));
            }
        }
    }
}
