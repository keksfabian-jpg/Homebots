//! Dev: find the cache entry for a MIC's first static parameter set and compare its fluid PS to a blob.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let sc = upk::shadercache::ShaderCache::load(std::path::Path::new(&a[2])).unwrap();
    let idx: usize = a[3].parse::<usize>().unwrap() - 1;
    let want = std::fs::read(&a[4]).ok();
    let mut r = p.export_reader(idx);
    p.read_props(&mut r).unwrap();
    let b = r.b;
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let mut o = r.pos;
    while o + 40 < b.len() {
        let n = u(o) as usize;
        if (1..64).contains(&n) && o + 4 + n * 32 <= b.len() && (0..n).all(|k| { let q = o + 4 + k * 32; (u(q) as usize) < p.names.len() && u(q + 4) == 0 && u(q + 8) <= 1 && u(q + 12) <= 1 }) {
            let base: [u8; 16] = b[o - 16..o].try_into().unwrap();
            let sw: Vec<([u8; 16], bool)> = (0..n).map(|k| { let q = o + 4 + k * 32; (b[q + 16..q + 32].try_into().unwrap(), u(q + 8) != 0) }).collect();
            let t = std::time::Instant::now();
            let rec = sc.record_for_static_set(&base, &sw);
            let m = rec.as_ref().and_then(|r| r.shader("FFluidVertexFactory", "TBasePassPixelShaderFNoLightMapPolicySkyLight")).and_then(|g| sc.code(&g)).map(|c| Some(c) == want.as_deref());
            println!("@{o} n={n} found {} fluidPS==trace {m:?} ({:?})", rec.is_some(), t.elapsed());
            o += 4 + n * 32;
            continue;
        }
        o += 4;
    }
}
