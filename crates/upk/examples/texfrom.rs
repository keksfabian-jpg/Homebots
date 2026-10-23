//! Dev: decode a Texture2D whose separate-file mips live in a given file (tests where the
//! streamed mips of a cooked texture are). usage: texfrom <pkg> <name> <bulk file> <out.png>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "Texture2D" && p.object_name_of(e) == a[2]).unwrap();
    let mut t = p.read_texture2d(i).unwrap();
    let src = p.source_package(i);
    let lib = upk::library::Library { root: ".".into(), files: [(src.to_lowercase(), std::path::PathBuf::from(&a[3]))].into() };
    t.load_full(&lib, &src, 4096);
    match t.decode_rgba() {
        Some((w, h, px)) => { image::save_buffer(&a[4], &px, w, h, image::ColorType::Rgba8).unwrap(); println!("{w}x{h} (source {src})"); }
        None => println!("no decode"),
    }
}
