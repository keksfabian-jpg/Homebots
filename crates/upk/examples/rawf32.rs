//! Dev: raw bytes of an export's tagged property as f32s (atomic structs such as Vector2D).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let pr = p.export_own_props(i).unwrap();
    for x in &pr.0 {
        if !a[3..].iter().any(|n| *n == x.name) { continue; }
        if let upk::props::Value::Raw { data, .. } = &x.value {
            let f: Vec<f32> = data.chunks(4).filter(|c| c.len() == 4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            println!("{} {:?}", x.name, f);
        } else { println!("{} {:?}", x.name, x.value); }
    }
}
