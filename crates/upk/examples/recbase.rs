//! Dev: records with a given base material id (hex) and their switch values.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[1])).unwrap();
    let want = std::fs::read(&a[3]).ok();
    for id in sc.record_ids() {
        let Some(r) = sc.record(&id) else { continue };
        let hex: String = r.base_id.iter().map(|b| format!("{b:02x}")).collect();
        if hex != a[2] { continue; }
        let idh: String = id.iter().map(|b| format!("{b:02x}")).collect();
        let m = r.shader("FFluidVertexFactory", "TBasePassPixelShaderFNoLightMapPolicySkyLight").and_then(|g| sc.code(&g)).map(|c| Some(c) == want.as_deref());
        println!("{idh} {} fluidPS==trace {m:?} on {:?}", r.friendly_name, r.switches.iter().filter(|s| s.1).map(|s| s.0.clone()).collect::<Vec<_>>());
    }
}
