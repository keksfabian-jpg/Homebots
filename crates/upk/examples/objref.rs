//! Dev: exports whose serialized data contain an object reference (i32) to the given index.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for t in &a[2..] {
        let o: i32 = t.parse().unwrap();
        let needle = o.to_le_bytes();
        for (i, e) in p.exports.iter().enumerate() {
            let r = p.export_reader(i);
            let b = &r.b[..];
            let hits: Vec<usize> = b.windows(4).enumerate().filter(|(_, w)| *w == needle).map(|(k, _)| k).collect();
            if !hits.is_empty() {
                println!("{o} <- #{} {} [{}] at {:?}", i + 1, p.object_path(i as i32 + 1), p.export_class(e), &hits[..hits.len().min(6)]);
            }
        }
    }
}
