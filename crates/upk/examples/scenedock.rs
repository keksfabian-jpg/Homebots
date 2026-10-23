//! Dev: docks, values and rects (1280x720) of a scene's widgets whose tag contains a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut list = Vec::new();
    for rel in ["UI/PC_UI_FrontEndMenus.upk", "UI/PC_UI_OnlineMenus.upk", "UI/PC_UI_InGameMenus.upk", "UI/PC_Skins.upk", "UI/UI_Fonts.upk", "UI/PC_UI_Fonts.upk", "Engine.u", "GCGame.u", "Core.u"] {
        let p = root.join(rel);
        if let Ok(pk) = upk::Package::open(&p) {
            list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk));
        }
    }
    let w = upk::ui::UiWorld::new(list);
    let r = w.find(&a[2]).or_else(|| {
        for (pi, p) in w.pkgs.iter().enumerate() {
            for (i, e) in p.exports.iter().enumerate() {
                if p.object_name_of(e).eq_ignore_ascii_case(&a[2]) && p.export_class(e).contains("Scene") {
                    return Some(upk::ui::GRef { pkg: pi, idx: i as i32 + 1 });
                }
            }
        }
        None
    }).expect("scene");
    let mut s = w.load_scene(r);
    s.layout(1280.0, 720.0);
    for (i, x) in s.widgets.iter().enumerate() {
        if x.tag.contains(&a[3]) {
            println!("{i} {} [{}] parent {:?} rect {:?} value {:?} eval {:?} hidden {}", x.tag, x.class, x.parent, x.rect, x.value, x.eval, x.hidden);
            for d in x.dock.iter().flatten() {
                println!("    dock -> {:?} ({}) face {} pad {} type {}", d.target, d.target.map(|t| s.widgets[t].tag.clone()).unwrap_or_default(), d.face, d.pad, d.pad_type);
            }
        }
    }
}
