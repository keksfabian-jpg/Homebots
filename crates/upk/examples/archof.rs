//! Dev: archetype chain of an export (index).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for s in &a[2..] {
        let mut i: i32 = s.parse().unwrap();
        for _ in 0..6 {
            if i == 0 { break; }
            let name = p.object_path(i);
            if i > 0 {
                let e = &p.exports[i as usize - 1];
                println!("  {i} {name} [{}] archetype {}", p.export_class(e), e.archetype_index);
                i = e.archetype_index;
            } else { println!("  {i} {name} (import)"); break; }
        }
        println!();
    }
}
