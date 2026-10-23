//! Dev: an AnimSet sequence's keys per track at a few times (position, rotation).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(i).unwrap();
    let q = s.seqs.iter().find(|q| q.name == a[3]).unwrap();
    for (t, bn) in s.bones.iter().enumerate() {
        let r = |x: f32| (x * 100.0).round() / 100.0;
        let v: Vec<String> = [0.0, 0.25, 0.5, 0.75, 1.0].iter().map(|f| { let (p, q2) = q.sample(t, f * q.length).unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0])); format!("p{:?} q{:?}", p.map(r), q2.map(r)) }).collect();
        println!("{bn:18} {}", v.join(" | "));
    }
}
