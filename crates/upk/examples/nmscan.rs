//! Dev: textures flagged TC_Normalmap* whose LODGroup is not a NormalMap group and name ends _c.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "Texture2D" { continue; }
            let Some(pr) = p.export_props(i) else { continue };
            let cs = pr.enum_name("CompressionSettings").unwrap_or("");
            if !cs.starts_with("TC_Normalmap") { continue; }
            let g = pr.enum_name("LODGroup").unwrap_or("-");
            let n = p.object_name(i as i32 + 1);
            println!("{} {} {cs} {g} {}", f.rsplit('/').next().unwrap_or(""), p.object_path(i as i32 + 1), if !g.contains("NormalMap") && g != "-" && n.to_ascii_lowercase().ends_with("_c") { "<-- RULE" } else { "" });
        }
    }
}
