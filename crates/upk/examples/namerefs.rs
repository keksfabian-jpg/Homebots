//! Dev: exports whose serialized data contains a reference to a given name (index, number 0).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let Some(ni) = p.names.iter().position(|n| n.name == a[2]) else { println!("no name"); return };
    let pat: Vec<u8> = (ni as i32).to_le_bytes().iter().chain(0i32.to_le_bytes().iter()).copied().collect();
    for i in 0..p.exports.len() {
        let r = p.export_reader(i);
        let b = &r.b[r.pos..(r.pos + p.exports[i].serial_size as usize).min(r.b.len())];
        let hits: Vec<usize> = b.windows(8).enumerate().filter(|(_, w)| *w == &pat[..]).map(|(k, _)| k).collect();
        if !hits.is_empty() { println!("#{} {} [{}] at {:?}", i + 1, p.object_path(i as i32 + 1), p.export_class(&p.exports[i]), hits); }
    }
}
