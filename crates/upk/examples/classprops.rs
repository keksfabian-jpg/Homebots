//! Dev: print all tagged properties of every export of a class.
use upk::props::{Rd, Value};
fn show(p: &upk::Package, v: &Value) -> String {
    match v {
        Value::Object(o) => format!("#{o} {}", p.object_path(*o)),
        Value::Raw { struct_name: Some(sn), data, .. } => format!("{sn}{{{}}}", p.read_props(&mut Rd::new(data)).map(|x| x.0.iter().map(|y| format!("{}={}", y.name, show(p, &y.value))).collect::<Vec<_>>().join(", ")).unwrap_or_else(|| format!("{} bytes", data.len()))),
        Value::Raw { type_name, data, .. } => format!("{type_name}[{} bytes]", data.len()),
        v => format!("{v:?}"),
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.export_class(e) == a[2]) {
        println!("#{} {}", i + 1, p.object_path(i as i32 + 1));
        let mut r = p.export_reader(i);
        if let Some(pr) = p.read_props(&mut r) {
            for q in &pr.0 { println!("   {}[{}] = {}", q.name, q.index, show(&p, &q.value)); }
        }
    }
}
