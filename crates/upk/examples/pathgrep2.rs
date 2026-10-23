//! Dev: export paths containing a substring, with class.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let path = p.object_path(i as i32 + 1);
        if path.contains(&a[2]) { println!("#{} {} [{}]", i + 1, path, p.export_class(e)); }
    }
}
