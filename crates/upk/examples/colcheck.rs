//! Dev: collision mesh vs. rendered geometry of the static meshes placed in a map folder.
//! Usage: colcheck <map folder> [mesh substring] [--near <bevy x> <bevy z> <radius m>]
//! Per mesh: render bounds (inline LOD positions, mesh space, UE units), collision-model bounds,
//! how far the collision model sticks out of the render bounds (OUT, uu, max over axes), the
//! collision triangle count, MASK (upk static_mesh_collision_mask) and the placement count.
//! With --near: only placements within the radius, plus fluid surfaces and BlockingVolumes there.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let filt = a.get(2).filter(|s| !s.starts_with("--")).cloned().unwrap_or_default().to_ascii_lowercase();
    let near = a.iter().position(|s| s == "--near").map(|i| (a[i + 1].parse::<f32>().unwrap(), a[i + 2].parse::<f32>().unwrap(), a[i + 3].parse::<f32>().unwrap()));
    let detail = a.iter().any(|s| s == "--detail");
    let lib = a.iter().position(|s| s == "--lib").map(|i| upk::library::Library::scan(&a[i + 1]).unwrap());
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
    // --below <z uu>: ASCII map (20 m cells, Bevy x right / z down) of terrain below that height
    if let Some(i) = a.iter().position(|s| s == "--below") {
        let zl: f32 = a[i + 1].parse().unwrap();
        // optional: cell size m, then window x0 x1 y0 y1 (m)
        let cs: f32 = a.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(20.0);
        let win: Vec<f32> = a.iter().skip(i + 3).take(4).filter_map(|s| s.parse().ok()).collect();
        let mut cells: std::collections::BTreeSet<(i32, i32)> = Default::default();
        for p in &pkgs {
            for t in p.terrains() {
                for g in &t.grids {
                    for v in &g.positions {
                        let ok = win.len() < 4 || (v[0] * 0.01 >= win[0] && v[0] * 0.01 <= win[1] && v[1] * 0.01 >= win[2] && v[1] * 0.01 <= win[3]);
                        if v[2] < zl && ok {
                            cells.insert(((v[0] * 0.01 / cs).floor() as i32, (v[1] * 0.01 / cs).floor() as i32));
                        }
                    }
                }
            }
        }
        let (x0, x1) = (cells.iter().map(|c| c.0).min().unwrap_or(0), cells.iter().map(|c| c.0).max().unwrap_or(0));
        let (y0, y1) = (cells.iter().map(|c| c.1).min().unwrap_or(0), cells.iter().map(|c| c.1).max().unwrap_or(0));
        println!("x from {} m, cell {cs} m", x0 as f32 * cs);
        for y in y0..=y1 {
            let row: String = (x0..=x1).map(|x| if cells.contains(&(x, y)) { '#' } else { '.' }).collect();
            println!("{:7.1} {row}", y as f32 * cs);
        }
        return;
    }
    let inside = |l: [f32; 3]| near.map_or(true, |(x, z, r)| ((l[0] * 0.01 - x).powi(2) + (l[1] * 0.01 - z).powi(2)).sqrt() <= r);
    // mesh path -> (placements, scales seen)
    let mut uses: std::collections::BTreeMap<String, (u32, Vec<String>)> = Default::default();
    for p in &pkgs {
        for pl in p.static_mesh_placements() {
            if pl.sky || pl.nudgeable || pl.collide & 1 == 0 || !inside(pl.location) {
                continue;
            }
            let path = p.object_path(pl.mesh);
            if !path.to_ascii_lowercase().contains(&filt) {
                continue;
            }
            let u = uses.entry(path).or_default();
            u.0 += 1;
            if near.is_some() {
                u.1.push(format!("({:.1} {:.1} {:.1}) rot {:?} scale {:?}", pl.location[0] * 0.01, pl.location[2] * 0.01, pl.location[1] * 0.01, pl.rotation, pl.scale));
            }
        }
    }
    let bb = |pts: &mut dyn Iterator<Item = [f32; 3]>| {
        let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in pts {
            for k in 0..3 {
                mn[k] = mn[k].min(p[k]);
                mx[k] = mx[k].max(p[k]);
            }
        }
        (mn, mx)
    };
    println!("{:>6} {:>5} {:>5} {:>4} MASK  render-min / render-max  |  col-min / col-max  mesh", "OUT", "CM", "N", "");
    for (path, (n, locs)) in &uses {
        let Some(&(mp, mi)) = by_path.get(&path.to_ascii_lowercase()) else {
            println!("{:>6} {:>5} {n:>5}      ? {path} (not in these packages)", "", "");
            continue;
        };
        let p = &pkgs[mp];
        let mask = p.static_mesh_collision_mask(mi);
        let Some(sm) = (match &lib { Some(l) => p.read_static_mesh_full(mi, l), None => p.read_static_mesh(mi) }) else { continue };
        let (rmn, rmx) = bb(&mut sm.lods[0].positions.iter().copied());
        let cm = p.static_mesh_collision_model(mi);
        let (out, cmn, cmx, ncm) = match &cm {
            Some(t) if !t.is_empty() => {
                let (cmn, cmx) = bb(&mut t.iter().flatten().copied());
                // max distance of a collision vertex / triangle centre from the render surface
                let l = &sm.lods[0];
                let rt: Vec<[[f32; 3]; 3]> = l.indices.chunks_exact(3).map(|c| [l.positions[c[0] as usize], l.positions[c[1] as usize], l.positions[c[2] as usize]]).collect();
                let mut pts: Vec<(f32, [f32; 3])> = t.iter().flat_map(|tr| [tr[0], tr[1], tr[2], [(tr[0][0] + tr[1][0] + tr[2][0]) / 3.0, (tr[0][1] + tr[1][1] + tr[2][1]) / 3.0, (tr[0][2] + tr[1][2] + tr[2][2]) / 3.0]]).map(|v| (rt.iter().map(|r| dist_tri(v, r)).fold(f32::MAX, f32::min), v)).collect();
                pts.sort_by(|a, b| b.0.total_cmp(&a.0));
                if detail {
                    for (d, v) in pts.iter().take(12) {
                        println!("      {d:6.1} uu at mesh ({:.0} {:.0} {:.0})", v[0], v[1], v[2]);
                    }
                }
                let out = pts.first().map_or(0.0, |p| p.0);
                (out, cmn, cmx, t.len() as i64)
            }
            Some(_) => (0.0, [0.0; 3], [0.0; 3], 0),
            None => (0.0, [0.0; 3], [0.0; 3], if p.static_mesh_collision_model_count(mi).is_none() { -2 } else { -1 }),
        };
        let f = |v: [f32; 3]| format!("{:.0},{:.0},{:.0}", v[0], v[1], v[2]);
        println!("{out:>6.0} {ncm:>5} {n:>5} {:>4}  {mask}   {} / {}  |  {} / {}  {path}", sm.lods[0].indices.len() / 3, f(rmn), f(rmx), f(cmn), f(cmx));
        for l in locs {
            println!("        at {l}");
        }
    }
    if let Some((x, z, r)) = near {
        for (pi, p) in pkgs.iter().enumerate() {
            for (_, fw, fh, loc, _, _) in p.fluid_surfaces() {
                if ((loc[0] * 0.01 - x).powi(2) + (loc[1] * 0.01 - z).powi(2)).sqrt() <= r + (fw.max(fh) * 0.005) {
                    println!("fluid {} at ({:.1} {:.1} {:.1}) {fw}x{fh}", files[pi].display(), loc[0] * 0.01, loc[2] * 0.01, loc[1] * 0.01);
                }
            }
            for class in ["BlockingVolume", "GCVehicleBlockingVolume"] {
                for v in p.brush_volumes(class) {
                    let (mn, mx) = bb(&mut v.polys.iter().flatten().copied());
                    let c = [(mn[0] + mx[0]) * 0.005, (mn[1] + mx[1]) * 0.005];
                    let hr = ((mx[0] - mn[0]).max(mx[1] - mn[1])) * 0.005;
                    if ((c[0] - x).powi(2) + (c[1] - z).powi(2)).sqrt() <= r + hr {
                        println!("{class} {} min ({:.1} {:.1} {:.1}) max ({:.1} {:.1} {:.1}) polys {}", v.name, mn[0] * 0.01, mn[2] * 0.01, mn[1] * 0.01, mx[0] * 0.01, mx[2] * 0.01, mx[1] * 0.01, v.polys.len());
                    }
                }
            }
        }
    }
}

