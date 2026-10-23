//! Dev: ASCII strings in an export's native data.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let mut r = p.export_reader(i);
    p.read_props(&mut r).unwrap();
    println!("native starts at {} of {}", r.pos, r.b.len());
    let b = &r.b[r.pos..];
    let mut s = String::new(); let mut st = 0;
    for (k, &c) in b.iter().enumerate() {
        if (32..127).contains(&c) { if s.is_empty() { st = k; } s.push(c as char); } else { if s.len() >= 5 { println!("{st}: {s}"); } s.clear(); }
    }
}
