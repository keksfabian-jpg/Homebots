//! Dev: per sprite emitter of the named ParticleSystems (pkg.Group.Name): particle alpha range
//! (Color / ColorOverLife alpha with bClampAlpha, ColorScaleOverLife alpha), lifetime modules and
//! unhandled modules (black smoke squares, 2026-10-21).
//! Args: <game root> <system path...>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let defs = upk::particles::ClassDefaults::new(&lib);
    let mx = |r: &upk::particles::RawDist| {
        let v: Vec<f32> = r.lut.get(2..).map(|v| v.to_vec()).unwrap_or_default();
        let hi = v.iter().cloned().fold(f32::MIN, f32::max);
        let lo = v.iter().cloned().fold(f32::MAX, f32::min);
        format!("[{lo:.2}..{hi:.2}]{}", r.param.as_ref().map(|p| format!(" param {} {:?}", p.0, p.1)).unwrap_or_default())
    };
    for path in &a[2..] {
        let (pk, rest) = path.split_once('.').unwrap();
        let Some(p) = lib.open(pk) else { println!("{pk} not found"); continue };
        let Some(i) = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "ParticleSystem" && p.object_path(i as i32 + 1).eq_ignore_ascii_case(rest)) else { println!("{path} missing"); continue };
        println!("== {path}");
        for e in p.read_particle_system(i, &defs).unwrap_or_default().iter().filter(|e| !e.mesh && e.material != 0) {
            let mut s = Vec::new();
            for m in &e.modules {
                match m {
                    upk::particles::Module::Color { alpha, clamp_alpha, .. } => s.push(format!("Color a{} clamp {clamp_alpha}", mx(alpha))),
                    upk::particles::Module::ColorOverLife { alpha, clamp_alpha, .. } => s.push(format!("ColorOverLife a{} a(0) {:.2} clamp {clamp_alpha}", mx(alpha), alpha.float(0.0, &mut || 0.5))),
                    upk::particles::Module::ColorScaleOverLife { alpha, color, emitter_time } => s.push(format!("ColorScale c{} a{} et {emitter_time}", mx(color), mx(alpha))),
                    upk::particles::Module::Lifetime(d) => s.push(format!("Lifetime {}", mx(d))),
                    upk::particles::Module::Other(n) => s.push(n.to_string()),
                    _ => {}
                }
            }
            println!("  {} | {} | sub {}x{} {} | {}", e.name, p.object_path(e.material), e.sub_h, e.sub_v, e.interpolation, s.join("; "));
        }
    }
}
