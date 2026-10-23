//! Dev: rotation jumps between neighbouring samples per track of a sequence.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let si = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(si).unwrap();
    let q = s.seqs.iter().find(|q| q.name == a[3]).unwrap();
    let n = 120;
    for (k, tr) in q.tracks.iter().enumerate() {
        let mut worst = (0.0f32, 0.0f32);
        for i in 0..n {
            let t0 = q.length * i as f32 / n as f32; let t1 = q.length * (i + 1) as f32 / n as f32;
            let (Some((_, r0)), Some((_, r1))) = (q.sample(k, t0), q.sample(k, t1)) else { continue };
            let d = (r0[0]*r1[0]+r0[1]*r1[1]+r0[2]*r1[2]+r0[3]*r1[3]).abs().min(1.0);
            let ang = 2.0 * d.acos().to_degrees();
            if ang > worst.0 { worst = (ang, t0); }
        }
        if worst.0 > 20.0 {
            println!("track {k} {} keys rot {} pos {}: jump {:.1} deg at {:.3}", s.bones[k], tr.rot.len(), tr.pos.len(), worst.0, worst.1);
            for (j, r) in tr.rot.iter().enumerate().take(8) { println!("    key {j} {:?}", r.map(|x| (x * 1000.0).round() / 1000.0)); }
        }
    }
}
