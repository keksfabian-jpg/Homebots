//! Dev: bounds and section materials of a static mesh (by object path suffix), full LOD read.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let p = lib.open(&a[2]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "StaticMesh" || !p.object_path(i as i32 + 1).ends_with(&a[3]) { continue; }
        let Some(sm) = p.read_static_mesh_full(i, &lib) else { println!("{} unreadable", p.object_path(i as i32 + 1)); continue };
        let lod = &sm.lods[0];
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in &lod.positions { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
        println!("{} verts {} lo {lo:?} hi {hi:?}", p.object_path(i as i32 + 1), lod.positions.len());
        for s in &lod.sections { println!("  section mat {} {} tris {}", s.material, if s.material != 0 { p.object_path(s.material) } else { String::new() }, s.num_triangles); }
    }
}
