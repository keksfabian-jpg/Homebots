//! Dev: static mesh placements near a point of a map folder, with their collision.
//! Usage: placenear <map folder> <bevy x> <bevy z> [radius m]  (Bevy metres as in the bot logs)
//! MASK = static_mesh_collision_mask (bit 0 pawns, bit 1 shots), CM = collision-model triangles.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (x, z): (f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let r: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(6.0);
    let mut files: Vec<_> = std::fs::read_dir(&a[1]).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "map")).collect();
    files.sort();
    let pkgs: Vec<upk::Package> = files.iter().filter_map(|f| upk::Package::open(f).ok()).collect();
    let mut by_path = std::collections::HashMap::new();
    for (pi, p) in pkgs.iter().enumerate() {
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) == "StaticMesh" {
                by_path.entry(p.object_path(i as i32 + 1).to_ascii_lowercase()).or_insert((pi, i));
            }
        }
    }
    for p in &pkgs {
        for pl in p.static_mesh_placements() {
            let (ux, uy, uz) = (pl.location[0] * 0.01, pl.location[1] * 0.01, pl.location[2] * 0.01);
            let path = p.object_path(pl.mesh);
            let res = by_path.get(&path.to_ascii_lowercase()).copied();
            let (ext, mask, cm) = match res {
                Some((mp, mi)) => {
                    let sm = pkgs[mp].read_static_mesh(mi);
                    let ext = sm.map(|s| s.extent.iter().zip(pl.scale.iter()).map(|(e, s)| e * s.abs() * 0.01).fold(0.0f32, f32::max)).unwrap_or(0.0);
                    (ext, pkgs[mp].static_mesh_collision_mask(mi), pkgs[mp].static_mesh_collision_model(mi).map(|t| t.len() as i64).unwrap_or(-1))
                }
                None => (0.0, 9, -2),
            };
            let d = ((ux - x).powi(2) + (uy - z).powi(2)).sqrt();
            if d - ext <= r {
                println!("{d:5.1} m ext {ext:4.1} at ({ux:.1} {uz:.1} {uy:.1}) collide {} MASK {mask} CM {cm} sky {} {}", pl.collide, pl.sky as u8, path);
            }
        }
    }
}
