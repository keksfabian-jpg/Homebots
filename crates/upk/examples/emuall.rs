//! Dev: translate every shader in a cache and validate the WGSL with naga; print failures.
use upk::sm3::{Bindings, Shader};
struct B;
impl Bindings for B {
    fn constant(&self, _reg: u32, _name: Option<&str>) -> String { "vec4<f32>(0.0)".into() }
    fn sampler(&self, reg: u32, _name: Option<&str>) -> (String, String) { (format!("tex{reg}"), format!("smp{reg}")) }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let (mut ok, mut bad) = (0, 0);
    let mut errs: std::collections::HashMap<String, usize> = Default::default();
    let mut ex: std::collections::HashMap<String, String> = Default::default();
    for (_g, e) in &sc.shaders {
        let code = &sc.data[e.code_offset..e.code_offset + e.code_len];
        let Some(s) = Shader::parse(code) else { *errs.entry("parse".into()).or_default() += 1; continue };
        let t = upk::sm3::translate(&s, &B);
        let mut m = String::new();
        for (r, k) in &s.samplers {
            let ty = match k { upk::sm3::SamplerKind::Cube => "texture_cube<f32>", upk::sm3::SamplerKind::Volume => "texture_3d<f32>", _ => "texture_2d<f32>" };
            m += &format!("@group(0) @binding({}) var tex{r}: {ty};\n@group(0) @binding({}) var smp{r}: sampler;\n", r * 2, r * 2 + 1);
        }
        m += "var<private> hf_c: array<vec4<f32>, 256>;\n";
        let ins: Vec<String> = s.inputs.iter().enumerate().map(|(k, d)| format!("@location({k}) v{}: vec4<f32>", d.reg)).collect();
        if s.pixel {
            m += &format!("@fragment fn main({}) -> @location(0) vec4<f32> {{\n var oc0: vec4<f32>; var oc1: vec4<f32>; var oc2: vec4<f32>; var oc3: vec4<f32>; var o_depth: vec4<f32>; let vpos = vec4<f32>(0.0); let vface = vec4<f32>(1.0);\n{}{}  return oc0;\n}}\n", ins.join(", "), t.declarations, t.body);
        } else {
            let mut outs = String::new();
            for r in 0..12 { outs += &format!(" var o{r}: vec4<f32>;"); }
            m += &format!("@vertex fn main({}) -> @builtin(position) vec4<f32> {{\n{outs} var o_pos: vec4<f32>; var o_d0: vec4<f32>; var o_d1: vec4<f32>;\n{}{}  return o0;\n}}\n", ins.join(", "), t.declarations, t.body);
        }
        let r = naga::front::wgsl::parse_str(&m).map_err(|e| e.message().to_string()).and_then(|module| {
            let mut v = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
            v.validate(&module).map(|_| ()).map_err(|e| format!("{:?}", e.as_inner()).chars().take(120).collect())
        });
        match r {
            Ok(()) => ok += 1,
            Err(e) => {
                bad += 1;
                let k: String = e.chars().take(80).collect();
                *errs.entry(k.clone()).or_default() += 1;
                ex.entry(k).or_insert(m);
            }
        }
    }
    println!("ok {ok} bad {bad}");
    let mut v: Vec<_> = errs.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    for (k, n) in v.iter().take(15) { println!("{n:6} {k}"); }
    if let Some((k, _)) = v.first() { if let Some(m) = ex.get(k) { std::fs::write("/tmp/claude-0/-home-claude/026f9b4d-8882-5f13-9237-52fde54ee789/scratchpad/bad.wgsl", m).ok(); } }
}
