fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = upk::Package::open(&a[1]).unwrap();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "SoundNodeWave" { continue; }
        let path = p.object_path(i as i32 + 1);
        if !path.to_lowercase().contains(&a[2].to_lowercase()) { continue; }
        let o = p.read_wave_ogg(i);
        println!("{path}: ogg {:?} dur {:?} size {}", o.as_ref().map(|x| x.0.len()), p.wave_duration(i), e.serial_size);
    }
}
