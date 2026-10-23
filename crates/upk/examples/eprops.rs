//! Dev: properties (export_props, with nested structs) of exports by index.
fn show(p: &upk::Package, pr: &upk::props::Props, ind: usize) {
    for x in &pr.0 {
        match &x.value {
            upk::props::Value::Raw { type_name, struct_name, data } => {
                println!("{:ind$}{}[{}] = {type_name} {struct_name:?} {} bytes", "", x.name, x.index, data.len(), ind = ind);
                if type_name == "StructProperty" { if let Some(sp) = p.read_props(&mut upk::props::Rd::new(data)) { show(p, &sp, ind + 4); } }
                if type_name == "ArrayProperty" && data.len() > 4 {
                    let n = u32::from_le_bytes(data[0..4].try_into().unwrap());
                    let mut r = upk::props::Rd::new(&data[4..]);
                    let mut ok = false;
                    for k in 0..n { match p.read_props(&mut r) { Some(sp) if !sp.0.is_empty() => { ok = true; println!("{:ind$}  [{k}]", "", ind = ind); show(p, &sp, ind + 4); } _ => break } }
                    if !ok && (data.len() - 4) == n as usize * 4 {
                        let v: Vec<String> = (0..n as usize).map(|k| { let o = i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap()); if o != 0 && o.unsigned_abs() as usize <= p.exports.len().max(p.imports.len()) { format!("{o}:{}", p.object_path(o)) } else { format!("{}", f32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap())) } }).collect();
                        println!("{:ind$}  = {:?}", "", v, ind = ind);
                    }
                }
            }
            upk::props::Value::Object(o) => println!("{:ind$}{}[{}] = {} ({o})", "", x.name, x.index, if *o != 0 { p.object_path(*o) } else { "None".into() }, ind = ind),
            v => println!("{:ind$}{}[{}] = {v:?}", "", x.name, x.index, ind = ind),
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for s in &a[2..] {
        let i: usize = s.parse::<usize>().unwrap() - 1;
        println!("== #{} {} [{}]", i + 1, p.object_path(i as i32 + 1), p.export_class(&p.exports[i]));
        if let Some(pr) = p.export_props(i) { show(&p, &pr, 0); }
    }
}
