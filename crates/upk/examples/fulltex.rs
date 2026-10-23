fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library { root: ".".into(), files: [(std::path::Path::new(&a[3]).file_stem().unwrap().to_string_lossy().to_lowercase(), std::path::PathBuf::from(&a[3]))].into() };
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "Texture2D" && p.object_name_of(e) == a[2]).unwrap();
    let mut t = p.read_texture2d(i).unwrap();
    t.load_full(&lib, &p.source_package(i), 4096);
    let (w, h, px) = t.decode_rgba().unwrap();
    image::save_buffer(&a[4], &px, w, h, image::ColorType::Rgba8).unwrap();
    println!("{w}x{h} from {}", p.source_package(i));
}
