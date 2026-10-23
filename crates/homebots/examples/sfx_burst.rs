//! Offline audio test (no window, no device): a bot-fight burst rendered twice to WAV — through the
//! old path (one rodio Sink per sound in rodio's DynamicMixer, bevy_audio's chain; sfx.rs channel
//! limit + 3-frame x0.35 fade as of 2026-10-19; the f32 sum clamped like the output device does)
//! and through crate::mixer (sfx.rs since 2026-10-21) — and measured: clipped samples, peak,
//! discontinuities (|2nd difference| spikes), a single-voice stop click, CPU time per second of
//! audio.
//!
//!   cargo run --release -p homebots --example sfx_burst -- <HOMEFRONT dir> <out dir>
//!
//! The WAVs contain game sounds: for local listening only, never commit them.
#[path = "../src/mixer.rs"]
#[allow(dead_code)]
mod mixer;

use rodio::Source;
use std::collections::HashMap;
use std::sync::Arc;
use upk::sound::SoundNode;

const RATE: u32 = 48000;
const FPS: f32 = 30.0;
const SECONDS: f32 = 8.0;
/// bevy GlobalVolume (main.rs: 0.8)
const GLOBAL: f32 = 0.8;

struct Rng(u32);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 % 1_000_000) as f32 / 1_000_000.0
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[((self.f() * v.len() as f32) as usize).min(v.len() - 1)]
    }
}

fn waves_of(n: &SoundNode, out: &mut Vec<String>) {
    match n {
        SoundNode::Wave(p) => out.push(p.clone()),
        SoundNode::Random(c) | SoundNode::Mixer(c) | SoundNode::Concat(c) => c.iter().for_each(|x| waves_of(&x.1, out)),
        SoundNode::Modulator { child, .. } | SoundNode::Delay { child, .. } | SoundNode::Attenuation { child, .. } | SoundNode::Looping { child, .. } | SoundNode::WaveParam { child, .. } => waves_of(child, out),
        SoundNode::Switch { children, .. } => children.iter().for_each(|x| waves_of(x, out)),
        SoundNode::CrossFade(c) => c.iter().for_each(|x| waves_of(&x.3, out)),
        SoundNode::Empty => {}
    }
}

struct Lib {
    lib: upk::library::Library,
    pkgs: HashMap<String, Option<upk::Package>>,
    pcm: HashMap<String, Option<Arc<mixer::Pcm>>>,
}

impl Lib {
    fn pkg(&mut self, n: &str) -> Option<&upk::Package> {
        let k = n.to_ascii_lowercase();
        if !self.pkgs.contains_key(&k) {
            let p = self.lib.open(n);
            self.pkgs.insert(k.clone(), p);
        }
        self.pkgs.get(&k)?.as_ref()
    }
    fn wave(&mut self, path: &str) -> Option<Arc<mixer::Pcm>> {
        if let Some(p) = self.pcm.get(path) {
            return p.clone();
        }
        let (pk, rest) = path.split_once('.')?;
        let p = self.pkg(pk).and_then(|p| {
            let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SoundNodeWave" && p.object_path(i as i32 + 1).eq_ignore_ascii_case(rest))?;
            let (ogg, _) = p.read_wave_ogg(i)?;
            mixer::ogg_to_pcm(&ogg).map(Arc::new)
        });
        self.pcm.insert(path.to_string(), p.clone());
        p
    }
    /// cues of a package whose name contains one of `pat`: (waves, max concurrent, volume,
    /// attenuation (min, max radius UE units, model, dB at max) of the first Attenuation node)
    fn cues(&mut self, pk: &str, pat: &[&str], limit: usize) -> Vec<(String, Vec<Arc<mixer::Pcm>>, i32, f32, (f32, f32, u8, f32))> {
        let Some(p) = self.pkg(pk) else { return Vec::new() };
        let mut found = Vec::new();
        for i in 0..p.exports.len() {
            if p.export_class(&p.exports[i]) != "SoundCue" {
                continue;
            }
            let name = p.object_path(i as i32 + 1);
            let l = name.to_ascii_lowercase();
            if pat.iter().any(|x| l.contains(x)) && !l.contains("1p") && !l.contains("silenced") && !l.contains("atrl") && !l.contains("jav") {
                if let Some(c) = p.read_sound_cue(i, pk) {
                    found.push((name, c));
                }
            }
        }
        let mut out = Vec::new();
        for (name, c) in found {
            let mut w = Vec::new();
            waves_of(&c.root, &mut w);
            w.sort();
            w.dedup();
            let pcm: Vec<Arc<mixer::Pcm>> = w.iter().filter_map(|x| self.wave(x)).filter(|p| p.frames > 0).collect();
            fn att(n: &SoundNode) -> Option<(f32, f32, u8, f32)> {
                match n {
                    SoundNode::Attenuation { radius_min, radius_max, model, db_at_max, .. } => Some((*radius_min, *radius_max, *model, *db_at_max)),
                    SoundNode::Random(c) | SoundNode::Mixer(c) | SoundNode::Concat(c) => c.iter().find_map(|x| att(&x.1)),
                    SoundNode::Modulator { child, .. } | SoundNode::Delay { child, .. } | SoundNode::Looping { child, .. } | SoundNode::WaveParam { child, .. } => att(child),
                    SoundNode::Switch { children, .. } => children.iter().find_map(att),
                    SoundNode::CrossFade(c) => c.iter().find_map(|x| att(&x.3)),
                    _ => None,
                }
            }
            if !pcm.is_empty() {
                out.push((name, pcm, c.max_concurrent, c.volume, att(&c.root).unwrap_or((400.0, 5000.0, 0, -60.0))));
            }
            if out.len() >= limit {
                break;
            }
        }
        out
    }
}

