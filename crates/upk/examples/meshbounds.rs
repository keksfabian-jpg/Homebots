//! Dev: static meshes whose (full-LOD) vertex positions leave the cooked bounds (parse errors).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "StaticMesh" { continue; }
            let Some(base) = p.read_static_mesh(i) else { continue };
            let full = p.read_static_mesh_full(i, &lib);
            for (tag, sm) in [("inline", Some(base)), ("full", full)] {
                let Some(sm) = sm else { continue };
                let Some(l) = sm.lods.first() else { continue };
                let r = sm.extent.iter().map(|x| x * x).sum::<f32>().sqrt().max(sm.radius).max(1.0);
                let mut worst = 0f32;
                let mut bad = 0;
                for v in &l.positions {
                    let d = ((v[0]-sm.origin[0]).powi(2)+(v[1]-sm.origin[1]).powi(2)+(v[2]-sm.origin[2]).powi(2)).sqrt();
                    if !d.is_finite() || d > r * 1.5 + 2.0 { bad += 1; }
                    worst = worst.max(if d.is_finite() { d / r } else { 1e9 });
                }
                let mut long = 0;
                for t in l.indices.chunks_exact(3) {
                    let g = |i: u32| l.positions.get(i as usize).copied().unwrap_or([0.0; 3]);
                    let (a, b, c) = (g(t[0]), g(t[1]), g(t[2]));
                    let d = |x: [f32; 3], y: [f32; 3]| ((x[0]-y[0]).powi(2)+(x[1]-y[1]).powi(2)+(x[2]-y[2]).powi(2)).sqrt();
                    if d(a, b).max(d(b, c)).max(d(a, c)) > r * 1.2 { long += 1; }
                }
                let ntri = l.indices.len() / 3;
                let sec_tris: u32 = l.sections.iter().map(|s| s.num_triangles).sum();
                if long * 20 > ntri || sec_tris as usize != ntri { println!("LONG {tag} lod{} {} [{}]: tris {} long {} sec_tris {}", sm.lod_index, p.object_path(i as i32 + 1), f.rsplit('/').next().unwrap(), ntri, long, sec_tris); }
                let badidx = l.indices.iter().filter(|&&x| x as usize >= l.positions.len()).count();
                if bad > 0 || badidx > 0 {
                    println!("{tag} lod{} {} [{}]: verts {} out {} worst {:.1}x r {:.0} badidx {}", sm.lod_index, p.object_path(i as i32 + 1), f.rsplit('/').next().unwrap(), l.positions.len(), bad, worst, r, badidx);
                }
            }
        }
    }
}
