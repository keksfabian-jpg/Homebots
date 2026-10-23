//! Dev: records whose shader maps contain a given shader blob (by code), with their switches.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let want = std::fs::read(&a[2]).unwrap();
    for id in sc.record_ids() {
        let Some(r) = sc.record(&id) else { continue };
        let hit = r.maps.iter().any(|m| m.shaders.iter().any(|(_, g)| sc.code(g) == Some(&want[..])));
        if hit {
            let hex: String = id.iter().map(|b| format!("{b:02x}")).collect();
            println!("{hex} {} switches on {:?}", r.friendly_name, r.switches.iter().filter(|s| s.1).map(|s| (&s.0, s.2)).collect::<Vec<_>>());
            println!("   all {:?}", r.switches.iter().map(|s| (&s.0, s.1, s.2)).collect::<Vec<_>>());
        }
    }
}
