//! Dev: match captured D3D9 shader blobs (apitrace dump) to shader cache entries and the material
//! record (friendly name + vertex factory) that owns them.
use std::collections::HashMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let mut by_code: HashMap<&[u8], Vec<[u8; 16]>> = HashMap::new();
    for (g, _) in &sc.shaders {
        by_code.entry(sc.code(g).unwrap()).or_default().push(*g);
    }
    let idx = sc.entry_index();
    eprintln!("{} indexed", idx.len());
    for dir in &a[2..] {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let b = std::fs::read(&p).unwrap();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            match by_code.get(&b[..]) {
                Some(gs) => {
                    for g in gs {
                        let ty = &sc.shaders[g].type_name;
                        let owners = sc.owners(&idx, g);
                        println!("{name}\t{ty}\t{}\t{}", hex(g), owners.iter().map(|(n, vf)| format!("{n}|{vf}")).collect::<Vec<_>>().join(";"));
                    }
                }
                None => println!("{name}\t?"),
            }
        }
    }
}
fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
