//! Dev: Materials vs lodOverride materials of components whose mesh matches a pattern.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pat = a[1].to_lowercase();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if !p.export_class(e).contains("StaticMeshActor") { continue; }
            let Some(ap) = p.export_props(i) else { continue };
            let Some(c) = ap.object("StaticMeshComponent").filter(|&c| c > 0) else { continue };
            let Some(cp) = p.export_props(c as usize - 1) else { continue };
            let Some(m) = cp.object("StaticMesh") else { continue };
            if !p.object_path(m).to_lowercase().contains(&pat) { continue; }
            let mats: Vec<String> = cp.get("Materials").map(|v| p.array_objects(v)).unwrap_or_default().iter().map(|&o| if o != 0 { p.object_path(o) } else { "-".into() }).collect();
            let mut ov = Vec::new();
            for (l, cont) in cp.get("lodOverrideMaterialContainers").map(|v| p.array_structs(v)).unwrap_or_default().iter().enumerate() {
                for (k, el) in cont.get("lodOverrideElementContainers").map(|v| p.array_structs(v)).unwrap_or_default().iter().enumerate() {
                    let o = el.object("pOverrideMaterial").unwrap_or(0);
                    ov.push(format!("L{l}s{k}={}", if o != 0 { p.object_path(o) } else { "-".into() }));
                }
            }
            println!("{} {} {:?}\n   Materials {:?}\n   override {:?}", f.rsplit('/').next().unwrap(), p.object_path(i as i32 + 1), ap.vector("Location"), mats, ov);
        }
    }
}
