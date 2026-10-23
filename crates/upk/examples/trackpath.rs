//! Dev: local translation of one bone's track over a sequence.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(i).unwrap();
    let b = s.bones.iter().position(|n| n.eq_ignore_ascii_case(&a[4])).expect("bone");
    let q = s.seqs.iter().find(|q| q.name == a[3]).unwrap();
    for k in 0..=10 {
        let t = q.length * k as f32 / 10.0;
        let (pos, rot) = q.sample(b, t).unwrap_or(([0.0; 3], [0.0; 4]));
        println!("t {t:.2} pos {:?} rot {:?}", pos.map(|x| (x * 10.0).round() / 10.0), rot.map(|x| (x * 100.0).round() / 100.0));
    }
}
