//! Dev: find which cache shader(s) match given shader blob files (by code bytes) and print records using them.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let mut by_code: std::collections::HashMap<&[u8], Vec<[u8; 16]>> = Default::default();
    for (g, e) in &sc.shaders { by_code.entry(&sc.data[e.code_offset..e.code_offset + e.code_len]).or_default().push(*g); }
    for f in &a[2..] {
        let b = std::fs::read(f).unwrap();
        match by_code.get(b.as_slice()) {
            Some(gs) => {
                for g in gs {
                    let e = &sc.shaders[g];
                    // records referencing this shader id
                    let mut users = vec![];
                    let mut from = 0;
                    while let Some(p) = sc.data[from..].windows(16).position(|w| w == g) {
                        let o = from + p; from = o + 1;
                        if o + 26 == e.code_offset { continue; }
                        // walk back to a friendly name
                        let mut q = o;
                        while q > o.saturating_sub(200000) {
                            q -= 1;
                            let l = u32::from_le_bytes(sc.data[q..q+4].try_into().unwrap()) as usize;
                            if (4..120).contains(&l) && sc.data.get(q + 4 + l - 1) == Some(&0) && sc.data[q+4].is_ascii_alphabetic() && sc.data[q+4..q+4+l-1].iter().all(|c| (32..127).contains(c)) {
                                users.push(String::from_utf8_lossy(&sc.data[q+4..q+4+l-1]).to_string()); break;
                            }
                        }
                    }
                    users.sort(); users.dedup();
                    println!("{f}: {} {:?}", e.type_name, users);
                }
            }
            None => println!("{f}: no match"),
        }
    }
}
