//! Dev: class chain (export super_index) of a class in a script package.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for n in &a[2..] {
        let mut i = p.exports.iter().position(|e| p.object_name_of(e) == *n && p.export_class(e) == "Class").map(|x| x as i32 + 1).unwrap_or(0);
        let mut chain = Vec::new();
        for _ in 0..12 {
            if i <= 0 { if i < 0 { chain.push(format!("{} (import)", p.object_name(i))); } break; }
            chain.push(p.object_name(i));
            i = p.exports[i as usize - 1].super_index;
        }
        println!("{}", chain.join(" -> "));
    }
}
