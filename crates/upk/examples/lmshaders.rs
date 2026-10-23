//! Dev: shader types in the shader cache whose name mentions a light-map policy, with counts.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let mut c = std::collections::BTreeMap::new();
    for e in sc.shaders.values() { if e.type_name.contains("LightMap") || e.type_name.contains("ShadowMap") || e.type_name.contains("VertexLight") { *c.entry(e.type_name.clone()).or_insert(0) += 1; } }
    for (k, v) in c { println!("{v:6} {k}"); }
}
