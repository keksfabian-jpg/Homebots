//! Dev: Kismet level streaming actions (class, props) of a map.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let pat = a.get(2).cloned().unwrap_or("LevelStreaming".into());
    for (i, e) in p.exports.iter().enumerate() {
        let c = p.export_class(e);
        if !c.contains(&pat) || c.starts_with("LevelStreaming") { continue; }
        let Some(pr) = p.export_props(i) else { continue };
        let keys: Vec<String> = pr.0.iter().filter(|q| !["ObjPosX","ObjPosY","DrawWidth","ObjComment","DrawHeight"].contains(&q.name.as_str())).map(|q| format!("{}={:?}", q.name, q.value)).collect();
        println!("{} [{c}] {}", p.object_path(i as i32 + 1), keys.join(" ").chars().take(900).collect::<String>());
    }
}
