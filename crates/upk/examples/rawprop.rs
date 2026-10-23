//! Dev: raw bytes of a struct property (export by index) as f32 / i32.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let pr = p.export_props(i).unwrap();
    for x in pr.0.iter().filter(|x| x.name == a[3]) {
        if let upk::props::Value::Raw { data, .. } = &x.value {
            println!("{} bytes", data.len());
            for c in data.chunks(4) { if c.len() == 4 { let f = f32::from_le_bytes(c.try_into().unwrap()); let n = i32::from_le_bytes(c.try_into().unwrap()); print!("{} ", if n.abs() < 100000 { format!("i{n}") } else { format!("{f:.3}") }); } }
            println!();
        }
    }
}
