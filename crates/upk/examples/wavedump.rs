//! Dev: write a SoundNodeWave's Ogg data to a file (local analysis only).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let (pk, rest) = a[2].split_once('.').unwrap();
    let p = lib.open(pk).unwrap();
    let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SoundNodeWave" && p.object_path(i as i32 + 1).eq_ignore_ascii_case(rest)).expect("wave");
    let (ogg, dur) = p.read_wave_ogg(i).unwrap();
    println!("duration {dur}");
    std::fs::write(&a[3], ogg).unwrap();
}
