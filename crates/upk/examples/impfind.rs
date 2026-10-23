fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for i in 0..p.imports.len() { let o = -(i as i32) - 1; let path = p.object_path(o); if path.contains(&a[2]) { println!("import {o} {path}"); } }
    for i in 0..p.exports.len() { let path = p.object_path(i as i32 + 1); if path.contains(&a[2]) { println!("export {} {path} [{}]", i + 1, p.export_class(&p.exports[i])); } }
}
