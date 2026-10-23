//! Dev: SoundNodeWave / SoundCue names (with durations of waves) matching a pattern, across packages.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pats: Vec<String> = a[1].split(',').map(|s| s.to_lowercase()).collect();
    for f in &a[2..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            let c = p.export_class(e);
            if c != "SoundNodeWave" && c != "SoundCue" { continue; }
            let path = p.object_path(i as i32 + 1);
            let l = path.to_lowercase();
            if !pats.iter().any(|x| l.contains(x.as_str())) { continue; }
            let d = if c == "SoundNodeWave" { p.wave_duration(i).unwrap_or(0.0) } else { 0.0 };
            println!("{} {c} {path} {d:.2}", f.rsplit('/').next().unwrap());
        }
    }
}
