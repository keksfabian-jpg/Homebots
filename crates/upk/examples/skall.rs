//! Dev: read every SkeletalMesh export; print failures.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mut ok, mut bad) = (0, 0);
    for f in &a[1..] {
        let Ok(p) = upk::Package::open(f) else { continue };
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "SkeletalMesh" { continue; }
            if p.read_skeletal_mesh(i).is_some() { ok += 1 } else { bad += 1; println!("FAIL {f} {}", p.object_path(i as i32 + 1)); }
        }
    }
    println!("ok {ok} bad {bad}");
}
