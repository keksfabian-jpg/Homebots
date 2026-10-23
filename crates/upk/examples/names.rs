//! Dev: print the name table of a package (index: name).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, n) in p.names.iter().enumerate() { println!("{i} {}", n.name); }
}
