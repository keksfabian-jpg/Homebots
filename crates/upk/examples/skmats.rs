//! Dev: skeletal mesh material slots and sections (with bounds of each section's vertices).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for n in &a[2..] {
        let i = p.exports.iter().position(|e| p.object_name_of(e) == *n && p.export_class(e) == "SkeletalMesh").unwrap();
        let m = p.read_skeletal_mesh(i).unwrap();
        println!("== {n}");
        for (k, &o) in m.materials.iter().enumerate() { println!("  mat {k}: {}", if o != 0 { p.object_path(o) } else { "None".into() }); }
        for s in &m.lod.sections {
            let idx = &m.lod.indices[s.first_index as usize..(s.first_index + s.num_triangles * 3) as usize];
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for &v in idx { let q = m.lod.positions[v as usize]; for c in 0..3 { lo[c] = lo[c].min(q[c]); hi[c] = hi[c].max(q[c]); } }
            println!("  section mat {} tris {} bounds {:?}..{:?}", s.material, s.num_triangles, lo.map(|x| (x * 10.0).round() / 10.0), hi.map(|x| (x * 10.0).round() / 10.0));
        }
    }
}
