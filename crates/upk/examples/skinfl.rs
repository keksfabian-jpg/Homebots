//! Dev: vertices per dominant bone of a SkeletalMesh (object name).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(i).unwrap();
    let mut c: std::collections::BTreeMap<String, (u32, [f32; 3], [f32; 3])> = std::collections::BTreeMap::new();
    for (vi, (b, w)) in m.influences.iter().enumerate() {
        let k = (0..4).max_by_key(|&k| w[k]).unwrap();
        let e = c.entry(m.bones.get(b[k] as usize).map(|x| x.name.clone()).unwrap_or(format!("#{}", b[k]))).or_insert((0, [f32::MAX; 3], [f32::MIN; 3]));
        e.0 += 1;
        let p = m.lod.positions[vi];
        for a in 0..3 { e.1[a] = e.1[a].min(p[a]); e.2[a] = e.2[a].max(p[a]); }
    }
    println!("{} verts", m.lod.positions.len());
    for (k, v) in c { println!("  {k}: {} lo {:?} hi {:?}", v.0, v.1.map(|x| x.round()), v.2.map(|x| x.round())); }
}
