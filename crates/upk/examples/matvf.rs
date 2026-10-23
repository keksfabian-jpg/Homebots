//! Dev: vertex factories / shader types of a material's shader map; dump shaders whose VF matches.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let p = upk::Package::open(&a[2]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[3]).unwrap();
    let (id, _) = p.material_resource(i as i32 + 1, &|g| sc.is_record(g)).unwrap();
    let rec = sc.record(&id).unwrap();
    println!("{}", rec.friendly_name);
    for m in &rec.maps {
        println!("VF {}", m.vertex_factory);
        for (t, g) in &m.shaders {
            println!("  {t}");
            if a.len() > 5 && m.vertex_factory.contains(&a[4]) {
                std::fs::write(format!("{}/{}_{}.bin", a[5], m.vertex_factory, t), sc.code(g).unwrap()).unwrap();
            }
        }
    }
}
