//! Dev: decode StaticMeshComponent LODData lightmaps with the layout found in the data:
//! LODCount, then per LOD: ShadowMaps[], ShadowVertexBuffers[], LightMapType,
//! (type 2: u32, LightGuids[], 3 x {Texture, FVector Scale}, CoordScale, CoordBias).
use upk::props::Rd;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let (mut total, mut with_lm, mut bad) = (0, 0, 0);
    let mut texs = std::collections::BTreeMap::new();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "StaticMeshComponent" { continue; }
        total += 1;
        let mut r = p.export_reader(i);
        if p.read_props(&mut r).is_none() { bad += 1; continue; }
        let nlod = r.i32().unwrap_or(0);
        if nlod <= 0 { continue; }
        let ok = (|| -> Option<()> {
            let ns = r.i32()?; if ns != 0 { return None; }
            let nv = r.i32()?; if nv != 0 { return None; }
            let ty = r.i32()?;
            if ty != 2 { texs.entry(format!("type {ty}")).and_modify(|c| *c += 1).or_insert(1); return Some(()); }
            let _u = r.i32()?;
            let ng = r.i32()?; r.bytes(ng as usize * 16)?;
            let mut desc = Vec::new();
            for _ in 0..3 { let t = r.i32()?; let s = r.vec3()?; desc.push((t, s)); }
            let cs = [r.f32()?, r.f32()?]; let cb = [r.f32()?, r.f32()?];
            for (t, _) in &desc { if *t != 0 { *texs.entry(p.object_path(*t)).or_insert(0) += 1; } }
            if with_lm < 3 { println!("{}: {:?} scale {cs:?} bias {cb:?}", p.object_path(i as i32 + 1), desc); }
            with_lm += 1;
            Some(())
        })();
        if ok.is_none() { bad += 1; }
    }
    let _ = Rd::new(&[]);
    println!("components {total}, with 2D lightmap {with_lm}, undecodable {bad}");
    for (k, v) in texs { println!("  {v:4}  {k}"); }
}
