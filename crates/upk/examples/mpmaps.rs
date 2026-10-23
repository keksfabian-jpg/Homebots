//! Dev: GCUIInfo m_MPMapsInfo entries (create-game map list, creategame.rs) of every UIInfo object.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut list = Vec::new();
    for rel in ["UI/GCUIInfos.upk", "GCGame.u", "Engine.u", "Core.u"] {
        let p = root.join(rel);
        if let Ok(pk) = upk::Package::open(&p) {
            list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk));
        }
    }
    let objs: Vec<String> = {
        let pk = &list[0].1;
        pk.exports.iter().enumerate().filter(|(_, e)| pk.export_class(e).contains("UIInfo")).map(|(i, _)| format!("GCUIInfos.{}", pk.object_path(i as i32 + 1))).collect()
    };
    let w = upk::ui::UiWorld::new(list);
    for o in objs {
        let Some(r) = w.find(&o) else { continue };
        let p = w.props(r);
        let v = w.structs(p.get("m_MPMapsInfo"));
        if v.is_empty() {
            continue;
        }
        println!("== {o}: {} entries", v.len());
        for t in v {
            let mut s = String::new();
            for (k, i, v) in &t.0 {
                s.push_str(&format!("{k}[{i}]={} ", format!("{v:?}").chars().take(90).collect::<String>()));
            }
            println!("{s}");
        }
    }
}
