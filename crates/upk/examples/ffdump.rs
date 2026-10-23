//! Dev: dump an export's properties recursively (struct properties decoded, object refs as paths).
use upk::props::Value;
fn show(p: &upk::Package, pr: &upk::props::Props, depth: usize) {
    for x in &pr.0 {
        let pad = "  ".repeat(depth);
        match &x.value {
            Value::Object(o) => println!("{pad}{}[{}] -> {}", x.name, x.index, if *o != 0 { p.object_path(*o) } else { "None".into() }),
            Value::Raw { struct_name: Some(sn), data, .. } => {
                println!("{pad}{}[{}] struct {sn} ({} bytes)", x.name, x.index, data.len());
                let mut rd = upk::props::Rd::new(data);
                if let Some(ip) = p.read_props(&mut rd) { show(p, &ip, depth + 1); }
            }
            Value::Raw { type_name, data, .. } if type_name == "ArrayProperty" && data.len() >= 4 => {
                let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
                if data.len() != 4 + n * 4 {
                    // array of structs: tagged property lists one after another
                    let mut rd = upk::props::Rd::new(&data[4..]);
                    let mut ok = true;
                    for k in 0..n {
                        match p.read_props(&mut rd) {
                            Some(ip) if ok => { println!("{pad}{}[{k}] element", x.name); show(p, &ip, depth + 1); }
                            _ => { ok = false; }
                        }
                    }
                    if ok { continue; }
                }
                let objs: Vec<String> = if data.len() == 4 + n * 4 { (0..n).map(|k| { let o = i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap()); if o != 0 { p.object_path(o) } else { "None".into() } }).collect() } else { vec![format!("<{} bytes>", data.len())] };
                println!("{pad}{}[{}] array {n}: {:?}", x.name, x.index, objs);
            }
            v => println!("{pad}{}[{}] = {:?}", x.name, x.index, v),
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e) == a[2]) {
        println!("#{} {} [{}]", i + 1, p.object_path(i as i32 + 1), p.export_class(e));
        if let Some(pr) = p.export_props(i) { show(&p, &pr, 1); }
    }
}
