//! Dev: draw list of a UI scene at a viewport size.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let mut list = Vec::new();
    let mut rels: Vec<String> = std::fs::read_dir(root.join("UI")).unwrap().filter_map(|e| e.ok()).map(|e| format!("UI/{}", e.file_name().to_string_lossy())).filter(|n| n.ends_with(".upk")).collect();
    rels.sort();
    rels.extend(["Engine.u", "GCGame.u", "Core.u"].map(String::from));
    for rel in rels {
        let p = root.join(&rel);
        if let Ok(pk) = upk::Package::open(&p) {
            list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk));
        }
    }
    let w = upk::ui::UiWorld::new(list);
    let loc = upk::ui::Loc::load(&root.join("Coalesced_deu.bin")).unwrap();
    let ui = upk::ui::Ui::new(w, loc);
    let r = ui.world.find(&a[2]).expect("scene");
    let mut s = ui.world.load_scene(r);
    let vw: f32 = a[3].parse().unwrap();
    let vh: f32 = a[4].parse().unwrap();
    s.layout(vw, vh);
    if std::env::var("HB_TREE").is_ok() {
        fn pr(s: &upk::ui::Scene, i: usize, d: usize) {
            let w = &s.widgets[i];
            println!("{}{} #{i} [{}] {:?}{}", "  ".repeat(d), w.tag, w.class, w.rect.map(|x| (x * 10.0).round() / 10.0), if w.hidden { " hidden" } else { "" });
            for &c in &w.children { pr(s, c, d + 1); }
        }
        pr(&s, 0, 0);
        return;
    }
    if let Ok(t) = std::env::var("HB_SIMPLE") {
        for w in &s.widgets {
            if w.tag == t {
                for o in ui.world.objects(w.props.get("SimpleObjects")).into_iter().chain(ui.world.objects(w.props.get("CodeBasedSimpleObjects")).into_iter().map(|o| { println!("(code)"); o })) {
                    let op = ui.world.props(o);
                    println!("{} [{}]", ui.world.name(o), ui.world.class(o));
                    for (k, i, v) in &op.0 { println!("   {k}[{i}] = {}", format!("{v:?}").chars().take(160).collect::<String>()); }
                }
            }
        }
        return;
    }
    if let Ok(t) = std::env::var("HB_TAG") {
        println!("{:?}", ui.tag_text_style(&t, upk::ui::UiState::Enabled));
        return;
    }
    if let Ok(t) = std::env::var("HB_DUMP") {
        for w in &s.widgets {
            if w.tag == t {
                println!("{} {} rect {:?}", w.tag, w.class, w.rect); for (k, i, v) in &w.props.0 { println!("  {k}[{i}] = {}", format!("{v:?}").chars().take(300).collect::<String>()); }
            }
        }
        return;
    }
    let mut out = Vec::new();
    let mut rt = std::collections::HashMap::new();
    if let Ok(c) = std::env::var("HB_CRUMBS") {
        for (i, w) in s.widgets.iter().enumerate() {
            if w.class == "GCUIObject_CookieTrail" {
                rt.insert(i, upk::ui::WidgetRt { crumbs: c.split('|').map(String::from).collect(), ..Default::default() });
            }
        }
    }
    ui.draw_scene(&s, vw, vh, &rt, &mut out);
    for q in &out {
        println!("rect {:.2} {:.2} {:.2} {:.2} uv {:.4} {:.4} {:.4} {:.4} col {:?} tex {}", q.rect[0], q.rect[1], q.rect[2], q.rect[3], q.uv[0], q.uv[1], q.uv[2], q.uv[3], q.color.map(|c| (c * 1000.0).round() / 1000.0), q.tex.map(|t| ui.world.path(t)).unwrap_or_default());
    }
}
