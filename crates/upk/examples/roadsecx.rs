//! Dev: per section local X (road width) range of a StaticMesh by object name (road edge strips).
//! Usage: roadsecx <package> <mesh name>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.export_class(e) == "StaticMesh" && p.object_name_of(e) == a[2]).unwrap();
    let m = p.read_static_mesh(i).unwrap();
    let l = &m.lods[0];
    for (k, s) in l.sections.iter().enumerate() {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let first = s.first_index as usize;
        for t in 0..(s.num_triangles as usize * 3) {
            let v = l.indices[first + t] as usize;
            lo = lo.min(l.positions[v][0]);
            hi = hi.max(l.positions[v][0]);
        }
        println!("sec{k} mat {} x {lo:.0}..{hi:.0} tris {}", p.object_path(s.material), s.num_triangles);
    }
}
