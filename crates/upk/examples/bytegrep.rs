//! Dev: exports whose serialized bytes contain an ASCII string (script string constants etc.).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let pat = a[2].as_bytes();
    for (i, e) in p.exports.iter().enumerate() {
        let b = p.export_bytes(e);
        if let Some(pos) = b.windows(pat.len()).position(|w| w == pat) {
            let s = pos.saturating_sub(48);
            println!("#{} {} [{}] @{pos}: {:?}", i + 1, p.object_path(i as i32 + 1), p.export_class(e), String::from_utf8_lossy(&b[s..(pos + 80).min(b.len())]));
        }
    }
}
