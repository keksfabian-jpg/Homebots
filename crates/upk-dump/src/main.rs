//! CLI: dump a UE3 package's summary and an export-class histogram.
use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: upk-dump <package> [--exports]");
    let list = std::env::args().any(|a| a == "--exports");
    let p = upk::Package::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let s = &p.summary;
    println!(
        "{path}\n  ver {}/{} engine {} cooker {} compression {:#x} chunks {}\n  names {} imports {} exports {}",
        s.file_version, s.licensee_version, s.engine_version, s.cooker_version,
        s.compression_flags, s.chunks.len(), p.names.len(), p.imports.len(), p.exports.len()
    );
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    for e in &p.exports {
        *hist.entry(p.export_class(e)).or_default() += 1;
    }
    let mut v: Vec<_> = hist.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, n) in v.iter().take(25) {
        println!("  {n:6}  {k}");
    }
    if std::env::args().any(|a| a == "--raw") { raw(&p); }
    if list {
        for (i, e) in p.exports.iter().enumerate() {
            println!("{:5} {:24} {} ({} bytes)", i + 1, p.export_class(e), p.object_path(i as i32 + 1), e.serial_size);
        }
    }
}

#[allow(dead_code)]
pub fn raw(p: &upk::Package) {
    let o = p.summary.export_offset as usize;
    for row in p.data[o..o + 256].chunks(4).collect::<Vec<_>>().chunks(8) {
        println!("{}", row.iter().map(|c| format!("{:>10}", i32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect::<Vec<_>>().join(""));
    }
    println!("first names: {:?}", p.names.iter().take(12).map(|n| &n.name).collect::<Vec<_>>());
    for i in &p.imports[..4] { println!("imp {} {} {} outer {}", p.name(i.class_package), p.name(i.class_name), p.name(i.object_name), i.outer_index); }
}
