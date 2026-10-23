// Lists the texture mips the match preload would read for a map folder and checks them.
use std::collections::HashMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = &a[1];
    let dir = &a[2];
    let max: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(2048);
    let lib = upk::library::Library::scan(root).unwrap();
    let mut missing: HashMap<String, usize> = HashMap::new();
    let (mut n, mut tot, mut big) = (0usize, 0u64, 0u64);
    let mut bad = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let Ok(pk) = upk::Package::open(&p) else { continue };
        for (i, ex) in pk.exports.iter().enumerate() {
            if pk.export_class(ex) != "Texture2D" { continue }
            let Some(tx) = pk.read_texture2d(i) else { continue };
            let src = pk.source_package(i);
            for m in &tx.mips {
                if m.width <= max && m.height <= max && m.bulk.data.is_empty() && m.bulk.flags & upk::texture::BULK_SEPARATE_FILE != 0 && m.bulk.flags & upk::texture::BULK_UNUSED == 0 {
                    match lib.path_of(&src) {
                        None => *missing.entry(src.clone()).or_default() += 1,
                        Some(f) => {
                            let len = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
                            let end = m.bulk.offset_in_file as u64 + m.bulk.size_on_disk as u64;
                            if end > len || m.bulk.offset_in_file < 0 { bad += 1; if bad < 3 || m.bulk.size_on_disk != u32::MAX as _ { println!("BAD {} {} off {} size {} flags {:#x} {}x{} len {len}", p.display(), src, m.bulk.offset_in_file, m.bulk.size_on_disk, m.bulk.flags, m.width, m.height) } }
                            n += 1; tot += m.bulk.size_on_disk as u64; big = big.max(m.bulk.size_on_disk as u64);
                        }
                    }
                }
            }
        }
    }
    println!("items {n} disk {:.1} MB max {big} bad {bad}; missing pkgs {} ({} items)", tot as f64 / 1e6, missing.len(), missing.values().sum::<usize>());
    let mut v: Vec<_> = missing.into_iter().collect(); v.sort_by_key(|x| std::cmp::Reverse(x.1));
    for (k, c) in v.iter().take(15) { println!("  missing {k}: {c}") }
}
