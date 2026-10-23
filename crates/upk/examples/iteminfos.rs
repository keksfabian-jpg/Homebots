//! Dev: GCLoadoutItemInfos.LoadoutItemInfos / AbilityInfos entries.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut list = Vec::new();
    for rel in ["Game/GCLoadoutItemInfos.upk", "GCGame.u", "Engine.u", "Core.u"] {
        let p = root.join(rel);
        if let Ok(pk) = upk::Package::open(&p) {
            list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk));
        }
    }
    let w = upk::ui::UiWorld::new(list);
    let r = w.find(&a[2]).expect("object");
    let p = w.props(r);
    for t in w.structs(p.get(&a[3])) {
        let mut s = String::new();
        for (k, i, v) in &t.0 {
            s.push_str(&format!("{k}[{i}]={} ", format!("{v:?}").chars().take(120).collect::<String>()));
        }
        println!("{s}\n");
    }
}
