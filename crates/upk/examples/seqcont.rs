//! Dev: pose distance between the end / start of sequences of an AnimSet (continuity check).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let i = p.exports.iter().position(|e| p.object_name_of(e) == a[2] && p.export_class(e) == "AnimSet").unwrap();
    let s = p.read_anim_set(i).unwrap();
    let pose = |n: &str, end: bool| -> Option<Vec<([f32; 3], [f32; 4])>> {
        let q = s.seqs.iter().find(|q| q.name == n)?;
        let t = if end { q.length - 1e-3 } else { 0.0 };
        Some((0..s.bones.len()).filter_map(|b| q.sample(b, t)).collect())
    };
    let names: Vec<String> = s.seqs.iter().map(|q| q.name.clone()).collect();
    let dist = |x: &Vec<([f32; 3], [f32; 4])>, y: &Vec<([f32; 3], [f32; 4])>| -> f32 {
        x.iter().zip(y).map(|(u, v)| ((u.0[0]-v.0[0]).powi(2)+(u.0[1]-v.0[1]).powi(2)+(u.0[2]-v.0[2]).powi(2)).sqrt()).sum::<f32>() / x.len().max(1) as f32
    };
    for from in &names {
        let Some(e) = pose(from, true) else { continue };
        let mut row = Vec::new();
        for to in &names {
            if to.starts_with("ADD") { continue; }
            let Some(st) = pose(to, false) else { continue };
            row.push((dist(&e, &st), to.clone()));
        }
        row.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        println!("end {from:20} -> {}", row.iter().take(4).map(|(d, n)| format!("{n} {d:.2}")).collect::<Vec<_>>().join(", "));
    }
}
