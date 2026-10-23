//! Dev: compile every Material / MaterialInstanceConstant exported by the given packages and
//! list those whose WGSL reads vertex colour, screen / world position, or has stubbed nodes
//! (rainbow / gradient surfaces, 2026-10-21).
fn main() {
    for f in std::env::args().skip(1) {
        let Ok(p) = upk::Package::open(&f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            let cls = p.export_class(e);
            if cls != "Material" && cls != "MaterialInstanceConstant" { continue; }
            let Some(c) = p.compile_material(i as i32 + 1) else { continue };
            let mut tags = vec![];
            for k in ["m.vcolor", "m.screen", "m.world_pos", "m.depth", "m.camera_world"] {
                if c.code.contains(k) { tags.push(k.to_string()); }
            }
            if !c.stubbed.is_empty() { tags.push(format!("stub{:?}", c.stubbed)); }
            if tags.is_empty() { continue; }
            println!("{} | {} | {} {:?} | {}", f.rsplit('/').next().unwrap(), p.object_path(i as i32 + 1), c.lighting, c.info.blend, tags.join(" "));
        }
    }
}
