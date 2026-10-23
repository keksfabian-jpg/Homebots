//! Dev: all static mesh placements whose mesh or material name contains a pattern.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pat = a[1].to_lowercase();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for pl in p.static_mesh_placements() {
            let mp = p.object_path(pl.mesh);
            let mats: Vec<String> = pl.materials.iter().map(|&m| if m != 0 { p.object_path(m) } else { "-".into() }).collect();
            if !mp.to_lowercase().contains(&pat) && !mats.iter().any(|m| m.to_lowercase().contains(&pat)) { continue; }
            println!("{} | {} {:?} rot {:?} scale {:?} mats {:?}", f.rsplit('/').next().unwrap(), mp, pl.location, pl.rotation, pl.scale, mats);
        }
    }
}
