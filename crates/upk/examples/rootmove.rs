//! Dev: translation of the first tracks (root, pelvis) over a sequence.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let si = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(si).unwrap();
    for name in &a[3..] {
        let Some(q) = s.seqs.iter().find(|q| &q.name == name) else { println!("{name} missing"); continue };
        for k in 0..3.min(q.tracks.len()) {
            let first = q.sample(k, 0.0).map(|x| x.0); let last = q.sample(k, q.length * 0.999).map(|x| x.0);
            let mid = q.sample(k, q.length * 0.5).map(|x| x.0);
            println!("{name} track {k} ({}) t0 {:?} mid {:?} end {:?}", s.bones.get(k).cloned().unwrap_or_default(), first, mid, last);
        }
    }
}
