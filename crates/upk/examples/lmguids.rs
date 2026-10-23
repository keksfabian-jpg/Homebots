//! Dev: which lights the 2D light maps of StaticMeshComponents were baked from (LightGuids),
//! with the light's class, owner and settings. Args: packages (all sublevels of a map).
use upk::props::Value;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut lights: std::collections::HashMap<Vec<u8>, String> = Default::default();
    let mut refs: std::collections::BTreeMap<Vec<u8>, usize> = Default::default();
    let mut ngu = std::collections::BTreeMap::new();
    for f in &a[1..] {
        let p = upk::Package::open(f).unwrap();
        let pk = std::path::Path::new(f).file_name().unwrap().to_string_lossy().to_string();
        for (i, e) in p.exports.iter().enumerate() {
            let c = p.export_class(e);
            if c.ends_with("LightComponent") {
                let Some(pr) = p.export_props(i) else { continue };
                let s: Vec<String> = pr.0.iter().filter(|q| !matches!(q.name.as_str(), "LightGuid" | "LightmapGuid" | "Function" | "LightAffectsClassification")).map(|q| format!("{}={:?}", q.name, match &q.value { Value::Raw { data, .. } => format!("raw{}", data.len()), v => format!("{v:?}") })).collect();
                for k in ["LightGuid", "LightmapGuid"] {
                    if let Some(Value::Raw { data, .. }) = pr.get(k) { lights.entry(data.clone()).or_insert(format!("{pk} {c} {} ({k}) {}", p.object_path(i as i32 + 1), s.join(" "))); }
                }
            }
            if c != "StaticMeshComponent" { continue; }
            let mut r = p.export_reader(i);
            if p.read_props(&mut r).is_none() { continue; }
            let nlod = r.i32().unwrap_or(0);
            if nlod <= 0 { continue; }
            let _ = (|| -> Option<()> {
                let ns = r.i32()?; if ns != 0 { return None; }
                let nv = r.i32()?; if nv != 0 { return None; }
                let ty = r.i32()?; if ty != 2 { return None; }
                let _u = r.i32()?;
                let ng = r.i32()?;
                *ngu.entry(ng).or_insert(0) += 1;
                for _ in 0..ng { let g = r.bytes(16)?.to_vec(); *refs.entry(g).or_insert(0) += 1; }
                Some(())
            })();
        }
    }
    println!("guid counts per component: {ngu:?}");
    for (g, n) in &refs { println!("{n:5} {}", lights.get(g).cloned().unwrap_or_else(|| format!("UNKNOWN {:02x?}", g))); }
}
