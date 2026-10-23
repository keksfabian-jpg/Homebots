//! Dev: float view of the start of a USpeedTree's native data (after the properties).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let mut r = p.export_reader(i);
    p.read_props(&mut r).unwrap();
    let n: usize = a[3].parse().unwrap();
    let b = &r.b[r.pos..];
    for k in 0..n.min(b.len() / 4) {
        let u = u32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap());
        print!("{k}:{:.6}({}) ", f32::from_bits(u), u);
        if k % 6 == 5 { println!(); }
    }
    println!();
}
