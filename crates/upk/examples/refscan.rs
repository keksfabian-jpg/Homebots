//! Dev: exports whose serialized data contains a given object index (i32), any class.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for v in &a[2..] {
        let n: i32 = v.parse().unwrap();
        let pat = n.to_le_bytes();
        for (i, e) in p.exports.iter().enumerate() {
            let d = p.export_bytes(e);
            if d.windows(4).any(|w| w == pat) {
                println!("{n} <- #{} {} [{}]", i + 1, p.object_path(i as i32 + 1), p.export_class(e));
            }
        }
    }
}
