//! Dev: sound notifies (time, cue) of every sequence in an AnimSet.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(i).unwrap();
    for q in &s.seqs { println!("{} len {:.2} rate {}: {:?}", q.name, q.length, q.rate, q.sounds.iter().map(|(t, c)| format!("{t:.2} {}", c.rsplit('.').next().unwrap())).collect::<Vec<_>>()); }
}
