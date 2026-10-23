//! Dev: list every (12,n,12,n) position-stream header in a StaticMesh export.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[2]).unwrap();
    let b = p.export_bytes(&p.exports[i]);
    let d = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let mut o = 0;
    while o + 16 <= b.len() {
        if d(o) == 12 && d(o + 8) == 12 && d(o + 4) == d(o + 12) && d(o + 4) > 2 && d(o + 4) < 100000 {
            let n = d(o + 4) as usize;
            let before: Vec<u32> = (1..=8).filter(|k| o >= k * 4).map(|k| d(o - k * 4)).collect();
            println!("pos @{o} n={n} preceding dwords (nearest first): {before:?}");
            // look back for a UV header with count n
            for back in (24..20000).step_by(4) {
                if o < back { break; }
                let h = o - back;
                if d(h + 8) as usize == n && d(h + 20) as usize == n && d(h + 4) == d(h + 16) {
                    println!("   uv hdr @{h}: nt={} stride={} full={} -> data ends @{} (pos hdr @{o})", d(h), d(h + 4), d(h + 12), h + 24 + d(h + 4) as usize * n);
                    break;
                }
            }
        }
        o += 4;
    }
}
