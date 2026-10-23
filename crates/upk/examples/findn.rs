//! Dev: find any (stride, n, stride, n) or (x, n) bulk headers for given n values in a StaticMesh.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[2]).unwrap();
    let b = p.export_bytes(&p.exports[i]);
    let d = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let ns: Vec<u32> = a[3..].iter().map(|s| s.parse().unwrap()).collect();
    for o in (0..b.len().saturating_sub(16)).step_by(1) {
        let v = d(o);
        if ns.contains(&v) {
            let ctx: Vec<u32> = (0..8).filter(|k| o >= 12 && o - 12 + k * 4 + 4 <= b.len()).map(|k| d(o - 12 + k * 4)).collect();
            println!("@{o} (mod4={}) n={v} ctx[-12..+16]: {ctx:?}", o % 4);
        }
    }
    println!("size {}", b.len());
}
