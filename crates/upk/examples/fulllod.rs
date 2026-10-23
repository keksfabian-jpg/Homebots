//! Dev: stats of the full (source package) LOD of a static mesh.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let p = upk::Package::open(&a[2]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[3]).unwrap();
    for sm in [p.read_static_mesh(i).unwrap(), p.read_static_mesh_full(i, &lib).unwrap()] {
        let l = &sm.lods[0];
        let fin = |v: &[f32]| v.iter().all(|x| x.is_finite());
        let pmin = (0..3).map(|k| l.positions.iter().map(|p| p[k]).fold(f32::MAX, f32::min)).collect::<Vec<_>>();
        let pmax = (0..3).map(|k| l.positions.iter().map(|p| p[k]).fold(f32::MIN, f32::max)).collect::<Vec<_>>();
        println!("lod {} verts {} idx {} pos finite {} uv finite {} pmin {:?} pmax {:?} uv0 {:?}", sm.lod_index, l.positions.len(), l.indices.len(),
            l.positions.iter().all(|p| fin(p)), l.uv_sets.iter().all(|s| s.iter().all(|u| fin(u))), pmin, pmax, &l.uvs[..3]);
        for s in &l.sections { println!("   {:?}", s); }
    }
}
