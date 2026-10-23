//! Dev: dump a skeletal mesh (by object path) as JSON for the Blender import (local use only,
//! never committed: the output is game data). Positions in the mesh space (UE units), bones with
//! the local reference pose as the game composes it (viewmodel.rs comp_pose).
use std::fmt::Write;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let want = a[2].to_ascii_lowercase();
    for f in lib.cooked_files("int") {
        let Ok(p) = upk::Package::open(&f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "SkeletalMesh" || p.object_path(i as i32 + 1).to_ascii_lowercase() != want {
                continue;
            }
            let Some(m) = p.read_skeletal_mesh(i) else { continue };
            eprintln!("{} in {}: {} verts {} bones", a[2], f.display(), m.lod.positions.len(), m.bones.len());
            let mut s = String::from("{\"bones\":[");
            for (k, b) in m.bones.iter().enumerate() {
                let _ = write!(s, "{}{{\"name\":\"{}\",\"parent\":{},\"rot\":[{},{},{},{}],\"pos\":[{},{},{}]}}", if k > 0 { "," } else { "" }, b.name, b.parent, b.rot[0], b.rot[1], b.rot[2], b.rot[3], b.pos[0], b.pos[1], b.pos[2]);
            }
            s.push_str("],\"pos\":[");
            for (k, v) in m.lod.positions.iter().enumerate() {
                let _ = write!(s, "{}{:.4},{:.4},{:.4}", if k > 0 { "," } else { "" }, v[0], v[1], v[2]);
            }
            s.push_str("],\"uv\":[");
            for (k, v) in m.lod.uvs.iter().enumerate() {
                let _ = write!(s, "{}{:.5},{:.5}", if k > 0 { "," } else { "" }, v[0], v[1]);
            }
            s.push_str("],\"idx\":[");
            for (k, v) in m.lod.indices.iter().enumerate() {
                let _ = write!(s, "{}{}", if k > 0 { "," } else { "" }, v);
            }
            s.push_str("],\"inf\":[");
            for (k, (bi, w)) in m.influences.iter().enumerate() {
                let _ = write!(s, "{}{},{},{},{},{},{},{},{}", if k > 0 { "," } else { "" }, bi[0], bi[1], bi[2], bi[3], w[0], w[1], w[2], w[3]);
            }
            s.push_str("]}");
            std::fs::write(&a[3], s).unwrap();
            return;
        }
    }
    eprintln!("not found");
}
