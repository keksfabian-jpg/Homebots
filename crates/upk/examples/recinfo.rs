//! Dev: base id and switches of records given by id (hex).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    for h in &a[2..] {
        let id: [u8; 16] = (0..16).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect::<Vec<_>>().try_into().unwrap();
        let Some(r) = sc.record(&id) else { println!("{h}: no record"); continue };
        let b: String = r.base_id.iter().map(|b| format!("{b:02x}")).collect();
        println!("{h} {} base {b} {:?}", r.friendly_name, r.switches.iter().map(|s| (s.0.as_str(), s.1, s.2)).collect::<Vec<_>>());
    }
}
