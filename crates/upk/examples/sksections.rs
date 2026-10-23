//! Dev: sections and the vertex range each uses, for SkeletalMesh exports matching a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for i in 0..p.exports.len() {
        if p.export_class(&p.exports[i]) != "SkeletalMesh" || !p.object_path(i as i32 + 1).contains(&a[2]) { continue }
        let Some(m) = p.read_skeletal_mesh(i) else { continue };
        let secs: Vec<String> = m.lod.sections.iter().map(|s| {
            let idx = &m.lod.indices[s.first_index as usize..(s.first_index + s.num_triangles * 3) as usize];
            let mut u: Vec<u32> = idx.to_vec(); u.sort(); u.dedup();
            format!("{} tris {} verts", s.num_triangles, u.len())
        }).collect();
        println!("{} verts {}: {:?}", p.object_path(i as i32 + 1), m.lod.positions.len(), secs);
    }
}
