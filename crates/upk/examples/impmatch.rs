//! Dev: imports whose path contains a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let q = a[2].to_lowercase();
    for i in 0..p.imports.len() {
        let path = p.object_path(-(i as i32) - 1);
        if path.to_lowercase().contains(&q) {
            println!("{} [{}]", path, p.name(p.imports[i].class_name));
        }
    }
}
