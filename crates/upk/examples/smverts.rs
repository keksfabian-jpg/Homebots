//! Dev: vertex positions and triangle count of a StaticMesh (by object name) in a package.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[2]).unwrap();
    let sm = p.read_static_mesh(i).unwrap();
    let lod = &sm.lods[0];
    println!("{} verts, {} indices, sections {}", lod.positions.len(), lod.indices.len(), lod.sections.len());
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &lod.positions { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
    println!("bounds {lo:?} .. {hi:?}");
    for (k, t) in lod.indices.chunks(3).enumerate().take(a.get(3).and_then(|x| x.parse().ok()).unwrap_or(40)) {
        let v: Vec<[f32; 3]> = t.iter().map(|&j| lod.positions[j as usize]).collect();
        println!("tri {k}: {:?}", v.iter().map(|p| format!("({:.2},{:.2},{:.2})", p[0], p[1], p[2])).collect::<Vec<_>>());
    }
}
