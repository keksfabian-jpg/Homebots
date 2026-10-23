//! Dev: list the terrain's cached material resources (Id -> shader cache record).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[2])).unwrap();
    let (ti, e) = p.exports.iter().enumerate().find(|(_, e)| p.export_class(e) == "Terrain").unwrap();
    let mut r = p.export_reader(ti);
    p.read_props(&mut r).unwrap();
    let b = p.export_bytes(e);
    let nat = &b[r.pos..];
    let ids = sc.record_ids();
    eprintln!("{} record ids", ids.len());
    let mut o = 0;
    while o + 16 < nat.len() {
        let mut id = [0u8; 16];
        id.copy_from_slice(&nat[o..o + 16]);
        if ids.contains(&id) {
            let cnt_at = o + 20;
            match sc.record(&id) {
                Some(rec) => {
                    let mut ty: Vec<String> = rec.maps.iter().flat_map(|m| m.shaders.iter().filter(|(t, _)| (t.contains("BasePassPixel") && (t.contains("FDirectionalLightLightMapPolicySkyLight") || t.contains("FNoLightMap"))) || t.contains("TerrainColor")).map(move |(t, _)| format!("{}:{}", m.vertex_factory.trim_start_matches('F').trim_end_matches("VertexFactory"), t.trim_start_matches("TBasePassPixelShader")))).collect();
                    ty.dedup();
                    println!("@{cnt_at} {} {}  {:?}", hex(&id), rec.friendly_name, ty);
                }
                None => println!("@{cnt_at} {} -", hex(&id)),
            }
            o += 16;
            continue;
        }
        o += 1;
    }
}
fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
