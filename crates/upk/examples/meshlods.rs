//! Dev: all LODs and sections of a static mesh.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "StaticMesh" || p.object_name_of(e) != a[2] { continue; }
        let m = p.read_static_mesh(i).unwrap();
        for (l, lod) in m.lods.iter().enumerate() {
            println!("LOD{l}: {} verts {} idx, uv sets {}", lod.positions.len(), lod.indices.len(), lod.uv_sets.len());
            for s in &lod.sections { println!("   {:?} {}", s, if s.material != 0 { p.object_path(s.material) } else { "-".into() }); }
        }
    }
}
