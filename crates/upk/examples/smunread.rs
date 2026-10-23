//! Dev: StaticMesh exports of map packages that `read_static_mesh` cannot read, and whether
//! `read_static_mesh_full` (all LODs separate bulk -> source package) loads them.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "StaticMesh" || p.read_static_mesh(i).is_some() {
                continue;
            }
            let full = p.read_static_mesh_full(i, &lib).map(|sm| format!("LOD {} {} verts", sm.lod_index, sm.lods[0].positions.len()));
            println!("{} {} (source {}): full {:?}", f.rsplit('/').next().unwrap_or(""), p.object_path(i as i32 + 1), p.source_package(i), full);
        }
    }
}
