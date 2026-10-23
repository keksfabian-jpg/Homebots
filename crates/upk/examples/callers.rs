//! Dev: functions / states whose script bytes contain the object index of an export path
//! (rough "who calls X"). Usage: callers <package> <export path>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let Some(t) = (0..p.exports.len()).find(|&i| p.object_path(i as i32 + 1).eq_ignore_ascii_case(&a[2])) else { return };
    let needle = (t as i32 + 1).to_le_bytes();
    for (i, e) in p.exports.iter().enumerate() {
        let c = p.export_class(e);
        if c != "Function" && c != "State" {
            continue;
        }
        let b = p.export_bytes(e);
        if b.windows(4).any(|w| w == needle) {
            println!("{} [{c}]", p.object_path(i as i32 + 1));
        }
    }
}
