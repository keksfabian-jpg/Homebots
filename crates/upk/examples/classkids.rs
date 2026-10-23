//! Dev: exports whose outer path starts with a class name (declaration children), with class.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let path = p.object_path(i as i32 + 1);
        if path.starts_with(&format!("{}.", a[2])) && path.matches('.').count() == a[2].matches('.').count() + 1 {
            println!("#{} {} [{}] size {}", i + 1, path, p.export_class(e), e.serial_size);
        }
    }
}
