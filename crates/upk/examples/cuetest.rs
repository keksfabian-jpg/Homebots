//! Dev: SoundCue graph and the Ogg streams of its waves.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    let (pk, rest) = a[2].split_once('.').unwrap();
    let p = lib.open(pk).unwrap();
    let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SoundCue" && p.object_path(i as i32 + 1).eq_ignore_ascii_case(rest)).unwrap();
    let c = p.read_sound_cue(i, pk).unwrap();
    println!("{:#?}", c);
    fn waves(n: &upk::sound::SoundNode, out: &mut Vec<String>) {
        use upk::sound::SoundNode::*;
        match n { Wave(p) => out.push(p.clone()), Random(c) | Mixer(c) => for (_, x) in c { waves(x, out) }, Modulator { child, .. } | Delay { child, .. } | Attenuation { child, .. } | Looping { child, .. } => waves(child, out), Switch { children, .. } => for x in children { waves(x, out) }, Empty => {} }
    }
    let mut w = Vec::new();
    waves(&c.root, &mut w);
    for path in w {
        let (pk, rest) = path.split_once('.').unwrap();
        let Some(q) = lib.open(pk) else { println!("{path}: package missing"); continue };
        let Some(j) = q.exports.iter().enumerate().position(|(j, e)| q.export_class(e) == "SoundNodeWave" && q.object_path(j as i32 + 1).eq_ignore_ascii_case(rest)) else { println!("{path}: wave missing"); continue };
        match q.read_wave_ogg(j) { Some((d, dur)) => println!("{path}: {} bytes ogg, {dur}s, magic {:?}, last {:?}", d.len(), &d[..4], &d[d.len()-4..]), None => println!("{path}: no ogg") }
    }
}
