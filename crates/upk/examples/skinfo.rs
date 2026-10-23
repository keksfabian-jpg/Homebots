//! Dev: skeletal mesh bones and influence sanity.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "SkeletalMesh").unwrap();
    let m = p.read_skeletal_mesh(i).unwrap();
    for (k, b) in m.bones.iter().enumerate() { println!("{k} {} parent {} rot {:?} pos {:?}", b.name, b.parent, b.rot.map(|x| (x * 1000.0).round() / 1000.0), b.pos.map(|x| (x * 100.0).round() / 100.0)); }
    let sums: std::collections::BTreeMap<u32, usize> = m.influences.iter().fold(Default::default(), |mut h, (_, w)| { *h.entry(w.iter().map(|&x| x as u32).sum()).or_default() += 1; h });
    println!("verts {} weight sums {:?}", m.lod.positions.len(), sums);
    println!("first infl {:?}", &m.influences[..5]);
}