/// What both paths are asked to do in one frame.
enum Ev {
    Play { key: u64, pcm: Arc<mixer::Pcm>, gain: f32, speed: f32, looped: bool },
    Gain(u64, f32),
    Stop(u64),
}

trait Backend {
    fn frame(&mut self, evs: &[Ev], out: &mut Vec<f32>, frames: usize);
}

/// Old path: rodio Sink per sound (bevy_audio: Sink::try_new = new_idle + mixer), sfx.rs
/// channel limit (lowest volume one-shots first, then oldest) and frame-wise fade (3 x 0.35).
struct Old {
    ctl: Arc<rodio::dynamic_mixer::DynamicMixerController<f32>>,
    mix: rodio::dynamic_mixer::DynamicMixer<f32>,
    sinks: HashMap<u64, (rodio::Sink, bool, u64)>,
    fading: HashMap<u64, u8>,
    serial: u64,
}

impl Old {
    fn new() -> Old {
        let (ctl, mix) = rodio::dynamic_mixer::mixer::<f32>(2, RATE);
        Old { ctl, mix, sinks: HashMap::new(), fading: HashMap::new(), serial: 0 }
    }
}

impl Backend for Old {
    fn frame(&mut self, evs: &[Ev], out: &mut Vec<f32>, frames: usize) {
        // stopping voices of earlier frames: fade x0.35 per frame, 3 frames, then stop
        let mut stopped = Vec::new();
        for (k, n) in self.fading.iter_mut() {
            let Some((s, ..)) = self.sinks.get(k) else { continue };
            let v = s.volume();
            if *n < 3 && v > 0.01 {
                *n += 1;
                s.set_volume(v * 0.35);
            } else {
                s.stop();
                stopped.push(*k);
            }
        }
        for k in stopped {
            self.fading.remove(&k);
            self.sinks.remove(&k);
        }
        // channel limit (before this frame's starts, as in sfx.rs)
        let mut live: Vec<(u64, bool, f32, u64)> = self.sinks.iter().filter(|(k, _)| !self.fading.contains_key(k)).map(|(k, (s, l, n))| (*k, *l, s.volume(), *n)).collect();
        if live.len() > 64 {
            live.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.partial_cmp(&b.2).unwrap()).then(a.3.cmp(&b.3)));
            for (k, ..) in live.iter().take(live.len() - 64) {
                self.fading.insert(*k, 0);
            }
        }
        for e in evs {
            match e {
                Ev::Play { key, pcm, gain, speed, looped } => {
                    let (s, q) = rodio::Sink::new_idle();
                    self.ctl.add(q);
                    let b = rodio::buffer::SamplesBuffer::new(pcm.ch, pcm.rate, pcm.data.clone());
                    if *looped {
                        s.append(b.repeat_infinite());
                    } else {
                        s.append(b);
                    }
                    s.set_volume(gain.min(1.5) * GLOBAL);
                    s.set_speed(*speed);
                    self.serial += 1;
                    self.sinks.insert(*key, (s, *looped, self.serial));
                }
                Ev::Gain(k, g) => {
                    if let Some((s, ..)) = self.sinks.get(k) {
                        if !self.fading.contains_key(k) {
                            s.set_volume(g.min(1.5) * GLOBAL);
                        }
                    }
                }
                Ev::Stop(k) => {
                    if self.sinks.contains_key(k) {
                        self.fading.entry(*k).or_insert(0);
                    }
                }
            }
        }
        // finished one-shots (bevy cleanup_finished_audio: sink.empty() -> despawn)
        self.sinks.retain(|_, (s, ..)| !s.empty());
        out.clear();
        for _ in 0..frames * 2 {
            out.push(self.mix.next().unwrap_or(0.0));
        }
    }
}

