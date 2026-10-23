//! Dev: composed reference-pose transform (mesh space) of a bone: comp = parent * local.
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]
}
fn rot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]);
    [p[0], p[1], p[2]]
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e).eq_ignore_ascii_case(&a[2]) && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(i).unwrap();
    let mut comp: Vec<([f32; 4], [f32; 3])> = Vec::new();
    for (k, b) in m.bones.iter().enumerate() {
        if k == 0 || b.parent < 0 { comp.push((b.rot, b.pos)); continue; }
        let (pr, pp) = comp[b.parent as usize];
        let t = rot(pr, b.pos);
        comp.push((qmul(pr, b.rot), [pp[0] + t[0], pp[1] + t[1], pp[2] + t[2]]));
    }
    for (k, b) in m.bones.iter().enumerate() {
        if a.len() > 3 && !a[3..].iter().any(|n| n.eq_ignore_ascii_case(&b.name)) { continue; }
        let (r, t) = comp[k];
        println!("{k} {} q {:?} t {:?} | X {:?} Y {:?} Z {:?}", b.name, r.map(|x| (x * 1000.0).round() / 1000.0), t.map(|x| (x * 100.0).round() / 100.0), rot(r, [1.0, 0.0, 0.0]).map(|x| (x * 100.0).round() / 100.0), rot(r, [0.0, 1.0, 0.0]).map(|x| (x * 100.0).round() / 100.0), rot(r, [0.0, 0.0, 1.0]).map(|x| (x * 100.0).round() / 100.0));
    }
}
