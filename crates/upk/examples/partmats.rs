//! Dev: every sprite emitter of every ParticleSystem in the given packages with its material's
//! lighting model and blend mode (night classification of particle materials, 2026-10-21).
//! Args: <game root> <package name...>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let defs = upk::particles::ClassDefaults::new(&lib);
    for pk in &a[2..] {
        let Some(p) = lib.open(pk) else { continue };
        let mut cache: std::collections::HashMap<i32, String> = Default::default();
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "ParticleSystem" { continue; }
            let Some(ems) = p.read_particle_system(i, &defs) else { continue };
            for em in ems.iter().filter(|e| !e.mesh && e.material != 0) {
                let d = cache.entry(em.material).or_insert_with(|| match p.compile_material(em.material) {
                    Some(c) => format!("{} {:?}", c.lighting, c.info.blend),
                    None => "nocompile".into(),
                });
                let mx = |r: &upk::particles::RawDist| r.lut.get(2..).map(|v| v.iter().cloned().fold(0.0f32, f32::max)).unwrap_or(0.0).max(r.param.as_ref().map(|q| q.1.iter().cloned().fold(0.0f32, f32::max)).unwrap_or(0.0));
                let (mut base, mut scale) = (1.0f32, 1.0f32);
                for m in &em.modules {
                    match m {
                        upk::particles::Module::Color { color, .. } | upk::particles::Module::ColorOverLife { color, .. } => base = mx(color),
                        upk::particles::Module::ColorScaleOverLife { color, .. } => scale *= mx(color),
                        _ => {}
                    }
                }
                println!("{pk}.{} | {} | {} | {d} | col {:.2}", p.object_path(i as i32 + 1), em.name, p.object_path(em.material), base * scale);
            }
        }
    }
}
