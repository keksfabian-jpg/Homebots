//! Dev: decode the largest mip available inside the package of a named Texture2D to PNG.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "Texture2D" || p.object_name_of(e) != a[2] { continue; }
        let t = p.read_texture2d(i).unwrap();
        match t.decode_rgba() {
            Some((w, h, px)) => { image::save_buffer(&a[3], &px, w, h, image::ColorType::Rgba8).unwrap(); println!("#{} {w}x{h} src {}", i + 1, p.source_package(i)); }
            None => println!("#{} no decodable mip, src {}", i + 1, p.source_package(i)),
        }
    }
}
