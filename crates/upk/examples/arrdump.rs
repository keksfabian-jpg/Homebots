//! Dev: print a struct-array property of an export (by object name), one element per block.
use upk::props::{Rd, Value};
fn show(p: &upk::Package, v: &Value) -> String {
    match v {
        Value::Object(o) => format!("#{o} {}", p.object_path(*o)),
        Value::Raw { struct_name: Some(sn), data, .. } => format!("{sn}{{{}}}", p.read_props(&mut Rd::new(data)).map(|x| x.0.iter().map(|y| format!("{}={}", y.name, show(p, &y.value))).collect::<Vec<_>>().join(", ")).unwrap_or_else(|| format!("{} bytes", data.len()))),
        Value::Raw { type_name, data, .. } => format!("{type_name}[{} bytes: {:?}]", data.len(), &data[..data.len().min(64)]),
        v => format!("{v:?}"),
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let (i, _) = p.exports.iter().enumerate().find(|(_, e)| p.object_name_of(e) == a[2]).unwrap();
    let mut r = p.export_reader(i);
    let pr = p.read_props(&mut r).unwrap();
    for (k, s) in p.array_structs(pr.get(&a[3]).unwrap()).iter().enumerate() {
        println!("[{k}]");
        for q in &s.0 { println!("   {}[{}] = {}", q.name, q.index, show(&p, &q.value)); }
    }
}
