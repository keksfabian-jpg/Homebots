//! Dev: PostProcessVolumes of a map with priority and bounds (Package::post_process_volumes).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for v in p.post_process_volumes() {
        println!("{} prio {} min {:?} max {:?} tint_mid {:?} desat {}", v.name, v.priority, v.min, v.max, v.settings.tint_mid, v.settings.desaturation);
    }
}