struct New {
    h: mixer::MixerHandle,
    m: mixer::Mixer,
    ids: HashMap<u64, mixer::VoiceId>,
    /// limiter gain reduction per block (dB)
    gr: Vec<f32>,
}

impl New {
    fn new() -> New {
        let h = mixer::MixerHandle::default();
        let m = h.mixer(RATE);
        New { h, m, ids: HashMap::new(), gr: Vec::new() }
    }
}

impl Backend for New {
    fn frame(&mut self, evs: &[Ev], out: &mut Vec<f32>, frames: usize) {
        for e in evs {
            match e {
                Ev::Play { key, pcm, gain, speed, looped } => {
                    let w = mixer::ready(mixer::Pcm { rate: pcm.rate, ch: pcm.ch, frames: pcm.frames, data: pcm.data.clone() });
                    let id = self.h.play(w, *gain, 0, *speed, *looped);
                    self.ids.insert(*key, id);
                }
                Ev::Gain(k, g) => {
                    if let Some(id) = self.ids.get(k) {
                        self.h.set_gain(*id, *g);
                    }
                }
                Ev::Stop(k) => {
                    if let Some(id) = self.ids.remove(k) {
                        self.h.stop(id);
                    }
                }
            }
        }
        out.clear();
        out.resize(frames * 2, 0.0);
        // the device pulls blocks of 256 frames (sfx.rs BUS_BLOCK)
        for c in out.chunks_mut(512) {
            self.m.render(c);
            self.gr.push(self.h.levels().1);
        }
        out.iter_mut().for_each(|x| *x *= GLOBAL);
    }
}

