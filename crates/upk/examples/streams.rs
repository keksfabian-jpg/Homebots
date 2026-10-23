//! Dev: list all (s, n, s, n) and (nt, s, n, fp, s, n) stream headers in a StaticMesh export.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[2]).unwrap();
    let b = p.export_bytes(&p.exports[i]);
    let d = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let mut o = 0;
    while o + 24 <= b.len() {
        let (s, n) = (d(o), d(o + 4));
        if s >= 2 && s <= 64 && n >= 3 && n < 200000 && d(o + 8) == s && d(o + 12) == n && o + 16 + (s * n) as usize <= b.len() {
            println!("@{o}: stride {s} n {n} (prev {:?}) -> ends {}", (1..=4).filter(|k| o >= k * 4).map(|k| d(o - k * 4)).collect::<Vec<_>>(), o + 16 + (s * n) as usize);
        } else if s >= 2 && s <= 64 && n >= 3 && n < 200000 && o + 8 + (s * n) as usize <= b.len() && (d(o.saturating_sub(4)) == n || d(o.saturating_sub(8)) == n) {
            println!("@{o}: bulk elem {s} x {n} (prev {:?}) -> ends {}", (1..=4).filter(|k| o >= k * 4).map(|k| d(o - k * 4)).collect::<Vec<_>>(), o + 8 + (s * n) as usize);
        }
        o += 4;
    }
    println!("size {}", b.len());
}
