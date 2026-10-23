//! Dev: the light-map textures a sublevel's components use (from its lightmaps/*_sl.upk):
//! format, size, mips, SRGB, LODGroup, decode check, mean texel. Args: map, sl package.
use upk::props::Value;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let m = upk::Package::open(&a[1]).unwrap();

    let mut seen = std::collections::BTreeSet::new();
    for pl in m.static_mesh_placements() {
        for l in pl.lightmaps.iter().flatten() { for &t in &l.textures { if t > 0 { seen.insert(t as usize - 1); } } }
    }
    for path in seen {
        let i = path; let sl = &m; let path = m.object_path(i as i32 + 1);
        let cls = sl.export_class(&sl.exports[i]);
        let Some(mut t) = sl.read_texture2d(i) else { println!("{path} ({cls}): read_texture2d failed"); continue };
        if let Ok(lib) = upk::library::Library::scan(&a[2]) { t.load_full(&lib, &sl.source_package(i), std::env::var("MAXDIM").ok().and_then(|v| v.parse().ok()).unwrap_or(4096)); println!("source {}", sl.source_package(i)); }
        let srgb = t.props.get("SRGB").cloned();
        let lg = t.props.get("LODGroup").cloned();
        let mips: Vec<String> = t.mips.iter().map(|x| format!("{}x{}:{}", x.width, x.height, x.bulk.data.len())).collect();
        let fmt = t.format.clone();
        let dec = t.decode_rgba();
        let mean = dec.as_ref().map(|(w, h, px)| { let n = (*w * *h) as f64; let mut s = [0f64; 4]; for c in px.chunks(4) { for k in 0..4 { s[k] += c[k] as f64; } } s.map(|x| (x / n) as f32) });
        println!("{path} ({cls}) {fmt} mips {:?} SRGB {:?} LODGroup {:?} mean {:?}", &mips[..mips.len().min(3)], srgb.map(|v| matches!(v, Value::Bool(true))), lg, mean);
    }
}
