//! Dev: collision flags of StaticMeshActor components (counts by flag combo, sample meshes).
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m: BTreeMap<String, (u32, Vec<String>)> = BTreeMap::new();
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if !p.object_path(i as i32 + 1).starts_with("TheWorld.PersistentLevel.") { continue; }
            let class = p.export_class(e);
            if class != "StaticMeshActor" && class != "InterpActor" && class != "GCStaticMeshActor" { continue; }
            let Some(ap) = p.export_props(i) else { continue };
            let Some(c) = ap.object("StaticMeshComponent").filter(|&c| c > 0) else { continue };
            let Some(cp) = p.export_props(c as usize - 1) else { continue };
            let b = |n: &str| match cp.get(n) { Some(upk::props::Value::Bool(v)) => if *v { "1" } else { "0" }, _ => "-" };
            let ab = |n: &str| match ap.get(n) { Some(upk::props::Value::Bool(v)) => if *v { "1" } else { "0" }, _ => "-" };
            let key = format!("{class} CollideActors {} BlockZeroExtent {} BlockNonZero {} BlockActors {} RB {} | actor bCollideActors {} bBlockActors {}", b("CollideActors"), b("BlockZeroExtent"), b("BlockNonZeroExtent"), b("BlockActors"), b("BlockRigidBody"), ab("bCollideActors"), ab("bBlockActors"));
            let mesh = cp.object("StaticMesh").map(|o| p.object_name(o)).unwrap_or_default();
            let ent = m.entry(key).or_default();
            ent.0 += 1;
            if ent.1.len() < 6 && !ent.1.contains(&mesh) { ent.1.push(mesh); }
        }
    }
    for (k, v) in m { println!("{} x {k}\n    {:?}", v.0, v.1); }
}
