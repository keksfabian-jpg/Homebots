//! Dev: static mesh placements near x,y (UE) whose section materials are unlit, translucent /
//! additive / modulate, or have an EmissiveColor input (night glow hunt, 2026-10-21).
//! Args: <x> <y> <radius> <package...>   (add HB_ALL=1 to list every material)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (x, y, r): (f32, f32, f32) = (a[1].parse().unwrap(), a[2].parse().unwrap(), a[3].parse().unwrap());
    let all = std::env::var_os("HB_ALL").is_some();
    for f in &a[4..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        let mut cache: std::collections::HashMap<i32, String> = Default::default();
        for pl in p.static_mesh_placements() {
            let d = (pl.location[0] - x).hypot(pl.location[1] - y);
            if d > r || pl.mesh <= 0 { continue; }
            let Some(m) = p.read_static_mesh(pl.mesh as usize - 1) else { continue };
            let Some(l) = m.lods.first() else { continue };
            for (k, s) in l.sections.iter().enumerate() {
                let mat = pl.materials.get(k).copied().filter(|&o| o != 0).unwrap_or(s.material);
                if mat == 0 { continue; }
                let desc = cache.entry(mat).or_insert_with(|| {
                    let Some(c) = p.compile_material(mat) else { return "nocompile".into() };
                    let em = c.code.lines().find(|l| l.contains("o.emissive =")).map(|l| l.trim().chars().take(140).collect::<String>()).unwrap_or_default();
                    format!("{} {:?} 2s={} | {}", c.lighting, c.info.blend, c.info.two_sided, em)
                }).clone();
                let interesting = !desc.starts_with("MLM_Phong Opaque") || !desc.contains("vec3<f32>(0.0)") ;
                if all || interesting {
                    println!("{d:.0} {} {} z{:.0} sec{k} {} :: {desc}", f.rsplit('/').next().unwrap(), p.object_path(pl.mesh), pl.location[2], p.object_path(mat));
                }
            }
        }
    }
}
