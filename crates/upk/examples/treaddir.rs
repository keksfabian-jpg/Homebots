//! Dev: V gradient along the forward axis (mesh z) on the bottom and top runs of a section.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(i).unwrap();
    let want: u32 = a[3].parse().unwrap();
    for s in m.lod.sections.iter().filter(|s| s.material as u32 == want) {
        let idx = &m.lod.indices[s.first_index as usize..(s.first_index + s.num_triangles * 3) as usize];
        let ys: Vec<f32> = idx.iter().map(|&v| m.lod.positions[v as usize][1]).collect();
        let (ymin, ymax) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
        for (name, lo, hi) in [("bottom", ymax - (ymax - ymin) * 0.1, ymax), ("top", ymin, ymin + (ymax - ymin) * 0.25)] {
            // edges within the band: dV/dz
            let (mut sp, mut sn) = (0, 0);
            for t in idx.chunks(3) {
                for k in 0..3 {
                    let (x, y) = (t[k] as usize, t[(k + 1) % 3] as usize);
                    let (pa, pb) = (m.lod.positions[x], m.lod.positions[y]);
                    if pa[1] < lo || pa[1] > hi || pb[1] < lo || pb[1] > hi { continue }
                    let dz = pb[2] - pa[2];
                    let dv = m.lod.uvs[y][1] - m.lod.uvs[x][1];
                    if dz.abs() > 2.0 && dv.abs() > 0.01 && dv.abs() < 2.0 { if dz * dv > 0.0 { sp += 1 } else { sn += 1 } }
                }
            }
            println!("section mat {} {name} run (y {lo:.1}..{hi:.1}): V grows forward on {sp} edges, backward on {sn}", s.material);
        }
    }
}
