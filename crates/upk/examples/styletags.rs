//! Dev: UIStyle exports per StyleTag across the UI packages (which skin defines a tag).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut rels: Vec<String> = std::fs::read_dir(root.join("UI")).unwrap().filter_map(|e| e.ok()).map(|e| format!("UI/{}", e.file_name().to_string_lossy())).filter(|n| n.ends_with(".upk")).collect();
    rels.sort();
    let mut list = Vec::new();
    for rel in rels {
        let p = root.join(&rel);
        if let Ok(pk) = upk::Package::open(&p) { list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk)); }
    }
    let w = upk::ui::UiWorld::new(list);
    let mut tags: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (pi, p) in w.pkgs.iter().enumerate() {
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "UIStyle" { continue; }
            let r = upk::ui::GRef { pkg: pi, idx: i as i32 + 1 };
            if let Some(t) = w.props(r).enum_i("StyleTag", 0) { tags.entry(t.to_string()).or_default().push(format!("{}:{}", w.pkgs[pi].object_path(i as i32 + 1), pi)); }
        }
    }
    let q = a.get(2).cloned().unwrap_or_default().to_lowercase();
    for (t, v) in tags { if t.to_lowercase().contains(&q) { println!("{t}: {}", v.join("  ")); } }
}
