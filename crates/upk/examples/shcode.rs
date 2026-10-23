//! Dev: write the bytecode of cached shaders (by id hex) to <outdir>/<id>.bin.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    for h in &a[3..] {
        let mut id = [0u8; 16];
        for i in 0..16 { id[i] = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap(); }
        if let Some(e) = sc.shaders.get(&id) {
            std::fs::write(format!("{}/{h}.bin", a[2]), &sc.data[e.code_offset..e.code_offset + e.code_len]).unwrap();
        }
    }
}
