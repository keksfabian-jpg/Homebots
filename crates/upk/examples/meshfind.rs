//! Dev: StaticMesh / SkeletalMesh exports whose path matches any pattern.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pats: Vec<String> = a[1].split(',').map(|s| s.to_lowercase()).collect();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            let c = p.export_class(e);
            if c != "StaticMesh" && c != "SkeletalMesh" { continue; }
            let path = p.object_path(i as i32 + 1);
            let l = path.to_lowercase();
            if pats.iter().any(|x| l.contains(x.as_str())) { println!("{} {c} {path}", f.rsplit('/').next().unwrap()); }
        }
    }
}
