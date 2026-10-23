//! Dev: owner (outer) class of light-mapped StaticMeshComponents. Args: packages.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for f in &a[1..] {
        let p = upk::Package::open(f).unwrap();
        let mut h = std::collections::BTreeMap::new();
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "StaticMeshComponent" { continue; }
            let lm = p.component_lightmaps(i);
            if !lm.iter().any(|l| l.is_some()) { continue; }
            let outer = e.outer_index;
            let oc = if outer > 0 { p.export_class(&p.exports[outer as usize - 1]) } else { "-".into() };
            *h.entry(oc).or_insert(0) += 1;
        }
        println!("{f}: {h:?}");
    }
}
