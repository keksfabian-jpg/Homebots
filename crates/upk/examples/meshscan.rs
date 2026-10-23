//! Dev tool: walk a StaticMesh export after its properties and print every dword,
//! collapsing anything that looks like a bulk array (elem_size, count, data).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p
        .exports
        .iter()
        .position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[2])
        .unwrap();
    let mut r = p.export_reader(i);
    let props = p.read_props(&mut r).unwrap();
    println!("props: {:?}", props.0.iter().map(|p| &p.name).collect::<Vec<_>>());
    let b = r.b;
    let mut o = r.pos;
    let dw = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    while o + 4 <= b.len() {
        let v = dw(o);
        if o + 8 <= b.len() {
            let n = dw(o + 4);
            if [2, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40].contains(&v) && n > 1 {
                let end = o + 8 + (v as usize) * (n as usize);
                if end <= b.len() {
                    println!("{o:6}: ARRAY elem {v} x {n} -> {end}");
                    o = end;
                    continue;
                }
            }
        }
        let f = f32::from_bits(v as u32);
        let ff = if f.is_finite() && f.abs() > 1e-4 && f.abs() < 1e7 { format!("{f:.3}") } else { String::new() };
        println!("{o:6}: {v:>12} {:#010x} {ff}", v as u32);
        o += 4;
    }
}
