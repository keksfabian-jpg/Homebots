//! Dev: StaticMesh LightMapCoordinateIndex / LightMapResolution histogram (serialized props).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut h = std::collections::BTreeMap::new();
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            let c = p.export_class(e);
            if c != "StaticMesh" && !p.object_path(i as i32 + 1).starts_with("Default__StaticMesh") { continue; }
            let Some(pr) = p.export_props(i) else { continue };
            *h.entry(format!("{c} LightMapCoordinateIndex={:?}", pr.int("LightMapCoordinateIndex"))).or_insert(0) += 1;
        }
    }
    for (k, v) in h { println!("{v:6} {k}"); }
}
