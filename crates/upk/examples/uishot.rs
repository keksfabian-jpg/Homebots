//! Dev: CPU render of a UI scene to PNG (textures + fonts; canvas materials are drawn as a flat
//! dark placeholder, NOT their shader). Offline check of menu screens without the game window.
//! Usage: uishot <HOMEFRONT dir> <scene path> <w> <h> <out.png>
//!   HB_STATE=Tag:Focused,Tag2:Pressed  widget states (Enabled/Focused/Active/Pressed/Disabled)
//!   HB_HIDE=Tag,..  HB_SHOW=Tag,..     hide / show widgets
//!   HB_TEXT=Tag=text;Tag2=text         label texts
//!   HB_BG=r,g,b                        background colour (0-255, default 40,48,40)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let game = std::path::Path::new(&a[1]);
    let root = game.join("GCGame").join("CookedPC");
    let lib = upk::library::Library::scan(game).expect("library");
    let mut list = Vec::new();
    let mut rels: Vec<String> = std::fs::read_dir(root.join("UI")).unwrap().filter_map(|e| e.ok()).map(|e| format!("UI/{}", e.file_name().to_string_lossy())).filter(|n| n.ends_with(".upk")).collect();
    rels.sort();
    rels.extend(["Game/GCLoadoutItemInfos.upk", "Engine.u", "GCGame.u", "Core.u"].map(String::from));
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
    let mut rt: std::collections::HashMap<usize, upk::ui::WidgetRt> = std::collections::HashMap::new();
    let find = |t: &str| s.widgets.iter().enumerate().filter(|(_, w)| w.tag == t).map(|(i, _)| i).collect::<Vec<_>>();
    if let Ok(v) = std::env::var("HB_STATE") {
        for kv in v.split(',') {
            let Some((t, st)) = kv.split_once(':') else { continue };
            let st = match st {
                "Focused" => upk::ui::UiState::Focused,
                "Active" => upk::ui::UiState::Active,
                "Pressed" => upk::ui::UiState::Pressed,
                "Disabled" => upk::ui::UiState::Disabled,
                _ => upk::ui::UiState::Enabled,
            };
            for i in find(t) {
                rt.entry(i).or_default().state = Some(st);
            }
        }
    }
    for (var, hide) in [("HB_HIDE", true), ("HB_SHOW", false)] {
        if let Ok(v) = std::env::var(var) {
            for t in v.split(',') {
                for i in find(t) {
                    rt.entry(i).or_default().hidden = Some(hide);
                }
            }
        }
    }
    if let Ok(v) = std::env::var("HB_TEXT") {
        for kv in v.split(';') {
            if let Some((t, x)) = kv.split_once('=') {
                for i in find(t) {
                    rt.entry(i).or_default().text = Some(x.to_string());
                }
            }
        }
    }
    let mut out = Vec::new();
    ui.draw_scene(&s, vw, vh, &rt, &mut out);
    let (iw, ih) = (vw as usize, vh as usize);
    let bg: Vec<f32> = std::env::var("HB_BG").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect()).unwrap_or(vec![40.0, 48.0, 40.0]);
    // gamma-space canvas (the game blends UI in gamma space, MODLOG Phase 2)
    let mut px: Vec<[f32; 3]> = vec![[bg[0] / 255.0, bg[1] / 255.0, bg[2] / 255.0]; iw * ih];
    let mut cache: std::collections::HashMap<upk::ui::GRef, Option<(u32, u32, Vec<u8>)>> = std::collections::HashMap::new();
    let mut missing = std::collections::BTreeSet::new();
    for q in &out {
        let Some(t) = q.tex else { continue };
        let class = ui.world.class(t);
        let img = cache
            .entry(t)
            .or_insert_with(|| {
                if class.contains("Material") || t.idx <= 0 {
                    return None;
                }
                let pkg = &ui.world.pkgs[t.pkg];
                let idx = t.idx as usize - 1;
                let mut tx = pkg.read_texture2d(idx)?;
                tx.load_full(&lib, &pkg.source_package(idx), 2048);
                tx.decode_rgba()
            })
            .as_ref();
        if img.is_none() && !class.contains("Material") {
            missing.insert(ui.world.path(t));
        }
        let x0 = q.rect[0].max(0.0).floor() as usize;
        let y0 = q.rect[1].max(0.0).floor() as usize;
        let x1 = (q.rect[2].min(vw).ceil().max(0.0) as usize).min(iw);
        let y1 = (q.rect[3].min(vh).ceil().max(0.0) as usize).min(ih);
        let (rw, rh) = (q.rect[2] - q.rect[0], q.rect[3] - q.rect[1]);
        if rw <= 0.0 || rh <= 0.0 {
            continue;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let fx = (x as f32 + 0.5 - q.rect[0]) / rw;
                let fy = (y as f32 + 0.5 - q.rect[1]) / rh;
                if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
                    continue;
                }
                let u = q.uv[0] + (q.uv[2] - q.uv[0]) * fx;
                let v = q.uv[1] + (q.uv[3] - q.uv[1]) * fy;
                let (tc, ta) = match img {
                    Some((tw, th, d)) => {
                        let tx = ((u.rem_euclid(1.0) * *tw as f32) as usize).min(*tw as usize - 1);
                        let ty = ((v.rem_euclid(1.0) * *th as f32) as usize).min(*th as usize - 1);
                        let o = (ty * *tw as usize + tx) * 4;
                        ([d[o] as f32 / 255.0, d[o + 1] as f32 / 255.0, d[o + 2] as f32 / 255.0], d[o + 3] as f32 / 255.0)
                    }
                    // material placeholder / missing texture: magenta for missing textures
                    None if class.contains("Material") => ([0.12, 0.08, 0.08], 0.6),
                    None => ([1.0, 0.0, 1.0], 1.0),
                };
                let al = (ta * q.color[3]).clamp(0.0, 1.0);
                if al <= 0.0 {
                    continue;
                }
                let p = &mut px[y * iw + x];
                for c in 0..3 {
                    // canvas PS: pow(sat(tex_linear * vcolor), 1/2.2)
                    let s = (tc[c].powf(2.2) * q.color[c]).clamp(1e-6, 1.0).powf(1.0 / 2.2);
                    p[c] = p[c] * (1.0 - al) + s * al;
                }
            }
        }
    }
    let bytes: Vec<u8> = px.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8)).collect();
    image::save_buffer(&a[5], &bytes, iw as u32, ih as u32, image::ColorType::Rgb8).unwrap();
    println!("{} quads -> {}", out.len(), a[5]);
    for m in missing {
        println!("missing texture: {m}");
    }
}
