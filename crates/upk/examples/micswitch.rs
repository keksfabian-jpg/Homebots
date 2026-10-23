//! Dev: static switch parameter runs (count + FName,Value,bOverride,GUID) in a material's native data.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let idx: usize = a[2].parse::<usize>().unwrap() - 1;
    let mut r = p.export_reader(idx);
    p.read_props(&mut r).unwrap();
    let b = r.b;
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let mut o = r.pos;
    while o + 40 < b.len() {
        let n = u(o) as usize;
        if (1..64).contains(&n) && o + 4 + n * 32 <= b.len() {
            let ok = (0..n).all(|k| { let q = o + 4 + k * 32; (u(q) as usize) < p.names.len() && u(q + 4) == 0 && u(q + 8) <= 1 && u(q + 12) <= 1 });
            if ok {
                let base: String = b[o - 16..o].iter().map(|x| format!("{x:02x}")).collect();
                println!("@{o} base {base} n={n}");
                for k in 0..n { let q = o + 4 + k * 32; println!("   {} v={} ov={}", p.names[u(q) as usize].name, u(q + 8), u(q + 12)); }
                o += 4 + n * 32;
                continue;
            }
        }
        o += 4;
    }
}
