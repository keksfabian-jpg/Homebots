//! Dev: dump cooked grass instances of all TerrainComponents: layer x y z slope(u16 hex).
fn half(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    if e == 0 { return s * m / 1024.0 * 2f32.powi(-14); }
    s * (1.0 + m / 1024.0) * 2f32.powi(e - 15)
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "TerrainComponent" { continue; }
        let mut r = p.export_reader(i);
        p.read_props(&mut r).unwrap();
        let b = r.b;
        let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let mut o = r.pos + 8;
        let n = u(o) as usize; o += 4 + 2 * n;
        let k = u(o) as usize; o += 4 + 4 * k;
        let k = u(o) as usize; o += 4 + 2 * k;
        let k = u(o) as usize; o += 4 + 2 * k;
        if o + 4 > b.len() { continue; }
        let layers = u(o) as usize; o += 4;
        for l in 0..layers {
            let n = u(o) as usize; o += 4;
            for j in 0..n {
                let q = o + 8 * j;
                let h = |k: usize| u16::from_le_bytes(b[q + k..q + k + 2].try_into().unwrap());
                println!("{} {} {:.1} {:.1} {:.1} {:04x}", i + 1, l, half(h(0)), half(h(2)), half(h(4)), h(6));
            }
            o += 8 * n;
        }
    }
}
