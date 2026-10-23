//! Dev: props of an object referenced from a scene widget property (by path in the UI world).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut list = Vec::new();
    let mut rels: Vec<String> = std::fs::read_dir(root.join("UI")).unwrap().filter_map(|e| e.ok()).map(|e| format!("UI/{}", e.file_name().to_string_lossy())).filter(|n| n.ends_with(".upk")).collect();
    rels.sort();
    rels.extend(["Engine.u", "GCGame.u", "Core.u"].map(String::from));
    for rel in rels {
        let p = root.join(&rel);
        if let Ok(pk) = upk::Package::open(&p) { list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk)); }
    }
    let ui = upk::ui::UiWorld::new(list);
    let w = ui.find(&a[2]).expect("widget");
    let p = ui.props(w);
    for name in &a[3..] {
        let Some(o) = p.obj(name) else { println!("{name}: none"); continue };
        println!("== {name}: {} [{}]", ui.path(o), ui.class(o));
        for (k, i, v) in &ui.props(o).0 { println!("   {k}[{i}] = {}", format!("{v:?}").chars().take(200).collect::<String>()); }
        for (k, i, v) in &ui.props(o).0 { if let upk::ui::PVal::Obj(Some(g)) = v { println!("      {k}[{i}] -> {} [{}]", ui.path(*g), ui.class(*g)); } }
    }
}
