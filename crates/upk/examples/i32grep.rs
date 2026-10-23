//! Dev: Function / State exports whose script bytes contain a given i32 (object index).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for v in &a[2..] {
        let n: i32 = v.parse().unwrap();
        let pat = n.to_le_bytes();
        for (i, e) in p.exports.iter().enumerate() {
            let c = p.export_class(e);
            if std::env::var_os("ALL").is_none() && c != "Function" && c != "State" { continue; }
            let b = p.export_bytes(e);
            if b.windows(4).any(|w| w == pat) {
                println!("{n}: #{} {} [{c}]", i + 1, p.object_path(i as i32 + 1));
            }
        }
    }
}
