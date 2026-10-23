//! Dev: skeletal mesh LOD0 triangles per section (bind pose, mesh space) as text:
//! "S <section> <material path>" then "T x y z x y z x y z" lines. Args: <pkg> <mesh name>.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(i).unwrap();
    for (si, s) in m.lod.sections.iter().enumerate() {
        let mi = m.lod0_material_map.get(s.material as usize).copied().unwrap_or(s.material);
        let o = m.materials.get(mi as usize).copied().unwrap_or(0);
        println!("S {si} {} {}", s.material, if o != 0 { p.object_path(o) } else { "None".into() });
        let idx = &m.lod.indices[s.first_index as usize..(s.first_index + s.num_triangles * 3) as usize];
        for t in idx.chunks(3) {
            let q: Vec<String> = t.iter().flat_map(|&v| m.lod.positions[v as usize].iter().map(|c| format!("{c:.1}")).collect::<Vec<_>>()).collect();
            println!("T {}", q.join(" "));
        }
    }
}
