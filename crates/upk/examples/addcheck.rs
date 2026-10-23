//! Dev: additive sequence on a base pose (viewmodel.rs convention), both multiplication orders:
//! distance right hand <-> weapon and left hand <-> weapon (should stay as in the base pose).
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a; let [bx, by, bz, bw] = b;
    [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]
}
fn conj(q: [f32; 4]) -> [f32; 4] { [-q[0], -q[1], -q[2], q[3]] }
fn rot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] { let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), conj(q)); [p[0], p[1], p[2]] }
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let mi = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(mi).unwrap();
    let si = p.exports.iter().position(|e| p.object_name_of(e) == a[3] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(si).unwrap();
    let tr: Vec<Option<usize>> = m.bones.iter().map(|b| s.bones.iter().position(|x| x.eq_ignore_ascii_case(&b.name))).collect();
    let seq = |n: &str| s.seqs.iter().find(|q| q.name == n).unwrap();
    let base_seq = seq(&a[4]);
    let add_seq = seq(&a[5]);
    let bt = base_seq.length;
    let base: Vec<([f32; 4], [f32; 3])> = m.bones.iter().enumerate().map(|(i, b)| match tr[i].and_then(|x| base_seq.sample(x, bt)) { Some((pp, r)) => (conj(r), pp), None => (b.rot, b.pos) }).collect();
    let compose = |local: &[([f32; 4], [f32; 3])]| {
        let mut out: Vec<([f32; 4], [f32; 3])> = Vec::new();
        for (i, b) in m.bones.iter().enumerate() {
            let (lr, lp) = local[i];
            if i == 0 || b.parent < 0 || b.parent as usize >= i { out.push((lr, lp)); } else {
                let (pr, pp) = out[b.parent as usize]; let rp = rot(pr, lp);
                out.push((qmul(pr, lr), [pp[0] + rp[0], pp[1] + rp[1], pp[2] + rp[2]]));
            }
        }
        out
    };
    let idx = |n: &str| m.bones.iter().position(|b| b.name == n).unwrap();
    let (rh, lh, wp) = (idx("RightHand"), idx("LeftHand"), idx("Weapon"));
    let d = |o: &Vec<([f32; 4], [f32; 3])>, x: usize, y: usize| { let (a, b) = (o[x].1, o[y].1); ((a[0]-b[0]).powi(2)+(a[1]-b[1]).powi(2)+(a[2]-b[2]).powi(2)).sqrt() };
    let b0 = compose(&base);
    println!("base: RH-W {:.2} LH-W {:.2}  W {:?}", d(&b0, rh, wp), d(&b0, lh, wp), b0[wp].1.map(|x| x.round()));
    println!("parents: RightHand {} Weapon {} ({})", m.bones[rh].parent, m.bones[wp].parent, m.bones[m.bones[wp].parent as usize].name);
    for k in 0..5 {
        let t = add_seq.length * k as f32 / 4.0;
        let delta: Vec<([f32; 4], [f32; 3])> = m.bones.iter().enumerate().map(|(i, _)| match tr[i].and_then(|x| add_seq.sample(x, t)) { Some((pp, r)) => (conj(r), pp), None => ([0.0, 0.0, 0.0, 1.0], [0.0; 3]) }).collect();
        for (name, order) in [("base*delta", 0), ("delta*base", 1), ("base*conj(d)", 2)] {
            let local: Vec<_> = base.iter().zip(&delta).map(|(b, dl)| (match order { 0 => qmul(b.0, dl.0), 1 => qmul(dl.0, b.0), _ => qmul(b.0, conj(dl.0)) }, [b.1[0] + dl.1[0], b.1[1] + dl.1[1], b.1[2] + dl.1[2]])).collect();
            let o = compose(&local);
            println!("t {t:.2} {name:12}: RH-W {:.2} LH-W {:.2} W {:?}", d(&o, rh, wp), d(&o, lh, wp), o[wp].1.map(|x| x.round()));
        }
    }
}
