//! Dev: minimap placements (MinimapRenderMode) of map packages: count per mode, a few samples.
fn main() {
    for f in std::env::args().skip(1) {
        let Ok(p) = upk::Package::open(&f) else { continue };
        let v = p.minimap_placements();
        let mut m = std::collections::BTreeMap::new();
        for (_, mode) in &v { *m.entry(mode.clone()).or_insert(0) += 1; }
        println!("{}: {:?}", f.rsplit('/').next().unwrap(), m);
        for (pl, mode) in v.iter().take(3) { println!("   {mode} {} {:?} {:?}", p.object_path(pl.mesh), pl.location, pl.scale); }
    }
}
