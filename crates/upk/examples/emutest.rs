//! Dev: translate a material's base-pass shaders from the shader cache to WGSL and validate with naga.
use upk::sm3::{Bindings, Shader};
struct B;
impl Bindings for B {
    fn constant(&self, reg: u32, name: Option<&str>) -> String {
        format!("vec4<f32>(0.0) /* c{reg} {} */", name.unwrap_or("?"))
    }
    fn sampler(&self, reg: u32, _name: Option<&str>) -> (String, String) {
        (format!("tex{reg}"), format!("smp{reg}"))
    }
}
fn module(s: &Shader, t: &upk::sm3::Translated) -> String {
    let mut m = String::new();
    for (r, k) in &s.samplers {
        let ty = match k { upk::sm3::SamplerKind::Cube => "texture_cube<f32>", _ => "texture_2d<f32>" };
        m += &format!("@group(0) @binding({}) var tex{r}: {ty};\n@group(0) @binding({}) var smp{r}: sampler;\n", r * 2, r * 2 + 1);
    }
    m += "var<private> hf_c: array<vec4<f32>, 256>;\n";
    let ins: Vec<String> = s.inputs.iter().enumerate().map(|(k, d)| format!("@location({k}) v{}: vec4<f32>", d.reg)).collect();
    if s.pixel {
        m += &format!("@fragment fn main({}) -> @location(0) vec4<f32> {{\n var oc0: vec4<f32>; var o_depth: vec4<f32>; let vpos = vec4<f32>(0.0); let vface = vec4<f32>(1.0);\n{}{}  return oc0;\n}}\n", ins.join(", "), t.declarations, t.body);
    } else {
        let mut outs = String::new();
        for d in &s.outputs { outs += &format!(" var o{}: vec4<f32>;", d.reg); }
        m += &format!("@vertex fn main({}) -> @builtin(position) vec4<f32> {{\n{outs} var o_pos: vec4<f32>;\n{}{}  return o6;\n}}\n", ins.join(", "), t.declarations, t.body);
    }
    m
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    eprintln!("{} shaders", sc.shaders.len());
    let mut id = [0u8; 16];
    for i in 0..16 { id[i] = u8::from_str_radix(&a[2][2 * i..2 * i + 2], 16).unwrap(); }
    let rec = sc.record(&id).unwrap();
    eprintln!("{}: {} maps", rec.friendly_name, rec.maps.len());
    let vf = a.get(3).map(|s| s.as_str()).unwrap_or("FLocalVertexFactory");
    for ty in ["TBasePassVertexShaderFDirectionalLightLightMapPolicyFNoDensityPolicy", "TBasePassPixelShaderFDirectionalLightLightMapPolicySkyLight"] {
        let Some(g) = rec.shader(vf, ty) else { eprintln!("missing {ty}"); continue };
        let code = sc.code(&g).unwrap();
        let s = Shader::parse(code).unwrap();
        let t = upk::sm3::translate(&s, &B);
        let m = module(&s, &t);
        match naga::front::wgsl::parse_str(&m) {
            Ok(module) => {
                let mut v = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
                match v.validate(&module) {
                    Ok(_) => eprintln!("{ty}: OK ({} bytes wgsl)", m.len()),
                    Err(e) => { eprintln!("{ty}: validation {e:?}"); println!("{m}"); }
                }
            }
            Err(e) => { eprintln!("{ty}: parse {}", e.emit_to_string(&m)); println!("{m}"); }
        }
        if a.len() > 4 { println!("{m}"); }
    }
}
