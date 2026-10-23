//! Dev: for one material record (smallest code), print the FLocalVertexFactory base-pass VS/PS of
//! a light-map policy next to the FDirectionalLightLightMapPolicy pair (WGSL via upk::sm3).
//! Args: shadercache policy-substring (e.g. DirectionalLightWithSimpleLightMapTexturePolicy)
struct B;
impl upk::sm3::Bindings for B {
    fn constant(&self, reg: u32, name: Option<&str>) -> String { format!("C{reg}_{}", name.unwrap_or("?").replace(['[', ']'], "_")) }
    fn sampler(&self, reg: u32, name: Option<&str>) -> (String, String) { (format!("T{reg}_{}", name.unwrap_or("?")), format!("S{reg}")) }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let pol = &a[2];
    let mut best: Option<(usize, upk::shadercache::MaterialRecord)> = None;
    for id in sc.record_ids() {
        let Some(r) = sc.record(&id) else { continue };
        if let Some(f) = a.get(3) { if &r.friendly_name != f { continue } }
        let ps = format!("TBasePassPixelShaderF{pol}SkyLight");
        let Some(p) = r.shader("FLocalVertexFactory", &ps) else { continue };
        if r.shader("FLocalVertexFactory", "TBasePassPixelShaderFDirectionalLightLightMapPolicySkyLight").is_none() { continue }
        let n = sc.code(&p).map(|c| c.len()).unwrap_or(usize::MAX);
        if best.as_ref().is_none_or(|b| n < b.0) { best = Some((n, r)); }
    }
    let (_, r) = best.unwrap();
    println!("// record {}", r.friendly_name);
    for t in [format!("TBasePassVertexShaderF{pol}FNoDensityPolicy"), format!("TBasePassPixelShaderF{pol}SkyLight"), "TBasePassVertexShaderFDirectionalLightLightMapPolicyFNoDensityPolicy".into(), "TBasePassPixelShaderFDirectionalLightLightMapPolicySkyLight".into()] {
        let Some(id) = r.shader("FLocalVertexFactory", &t) else { println!("// {t}: none"); continue };
        let s = upk::sm3::Shader::parse(sc.code(&id).unwrap()).unwrap();
        println!("// ===== {t}\n// defs {:?}\n// inputs {:?}\n// ctab {:?}", s.defs, s.inputs, s.ctab);
        println!("{}", upk::sm3::translate(&s, &B).body);
    }
}
