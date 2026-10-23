//! Dev: component-space positions of some bones for sequences of two anim sets on one skeleton
//! (viewmodel.rs convention: tracks matched by bone name, rotations conjugated).
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a; let [bx, by, bz, bw] = b;
    [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]
}
fn conj(q: [f32; 4]) -> [f32; 4] { [-q[0], -q[1], -q[2], q[3]] }
fn rot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] { let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), conj(q)); [p[0], p[1], p[2]] }
fn main() {
    let a: Vec<String> = std::env::args().collect();
    // mesh_pkg mesh set_pkg set seq t [set_pkg set seq t ...]
    let mp = upk::Package::open(&a[1]).unwrap();
    let mi = mp.exports.iter().position(|e| mp.object_name_of(e) == a[2] && mp.export_class(e) == "SkeletalMesh").unwrap();
    let m = mp.read_skeletal_mesh(mi).unwrap();
    let mut k = 3;
    while k + 3 < a.len() + 0 && k + 3 <= a.len() - 1 {
        let p = upk::Package::open(&a[k]).unwrap();
        let si = p.exports.iter().position(|e| p.object_name_of(e) == a[k + 1] && p.export_class(e) == "AnimSet").unwrap();
        let s = p.read_anim_set(si).unwrap();
        let q = s.seqs.iter().find(|q| q.name == a[k + 2]).unwrap();
        let t: f32 = a[k + 3].parse().unwrap();
        let tr: Vec<Option<usize>> = m.bones.iter().map(|b| s.bones.iter().position(|x| x.eq_ignore_ascii_case(&b.name))).collect();
        let mut out: Vec<([f32; 4], [f32; 3])> = Vec::new();
        for (i, b) in m.bones.iter().enumerate() {
            let (lr, lp) = match tr[i].and_then(|x| q.sample(x, t)) { Some((pp, r)) => (conj(r), pp), None => (b.rot, b.pos) };
            if i == 0 || b.parent < 0 || b.parent as usize >= i { out.push((lr, lp)); } else {
                let (pr, pp) = out[b.parent as usize]; let rp = rot(pr, lp);
                out.push((qmul(pr, lr), [pp[0] + rp[0], pp[1] + rp[1], pp[2] + rp[2]]));
            }
        }
        println!("{} {} t {t}: tracks matched {}", a[k + 1], a[k + 2], tr.iter().filter(|x| x.is_some()).count());
        if let Ok(v) = std::env::var("PT") {
            // point in Weapon bone space -> component space
            let f: Vec<f32> = v.split(',').map(|x| x.parse().unwrap()).collect();
            let w = m.bones.iter().position(|b| b.name == "Weapon").unwrap();
            let r = rot(out[w].0, [f[0], f[1], f[2]]);
            println!("   point {:?} -> {:?}", f, [out[w].1[0] + r[0], out[w].1[1] + r[1], out[w].1[2] + r[2]].map(|x| (x * 100.0).round() / 100.0));
        }
        for name in ["Root", "Spine2", "RightHand", "LeftHand", "Weapon"] {
            if let Some(i) = m.bones.iter().position(|b| b.name == name) { println!("   {name:10} {:?}", out[i].1.map(|x| x.round())); }
        }
        k += 4;
    }
}
