//! Dev: placements of a mesh (substring) with collide flags; BlockingVolume bounds.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for pl in p.static_mesh_placements() {
        let n = p.object_path(pl.mesh);
        if n.contains(&a[2]) { println!("{} collide {} loc {:?} rot {:?} scale {:?}", n, pl.collide, pl.location.map(|x| x.round()), pl.rotation, pl.scale); }
    }
    for v in p.brush_volumes("BlockingVolume") {
        let pts: Vec<[f32; 3]> = v.polys.iter().flatten().copied().collect();
        let mn = pts.iter().fold([f32::MAX; 3], |m, q| [m[0].min(q[0]), m[1].min(q[1]), m[2].min(q[2])]);
        let mx = pts.iter().fold([f32::MIN; 3], |m, q| [m[0].max(q[0]), m[1].max(q[1]), m[2].max(q[2])]);
        println!("BV {} polys {} {:?}..{:?}", v.name, v.polys.len(), mn.map(|x| x.round()), mx.map(|x| x.round()));
    }
}
