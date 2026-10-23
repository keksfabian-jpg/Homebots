//! Dev: per section vertex count by x side and y (height) range.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(i).unwrap();
    for (k, s) in m.lod.sections.iter().enumerate() {
        let idx = &m.lod.indices[s.first_index as usize..(s.first_index + s.num_triangles * 3) as usize];
        let mut v: Vec<u32> = idx.to_vec(); v.sort(); v.dedup();
        let neg = v.iter().filter(|&&j| m.lod.positions[j as usize][0] < 0.0).count();
        println!("sec {k} mat {} verts {} (x<0 {neg}) idx range {}..{} first {}", s.material, v.len(), v[0], v[v.len()-1], s.first_index);
    }
}
