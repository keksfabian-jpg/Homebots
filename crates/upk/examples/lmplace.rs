//! Dev: light-mapped StaticMesh placements per package (level.rs component_lightmaps) and the
//! light-map texture packages they import from. Args: packages.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mut tp, mut tl) = (0, 0);
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        let pl = p.static_mesh_placements();
        let mut lods = std::collections::BTreeMap::new();
        let mut pk = std::collections::BTreeMap::new();
        let mut with = 0;
        for x in &pl {
            *lods.entry(x.lightmaps.len()).or_insert(0) += 1;
            if x.lightmaps.iter().any(|l| l.is_some()) { with += 1; }
            for l in x.lightmaps.iter().flatten() {
                for &t in l.textures.iter().filter(|&&t| t != 0) {
                    let path = p.object_path(t);
                    *pk.entry(format!("{}{}", if t > 0 { "export " } else { "IMPORT " }, path.split('.').next().unwrap_or(""))).or_insert(0) += 1;
                }
                *pk.entry(if l.directional() { "mode HQ".into() } else { "mode LQ".into() }).or_insert(0) += 1;
                if l.directional() && l.textures[2] != 0 { *pk.entry("HQ+LQ".into()).or_insert(0) += 1; }
            }
        }
        tp += pl.len(); tl += with;
        if with > 0 || std::env::var_os("ALL").is_some() {
            println!("{}: placements {}, light-mapped {with}, LODData counts {lods:?}, texture packages {pk:?}", f.rsplit('/').next().unwrap(), pl.len());
        }
    }
    println!("total placements {tp}, light-mapped {tl}");
}
