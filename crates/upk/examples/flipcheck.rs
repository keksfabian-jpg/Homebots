//! Dev: 3P sequences composed on a body skeleton (bots.rs convention): per sequence the angle of
//! the head above the root over time, to find sequences whose pose tumbles.
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a; let [bx, by, bz, bw] = b;
    [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]
}
fn conj(q: [f32; 4]) -> [f32; 4] { [-q[0], -q[1], -q[2], q[3]] }
fn rot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] { let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), conj(q)); [p[0], p[1], p[2]] }
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let mp = upk::Package::open(&a[2]).unwrap();
    let mi = mp.exports.iter().position(|e| mp.object_name_of(e) == a[3] && mp.export_class(e) == "SkeletalMesh").unwrap();
    let m = mp.read_skeletal_mesh(mi).unwrap();
    let head = m.bones.iter().position(|b| b.name.eq_ignore_ascii_case("Head")).unwrap_or(m.bones.len() - 1);
    let feet = m.bones.iter().position(|b| b.name.to_lowercase().contains("toe")).unwrap_or(0);
    for set in &a[4..] {
        let si = p.exports.iter().position(|e| p.object_name_of(e) == *set && p.export_class(e) == "AnimSet").unwrap();
        let s = p.read_anim_set(si).unwrap();
        let tr: Vec<Option<usize>> = m.bones.iter().map(|b| s.bones.iter().position(|t| t.eq_ignore_ascii_case(&b.name))).collect();
        for q in &s.seqs {
            let mut worst = 0.0f32; let mut wt = 0.0; let mut first: Option<[f32;3]> = None;
            let n = 30;
            for k in 0..=n {
                let t = q.length * k as f32 / n as f32;
                let mut out: Vec<([f32; 4], [f32; 3])> = Vec::new();
                for (i, b) in m.bones.iter().enumerate() {
                    let (lr, lp) = match tr[i].and_then(|x| q.sample(x, t)) { Some((pp, r)) => (conj(r), pp), None => (b.rot, b.pos) };
                    if i == 0 || b.parent < 0 || b.parent as usize >= i { out.push((lr, lp)); } else {
                        let (pr, pp) = out[b.parent as usize]; let rp = rot(pr, lp);
                        out.push((qmul(pr, lr), [pp[0] + rp[0], pp[1] + rp[1], pp[2] + rp[2]]));
                    }
                }
                let h = out[head].1; let f = out[feet].1;
                let d = [h[0] - f[0], h[1] - f[1], h[2] - f[2]];
                let l = (d[0]*d[0]+d[1]*d[1]+d[2]*d[2]).sqrt().max(1e-3);
                let d = [d[0]/l, d[1]/l, d[2]/l];
                let f0 = *first.get_or_insert(d);
                if std::env::var("SEQ").ok().as_deref() == Some(q.name.as_str()) { println!("  t {:.2} head {:?} toe {:?} hips {:?}", t, h.map(|x| x.round()), f.map(|x| x.round()), out[3].1.map(|x| x.round())); }
                let ang = (f0[0]*d[0]+f0[1]*d[1]+f0[2]*d[2]).clamp(-1.0,1.0).acos().to_degrees();
                if ang > worst { worst = ang; wt = t; }
            }
            println!("{set} {:28} len {:.2} max head-axis change {:6.1} deg at {:.2} (up0 {:?})", q.name, q.length, worst, wt, first.map(|v| v.map(|x| (x*100.0).round()/100.0)));
        }
    }
}
