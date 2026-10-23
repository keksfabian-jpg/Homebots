//! Dev: GameMode_PlayerStart groups of a map (count, mean location).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m: std::collections::BTreeMap<String, (usize, [f32; 3])> = Default::default();
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "GameMode_PlayerStart" { continue; }
            let Some(pr) = p.export_props(i) else { continue };
            let g = pr.enum_name("Group").unwrap_or("").to_string();
            let l = pr.vector("Location").unwrap_or([0.0; 3]);
            let x = m.entry(g).or_default();
            x.0 += 1;
            for k in 0..3 { x.1[k] += l[k]; }
        }
    }
    for (g, (n, s)) in m { println!("{g}: {n} mean {:?}", s.map(|v| (v / n as f32).round())); }
}
