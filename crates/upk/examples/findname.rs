fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() { if p.object_name_of(e).contains(&a[2]) { println!("#{} {} [{}]", i + 1, p.object_path(i as i32 + 1), p.export_class(e)); } }
}
