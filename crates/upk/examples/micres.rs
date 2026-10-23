//! Dev: all shader-cache record ids in a material's native data, their switches and fluid PS.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[2])).unwrap();
    let want = std::fs::read(&a[4]).ok();
    for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.object_name_of(e) == a[3]) {
        println!("#{} {} [{}]", i + 1, p.object_path(i as i32 + 1), p.export_class(e));
        let mut r = p.export_reader(i);
        p.read_props(&mut r).unwrap();
        let nat = &r.b[r.pos..];
        for o in 0..nat.len().saturating_sub(16) {
            let mut id = [0u8; 16];
            id.copy_from_slice(&nat[o..o + 16]);
            if !sc.is_record(&id) { continue; }
            let u = p.read_uniform_set(nat, o).map(|(u, _)| (u.vectors.len(), u.scalars.len(), u.tex2d.len(), u.cubes.len()));
            let rec = sc.record(&id);
            let m = rec.as_ref().and_then(|r| r.shader("FFluidVertexFactory", "TBasePassPixelShaderFNoLightMapPolicySkyLight")).and_then(|g| sc.code(&g)).map(|c| Some(c) == want.as_deref());
            println!("  @{o} uniforms {u:?} fluidPS==trace {m:?} switches {:?}", rec.map(|r| r.switches.iter().filter(|s| s.1).map(|s| s.0.clone()).collect::<Vec<_>>()));
        }
    }
}
