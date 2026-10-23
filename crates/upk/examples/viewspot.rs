//! Dev: test-camera spots for a target in a map folder (tests/scenarios.txt HOMEBOTS_SPAWN_AT).
//! Usage: viewspot <map folder> <x> <y> <z> [dmin] [dmax] [target mesh substring] [step deg]
//! (UE units). Builds the triangles within 45 m of the target (terrain grids + static mesh
//! placements, LOD as cooked), then for every direction and distance finds the standable
//! surface (upward normal, 2 m headroom) nearest the target's base height, the player's eye
//! there (feet + 41.5 + 41.5 + 35.5 uu, player.rs) and how many of the target's sample points
//! (its bounds: centre, low, high, left, right) the eye sees without another triangle in the
//! way. The target placement itself (substring; pivot within 2 m of x/y or x/y inside its bounds) does
//! not occlude.
//! Prints: spawn x,y,z (cylinder centre + 5 uu), yaw deg (towards the target), pitch deg,
//! visible samples, distance, ground height, terrain height, ROOF <uu> when a surface is above
//! within 10 m (indoors).
type V = [f32; 3];
fn sub(a: V, b: V) -> V { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn add(a: V, b: V) -> V { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
fn mul(a: V, s: f32) -> V { [a[0] * s, a[1] * s, a[2] * s] }
fn dot(a: V, b: V) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: V, b: V) -> V { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn len(a: V) -> f32 { dot(a, a).sqrt() }

/// Ray / triangle (both sides): distance along `d` (unit) or None.
fn hit(o: V, d: V, t: &[V; 3]) -> Option<f32> {
    let e1 = sub(t[1], t[0]);
    let e2 = sub(t[2], t[0]);
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-7 { return None; }
    let inv = 1.0 / det;
    let s = sub(o, t[0]);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) { return None; }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 { return None; }
    let tt = dot(e2, q) * inv;
    (tt > 1e-3).then_some(tt)
}

/// UE FRotationMatrix rows (main.rs rotator_to_quat): v' = v.x X + v.y Y + v.z Z
fn rot(r: [i32; 3], v: V) -> V {
    let k = std::f32::consts::TAU / 65536.0;
    let (sp, cp) = (r[0] as f32 * k).sin_cos();
    let (sy, cy) = (r[1] as f32 * k).sin_cos();
    let (sr, cr) = (r[2] as f32 * k).sin_cos();
    let ax = [cp * cy, cp * sy, sp];
    let ay = [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp];
    let az = [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp];
    add(add(mul(ax, v[0]), mul(ay, v[1])), mul(az, v[2]))
}

struct Tri { t: [V; 3], terrain: bool, target: bool }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let tgt: V = [a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap()];
    let dmin: f32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(600.0);
    let dmax: f32 = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(1000.0);
    let sub_s = a.get(7).cloned().filter(|s| s != "-").map(|s| s.to_ascii_lowercase());
    let step: f32 = a.get(8).and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let reach = 4500.0f32;
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
    let mut tris: Vec<Tri> = Vec::new();
    let near2 = |v: V| (v[0] - tgt[0]).powi(2) + (v[1] - tgt[1]).powi(2) < reach * reach;
    for (_, p) in &pkgs {
        for t in p.terrains() {
            for g in &t.grids {
                for j in 0..g.ny.saturating_sub(1) {
                    for i in 0..g.nx.saturating_sub(1) {
                        let v = |i: usize, j: usize| g.positions[j * g.nx + i];
                        if !near2(v(i, j)) { continue; }
                        tris.push(Tri { t: [v(i, j), v(i + 1, j), v(i + 1, j + 1)], terrain: true, target: false });
                        tris.push(Tri { t: [v(i, j), v(i + 1, j + 1), v(i, j + 1)], terrain: true, target: false });
                    }
                }
            }
        }
    }
    let mut samples: Vec<V> = Vec::new();
    let mut target_base = tgt[2];
    let mut cache: std::collections::HashMap<(usize, usize), Option<(Vec<V>, Vec<u32>, V, V)>> = Default::default();
    for (fname, p) in &pkgs {
        for pl in p.static_mesh_placements() {
            if pl.mesh == 0 || pl.sky { continue; }
            let path = p.object_path(pl.mesh).to_ascii_lowercase();
            let Some(&(mp, mi)) = by_path.get(&path) else { continue };
            let dxy = ((pl.location[0] - tgt[0]).powi(2) + (pl.location[1] - tgt[1]).powi(2)).sqrt();
            let m = cache.entry((mp, mi)).or_insert_with(|| {
                let sm = pkgs[mp].1.read_static_mesh(mi)?;
                let l = sm.lods.first()?;
                Some((l.positions.clone(), l.indices.clone(), sm.origin, sm.extent))
            });
            let Some((pos, idx, org, ext)) = m else { continue };
            let sc = pl.scale;
            let r = len([ext[0] * sc[0], ext[1] * sc[1], ext[2] * sc[2]]) + len(*org);
            if dxy > reach + r { continue; }
            let w = |v: V| add(pl.location, rot(pl.rotation, [v[0] * sc[0], v[1] * sc[1], v[2] * sc[2]]));
            // the target: substring, pivot within 2 m of x/y or x/y inside its world bounds
            let named = sub_s.as_ref().is_some_and(|s| path.contains(s.as_str()));
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            if named {
                for v in pos.iter() {
                    let q = w(*v);
                    for k in 0..3 { lo[k] = lo[k].min(q[k]); hi[k] = hi[k].max(q[k]); }
                }
            }
            let is_tgt = named && (dxy < 200.0 || (lo[0]..=hi[0]).contains(&tgt[0]) && (lo[1]..=hi[1]).contains(&tgt[1]));
            if is_tgt {
                println!("target {fname} {path} loc {:?} rot {:?} scale {:?} extent {:?} lightmapped {}", pl.location, pl.rotation, pl.scale, ext, pl.lightmaps.iter().any(|l| l.is_some()));
                println!("  world bounds lo {lo:?} hi {hi:?}");
                let c = mul(add(lo, hi), 0.5);
                target_base = lo[2];
                samples = vec![c, [c[0], c[1], lo[2] + (hi[2] - lo[2]) * 0.2], [c[0], c[1], lo[2] + (hi[2] - lo[2]) * 0.8], [lo[0], lo[1], c[2]], [hi[0], hi[1], c[2]]];
                // pull the samples slightly in so they are not exactly on the surface
                samples = samples.iter().map(|s| add(mul(*s, 0.9), mul(c, 0.1))).collect();
            }
            for f in idx.chunks_exact(3) {
                let (Some(&v0), Some(&v1), Some(&v2)) = (pos.get(f[0] as usize), pos.get(f[1] as usize), pos.get(f[2] as usize)) else { continue };
                tris.push(Tri { t: [w(v0), w(v1), w(v2)], terrain: false, target: is_tgt });
            }
        }
    }
    if samples.is_empty() {
        samples.push(tgt);
    }
    eprintln!("{} triangles", tris.len());
    // column: all surfaces crossing the vertical at (x, y): (z, normal z, terrain)
    let column = |x: f32, y: f32| -> Vec<(f32, f32, bool)> {
        let o = [x, y, tgt[2] + 5000.0];
        let mut v: Vec<(f32, f32, bool)> = tris.iter().filter_map(|t| {
            let h = hit(o, [0.0, 0.0, -1.0], &t.t)?;
            let n = cross(sub(t.t[1], t.t[0]), sub(t.t[2], t.t[0]));
            Some((o[2] - h, (n[2] / len(n)).abs(), t.terrain))
        }).collect();
        v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        v
    };
    // VS_OPEN=<radius uu>: open-ground spots instead (vehicle tests): grid points every 2.5 m
    // within dmax of x/y whose terrain varies < 1.5 m (VS_VAR uu) over 8 points on the circle and where no
    // mesh triangle reaches above the terrain + 0.3 m inside the circle (bounding-box test)
    if let Some(rad) = std::env::var("VS_OPEN").ok().and_then(|v| v.parse::<f32>().ok()) {
        let terr_at = |x: f32, y: f32| column(x, y).into_iter().find(|h| h.2).map(|h| h.0);
        let mut res = Vec::new();
        let maxvar: f32 = std::env::var("VS_VAR").ok().and_then(|v| v.parse().ok()).unwrap_or(150.0);
        let mut fails = [0u32; 3];
        let n = (dmax / 250.0) as i32;
        for i in -n..=n {
            for j in -n..=n {
                let (x, y) = (tgt[0] + i as f32 * 250.0, tgt[1] + j as f32 * 250.0);
                if ((x - tgt[0]).powi(2) + (y - tgt[1]).powi(2)).sqrt() > dmax { continue; }
                let Some(g) = terr_at(x, y) else { continue };
                let ring: Vec<f32> = (0..8).filter_map(|k| { let a = k as f32 * std::f32::consts::FRAC_PI_4; terr_at(x + a.cos() * rad, y + a.sin() * rad) }).collect();
                if ring.len() < 8 { fails[0] += 1; continue; }
                let var = ring.iter().fold(g, |m, &h| m.max(h)) - ring.iter().fold(g, |m, &h| m.min(h));
                if var > maxvar { fails[1] += 1; continue; }
                // mesh triangles near the circle, then columns every 1 m inside it: a mesh
                // surface more than 0.4 m above the terrain there is clutter (roads, decals and
                // flat foundations on the ground are not)
                let near_all: Vec<&Tri> = tris.iter().filter(|t| {
                    let (mut lo, mut hi) = (t.t[0], t.t[0]);
                    for v in &t.t[1..] { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
                    let cx = x.clamp(lo[0], hi[0]);
                    let cy = y.clamp(lo[1], hi[1]);
                    (cx - x).powi(2) + (cy - y).powi(2) < (rad + 100.0) * (rad + 100.0) && hi[2] > g - var - 100.0
                }).collect();
                let near: Vec<&&Tri> = near_all.iter().filter(|t| !t.terrain).collect();
                let near_terr: Vec<&&Tri> = near_all.iter().filter(|t| t.terrain).collect();
                let mut clutter = false;
                let steps = (rad / 100.0) as i32;
                'c: for u in -steps..=steps {
                    for w in -steps..=steps {
                        let (px, py) = (x + u as f32 * 100.0, y + w as f32 * 100.0);
                        if ((px - x).powi(2) + (py - y).powi(2)).sqrt() > rad { continue; }
                        let o = [px, py, tgt[2] + 5000.0];
                        let tz = near_terr.iter().filter_map(|t| hit(o, [0.0, 0.0, -1.0], &t.t)).map(|h| o[2] - h).fold(f32::MIN, f32::max);
                        let tz = if tz == f32::MIN { g } else { tz };
                        if near.iter().any(|t| hit(o, [0.0, 0.0, -1.0], &t.t).is_some_and(|h| o[2] - h > tz + 40.0)) {
                            clutter = true;
                            break 'c;
                        }
                    }
                }
                if clutter { fails[2] += 1; continue; }
                res.push((var, x, y, g, ((x - tgt[0]).powi(2) + (y - tgt[1]).powi(2)).sqrt()));
            }
        }
        eprintln!("rejected: no terrain ring {}, slope {}, meshes {}", fails[0], fails[1], fails[2]);
        res.sort_by(|a, b| a.4.partial_cmp(&b.4).unwrap());
        for (var, x, y, g, d) in res.iter().take(40) {
            println!("open r {rad}: SPAWN_AT={x:.0},{y:.0},{:.0}  terrain {g:.0} variation {var:.0} d {d:.0}", g + 41.5 + 60.0);
        }
        return;
    }
    let blocked = |o: V, p: V| -> bool {
        let d = sub(p, o);
        let l = len(d);
        let d = mul(d, 1.0 / l);
        tris.iter().any(|t| !t.target && hit(o, d, &t.t).is_some_and(|h| h < l - 5.0))
    };
    let mut out = Vec::new();
    let mut ang = 0.0f32;
    while ang < 360.0 {
        let mut d = dmin;
        while d <= dmax + 1.0 {
            let (s, c) = ang.to_radians().sin_cos();
            let (x, y) = (tgt[0] + c * d, tgt[1] + s * d);
            let col = column(x, y);
            let terr = col.iter().find(|h| h.2).map(|h| h.0);
            // standable: upward-ish normal, nothing within 2 m above
            let mut best: Option<(f32, f32)> = None;
            for (i, h) in col.iter().enumerate() {
                if h.1 < 0.7 { continue; }
                let above = if i == 0 { f32::MAX } else { col[i - 1].0 };
                if above - h.0 < 200.0 { continue; }
                if best.is_none_or(|b| (h.0 - target_base).abs() < (b.0 - target_base).abs()) { best = Some((h.0, above - h.0)); }
            }
            if let Some((g, roof)) = best {
                let centre = [x, y, g + 41.5];
                let eye = [x, y, g + 41.5 + 35.5];
                // the cylinder (r 15) must be free: 8 short horizontal rays at knee and head
                let tight = [20.0f32, 70.0].iter().any(|&z| (0..8).any(|k| {
                    let a2 = k as f32 * std::f32::consts::FRAC_PI_4;
                    let o = [x, y, g + z];
                    tris.iter().any(|t| hit(o, [a2.cos(), a2.sin(), 0.0], &t.t).is_some_and(|h| h < 20.0))
                }));
                let vis = samples.iter().filter(|s| !blocked(eye, **s)).count();
                let to = sub(samples[0], eye);
                let yaw = to[1].atan2(to[0]).to_degrees();
                let pitch = to[2].atan2((to[0] * to[0] + to[1] * to[1]).sqrt()).to_degrees();
                out.push((vis, tight, ang, d, centre, yaw, pitch, g, terr, roof));
            }
            d += 200.0;
        }
        ang += step;
    }
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.3.partial_cmp(&b.3).unwrap()));
    println!("target base z {target_base:.0}, samples {}", samples.len());
    for (vis, tight, ang, d, c, yaw, pitch, g, terr, roof) in out.iter().take(std::env::var("VS_N").ok().and_then(|v| v.parse().ok()).unwrap_or(40)) {
        println!("vis {vis}/{} {} dir {ang:>3.0} d {d:>4.0}  SPAWN_AT={:.0},{:.0},{:.0},{:.0}  pitch {pitch:>5.1}  ground {g:.0} terrain {:?} {}", samples.len(), if *tight { "TIGHT" } else { "free " }, c[0], c[1], c[2] + 5.0, yaw, terr.map(|t| t.round()), if *roof < 1000.0 { format!("ROOF {roof:.0}") } else { "open".into() });
    }
}
