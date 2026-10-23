//! Dev: recursive material expression tree from the material inputs.
use upk::props::{Rd, Value};
fn walk(p: &upk::Package, o: i32, ind: usize, seen: &mut Vec<i32>) {
    if o <= 0 { return; }
    let i = o as usize - 1;
    let class = p.export_class(&p.exports[i]);
    let pr = p.export_props(i).unwrap_or_default();
    let mut info = Vec::new();
    let mut kids = Vec::new();
    for x in &pr.0 {
        match &x.value {
            Value::Raw { type_name, data, .. } if type_name == "StructProperty" => {
                if let Some(sp) = p.read_props(&mut Rd::new(data)) {
                    if let Some(e) = sp.object("Expression") { if e != 0 { kids.push((x.name.clone(), e)); } }
                }
            }
            Value::Object(r) if x.name == "Texture" => info.push(format!("Texture={}", p.object_path(*r))),
            Value::Object(_) => {}
            Value::Raw { .. } => {}
            v if !x.name.starts_with("Editor") && x.name != "ExpressionGUID" => info.push(format!("{}={:?}", x.name, v)),
            _ => {}
        }
    }
    println!("{:ind$}#{o} {class} {}", "", info.join(" "), ind = ind);
    if seen.contains(&o) { return; }
    seen.push(o);
    for (n, e) in kids { println!("{:ind$}  .{n}:", "", ind = ind); walk(p, e, ind + 4, seen); }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "Material").unwrap();
    let pr = p.export_props(i).unwrap();
    let mut seen = Vec::new();
    for inp in ["EmissiveColor", "Opacity", "OpacityMask", "DiffuseColor"] {
        if let Some(Value::Raw { data, .. }) = pr.get(inp) { if let Some(sp) = p.read_props(&mut Rd::new(data)) { if let Some(e) = sp.object("Expression") { println!("{inp}:"); walk(&p, e, 2, &mut seen); } } }
    }
}
