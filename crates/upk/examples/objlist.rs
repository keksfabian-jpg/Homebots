//! Dev: list exports whose path contains a substring (class + path + size).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let pat = a[2].to_lowercase();
    for (i, e) in p.exports.iter().enumerate() {
        let path = p.object_path(i as i32 + 1);
        if path.to_lowercase().contains(&pat) { println!("#{} {} [{}] size {} off {}", i + 1, path, p.export_class(e), e.serial_size, e.serial_offset); }
    }
}
