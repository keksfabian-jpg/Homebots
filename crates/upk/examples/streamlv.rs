//! Dev: LevelStreaming objects of a map (class, PackageName, flags).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        let c = p.export_class(e);
        if !c.starts_with("LevelStreaming") { continue; }
        let pr = p.export_props(i).unwrap();
        let keys: Vec<String> = pr.0.iter().map(|q| format!("{}={:?}", q.name, q.value)).collect();
        println!("{} [{c}] {}", p.object_path(i as i32 + 1), keys.join(" ").chars().take(400).collect::<String>());
    }
}
