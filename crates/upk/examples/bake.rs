//! Dev: bake materials to PNG via graph evaluation.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let lib = upk::library::Library::scan(&a[2]).unwrap_or(upk::library::Library { root: ".".into(), files: Default::default() });
    let mut cache = upk::material::TexCache::new();
    for name in &a[4..] {
        let i = p.exports.iter().position(|e| &p.object_name_of(e) == name && p.export_class(e).contains("Material")).unwrap();
        let t = std::time::Instant::now();
        match p.bake_material(&lib, i as i32 + 1, 512, &mut cache) {
            Some(b) => {
                image::save_buffer(format!("{}/{name}.png", a[3]), &b.rgba, b.width, b.height, image::ColorType::Rgba8).unwrap();
                println!("{name}: {}x{} {:?} unlit={} in {:?}", b.width, b.height, b.info.blend, b.info.unlit, t.elapsed());
            }
            None => println!("{name}: bake failed"),
        }
    }
}
