//! Dev: write the native bytes after the tagged properties of an export (by object name) to a file.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let (i, e) = p.exports.iter().enumerate().find(|(_, e)| p.object_name_of(e) == a[2]).unwrap();
    let mut r = p.export_reader(i);
    p.read_props(&mut r).unwrap();
    let b = p.export_bytes(e);
    std::fs::write(&a[3], &b[r.pos..]).unwrap();
    println!("props end {} / {}", r.pos, b.len());
}
