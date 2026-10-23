//! Dev: world units per texture V unit along a skeletal mesh section (tread scroll scale).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(i).unwrap();
    let want: u32 = a[3].parse().unwrap();
    for s in m.lod.sections.iter().filter(|s| s.material as u32 == want) {
        let idx = &m.lod.indices[s.first_index as usize..(s.first_index + s.num_triangles * 3) as usize];
        let mut r = Vec::new();
        let (mut vlo, mut vhi, mut ulo, mut uhi) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for t in idx.chunks(3) {
            for k in 0..3 {
                let (x, y) = (t[k] as usize, t[(k + 1) % 3] as usize);
                let (pa, pb) = (m.lod.positions[x], m.lod.positions[y]);
                let (ua, ub) = (m.lod.uvs[x], m.lod.uvs[y]);
                vlo = vlo.min(ua[1]); vhi = vhi.max(ua[1]); ulo = ulo.min(ua[0]); uhi = uhi.max(ua[0]);
                let d = ((pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2) + (pa[2] - pb[2]).powi(2)).sqrt();
                let (du, dv) = ((ua[0] - ub[0]).abs(), (ua[1] - ub[1]).abs());
                if dv > 1e-4 && du < 0.1 * dv { r.push(d / dv); }
            }
        }
        r.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("section mat {} tris {} U {ulo}..{uhi} V {vlo}..{vhi} edges {} uu/V p10 {:?} median {:?} p90 {:?}", s.material, s.num_triangles, r.len(), r.get(r.len() / 10), r.get(r.len() / 2), r.get(r.len() * 9 / 10));
    }
}
