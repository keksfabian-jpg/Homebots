//! Dev: try a texture's separate-file top mip against candidate files (LZO stream at the offset).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "Texture2D" && p.object_name_of(e) == a[2]).unwrap();
    let t = p.read_texture2d(i).unwrap();
    let m = &t.mips[0];
    for f in &a[3..] {
        let lib = upk::library::Library { root: ".".into(), files: [("x".to_string(), std::path::PathBuf::from(f))].into() };
        match lib.read_separate("x", &m.bulk) {
            Some(d) => println!("{f}: {} bytes (want {})", d.len(), m.bulk.element_count),
            None => println!("{f}: fail"),
        }
    }
}
