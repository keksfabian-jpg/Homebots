//! Dev: raw bytes of an export by path, with name-table lookups for 8-byte name refs.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for i in 0..p.exports.len() {
        if !p.object_path(i as i32 + 1).eq_ignore_ascii_case(&a[2]) { continue; }
        let b = p.export_bytes(&p.exports[i]);
        for (k, c) in b.chunks(16).enumerate() {
            println!("{:4}: {}", k * 16, c.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "));
        }
        println!("names: {}", p.names.len());
    }
    if a.len() > 3 { for n in a[3].split(',') { let i: usize = n.parse().unwrap(); println!("name {i} = {}", p.names.get(i).map(|x| x.name.as_str()).unwrap_or("?")); } }
}
