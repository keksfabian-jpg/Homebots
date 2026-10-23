//! Dev: glyph rects of a string with a font (scale, spacing) for comparison with traces.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "Font").unwrap();
    let f = p.read_font(i).unwrap();
    println!("chars {} tex {:?} remapped {} kerning {} remap {}", f.chars.len(), f.textures.iter().map(|&t| p.object_name(t)).collect::<Vec<_>>(), f.is_remapped, f.kerning, f.remap.len());
    let mh = f.chars.iter().map(|g| g.h).max().unwrap_or(0); let mvh = f.chars.iter().map(|g| g.h + g.voff).max().unwrap_or(0); println!("max h {mh} max h+voff {mvh}");
    let scale: f32 = a[4].parse().unwrap();
    let spacing: f32 = a[5].parse().unwrap();
    let mut x: f32 = a[6].parse().unwrap();
    for c in a[3].chars() {
        let g = f.glyph(c).unwrap();
        println!("{c} u {} v {} w {} h {} tex {} voff {}  x {:.3}..{:.3}", g.u, g.v, g.w, g.h, g.tex, g.voff, x, x + g.w as f32 * scale);
        x += (g.w as f32 + spacing) * scale;
    }
}
// (appended) stats printed via env FONTSTATS
