//! Dev: resolved widget rects of a UI scene (by export path) at a viewport size.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut list = Vec::new();
    for rel in ["UI/PC_UI_FrontEndMenus.upk", "UI/PC_UI_OnlineMenus.upk", "UI/PC_UI_PlayerHUD.upk", "UI/UI_PlayerHUD.upk", "UI/PC_Skins.upk", "UI/UI_Fonts.upk", "UI/PC_UI_Fonts.upk", "Engine.u", "GCGame.u", "Core.u"] {
        let p = root.join(rel);
        if let Ok(pk) = upk::Package::open(&p) {
            list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk));
        }
    }
    let w = upk::ui::UiWorld::new(list);
    let r = w.find(&a[2]).expect("scene");
    let mut s = w.load_scene(r);
    let vw: f32 = a[3].parse().unwrap();
    let vh: f32 = a[4].parse().unwrap();
    s.layout(vw, vh);
    for (i, x) in s.widgets.iter().enumerate() {
        let depth = { let mut d = 0; let mut p = x.parent; while let Some(q) = p { d += 1; p = s.widgets[q].parent; } d };
        println!("{}{i} {} [{}] {:.2} {:.2} {:.2} {:.2}{}", "  ".repeat(depth), x.tag, x.class, x.rect[0], x.rect[1], x.rect[2], x.rect[3], if x.hidden { " HIDDEN" } else { "" });
    }
}
