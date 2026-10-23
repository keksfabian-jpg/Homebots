//! Dev: compact SoundCue graph (switches, loops, attenuation radii) by cue object path.
use upk::sound::SoundNode::{self, *};
fn pr(n: &SoundNode, d: usize) {
    let i = "  ".repeat(d);
    match n {
        Wave(p) => println!("{i}wave {p}"),
        Random(c) => { println!("{i}random"); for (w, x) in c { println!("{i} w{w}"); pr(x, d + 1) } }
        Mixer(c) => { println!("{i}mixer"); for (w, x) in c { println!("{i} v{w}"); pr(x, d + 1) } }
        Modulator { volume, pitch, child } => { println!("{i}mod vol {volume:?} pitch {pitch:?}"); pr(child, d + 1) }
        Delay { delay, child } => { println!("{i}delay {delay:?}"); pr(child, d + 1) }
        Attenuation { radius_min, radius_max, model, db_at_max, child } => { println!("{i}atten {radius_min}..{radius_max} model {model} db {db_at_max}"); pr(child, d + 1) }
        Looping { forever, count, child } => { println!("{i}loop forever={forever} count={count}"); pr(child, d + 1) }
        Switch { param, children } => { println!("{i}switch {param}"); for (k, x) in children.iter().enumerate() { println!("{i} [{k}]"); pr(x, d + 1) } }
        Empty => println!("{i}empty"),
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lib = upk::library::Library::scan(&a[1]).unwrap();
    for path in &a[2..] {
        let (pk, rest) = path.split_once('.').unwrap();
        let p = lib.open(pk).unwrap();
        let Some(i) = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SoundCue" && p.object_path(i as i32 + 1).eq_ignore_ascii_case(rest)) else { println!("{path}: missing"); continue };
        let c = p.read_sound_cue(i, pk).unwrap();
        println!("== {path} vol {} pitch {} maxconc {}", c.volume, c.pitch, c.max_concurrent);
        pr(&c.root, 1);
    }
}
