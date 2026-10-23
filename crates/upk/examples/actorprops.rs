//! Dev: print properties of the first N exports of a class.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let n: usize = a[3].parse().unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.export_class(e) == a[2]).take(n) {
        let mut r = p.export_reader(i);
        let props = p.read_props(&mut r);
        println!("#{} {} (outer {}) props_end {} / {}", i + 1, p.object_path(i as i32 + 1), p.object_name(e.outer_index), r.pos, e.serial_size);
        if let Some(pr) = props {
            for x in pr.0 {
                let v = match &x.value {
                    upk::props::Value::Object(o) => format!("Object({o} = {})", p.object_path(*o)),
                    upk::props::Value::Raw { type_name, struct_name, data } => format!("{type_name} {struct_name:?} [{} bytes]", data.len()),
                    v => format!("{v:?}"),
                };
                println!("   {}[{}] = {v}", x.name, x.index);
            }
        }
    }
}
