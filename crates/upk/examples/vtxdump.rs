//! Dev: raw vertex records of a StaticMesh's first tangent/UV stream as halves.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[2]).unwrap();
    let b = p.export_bytes(&p.exports[i]);
    let o: usize = a[3].parse().unwrap();
    let st = u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let n = u32::from_le_bytes(b[o + 4..o + 8].try_into().unwrap()) as usize;
    for v in 0..n.min(a[4].parse().unwrap()) {
        let r = &b[o + 8 + v * st..o + 8 + (v + 1) * st];
        let hs: Vec<String> = r.chunks(2).map(|c| format!("{:.3}", half::f16::from_bits(u16::from_le_bytes([c[0], c[1]])).to_f32())).collect();
        let hx: Vec<String> = r.chunks(4).map(|c| format!("{:08x}", u32::from_le_bytes(c.try_into().unwrap()))).collect();
        println!("{} | {}", hx.join(" "), hs.join(" "));
    }
}
