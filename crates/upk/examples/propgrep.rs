//! Dev: exports that have a property with the given name, and its value.
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let Ok(Some(pr)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.export_own_props(i))) else { continue };
        for x in &pr.0 { if x.name == a[2] { println!("#{} {} [{}] {} = {:?}", i + 1, p.object_path(i as i32 + 1), p.export_class(e), x.name, x.value); } }
    }
}
