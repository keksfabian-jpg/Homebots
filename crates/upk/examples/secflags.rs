//! Dev: per-section EnableCollision of the StaticMeshes used by road splines / placements whose
//! path contains a substring. Usage: secflags <map folder> <mesh substring>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut files: Vec<_> = std::fs::read_dir(&a[1]).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "map")).collect();
    files.sort();
    let mut seen = std::collections::HashSet::new();
    for f in files {
        let Ok(p) = upk::Package::open(&f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "StaticMesh" {
                continue;
            }
            let path = p.object_path(i as i32 + 1);
            if !path.to_ascii_lowercase().contains(&a[2].to_ascii_lowercase()) || !seen.insert(path.clone()) {
                continue;
            }
            let flags = p.static_mesh_section_collision(i);
            let mats: Vec<String> = p.read_static_mesh(i).map(|m| m.lods[0].sections.iter().map(|s| format!("{}:{}", p.object_path(s.material), s.num_triangles)).collect()).unwrap_or_default();
            println!("{path} ({}) collide {:?} sections {:?}", f.file_name().unwrap().to_string_lossy(), flags, mats);
        }
    }
}
