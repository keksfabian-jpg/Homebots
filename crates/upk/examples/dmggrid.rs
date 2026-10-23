//! Dev: GCDamageGrid (DamageTypes x PhysicalMaterials, packed Int Data).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let pr = p.export_own_props(i).unwrap();
    let raw = |n: &str| pr.0.iter().find(|x| x.name == n).and_then(|x| if let upk::props::Value::Raw { data, .. } = &x.value { Some(data.clone()) } else { None }).unwrap();
    let dt = raw("DamageTypes");
    let pm = raw("PhysicalMaterials");
    let data = raw("Data");
    println!("dt {} bytes, pm {} bytes, data {} bytes", dt.len(), pm.len(), data.len());
    let n = u32::from_le_bytes(dt[0..4].try_into().unwrap()) as usize;
    let dts: Vec<String> = (0..n).map(|k| { let o = 4 + k * 8; p.name(upk::FName { index: i32::from_le_bytes(dt[o..o + 4].try_into().unwrap()), number: i32::from_le_bytes(dt[o + 4..o + 8].try_into().unwrap()) }) }).collect();
    let m = u32::from_le_bytes(pm[0..4].try_into().unwrap()) as usize;
    let per = (pm.len() - 4) / m.max(1);
    let pms: Vec<String> = (0..m).map(|k| { let o = i32::from_le_bytes(pm[4 + k * per..8 + k * per].try_into().unwrap()); if per == 8 { p.name(upk::FName { index: o, number: i32::from_le_bytes(pm[8 + k * per..12 + k * per].try_into().unwrap()) }) } else { p.object_path(o) } }).collect();
    println!("{} damage types, {} phys mats ({} bytes each)", n, m, per);
    for (k, s) in pms.iter().enumerate() { println!("pm {k} {s}"); }
    // Data: array of structs {Data: Int}
    let cnt = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut r = upk::props::Rd::new(&data[4..]);
    let mut vals = Vec::new();
    for _ in 0..cnt { let sp = p.read_props(&mut r).unwrap(); let v = sp.0.iter().find_map(|x| if let upk::props::Value::Int(v) = x.value { Some(v) } else { None }).unwrap_or(0); vals.push(v); }
    println!("{cnt} cells");
    let filt = a.get(3);
    for (d, name) in dts.iter().enumerate() {
        if filt.is_some_and(|f| !name.contains(f.as_str())) { continue; }
        let row: Vec<String> = (0..m).map(|k| { let v = vals.get(d * m + k).copied().unwrap_or(-1) as u32; format!("{}/{}", v >> 16, v & 0xffff) }).collect();
        println!("{d} {name}: {}", row.join(" "));
    }
}
