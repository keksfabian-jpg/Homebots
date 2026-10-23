//! Dev: static mesh placements without collision (collide 0) and BlockingVolumes per package.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        let pls = p.static_mesh_placements();
        let mut counts: std::collections::BTreeMap<String, (usize, usize, [f32; 3])> = Default::default();
        for pl in &pls {
            let name = p.object_path(pl.mesh);
            let e = counts.entry(name).or_insert((0, 0, pl.location));
            if pl.collide == 0 { e.0 += 1 } else { e.1 += 1 }
        }
        let bv = p.brush_volumes("BlockingVolume").len();
        println!("== {f}: {} placements, {} BlockingVolumes", pls.len(), bv);
        for (k, v) in counts.iter().filter(|x| x.1 .0 > 0) {
            println!("  nocol {:4} col {:4} {k} e.g. {:?}", v.0, v.1, v.2.map(|x| x.round()));
        }
    }
}
