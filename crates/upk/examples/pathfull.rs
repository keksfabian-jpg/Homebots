//! Dev: print all tagged properties (objfull style) of every export whose path contains a substring.
//! Arrays of objects/structs are shown raw; use for Kismet prefab sequences (GMS_ExplodingCars).
use upk::props::{Rd, Value};
fn show(p: &upk::Package, v: &Value) -> String {
    match v {
        Value::Object(o) => format!("#{o} {}", p.object_path(*o)),
        Value::Raw { struct_name: Some(sn), data, .. } => format!("{sn}{{{}}}", p.read_props(&mut Rd::new(data)).map(|x| x.0.iter().map(|y| format!("{}={}", y.name, show(p, &y.value))).collect::<Vec<_>>().join(", ")).unwrap_or_else(|| format!("{} bytes", data.len()))),
        Value::Raw { type_name, data, .. } if type_name == "ArrayProperty" && data.len() >= 4 => {
            let n = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            // object arrays: n * 4 bytes
            let ok = |o: i32| (o == 0) || (o > 0 && (o as usize) <= p.exports.len()) || (o < 0 && ((-o) as usize) <= p.imports.len());
            if n >= 0 && data.len() == 4 + n as usize * 4 && (0..n as usize).all(|k| ok(i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap()))) {
                let v: Vec<String> = (0..n as usize).map(|k| { let o = i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap()); format!("#{o} {}", p.object_path(o)) }).collect();
                format!("[{}]", v.join(", "))
            } else { format!("Array(n={n}, {} bytes)", data.len()) }
        }
        Value::Raw { type_name, data, .. } => format!("{type_name}[{} bytes]", data.len()),
        v => format!("{v:?}"),
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let pat = a[2].to_lowercase();
    for (i, e) in p.exports.iter().enumerate() {
        let path = p.object_path(i as i32 + 1);
        if !path.to_lowercase().contains(&pat) { continue; }
        println!("#{} {} [{}]", i + 1, path, p.export_class(e));
        let mut r = p.export_reader(i);
        if let Some(pr) = p.read_props(&mut r) {
            for q in &pr.0 { println!("   {}[{}] = {}", q.name, q.index, show(&p, &q.value)); }
        }
    }
}
