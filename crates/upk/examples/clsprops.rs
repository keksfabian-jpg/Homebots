//! Dev: properties of the first N exports of a class (substring exact class match).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let n: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(2);
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.export_class(e) == a[2]).take(n) {
        println!("#{} {} size {}", i + 1, p.object_path(i as i32 + 1), e.serial_size);
        if let Some(pr) = p.export_props(i) {
            for x in &pr.0 { println!("   {}[{}] = {}", x.name, x.index, match &x.value { upk::props::Value::Raw{type_name, struct_name, data} => format!("{type_name} {struct_name:?} {} bytes {}", data.len(), if data.len() <= 16 { data.chunks(4).map(|c| if c.len()==4 { format!("{}", f32::from_le_bytes(c.try_into().unwrap())) } else { String::new() }).collect::<Vec<_>>().join(",") } else { String::new() }), upk::props::Value::Object(o) => format!("obj {} ({})", o, if *o != 0 { p.object_path(*o) } else { String::new() }), v => format!("{v:?}") }); }
        }
    }
}
