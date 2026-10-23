//! Dev: hex/ints of an export's native data after its tagged properties.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let mut r = p.export_reader(i);
    p.read_props(&mut r).unwrap();
    let b = p.export_bytes(&p.exports[i]);
    let t = &b[r.pos..];
    println!("tail {} bytes", t.len());
    for (k, c) in t.chunks(4).enumerate().take(64) {
        if c.len() == 4 {
            let v = i32::from_le_bytes(c.try_into().unwrap());
            let name = if v != 0 && ((v > 0 && (v as usize) <= p.exports.len()) || (v < 0 && ((-v) as usize) <= p.imports.len())) { p.object_path(v) } else { String::new() };
            println!("{:4} {:11} {:12.5} {}", k * 4, v, f32::from_le_bytes(c.try_into().unwrap()), name);
        }
    }
}
