//! Dev: glyph table of a font as "char\tpage\tu\tv\tw\th\ttexW\ttexH" lines (for decoding traces).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "Font").unwrap();
    let f = p.read_font(i).unwrap();
    let sizes: Vec<(f32, f32)> = f.textures.iter().map(|&t| {
        let pr = if t > 0 { p.export_props(t as usize - 1) } else { None };
        let g = |n: &str| pr.as_ref().and_then(|x| match x.get(n) { Some(upk::props::Value::Int(v)) => Some(*v as f32), _ => None }).unwrap_or(0.0);
        (g("SizeX"), g("SizeY"))
    }).collect();
    for cp in (32u32..0x2000).chain([0x2026]) {
        let Some(c) = char::from_u32(cp) else { continue };
        if let Some(g) = f.glyph(c) {
            let (tw, th) = sizes.get(g.tex as usize).copied().unwrap_or((0.0, 0.0));
            println!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", cp, g.tex, g.u, g.v, g.w, g.h, tw, th, g.voff);
        }
    }
}
