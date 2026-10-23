//! Dev: RawDistributionFloat lookup tables of an export's struct props.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let pr = p.export_own_props(i).unwrap();
    for x in &pr.0 {
        if let Some(sp) = p.struct_prop(&pr, &x.name) {
            for y in &sp.0 {
                if let upk::props::Value::Raw { data, .. } = &y.value {
                    if y.name == "LookupTable" {
                        let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
                        let v: Vec<f32> = (0..n).map(|k| f32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap())).collect();
                        println!("{} LookupTable {:?}", x.name, v);
                    }
                } else {
                    println!("{} {} = {:?}", x.name, y.name, y.value);
                }
            }
        }
    }
}
