//! Dev: decode an array-of-structs property of an export (each element tagged properties).
use upk::props::Value;
fn show(p: &upk::Package, pr: &upk::props::Props, d: usize) {
    for x in &pr.0 {
        let pad = "  ".repeat(d);
        match &x.value {
            Value::Object(o) => println!("{pad}{}[{}] -> {}", x.name, x.index, if *o != 0 { p.object_path(*o) } else { "None".into() }),
            Value::Raw { type_name, struct_name, data } => {
                println!("{pad}{}[{}] {type_name} {struct_name:?} {} bytes", x.name, x.index, data.len());
                if type_name == "ArrayProperty" && data.len() > 4 {
                    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
                    let mut rd = upk::props::Rd::new(&data[4..]);
                    for k in 0..n {
                        match p.read_props(&mut rd) { Some(ip) => { println!("{pad}  [{k}]"); show(p, &ip, d + 2) } None => { println!("{pad}  [{k}] raw {:?}", &data[4..data.len().min(64)]); break } }
                    }
                } else if struct_name.is_some() {
                    let mut rd = upk::props::Rd::new(data);
                    if let Some(ip) = p.read_props(&mut rd) { show(p, &ip, d + 1) }
                }
            }
            v => println!("{pad}{}[{}] = {v:?}", x.name, x.index),
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e) == a[2]) {
        if a.len() > 3 && !p.object_path(i as i32 + 1).contains(&a[3]) { continue }
        println!("#{} {} [{}]", i + 1, p.object_path(i as i32 + 1), p.export_class(e));
        let mut r = p.export_reader(i);
        if let Some(pr) = p.read_props(&mut r) { show(&p, &pr, 1) }
    }
}
