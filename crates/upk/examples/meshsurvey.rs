//! Dev: PersistentLevel actors with a mesh component, by class: total / resolved mesh / import archetype.
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m: BTreeMap<String, [u32; 3]> = BTreeMap::new();
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if !p.object_path(i as i32 + 1).starts_with("TheWorld.PersistentLevel.") { continue; }
            let class = p.export_class(e);
            let Some(ap) = p.export_props(i) else { continue };
            for (key, mk) in [("StaticMeshComponent", "StaticMesh"), ("SkeletalMeshComponent", "SkeletalMesh"), ("BaseStaticMesh", "StaticMesh")] {
                let Some(c) = ap.object(key).filter(|&c| c > 0) else { continue };
                let Some(cp) = p.export_props(c as usize - 1) else { continue };
                let ent = m.entry(format!("{class}.{key}")).or_default();
                ent[0] += 1;
                if cp.object(mk).filter(|&x| x != 0).is_some() { ent[1] += 1; }
                let mut arch = p.exports[c as usize - 1].archetype_index;
                for _ in 0..8 { if arch <= 0 { break; } arch = p.exports[arch as usize - 1].archetype_index; }
                if arch < 0 && !p.object_path(arch).contains("Default__") { ent[2] += 1; }
            }
        }
    }
    for (k, v) in m { println!("{k}: {} resolved {} importarch {}", v[0], v[1], v[2]); }
}
