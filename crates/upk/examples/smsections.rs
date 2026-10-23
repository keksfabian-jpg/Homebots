//! Dev: per-section material and vertex bounds of a static mesh (by object name), e.g. where
//! the glass / cab of a level truck is (levelcars.rs static trucks: engine end of the bounds).
//! Usage: smsections <package> <mesh name>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "StaticMesh" || p.object_name_of(e) != a[2] {
            continue;
        }
        let Some(m) = p.read_static_mesh(i) else { continue };
        println!("origin {:?} extent {:?}", m.origin, m.extent);
        let l = &m.lods[0];
        for (k, s) in l.sections.iter().enumerate() {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for t in 0..s.num_triangles as usize * 3 {
                let Some(&vi) = l.indices.get(s.first_index as usize + t) else { break };
                let v = l.positions[vi as usize];
                for c in 0..3 {
                    lo[c] = lo[c].min(v[c]);
                    hi[c] = hi[c].max(v[c]);
                }
            }
            println!("section {k}: {} tris, material {}, lo {lo:?} hi {hi:?}", s.num_triangles, p.object_path(s.material));
        }
    }
}
