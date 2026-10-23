//! Dev: StaticMeshActors that static_mesh_placements skips, with the reason.
use std::collections::BTreeMap;
fn main() {
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for m in std::env::args().skip(1) {
        let p = upk::Package::open(&m).unwrap();
        for (i, e) in p.exports.iter().enumerate() {
            let class = p.export_class(e);
            if !class.contains("StaticMeshActor") && class != "InterpActor" { continue; }
            if !p.object_path(i as i32 + 1).starts_with("TheWorld") { continue; }
            let ap = p.export_props(i).unwrap_or_default();
            let r = if matches!(ap.get("bHidden"), Some(upk::props::Value::Bool(true))) { "hidden".to_string() }
            else if let Some(c) = ap.object("StaticMeshComponent").filter(|&c| c > 0) {
                let cp = p.export_props(c as usize - 1).unwrap_or_default();
                match cp.object("StaticMesh") { Some(m) if m > 0 => continue, Some(m) => format!("mesh {}", p.object_path(m)), None => "no mesh prop".into() }
            } else { format!("no component ({class})") };
            *reasons.entry(r).or_default() += 1;
        }
    }
    println!("{reasons:?}");
}
