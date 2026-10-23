//! Dev: an ArrayProperty of names / enums (count + FName per element) of an export.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let pr = p.export_own_props(i).unwrap();
    for x in &pr.0 {
        if x.name != a[3] { continue; }
        if let upk::props::Value::Raw { data, .. } = &x.value {
            let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
            for k in 0..n {
                let o = 4 + k * 8;
                let idx = i32::from_le_bytes(data[o..o + 4].try_into().unwrap());
                let num = i32::from_le_bytes(data[o + 4..o + 8].try_into().unwrap());
                println!("{}", p.name(upk::FName { index: idx, number: num }));
            }
        }
    }
}
