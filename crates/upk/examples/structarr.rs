//! Dev: print an ArrayProperty of structs (tagged properties per element) of an export.
fn show(p: &upk::Package, pr: &upk::props::Props, ind: usize) {
    for x in &pr.0 {
        match &x.value {
            upk::props::Value::Raw { type_name, struct_name, data } => {
                println!("{:ind$}{} [{}] = {type_name} {struct_name:?} {} bytes", "", x.name, x.index, data.len(), ind = ind);
                if type_name == "StructProperty" { if let Some(sp) = p.read_props(&mut upk::props::Rd::new(data)) { show(p, &sp, ind + 4); } }
                if type_name == "ArrayProperty" && data.len() > 4 {
                    let n = u32::from_le_bytes(data[0..4].try_into().unwrap());
                    let mut r = upk::props::Rd::new(&data[4..]);
                    for k in 0..n { match p.read_props(&mut r) { Some(sp) if !sp.0.is_empty() => { println!("{:ind$}  [{k}]", "", ind = ind); show(p, &sp, ind + 4); } _ => break } }
                }
            }
            upk::props::Value::Object(o) => println!("{:ind$}{} [{}] = {} ({o})", "", x.name, x.index, if *o != 0 { p.object_path(*o) } else { "None".into() }, ind = ind),
            v => println!("{:ind$}{} [{}] = {v:?}", "", x.name, x.index, ind = ind),
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let pr = p.export_own_props(i).unwrap();
    let only = a.get(3);
    let f = upk::props::Props(pr.0.into_iter().filter(|x| only.map_or(true, |o| &x.name == o)).collect());
    show(&p, &f, 0);
}
