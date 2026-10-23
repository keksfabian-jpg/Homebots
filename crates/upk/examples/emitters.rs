//! Dev: placed Emitter actors: template, location, auto-activate.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut hist = std::collections::BTreeMap::new();
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            let c = p.export_class(e);
            if !c.contains("Emitter") || !p.object_path(i as i32 + 1).starts_with("TheWorld.") { continue; }
            let Some(ap) = p.export_props(i) else { continue };
            let Some(pc) = ap.object("ParticleSystemComponent").filter(|&x| x > 0) else { continue };
            let Some(cp) = p.export_props(pc as usize - 1) else { continue };
            let t = cp.object("Template").unwrap_or(0);
            let tn = if t != 0 { p.object_path(t) } else { "-".into() };
            *hist.entry(tn.clone()).or_insert(0) += 1;
            if a.len() > 0 && std::env::var("V").is_ok() { println!("{} {} {:?} auto={:?}", f.rsplit('/').next().unwrap(), tn, ap.vector("Location"), cp.get("bAutoActivate")); }
        }
    }
    for (k, v) in hist { println!("{v:4} {k}"); }
}
