//! Dev: print a material's full expression tree for DiffuseColor / Opacity / EmissiveColor.
use upk::props::{Rd, Value};
fn node(p: &upk::Package, idx: i32, depth: usize, seen: &mut Vec<i32>) {
    let pad = "  ".repeat(depth);
    if idx <= 0 { println!("{pad}(none)"); return; }
    if seen.contains(&idx) || depth > 14 { println!("{pad}#{idx} ..."); return; }
    seen.push(idx);
    let e = &p.exports[idx as usize - 1];
    let mut r = p.export_reader(idx as usize - 1);
    let pr = p.read_props(&mut r).unwrap_or_default();
    let mut scal = Vec::new(); let mut ins = Vec::new();
    for x in &pr.0 {
        if matches!(x.name.as_str(), "EditorX" | "EditorY" | "Desc" | "ExpressionGUID" | "Material" | "bRealtimePreview" | "bHidePreviewWindow") { continue; }
        match &x.value {
            Value::Raw { struct_name: Some(_), data, .. } => {
                let ip = p.read_props(&mut Rd::new(data)).unwrap_or_default();
                if let Some(o) = ip.object("Expression") {
                    let m: Vec<String> = ip.0.iter().filter(|q| q.name.starts_with("Mask")).map(|q| format!("{}={:?}", q.name, q.value)).collect();
                    ins.push((x.name.clone(), o, m.join(",")));
                } else { scal.push(format!("{}={:?}", x.name, ip.0.iter().map(|q| format!("{}:{:?}", q.name, q.value)).collect::<Vec<_>>())); }
            }
            Value::Object(o) => scal.push(format!("{}=#{} {}", x.name, o, p.object_name(*o))),
            v => scal.push(format!("{}={:?}", x.name, v)),
        }
    }
    println!("{pad}#{idx} {} {}", p.export_class(e).trim_start_matches("MaterialExpression"), scal.join(" "));
    for (n, o, m) in ins { println!("{pad} .{n} [{m}]"); node(p, o, depth + 1, seen); }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let mut cur = p.exports.iter().position(|e| p.object_name_of(e) == a[2]).unwrap() as i32 + 1;
    loop {
        let mut r = p.export_reader(cur as usize - 1);
        let pr = p.read_props(&mut r).unwrap();
        println!("== {} [{}]", p.object_name(cur), p.export_class(&p.exports[cur as usize - 1]));
        for x in &pr.0 { if x.name.ends_with("ParameterValues") || x.name == "BlendMode" || x.name == "LightingModel" { println!("  {} = {:?}", x.name, match &x.value { Value::Raw{data,..} => format!("{} bytes", data.len()), v => format!("{v:?}") }); } }
        if p.export_class(&p.exports[cur as usize - 1]) == "Material" {
            for inp in ["DiffuseColor", "Opacity", "OpacityMask", "EmissiveColor"] {
                if let Some(Value::Raw { data, .. }) = pr.get(inp) {
                    let ip = p.read_props(&mut Rd::new(data)).unwrap_or_default();
                    println!("-- {inp}"); node(&p, ip.object("Expression").unwrap_or(0), 1, &mut vec![]);
                }
            }
            break;
        }
        cur = pr.object("Parent").unwrap_or(0);
        if cur <= 0 { break; }
    }
}
