//! Dev: widgets of a UI scene with depth, tag and class (first N levels).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut list = Vec::new();
    for rel in ["UI/PC_UI_PlayerHUD.upk", "UI/UI_PlayerHUD.upk", "UI/PC_UI_FrontEndMenus.upk", "UI/PC_UI_OnlineMenus.upk", "UI/PC_Skins.upk", "UI/UI_Fonts.upk", "UI/PC_UI_Fonts.upk", "Engine.u", "GCGame.u", "Core.u"] {
        let p = root.join(rel);
        if let Ok(pk) = upk::Package::open(&p) {
            list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk));
        }
    }
    let w = upk::ui::UiWorld::new(list);
    let r = w.find(&a[2]).expect("scene");
    let s = w.load_scene(r);
    let max: usize = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(3);
    for (i, x) in s.widgets.iter().enumerate() {
        let mut d = 0; let mut p = x.parent; while let Some(q) = p { d += 1; p = s.widgets[q].parent; }
        let mk = |n: &str| x.props.sub(n).and_then(|t| t.str("MarkupString").map(String::from));
        let ds = mk("DataSource").or_else(|| mk("ValueDataSource")).or_else(|| mk("CaptionDataSource")).unwrap_or_default();
        if d <= max { println!("{i} {}{} [{}]{} {ds}", "  ".repeat(d), x.tag, x.class, if x.hidden { " HIDDEN" } else { "" }); }
    }
}
