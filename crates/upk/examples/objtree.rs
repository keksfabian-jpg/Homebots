//! Dev: recursive property dump of an export and the exports it references (depth-limited).
use std::collections::HashSet;
fn dump(p: &upk::Package, o: i32, depth: usize, max: usize, seen: &mut HashSet<i32>) {
    if o <= 0 || depth > max || !seen.insert(o) { return; }
    let ind = "  ".repeat(depth);
    println!("{ind}#{o} {} [{}]", p.object_path(o), p.export_class(&p.exports[o as usize - 1]));
    let Some(pr) = p.export_own_props(o as usize - 1) else { return };
    let mut refs = Vec::new();
    for x in &pr.0 {
        match &x.value {
            upk::props::Value::Object(r) => { println!("{ind}  {}[{}] = {}", x.name, x.index, if *r != 0 { p.object_path(*r) } else { "None".into() }); refs.push(*r); }
            upk::props::Value::Raw { type_name, struct_name, data } => {
                if type_name == "ArrayProperty" && data.len() >= 4 {
                    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
                    if data.len() == 4 + 4 * n && n > 0 {
                        let objs: Vec<i32> = (0..n).map(|k| i32::from_le_bytes(data[4 + 4 * k..8 + 4 * k].try_into().unwrap())).collect();
                        if objs.iter().all(|&r| r == 0 || ((r > 0 && (r as usize) <= p.exports.len()) || (r < 0 && ((-r) as usize) <= p.imports.len()))) {
                            println!("{ind}  {}[] = {:?}", x.name, objs.iter().map(|&r| if r != 0 { p.object_name(r) } else { "None".into() }).collect::<Vec<_>>());
                            refs.extend(objs);
                            continue;
                        }
                    }
                }
                println!("{ind}  {}[{}] = {type_name} {struct_name:?} {} bytes", x.name, x.index, data.len());
                if type_name == "StructProperty" { if let Some(sp) = p.read_props(&mut upk::props::Rd::new(data)) { for y in sp.0 { println!("{ind}      {} = {:?}", y.name, match &y.value { upk::props::Value::Raw{data, ..} => format!("raw {}b", data.len()), v => format!("{v:?}") }); } } }
            }
            v => println!("{ind}  {}[{}] = {v:?}", x.name, x.index),
        }
    }
    for r in refs { dump(p, r, depth + 1, max, seen); }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let o: i32 = a[2].parse().unwrap();
    let max: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(6);
    dump(&p, o, 0, max, &mut HashSet::new());
}
