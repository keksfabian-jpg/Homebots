//! Dev: print class, path and tagged properties of exports given by 1-based index.
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
    for s in &a[2..] {
        let i: usize = s.parse().unwrap();
        let e = &p.exports[i - 1];
        println!("#{i} {} [{}]", p.object_path(i as i32), p.export_class(e));
        let mut r = p.export_reader(i - 1);
        if let Some(pr) = p.read_props(&mut r) {
            for x in pr.0 { println!("   {} = {}", x.name, show(&p, &x.value)); }
        }
    }
}
