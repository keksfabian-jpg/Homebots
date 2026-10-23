//! Dev: hex dump (dwords) of an export range.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && (a.len() < 6 || p.export_class(e) == a[5])).unwrap();
    let b = p.export_bytes(&p.exports[i]);
    let (s, n): (usize, usize) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    for o in (s..(s + n).min(b.len())).step_by(32) {
        let w: Vec<String> = b[o..(o + 32).min(b.len())].chunks(4).map(|c| format!("{:>10}", u32::from_le_bytes([c[0], c[1], *c.get(2).unwrap_or(&0), *c.get(3).unwrap_or(&0)]))).collect();
        println!("{o:6}: {}", w.join(" "));
    }
    println!("export size {} serial_offset {}", b.len(), p.exports[i].serial_offset);
}
