//! Dev: bounds of a static mesh (by object name) from its LOD 0 vertices.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "StaticMesh" || p.object_name_of(e) != a[2] { continue; }
        let m = p.read_static_mesh(i).unwrap();
        println!("origin {:?} extent {:?} radius {}", m.origin, m.extent, m.radius);
        let l = &m.lods[0];
        println!("{} verts {} indices", l.positions.len(), l.indices.len());
        for (k, v) in l.positions.iter().enumerate().take(64) { println!("{k}: {v:?} uv {:?}", l.uvs.get(k)); }
    }
}
