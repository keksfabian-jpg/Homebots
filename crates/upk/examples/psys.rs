//! Dev: emitters of a ParticleSystem (loops, duration, rate, bursts, material).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let defs = upk::particles::ClassDefaults::new(&lib);
    for path in &a[2..] {
        let (pk, rest) = path.split_once('.').unwrap();
        let p = lib.open(pk).unwrap();
        let Some(i) = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "ParticleSystem" && p.object_path(i as i32 + 1).eq_ignore_ascii_case(rest)) else { println!("{path} missing"); continue };
        let ems = p.read_particle_system(i, &defs).unwrap_or_default();
        println!("== {path}: {} emitters", ems.len());
        for e in &ems { println!("    local {} align {} modules {:?}", e.local_space, e.screen_alignment, e.modules); println!("  trail {:?}", e.trail); println!("  {} loops {} dur {} delay {} rate {:?} bursts {:?} mat {} mesh {}", e.name, e.loops, e.duration, e.delay, e.rate, e.bursts, if e.material != 0 { p.object_path(e.material) } else { "-".into() }, e.mesh); }
    }
}
