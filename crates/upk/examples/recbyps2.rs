//! Dev: locate the entry (MaterialId, name, switches) that follows a shader id's list position.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let want = std::fs::read(&a[2]).unwrap();
    let idx = sc.entry_index();
    let d = &sc.data;
    for (g, e) in &sc.shaders {
        if d[e.code_offset..e.code_offset + e.code_len] != want[..] { continue; }
        for &q in idx.get(g).map(|v| v.as_slice()).unwrap_or(&[]) {
            let mut s = q + 24;
            while s + 24 < d.len() && s < q + 2_000_000 {
                let l = u32::from_le_bytes(d[s + 16..s + 20].try_into().unwrap()) as usize;
                if (4..300).contains(&l) && s + 20 + l <= d.len() && d[s + 20 + l - 1] == 0 && d[s + 20].is_ascii_alphabetic() && d[s + 20..s + 20 + l - 1].iter().all(|c| (32..127).contains(c)) {
                    let name = String::from_utf8_lossy(&d[s + 20..s + 20 + l - 1]).to_string();
                    let id: String = d[s..s + 16].iter().map(|b| format!("{b:02x}")).collect();
                    let sw = sc.switches_at(s + 20 + l);
                    let mut idb = [0u8; 16]; idb.copy_from_slice(&d[s..s + 16]);
                    println!("pos {q}: id {id} {name} record_parse_ok {} switches {:?}", sc.record(&idb).is_some(), sw.map(|v| v.iter().map(|x| (x.0.clone(), x.1, x.2)).collect::<Vec<_>>()));
                    break;
                }
                s += 1;
            }
        }
    }
}
