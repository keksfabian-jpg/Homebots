//! Dev: option widgets of a UI scene: tag, class, data-store markup, slider ranges, visibility.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut list = Vec::new();
    for rel in ["UI/PC_UI_FrontEndMenus.upk", "UI/PC_UI_OnlineMenus.upk", "UI/PC_Skins.upk", "UI/UI_Fonts.upk", "UI/PC_UI_Fonts.upk", "Engine.u", "GCGame.u", "Core.u"] {
        let p = root.join(rel);
        if let Ok(pk) = upk::Package::open(&p) {
            list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk));
        }
    }
    let w = upk::ui::UiWorld::new(list);
    let r = w.find(&a[2]).expect("scene");
    let mut s = w.load_scene(r);
    s.layout(1280.0, 720.0);
    for (i, x) in s.widgets.iter().enumerate() {
        let mk = |n: &str| x.props.sub(n).and_then(|t| t.str("MarkupString").map(String::from));
        let ds = mk("DataSource").or_else(|| mk("ValueDataSource"));
        let sv = x.props.sub("SliderValue").map(|t| format!("slider {:?}..{:?} cur {:?} nudge {:?} int {:?}", t.f32("MinValue"), t.f32("MaxValue"), t.f32("CurrentValue"), t.f32("NudgeValue"), t.bool("bIntRange")));
        let interesting = ds.is_some() || sv.is_some() || x.class.contains("Check") || x.class.contains("Combo") || x.class.contains("Slider") || x.class.contains("Option") || x.class.contains("KeyBind") || x.class.contains("Button") || x.class.contains("Tab");
        if !interesting { continue }
        println!("{i} {} [{}]{} {:?} {}", x.tag, x.class, if x.hidden { " HIDDEN" } else { "" }, ds, sv.unwrap_or_default());
    }
}
