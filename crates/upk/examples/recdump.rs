//! Dev: dump a shader cache record (by MaterialId hex): mesh maps, and CTAB/inputs of shaders
//! whose type contains the optional filter.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let mut id = [0u8; 16];
    for i in 0..16 { id[i] = u8::from_str_radix(&a[2][2 * i..2 * i + 2], 16).unwrap(); }
    let rec = sc.record(&id).unwrap();
    println!("{}", rec.friendly_name);
    let filt = a.get(3).cloned().unwrap_or_default();
    for m in &rec.maps {
        println!("VF {}", m.vertex_factory);
        for (t, g) in &m.shaders {
            println!("  {t} {}", g.iter().map(|x| format!("{x:02x}")).collect::<String>());
            if !filt.is_empty() && t.contains(&filt) {
                let s = upk::sm3::Shader::parse(sc.code(g).unwrap()).unwrap();
                for ((set, r), n) in &s.ctab { println!("      ctab {set}:{r} {n}"); }
                for d in &s.inputs { println!("      in {:?}{} v{} m{:x}", d.usage, d.index, d.reg, d.mask); }
                for (r, k) in &s.samplers { println!("      s{r} {k:?}"); }
                if a.len() > 4 {
                    let p = format!("{}/{}_{}.bin", a[4], t, g.iter().take(4).map(|x| format!("{x:02x}")).collect::<String>());
                    std::fs::write(&p, sc.code(g).unwrap()).unwrap();
                }
            }
        }
    }
}
