//! Dev: write a texture's resident (inline) mip data of a given size to a file.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "Texture2D" && p.object_name_of(e) == a[2]).unwrap();
    let t = p.read_texture2d(i).unwrap();
    let w: u32 = a[3].parse().unwrap();
    let m = t.mips.iter().find(|m| m.width == w && !m.bulk.data.is_empty()).unwrap();
    std::fs::write(&a[4], &m.bulk.data).unwrap();
}
