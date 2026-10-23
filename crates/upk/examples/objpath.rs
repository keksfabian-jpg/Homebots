//! Dev: object path of reference indices.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for x in &a[2..] { let i: i32 = x.parse().unwrap(); println!("{i} {}", p.object_path(i)); }
}
