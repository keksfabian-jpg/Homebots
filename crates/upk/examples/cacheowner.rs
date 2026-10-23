//! Dev: for shader blob files, find identical cache shaders and the records (friendly name, VF) using them.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let idx = sc.entry_index();
    for f in &a[2..] {
        let b = std::fs::read(f).unwrap();
        for (g, e) in &sc.shaders {
            if sc.data[e.code_offset..e.code_offset + e.code_len] == b[..] {
                println!("{f}: {} {:?}", e.type_name, sc.owners(&idx, g));
            }
        }
    }
}
