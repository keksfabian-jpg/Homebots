//! Dev: write an export's serialized bytes to a file.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    std::fs::write(&a[3], p.export_bytes(&p.exports[i])).unwrap();
}
