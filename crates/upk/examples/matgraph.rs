//! Dev: dump a Material's DiffuseColor input struct and the expressions it points to.
use upk::props::Value;
fn show(p: &upk::Package, idx: i32, depth: usize) {
    if idx <= 0 || depth > 4 { return; }
    let e = &p.exports[idx as usize - 1];
    let mut r = p.export_reader(idx as usize - 1);
    let Some(pr) = p.read_props(&mut r) else { return };
    println!("{}#{} {} {}", "  ".repeat(depth), idx, p.export_class(e), p.object_name(idx));
    for x in &pr.0 {
        let v = match &x.value {
            Value::Object(o) => format!("-> #{o} {}", p.object_name(*o)),
            Value::Raw { struct_name: Some(sn), data, .. } => {
                let mut rd = upk::props::Rd::new(data);
                let inner = p.read_props(&mut rd).map(|ip| ip.0.iter().map(|q| format!("{}={:?}", q.name, q.value)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                format!("{sn} {{{inner}}}")
            }
            v => format!("{v:?}"),
        };
        println!("{}  {} = {}", "  ".repeat(depth), x.name, if v.len() > 160 { &v[..160] } else { &v });
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && (p.export_class(e) == "Material" || p.export_class(e) == "MaterialInstanceConstant")).unwrap();
    show(&p, i as i32 + 1, 0);
    for extra in &a[3..] { show(&p, extra.parse().unwrap(), 1); }
}
