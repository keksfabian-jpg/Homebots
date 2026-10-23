//! Dev: print properties of an export found by exact object name.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e) == a[2]) {
        let mut r = p.export_reader(i);
        println!("#{} {} [{}] start@{}", i + 1, p.object_path(i as i32 + 1), p.export_class(e), r.pos);
        if let Some(pr) = p.read_props(&mut r) {
            for x in pr.0 { println!("   {}[{}] = {:?}", x.name, x.index, match &x.value { upk::props::Value::Raw{type_name, struct_name, data} => format!("{type_name} {struct_name:?} {} bytes", data.len()), v => format!("{v:?}") }); }
        }
        println!("   props end @{} / {}", r.pos, e.serial_size);
    }
}
