//! Dev: extract terrain grids, check seams between neighbouring components.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for t in p.terrains() {
        let n: usize = t.grids.iter().map(|g| g.positions.len()).sum();
        println!("terrain: {} grids, {n} verts, material {:?}", t.grids.len(), t.material.map(|m| p.object_path(m)));
        if let Some(m) = t.material { println!("  diffuse {:?}", p.material_info(m).diffuse.map(|d| p.object_name(d as i32 + 1))); }
        // seam check: for each grid's right edge, find a grid whose left edge has same x and compare z
        let (mut pairs, mut err) = (0, 0f32);
        for g in &t.grids {
            for h in &t.grids {
                let gx = g.positions[g.nx - 1][0];
                if (h.positions[0][0] - gx).abs() > 1.0 { continue; }
                for j in 0..g.ny {
                    let pg = g.positions[j * g.nx + g.nx - 1];
                    for k in 0..h.ny {
                        let ph = h.positions[k * h.nx];
                        if (pg[1] - ph[1]).abs() < 1.0 { pairs += 1; err += (pg[2] - ph[2]).abs(); }
                    }
                }
            }
        }
        println!("  seam samples {pairs}, mean |dz| {:.2}", err / pairs.max(1) as f32);
        let zs: Vec<f32> = t.grids.iter().flat_map(|g| g.positions.iter().map(|p| p[2])).collect();
        println!("  z range {:.0} .. {:.0}", zs.iter().cloned().fold(f32::MAX, f32::min), zs.iter().cloned().fold(f32::MIN, f32::max));
    }
}
