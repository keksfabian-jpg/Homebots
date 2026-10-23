//! Dev: compare actor Z with terrain height under them (nearest grid sample).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = upk::Package::open(&a[1]).unwrap();
    let ter = &t.terrains()[0];
    let pts: Vec<[f32; 3]> = ter.grids.iter().flat_map(|g| g.positions.clone()).collect();
    for m in &a[2..] {
        let p = upk::Package::open(m).unwrap();
        let mut d: Vec<f32> = Vec::new();
        for pl in p.static_mesh_placements() {
            let (x, y, z) = (pl.location[0], pl.location[1], pl.location[2]);
            let best = pts.iter().min_by(|a, b| ((a[0]-x).powi(2)+(a[1]-y).powi(2)).total_cmp(&((b[0]-x).powi(2)+(b[1]-y).powi(2)))).unwrap();
            if ((best[0]-x).powi(2)+(best[1]-y).powi(2)).sqrt() < 300.0 { d.push(z - best[2]); }
        }
        d.sort_by(|a, b| a.total_cmp(b));
        let near = d.iter().filter(|v| v.abs() < 60.0).count();
        println!("{}: {} actors over terrain, median dz {:.0}, |dz|<60: {}", m.rsplit('/').next().unwrap(), d.len(), d.get(d.len()/2).copied().unwrap_or(0.0), near);
    }
}
