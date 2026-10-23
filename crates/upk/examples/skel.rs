//! Dev: list skeletal mesh placements and decode their meshes.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for pl in p.skeletal_mesh_placements() {
        let path = p.object_path(pl.mesh);
        let info = if pl.mesh > 0 { p.read_skeletal_mesh(pl.mesh as usize - 1).map(|m| format!("{} verts {} idx {} secs uv{} rot {:?} origin {:?}", m.lod.positions.len(), m.lod.indices.len(), m.lod.sections.len(), m.lod.uv_sets.len(), m.rot_origin, m.origin)).unwrap_or("PARSE FAIL".into()) } else { "import".into() };
        println!("{path} @ {:?} {:?}: {info}", pl.location, pl.rotation);
    }
}
