//! Dev: sections of a static mesh with materials and their texture parameters / graph textures.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "StaticMesh" || p.object_name_of(e) != a[2] { continue; }
        let m = p.read_static_mesh(i).unwrap();
        for (k, s) in m.lods[0].sections.iter().enumerate() {
            println!("section {k}: material {} ({})", if s.material != 0 { p.object_path(s.material) } else { "-".into() }, s.material);
            if s.material > 0 {
                let mp = p.material_params(s.material);
                for (n, o) in &mp.textures { println!("     tex param {n} = {}", p.object_path(*o)); }
                if let Some(c) = p.compile_material(s.material) { for t in &c.textures { println!("     graph tex {:?}", t); } }
            }
        }
    }
}
