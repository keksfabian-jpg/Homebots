//! Dev: static mesh LOD loading summary (inline LOD found, separate LODs, full load via library).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let (mut n, mut fail, mut sep, mut full) = (0, 0, 0, 0);
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "StaticMesh" { continue; }
            n += 1;
            let Some(sm) = p.read_static_mesh(i) else { fail += 1; println!("FAIL {}", p.object_name_of(e)); continue };
            if sm.lod_index > 0 {
                sep += 1;
                match p.read_static_mesh_full(i, &lib) {
                    Some(fm) if fm.lod_index == 0 => { full += 1; println!("full {} {} -> {} verts {} sections", p.object_name_of(e), p.source_package(i), fm.lods[0].positions.len(), fm.lods[0].sections.len()); }
                    _ => println!("nofull {} (src {})", p.object_name_of(e), p.source_package(i)),
                }
            }
        }
    }
    println!("meshes {n} fail {fail} lod>0 {sep} loaded full {full}");
}
