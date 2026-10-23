//! Dev: SkeletalMesh Origin / RotOrigin (native) of meshes by object name.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for n in &a[2..] {
        let Some(i) = p.exports.iter().position(|e| p.object_name_of(e) == *n && p.export_class(e) == "SkeletalMesh") else { println!("{n}: not found"); continue };
        let m = p.read_skeletal_mesh(i).unwrap();
        println!("{n}: origin {:?} rot_origin {:?} bone0 {:?} {:?}", m.origin, m.rot_origin, m.bones[0].rot, m.bones[0].pos);
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for q in &m.lod.positions { let q: [f32; 3] = (*q).into(); for k in 0..3 { lo[k] = lo[k].min(q[k]); hi[k] = hi[k].max(q[k]); } }
        println!("   vertex bounds min {lo:?} max {hi:?}");
    }
}
