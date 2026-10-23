//! Dev: the biggest static mesh placements near an UE point (x, y, radius) of a map folder, with
//! whether they carry a baked 2D light map. Usage: lmnear <map folder> <x> <y> <radius> [n]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (x, y, r): (f32, f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
    let n: usize = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(30);
    let mut files: Vec<_> = std::fs::read_dir(&a[1]).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "map")).collect();
    files.sort();
    let pkgs: Vec<(String, upk::Package)> = files.iter().filter_map(|f| Some((f.file_name()?.to_string_lossy().to_string(), upk::Package::open(f).ok()?))).collect();
    let mut by_path = std::collections::HashMap::new();
    for (pi, (_, p)) in pkgs.iter().enumerate() {
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) == "StaticMesh" {
                by_path.entry(p.object_path(i as i32 + 1).to_ascii_lowercase()).or_insert((pi, i));
            }
        }
    }
    let mut out = Vec::new();
    for (f, p) in &pkgs {
        for pl in p.static_mesh_placements() {
            let d = ((pl.location[0] - x).powi(2) + (pl.location[1] - y).powi(2)).sqrt();
            if d > r || pl.mesh == 0 { continue; }
            let path = p.object_path(pl.mesh);
            let ext = by_path.get(&path.to_ascii_lowercase()).and_then(|&(mp, mi)| pkgs[mp].1.read_static_mesh(mi)).map(|s| [s.extent[0] * pl.scale[0].abs(), s.extent[1] * pl.scale[1].abs(), s.extent[2] * pl.scale[2].abs()]).unwrap_or([0.0; 3]);
            let lm = pl.lightmaps.iter().any(|l| l.is_some());
            out.push((ext[0].max(ext[1]).max(ext[2]), f.clone(), path, pl.location.map(|v| v.round()), pl.rotation[1], ext.map(|v| v.round()), lm, d.round()));
        }
    }
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for o in out.iter().take(n) {
        println!("{} {} loc {:?} yaw {} ext {:?} lightmap {} d {}", o.1, o.2, o.3, o.4, o.5, o.6, o.7);
    }
}
