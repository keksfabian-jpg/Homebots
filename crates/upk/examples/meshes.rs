//! Dev tool: parse all StaticMeshes in a package, report stats, optionally write one OBJ.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let (mut ok, mut fail) = (0, 0);
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "StaticMesh" { continue; }
        let name = p.object_name_of(e);
        match p.read_static_mesh(i) {
            Some(m) if !m.lods.is_empty() => {
                ok += 1;
                let l = &m.lods[0];
                if a.get(2) == Some(&name) {
                    let mut s = String::new();
                    for v in &l.positions { s += &format!("v {} {} {}\n", v[0], v[2], -v[1]); }
                    for t in &l.uvs { s += &format!("vt {} {}\n", t[0], 1.0 - t[1]); }
                    for f in l.indices.chunks(3) { s += &format!("f {0}/{0} {1}/{1} {2}/{2}\n", f[0] + 1, f[1] + 1, f[2] + 1); }
                    std::fs::write(&a[3], s).unwrap();
                    println!("{name}: {} verts {} tris {} sections, uv0 {:?}", l.positions.len(), l.indices.len() / 3, l.sections.len(), &l.uvs[..3.min(l.uvs.len())]);
                }
            }
            _ => { fail += 1; eprintln!("fail: {}", p.object_path(i as i32 + 1)); }
        }
    }
    println!("static meshes ok {ok} fail {fail}");
}
