//! Dev: exports (any class) whose serialized data contains a 16-byte id (hex), over several packages.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let id: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&a[1][2 * i..2 * i + 2], 16).unwrap()).collect();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            let r = p.export_reader(i);
            let b = &r.b[..];
            if b.windows(16).any(|w| w == &id[..]) {
                println!("{f}: #{} {} [{}]", i + 1, p.object_path(i as i32 + 1), p.export_class(e));
            }
        }
    }
}
