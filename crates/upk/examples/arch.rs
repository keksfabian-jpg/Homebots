fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for s in &a[2..] {
        let i: usize = s.parse().unwrap();
        let e = &p.exports[i - 1];
        println!("#{i} {} class {} arch {} ({})", p.object_path(i as i32), p.export_class(e), e.archetype_index, p.object_path(e.archetype_index));
    }
}
