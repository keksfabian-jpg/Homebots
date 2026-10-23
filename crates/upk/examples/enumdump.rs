//! Dev: names of an Enum export (in order).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "Enum" || p.object_name_of(e) != a[2] { continue; }
        let r = p.export_reader(i);
        let b = &r.b[r.pos..(r.pos + e.serial_size as usize).min(r.b.len())]; println!("pos {} size {} len {}", r.pos, e.serial_size, r.b.len());
        // scan for an i32 count followed by that many (name index, number) pairs
        for o in 0..b.len().saturating_sub(4) {
            let n = i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            if !(1..64).contains(&n) || o + 4 + n as usize * 8 > b.len() { continue; }
            let names: Option<Vec<String>> = (0..n as usize).map(|k| {
                let q = o + 4 + k * 8;
                let ni = i32::from_le_bytes(b[q..q + 4].try_into().unwrap());
                (ni >= 0 && (ni as usize) < p.names.len()).then(|| p.names[ni as usize].name.clone())
            }).collect();
            if let Some(v) = names { if v.iter().all(|x| x.starts_with("GCABRI") || x.contains('_')) { println!("{v:?}"); } }
        }
    }
}
