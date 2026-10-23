//! Dev: every StaticMesh of the given packages with its section materials (one line each).
fn main() {
    for f in std::env::args().skip(1) {
        let Ok(p) = upk::Package::open(&f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "StaticMesh" { continue; }
            let Some(m) = p.read_static_mesh(i) else { continue };
            let Some(l) = m.lods.first() else { continue };
            let mats: Vec<String> = l.sections.iter().map(|s| if s.material != 0 { p.object_path(s.material) } else { "-".into() }).collect();
            println!("{} | {} | {}", f.rsplit('/').next().unwrap(), p.object_path(i as i32 + 1), mats.join(", "));
        }
    }
}
