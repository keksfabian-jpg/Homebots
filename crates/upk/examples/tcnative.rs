//! Dev: layout of a TerrainComponent's native data after the height array.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let idx: usize = a[2].parse::<usize>().unwrap() - 1;
    let mut r = p.export_reader(idx);
    let pr = p.read_props(&mut r).unwrap();
    let start = r.pos;
    let b = r.b;
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let mut o = start;
    println!("props: Size {:?} {:?} tess {:?}; native @{o} len {}", pr.int("SectionSizeX"), pr.int("SectionSizeY"), pr.int("MaxTessellationLevel"), b.len());
    println!("a={} b={}", u(o), u(o + 4)); o += 8;
    let n = u(o) as usize; println!("heights n={n}"); o += 4 + 2 * n;
    let k = u(o) as usize; println!("u32 array k={k} first {:?}", (0..k.min(8)).map(|i| u(o + 4 + 4 * i)).collect::<Vec<_>>()); o += 4 + 4 * k;
    let k = u(o) as usize; println!("u16 array k={k} first {:?}", (0..k.min(16)).map(|i| u16::from_le_bytes(b[o + 4 + 2 * i..o + 6 + 2 * i].try_into().unwrap())).collect::<Vec<_>>()); o += 4 + 2 * k;
    let k = u(o) as usize; println!("quad flags k={k}"); o += 4 + 2 * k;
    println!("rest @{o}: {} bytes", b.len() - o);
    for x in (o..b.len()).step_by(4).take(48) { let v = u(x); let f = f32::from_bits(v); println!("  {x}: {v} {:#x} {}", v, if f.is_finite() && f.abs() > 1e-3 && f.abs() < 1e7 { format!("{f:.3}") } else { String::new() }); }
}