/// The bot fight: `bots` shooters at 3..60 m firing bursts of their weapon's 3P cue (restart at
/// MaxConcurrentPlayCount: the oldest instance of that cue is stopped), impacts near the
/// listener, footsteps, and two moving looped sounds whose volume follows the distance per frame.
fn script(lib: &mut Lib, bots: usize) -> Vec<Vec<Ev>> {
    let shots = lib.cues("A2_Weapons_C", &["fire3p", "3p_fire", "fire_3p"], 12);
    let impacts = lib.cues("A2_ProjctileImpact_C", &["impact", "bullet", "dirt", "concrete", "metal"], 8);
    let steps = lib.cues("A2_CharacterMovement_C", &["step", "foot", "run", "walk"], 6);
    let loops = lib.cues("A2_M1114_C", &["idle", "engine", "loop"], 2);
    eprintln!("cues: {} shot, {} impact, {} step, {} loop", shots.len(), impacts.len(), steps.len(), loops.len());
    for s in &shots {
        eprintln!("  {} max {} vol {:.2} waves {} att {:?}", s.0, s.2, s.3, s.1.len(), s.4);
    }
    let mut r = Rng(0x2468_ace1);
    // Weapons3P 1.5 x Weapons 1.2 x SFX 0.7 / Movement 1.5 x SFX .7 / ProjCollission (1) x .7
    let (g_w3p, g_mov, g_imp, g_veh) = (1.5 * 1.2 * 0.7, 1.5 * 0.7, 0.7, 0.7 * 0.7);
    let n = (SECONDS * FPS) as usize;
    let mut evs: Vec<Vec<Ev>> = (0..n).map(|_| Vec::new()).collect();
    let mut key = 0u64;
    let mut active: HashMap<usize, Vec<u64>> = HashMap::new();
    struct Bot {
        weapon: usize,
        dist: f32,
        next: f32,
        burst: u32,
        step: f32,
    }
    let mut bs: Vec<Bot> = (0..bots).map(|_| Bot { weapon: (r.f() * shots.len() as f32) as usize % shots.len().max(1), dist: 3.0 + r.f() * 57.0, next: r.f() * 0.5, burst: 0, step: r.f() }).collect();
    // sfx.rs Att::gain (UE3 SoundNodeAttenuation), distance in UE units (sfx.rs: bevy x 100)
    let ga = |a: (f32, f32, u8, f32), d: f32| -> f32 {
        let d = d * 100.0;
        if d <= a.0 {
            return 1.0;
        }
        if d >= a.1 {
            return 0.0;
        }
        let x = (d - a.0) / (a.1 - a.0).max(1.0);
        let g = match a.2 {
            1 => -0.5 * x.max(1e-6).ln(),
            2 => 0.02 / x.max(1e-6),
            3 => 1.0 + 0.5 * (1.0 - x).max(1e-6).ln(),
            4 => 10f32.powf(x * a.3.min(0.0) / 20.0),
            _ => 1.0 - x,
        };
        g.clamp(0.0, 1.0)
    };
    let att = |d: f32| ga((400.0, 5000.0, 0, -60.0), d);
    let mut lps: Vec<(u64, f32)> = Vec::new();
    for (i, l) in loops.iter().enumerate().take(2) {
        key += 1;
        evs[0].push(Ev::Play { key, pcm: l.1[0].clone(), gain: g_veh * att(20.0), speed: 1.0, looped: true });
        lps.push((key, 20.0 + 10.0 * i as f32));
    }
    for f in 0..n {
        let t = f as f32 / FPS;
        for (k, d) in lps.iter_mut() {
            *d = (*d + (r.f() - 0.5) * 3.0).clamp(2.0, 70.0);
            evs[f].push(Ev::Gain(*k, g_veh * att(*d)));
        }
        if shots.is_empty() {
            continue;
        }
        for b in bs.iter_mut() {
            b.dist = (b.dist + (r.f() - 0.5) * 0.4).clamp(3.0, 60.0);
            if t >= b.step {
                b.step = t + 0.35 + r.f() * 0.1;
                if !steps.is_empty() && b.dist < 25.0 {
                    let s = r.pick(&steps);
                    key += 1;
                    evs[f].push(Ev::Play { key, pcm: r.pick(&s.1).clone(), gain: g_mov * s.3 * ga(s.4, b.dist), speed: 0.95 + r.f() * 0.1, looped: false });
                }
            }
            if t < b.next {
                continue;
            }
            // a shot: 10 / s in bursts of 3..8, pauses 0.3..1.2 s
            if b.burst == 0 {
                b.burst = 3 + (r.f() * 6.0) as u32;
            }
            b.burst -= 1;
            b.next = t + if b.burst == 0 { 0.3 + r.f() * 0.9 } else { 0.1 };
            let c = &shots[b.weapon];
            let act = active.entry(b.weapon).or_default();
            if c.2 > 0 && act.len() >= c.2 as usize {
                let old = act.remove(0);
                evs[f].push(Ev::Stop(old));
            }
            key += 1;
            act.push(key);
            evs[f].push(Ev::Play { key, pcm: r.pick(&c.1).clone(), gain: g_w3p * c.3 * ga(c.4, b.dist) * (0.9 + 0.1 * r.f()), speed: 0.95 + r.f() * 0.1, looped: false });
            if !impacts.is_empty() && r.f() < 0.5 {
                let s = r.pick(&impacts);
                key += 1;
                let fi = (f + 1 + (r.f() * 3.0) as usize).min(n - 1);
                evs[fi].push(Ev::Play { key, pcm: r.pick(&s.1).clone(), gain: g_imp * s.3 * ga(s.4, 2.0 + 10.0 * r.f()), speed: 1.0, looped: false });
            }
        }
    }
    evs
}

fn render(b: &mut dyn Backend, evs: &[Vec<Ev>]) -> (Vec<f32>, f64) {
    let per = (RATE as f32 / FPS) as usize;
    let mut all = Vec::with_capacity(evs.len() * per * 2);
    let mut buf = Vec::new();
    let t0 = std::time::Instant::now();
    for e in evs {
        b.frame(e, &mut buf, per);
        all.extend_from_slice(&buf);
    }
    let cpu = t0.elapsed().as_secs_f64() / (evs.len() as f64 / FPS as f64);
    (all, cpu)
}

struct Stats {
    clipped: usize,
    peak: f32,
    spikes: usize,
    max_d2: f32,
}