fn dist_tri(p: [f32; 3], t: &[[f32; 3]; 3]) -> f32 {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let lerp = |o: [f32; 3], d: [f32; 3], s: f32| [o[0] + d[0] * s, o[1] + d[1] * s, o[2] + d[2] * s];
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let q = if d1 <= 0.0 && d2 <= 0.0 {
        a
    } else {
        let bp = sub(p, b);
        let (d3, d4) = (dot(ab, bp), dot(ac, bp));
        if d3 >= 0.0 && d4 <= d3 {
            b
        } else {
            let vc = d1 * d4 - d3 * d2;
            if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
                lerp(a, ab, d1 / (d1 - d3))
            } else {
                let cp = sub(p, c);
                let (d5, d6) = (dot(ab, cp), dot(ac, cp));
                if d6 >= 0.0 && d5 <= d6 {
                    c
                } else {
                    let vb = d5 * d2 - d1 * d6;
                    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
                        lerp(a, ac, d2 / (d2 - d6))
                    } else {
                        let va = d3 * d6 - d5 * d4;
                        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
                            lerp(b, sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)))
                        } else {
                            let den = 1.0 / (va + vb + vc);
                            let (v, w) = (vb * den, vc * den);
                            [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
                        }
                    }
                }
            }
        }
    };
    let d = sub(p, q);
    dot(d, d).sqrt()
}
