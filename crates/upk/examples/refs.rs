//! Dev: find exports whose serialized data contains a reference (i32) to objects
//! whose path contains a given substring. Prints export, class and offsets.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let mut targets = Vec::new();
    for i in 0..p.exports.len() { if p.object_path(i as i32 + 1).contains(&a[2]) { targets.push(i as i32 + 1); } }
    for i in 0..p.imports.len() { if p.object_path(-(i as i32) - 1).contains(&a[2]) { targets.push(-(i as i32) - 1); } }
    println!("targets: {:?}", targets.iter().map(|t| (t, p.object_path(*t), if *t > 0 { p.export_class(&p.exports[*t as usize - 1]) } else { String::from("import") })).collect::<Vec<_>>());
    let mut by_class = std::collections::BTreeMap::new();
    for (i, e) in p.exports.iter().enumerate() {
        let b = p.export_bytes(e);
        let hits: Vec<usize> = (0..b.len().saturating_sub(4)).step_by(1).filter(|&o| targets.contains(&i32::from_le_bytes(b[o..o + 4].try_into().unwrap()))).collect();
        if !hits.is_empty() {
            let c = by_class.entry(p.export_class(e)).or_insert((0, String::new()));
            c.0 += 1;
            if c.1.is_empty() { c.1 = format!("#{} {} @{:?} (size {})", i + 1, p.object_path(i as i32 + 1), &hits[..hits.len().min(4)], e.serial_size); }
        }
    }
    for (k, v) in by_class { println!("{:5} {k:30} e.g. {}", v.0, v.1); }
}
