//! Dev: materials of the given packages whose compiled graph samples a cube map or uses
//! MaterialTintColor (1P weapon shine at night, 2026-10-21), with their spec power.
fn main() {
    for f in std::env::args().skip(1) {
        let Ok(p) = upk::Package::open(&f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            let cls = p.export_class(e);
            if cls != "Material" && cls != "MaterialInstanceConstant" { continue; }
            let Some(c) = p.compile_material(i as i32 + 1) else { continue };
            let cube = c.code.contains("hf_env_cube");
            let tint = c.code.contains("hf_material_tint");
            let spec = c.code.lines().find(|l| l.contains("o.specular =")).unwrap_or("").trim().to_string();
            let emis = c.code.lines().find(|l| l.contains("o.emissive =")).unwrap_or("").trim().to_string();
            println!("{} | {} | cube {cube} tint {tint} | {spec} | {emis}", f.rsplit('/').next().unwrap(), p.object_path(i as i32 + 1));
        }
    }
}
