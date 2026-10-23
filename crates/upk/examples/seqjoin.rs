//! Dev: where in sequence A the pose best matches the start of sequence B.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(i).unwrap();
    let seq = |n: &str| s.seqs.iter().find(|q| q.name == n).unwrap();
    let pose = |q: &upk::anim::AnimSeq, t: f32| -> Vec<([f32; 3], [f32; 4])> { (0..s.bones.len()).filter_map(|b| q.sample(b, t)).collect() };
    let dist = |x: &Vec<([f32; 3], [f32; 4])>, y: &Vec<([f32; 3], [f32; 4])>| -> f32 {
        x.iter().zip(y).map(|(u, v)| ((u.0[0]-v.0[0]).powi(2)+(u.0[1]-v.0[1]).powi(2)+(u.0[2]-v.0[2]).powi(2)).sqrt() + 10.0 * (1.0 - (u.1[0]*v.1[0]+u.1[1]*v.1[1]+u.1[2]*v.1[2]+u.1[3]*v.1[3]).abs())).sum::<f32>() / x.len().max(1) as f32
    };
    for an in a[3].split(',') {
        for bn in a[4].split(',') {
            let (qa, qb) = (seq(an), seq(bn));
            let b0 = pose(qb, 0.0);
            let mut best = (f32::MAX, 0.0);
            let n = qa.frames.max(2);
            for f in 0..n {
                let t = qa.length * f as f32 / (n - 1) as f32;
                let d = dist(&pose(qa, t), &b0);
                if d < best.0 { best = (d, t); }
                if std::env::var_os("CURVE").is_some() { print!("{:.2}@{:.2} ", d, t); }
            }
            println!("{an} ({:.2}s) best match for start of {bn}: t {:.3} (frame {:.0}) dist {:.3}", qa.length, best.1, best.1 * 30.0, best.0);
        }
    }
}
