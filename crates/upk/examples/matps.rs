//! Dev: the cooked base-pass pixel shader of a material (by object name) as WGSL text.
//! Args: <shader cache upk> <package> <material name> [vertex factory, default FGPUSkinVertexFactory]
struct B;
impl upk::sm3::Bindings for B {
    fn constant(&self, reg: u32, name: Option<&str>) -> String {
        format!("C{reg}_{}", name.unwrap_or("?").replace(['[', ']'], "_"))
    }
    fn sampler(&self, reg: u32, name: Option<&str>) -> (String, String) {
        (format!("T{reg}_{}", name.unwrap_or("?")), format!("S{reg}"))
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let p = upk::Package::open(&a[2]).unwrap();
    let vf = a.get(4).map(|s| s.as_str()).unwrap_or("FGPUSkinVertexFactory");
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e) == a[3]) {
        let mat = i as i32 + 1;
        println!("// #{mat} {} [{}]", p.object_path(mat), p.export_class(e));
        let Some((id, _u)) = p.material_resource(mat, &|g| sc.is_record(g)) else { println!("// no resource"); continue };
        let rec = p.static_parameter_set(mat).and_then(|(b, sw)| sc.record_for_static_set(&b, &sw)).or_else(|| sc.record(&id));
        let Some(rec) = rec else { println!("// no record"); continue };
        for m in &rec.maps {
            println!("// vf {} shaders {:?}", m.vertex_factory, m.shaders.iter().map(|s| s.0.clone()).collect::<Vec<_>>());
        }
        for ty in ["TBasePassPixelShaderFNoLightMapPolicySkyLight", "TBasePassPixelShaderFNoLightMapPolicyNoSkyLight", "TBasePassPixelShaderFDirectionalLightLightMapPolicySkyLight"] {
            if let Some(code) = rec.shader(vf, ty).and_then(|h| sc.code(&h)) {
                let s = upk::sm3::Shader::parse(code).unwrap();
                println!("// {ty}\n// ctab {:?}", s.ctab);
                println!("{}", upk::sm3::translate(&s, &B).body);
                break;
            }
        }
    }
}
