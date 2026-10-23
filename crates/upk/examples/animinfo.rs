//! Dev: AnimSet sequences and a comparison of frame 0 with a mesh's reference pose.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(i).unwrap();
    println!("{} bones {} seqs {}", s.path, s.bones.len(), s.seqs.len());
    for q in &s.seqs { println!("  {} len {} frames {} rate {} tracks {}", q.name, q.length, q.frames, q.rate, q.tracks.len()); }
    if a.len() > 4 {
        let mp = upk::Package::open(&a[3]).unwrap();
        let mi = mp.exports.iter().position(|e| mp.object_name_of(e) == a[4] && mp.export_class(e) == "SkeletalMesh").unwrap();
        let m = mp.read_skeletal_mesh(mi).unwrap();
        let seq = s.seqs.iter().find(|q| a.get(5).is_none_or(|n| &q.name == n)).unwrap();
        println!("compare {} frame 0 with mesh ref pose", seq.name);
        for (t, bn) in s.bones.iter().enumerate().take(60) {
            let Some(b) = m.bones.iter().find(|b| &b.name == bn) else { continue };
            let (pos, rot) = seq.sample(t, 0.0).unwrap();
            println!("{bn:20} ref q {:?} anim q {:?} | ref p {:?} anim p {:?}", b.rot.map(|x| (x * 100.0).round() / 100.0), rot.map(|x| (x * 100.0).round() / 100.0), b.pos.map(|x| (x * 10.0).round() / 10.0), pos.map(|x| (x * 10.0).round() / 10.0));
        }
    }
}
