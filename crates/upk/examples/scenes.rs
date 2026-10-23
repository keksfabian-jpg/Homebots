//! Dev: list UIScene exports (class name contains "Scene") of a package with their CookieName.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let c = p.export_class(e);
        if c.contains("Scene") && !p.object_path(i as i32 + 1).contains("Default__") {
            let cookie = p.export_props(i).and_then(|pr| pr.get("CookieName").and_then(|v| match v { upk::props::Value::Str(s) => Some(s.clone()), _ => None })).unwrap_or_default();
            println!("#{} {} [{c}] {cookie}", i + 1, p.object_path(i as i32 + 1));
        }
    }
}
