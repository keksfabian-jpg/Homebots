//! Dev: object references (and names) found in a function's / state's script bytes (rough
//! UnrealScript reading: every aligned-or-not i32 that is a valid object index, every
//! (name index, 0) pair), by export path substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for i in 0..p.exports.len() {
        let path = p.object_path(i as i32 + 1);
        if !path.eq_ignore_ascii_case(&a[2]) { continue; }
        let b = p.export_bytes(&p.exports[i]);
        println!("== {path} [{}] {} bytes", p.export_class(&p.exports[i]), b.len());
        let mut out = Vec::new();
        let mut k = 0;
        while k + 4 <= b.len() {
            let v = i32::from_le_bytes(b[k..k + 4].try_into().unwrap());
            let obj = (v > 0 && (v as usize) <= p.exports.len()) || (v < 0 && ((-v) as usize) <= p.imports.len());
            if obj && v.abs() > 8 {
                out.push(format!("@{k} obj {}", p.object_path(v)));
            } else if v > 0 && (v as usize) < p.names.len() && k + 8 <= b.len() && i32::from_le_bytes(b[k + 4..k + 8].try_into().unwrap()) == 0 && v > 50 {
                out.push(format!("@{k} name {}", p.names[v as usize].name));
            }
            k += 1;
        }
        for o in out { println!("{o}"); }
    }
}
