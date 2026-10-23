//! Dev: exports whose data contain the first 256 bytes of a file (e.g. a texture mip from a trace).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let needle = std::fs::read(&a[1]).unwrap();
    let n = &needle[..needle.len().min(256)];
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { eprintln!("cannot open {f}"); continue };
        for (i, e) in p.exports.iter().enumerate() {
            let b = p.export_bytes(e);
            if b.windows(n.len()).any(|w| w == n) {
                println!("{f}: #{} {} [{}]", i + 1, p.object_path(i as i32 + 1), p.export_class(e));
            }
        }
    }
}
