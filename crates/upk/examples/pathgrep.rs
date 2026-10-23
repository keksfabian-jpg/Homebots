//! Dev: exports and imports whose full path contains a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for i in 0..p.exports.len() {
            let path = p.object_path(i as i32 + 1);
            if path.contains(&a[1]) { println!("{f}: export #{} {path} [{}]", i + 1, p.export_class(&p.exports[i])); }
        }
        for i in 0..p.imports.len() {
            let path = p.object_path(-(i as i32) - 1);
            if path.contains(&a[1]) { println!("{f}: import {path}"); }
        }
    }
}
