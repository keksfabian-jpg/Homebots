//! Dev: bounds / bones of every SkeletalMesh export whose path contains a substring.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    let q = a[2].to_lowercase();
    for i in 0..p.exports.len() {
        if p.export_class(&p.exports[i]) != "SkeletalMesh" || !p.object_path(i as i32 + 1).to_lowercase().contains(&q) { continue }
        match p.read_skeletal_mesh(i) {
            Some(m) => {
                let mut lo = [f32::MAX; 3]; let mut hi = [f32::MIN; 3];
                for v in &m.lod.positions { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
                let big = m.lod.positions.iter().filter(|v| v.iter().any(|c| c.abs() > 5000.0 || !c.is_finite())).count();
                println!("{} {}: verts {} idx {} max_idx {} bones {} lo {:?} hi {:?} big {} rigid0 {}", i + 1, p.object_path(i as i32 + 1), m.lod.positions.len(), m.lod.indices.len(), m.lod.indices.iter().max().unwrap_or(&0), m.bones.len(), lo, hi, big, m.influences.iter().filter(|x| x.1 == [255,0,0,0] && x.0[0]==0).count());
            }
            None => println!("{} {}: read failed", i + 1, p.object_path(i as i32 + 1)),
        }
    }
}
