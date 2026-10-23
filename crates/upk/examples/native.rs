//! Dev: dump dwords after the property list of export #N (1-based).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let idx: usize = a[2].parse::<usize>().unwrap() - 1;
    let n: usize = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(48);
    let mut r = p.export_reader(idx);
    p.read_props(&mut r).unwrap();
    let b = r.b;
    println!("native starts @{} of {}", r.pos, b.len());
    let mut o = r.pos;
    for _ in 0..n {
        if o + 4 > b.len() { break; }
        let v = i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let f = f32::from_bits(v as u32);
        let fs = if f.is_finite() && f.abs() > 1e-3 && f.abs() < 1e7 { format!("{f:.3}") } else { String::new() };
        let w = [(v as u32 & 0xffff), (v as u32 >> 16)];
        println!("{o:6}: {v:>12} {:#010x} u16 {:?} {fs}", v as u32, w);
        o += 4;
    }
}