/// clipped: |x| >= 1 (the device clamps); spikes: |x[n] - 2x[n-1] + x[n-2]| > 0.5 after the clamp
fn stats(x: &[f32]) -> Stats {
    let mut s = Stats { clipped: 0, peak: 0.0, spikes: 0, max_d2: 0.0 };
    for c in 0..2 {
        let ch: Vec<f32> = x.iter().skip(c).step_by(2).copied().collect();
        for (i, v) in ch.iter().enumerate() {
            s.peak = s.peak.max(v.abs());
            if v.abs() >= 1.0 {
                s.clipped += 1;
            }
            if i >= 2 {
                let y = |k: usize| ch[k].clamp(-1.0, 1.0);
                let d2 = (y(i) - 2.0 * y(i - 1) + y(i - 2)).abs();
                s.max_d2 = s.max_d2.max(d2);
                if d2 > 0.5 {
                    s.spikes += 1;
                }
            }
        }
    }
    s
}

fn write_wav(path: &std::path::Path, x: &[f32]) {
    let mut w = Vec::with_capacity(44 + x.len() * 2);
    let len = (x.len() * 2) as u32;
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&len.to_le_bytes());
    for v in x {
        w.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, w).unwrap();
}

/// One looped tone voice, its gain changed every frame and then stopped mid-wave: the largest
/// |2nd difference| (a pure 220 Hz sine at 0.5 has 0.0052; anything well above is a step).
fn stop_click(b: &mut dyn Backend) -> f32 {
    let n = 4800;
    let data: Vec<f32> = (0..n).map(|i| (i as f32 / 48000.0 * 220.0 * std::f32::consts::TAU).sin()).collect();
    let pcm = Arc::new(mixer::Pcm { rate: 48000, ch: 1, frames: n, data });
    let mut evs: Vec<Vec<Ev>> = (0..30).map(|_| Vec::new()).collect();
    evs[0].push(Ev::Play { key: 1, pcm, gain: 0.5 / GLOBAL, speed: 1.0, looped: true });
    for f in 3..12 {
        evs[f].push(Ev::Gain(1, (0.5 - 0.03 * (f - 3) as f32) / GLOBAL));
    }
    evs[15].push(Ev::Stop(1));
    let (x, _) = render(b, &evs);
    let ch: Vec<f32> = x.iter().step_by(2).copied().collect();
    (2..ch.len()).map(|i| (ch[i] - 2.0 * ch[i - 1] + ch[i - 2]).abs()).fold(0.0, f32::max)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = a.get(1).cloned().unwrap_or("/mnt/user-data/uploads/HOMEFRONT".into());
    let out = std::path::PathBuf::from(a.get(2).cloned().unwrap_or("logs/sfx_burst".into()));
    let bots: usize = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(16);
    std::fs::create_dir_all(&out).unwrap();
    let mut lib = Lib { lib: upk::library::Library::scan(&root).expect("game dir"), pkgs: HashMap::new(), pcm: HashMap::new() };
    let evs = script(&mut lib, bots);
    let plays: usize = evs.iter().flatten().filter(|e| matches!(e, Ev::Play { .. })).count();
    let stops: usize = evs.iter().flatten().filter(|e| matches!(e, Ev::Stop(_))).count();
    println!("{bots} bots, {SECONDS} s at {FPS} fps: {plays} sounds started, {stops} restart cuts");
    for (name, b) in [("before_rodio", &mut Old::new() as &mut dyn Backend), ("after_mixer", &mut New::new() as &mut dyn Backend)] {
        let (x, cpu) = render(b, &evs);
        let s = stats(&x);
        write_wav(&out.join(format!("{name}.wav")), &x);
        println!("{name:13} clipped {:7} samples ({:.2} %), peak {:.2}, d2 spikes {:6}, max d2 {:.2}, cpu {:.1} ms per s of audio", s.clipped, 100.0 * s.clipped as f32 / x.len() as f32, s.peak, s.spikes, s.max_d2, cpu * 1000.0);
    }
    let mut nb = New::new();
    let _ = render(&mut nb, &evs);
    let mut g = nb.gr.clone();
    g.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f32| g[((g.len() - 1) as f32 * p) as usize];
    println!("limiter gain reduction per 5 ms block: median {:.1} dB, p95 {:.1} dB, max {:.1} dB, blocks limiting {:.0} %", q(0.5), q(0.95), q(1.0), 100.0 * g.iter().filter(|x| **x > 0.1).count() as f32 / g.len() as f32);
    println!("stop / gain-step click (sine 220 Hz, max |d2|; clean sine = 0.005): before {:.3}, after {:.3}", stop_click(&mut Old::new()), stop_click(&mut New::new()));
}
