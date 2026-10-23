//! Dev: dwords of a StaticMesh export after its LOD bulk data (Homefront's collision-model
//! data, see upk mesh.rs static_mesh_collision_model). Usage: coltail <map folder> <mesh substring> [n dwords]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(64);
    let mut files: Vec<_> = std::fs::read_dir(&a[1]).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "map")).collect();
    files.sort();
    let mut seen = std::collections::HashSet::new();
    for f in files {
        let Ok(p) = upk::Package::open(&f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "StaticMesh" {
                continue;
            }
            let path = p.object_path(i as i32 + 1);
            if !path.to_ascii_lowercase().contains(&a[2].to_ascii_lowercase()) || !seen.insert(path.clone()) {
                continue;
            }
            let Some(t) = p.static_mesh_tail(i) else {
                let mut r = p.export_reader(i);
                p.read_props(&mut r);
                let b = p.export_bytes(e);
                let w: Vec<String> = b[r.pos..].chunks(4).take(n).map(|c| (u32::from_le_bytes([c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0), *c.get(3).unwrap_or(&0)]) as i32).to_string()).collect();
                println!("{path}: tail parse failed; after props: {}", w.join(" "));
                continue;
            };
            let b = p.export_bytes(e);
            let w: Vec<String> = b[t..].chunks(4).take(n).map(|c| {
                let v = u32::from_le_bytes([c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0), *c.get(3).unwrap_or(&0)]);
                let fl = f32::from_bits(v);
                if v > 0x3000_0000 && v < 0xd000_0000 && fl.is_finite() && fl.abs() > 1e-3 && fl.abs() < 1e6 { format!("{fl:.2}f") } else { format!("{}", v as i32) }
            }).collect();
            println!("{path} ({}, tail at {t} left {}): {}", f.file_name().unwrap().to_string_lossy(), b.len() - t, w.join(" "));
            let d = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            let mut o = t;
            while o + 8 <= b.len() {
                if d(o) == 8 && d(o + 4) > 0 && d(o + 4) < 100000 && o >= 8 {
                    let ctx: Vec<String> = (o.saturating_sub(64)..o + 8).step_by(4).map(|k| (d(k) as i32).to_string()).collect();
                    println!("   (8,{}) at +{}: {}", d(o + 4), o - t, ctx.join(" "));
                }
                o += 4;
            }
            let ctx: Vec<String> = ((b.len() - 80).max(t)..b.len()).step_by(4).map(|k| (d(k) as i32).to_string()).collect();
            println!("   end: {}", ctx.join(" "));
            if let Some(cm) = p.static_mesh_collision_model(i) {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for v in cm.iter().flatten() {
                    for k in 0..3 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
                let rb = p.read_static_mesh(i).map(|m| (m.origin, m.extent));
                println!("   collision model: {} triangles, box {lo:?}..{hi:?}; render bounds (origin, extent) {rb:?}", cm.len());
            }
        }
    }
}
