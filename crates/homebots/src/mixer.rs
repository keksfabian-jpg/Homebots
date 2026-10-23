//! Software mixer for the in-game sounds (sfx.rs). One output stream for all voices instead of one
//! rodio Sink per sound (crackle / ticking hunt 2026-10-21, user: "Knacken und Ticken im Ton, wenn
//! Bots gegeneinander kämpfen"). Measured with examples/sfx_burst.rs (bot-fight burst, old rodio
//! path vs this mixer), see MODLOG "Ton: Knacken / Ticken im Bot-Gefecht".
//!
//! Why: (1) rodio sums all sinks and the f32 sum goes to the device unlimited: dozens of Weapons3P
//! shots (group volume 1.5 x 1.2 x 0.7) hard-clipped at 1.0; (2) rodio applies a sink's volume as
//! a step every 5 ms, so the frame-wise fade of a stopped voice (x0.35 per frame) and every
//! per-frame distance update of a fire loop were steps in the waveform (clicks / zipper);
//! (3) every sink is a chain of ~10 iterator adapters with a mutex poll every 5 ms, per sample —
//! with 64 voices the audio callback is heavy for a handheld APU (underrun = crackle).
//!
//! Here: voices are pre-decoded f32 PCM, resampled with linear interpolation, every gain / speed
//! change is smoothed per sample, stops fade out linearly, the 64-channel limit fades the stolen
//! voice, and the bus goes through a look-ahead peak limiter plus a soft clipper.
//! The channel limit is from data ([XAudio2.XAudio2Device] MaxChannels=64); all time constants,
//! the limiter threshold and the steal order are NOT from data.
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Decoded sound: interleaved f32 samples.
pub struct Pcm {
    pub rate: u32,
    pub ch: u16,
    pub frames: usize,
    pub data: Vec<f32>,
}

impl Pcm {
    pub fn duration(&self) -> f32 {
        self.frames as f32 / self.rate.max(1) as f32
    }

    pub fn empty() -> Pcm {
        Pcm { rate: 44100, ch: 1, frames: 0, data: Vec::new() }
    }
}

/// A wave that may still be decoding (one-shots are decoded on a worker thread; a voice of a wave
/// not yet decoded waits silently and starts when the data is there).
pub type Wave = Arc<OnceLock<Pcm>>;

pub fn ready(p: Pcm) -> Wave {
    let w: Wave = Arc::new(OnceLock::new());
    let _ = w.set(p);
    w
}

/// Ogg Vorbis -> f32 PCM (lewton). End trim to the last page's granule position (the stream's
/// sample count = SoundNodeWave Duration x SampleRate; lewton returns the whole last packet, up to
/// 1012 samples more, which broke loop seams: click hunt 2026-10-19).
pub fn ogg_to_pcm(ogg: &[u8]) -> Option<Pcm> {
    let mut r = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(ogg)).ok()?;
    let ch = r.ident_hdr.audio_channels as u16;
    let rate = r.ident_hdr.audio_sample_rate;
    let mut pcm: Vec<i16> = Vec::new();
    let mut end = 0u64;
    while let Ok(Some(p)) = r.read_dec_packet_itl() {
        pcm.extend_from_slice(&p);
        if let Some(g) = r.get_last_absgp() {
            end = g;
        }
    }
    if ch == 0 || pcm.is_empty() {
        return None;
    }
    if end > 0 && (end as usize) * (ch as usize) < pcm.len() {
        pcm.truncate(end as usize * ch as usize);
    }
    let frames = pcm.len() / ch as usize;
    let data = pcm[..frames * ch as usize].iter().map(|&s| s as f32 / 32768.0).collect();
    Some(Pcm { rate, ch, frames, data })
}

/// [XAudio2.XAudio2Device] MaxChannels=64 (BaseEngine.ini / GCEngine.ini).
pub const MAX_CHANNELS: usize = 64;
/// Volume clamp of a voice (as before with bevy_audio). NOT from data.
const MAX_VOICE_GAIN: f32 = 1.5;
/// Gain / speed changes glide with this time constant (s). NOT from data.
const SMOOTH_S: f32 = 0.004;
/// Fade-in of a starting voice (s): hides a non-zero first sample. NOT from data.
const ATTACK_S: f32 = 0.0007;
/// Fade-out of a stopped / stolen voice (s). NOT from data.
const RELEASE_S: f32 = 0.020;
/// A voice whose wave has not finished decoding after this long is dropped (s). NOT from data.
const MAX_WAIT_S: f32 = 2.0;
/// Limiter: look-ahead (s), threshold, release (s); soft clip knee. NOT from data.
const LIM_LOOKAHEAD_S: f32 = 0.0015;
const LIM_THRESHOLD: f32 = 0.85;
const LIM_RELEASE_S: f32 = 0.3;
const CLIP_KNEE: f32 = 0.9;
/// Bus gain before the limiter (-6 dB): the loudest single voice (3P fire cue 1.5 x Weapons3P
/// group 1.26, clamped to MAX_VOICE_GAIN) stays under the threshold, so the limiter only acts on
/// sums. The game itself mixes with DistortionMaster .3 x FL2SubMaster .4 (= -18.4 dB) of headroom,
/// which sfx.rs divides out; this is NOT from data (a compromise between that and the loudness of
/// single sounds the user is used to).
const BUS_GAIN: f32 = 0.5;

pub type VoiceId = u64;

enum Cmd {
    Play { id: VoiceId, wave: Wave, gain: f32, cat: u8, speed: f32, looped: bool },
    Gain(VoiceId, f32),
    Speed(VoiceId, f32),
    Stop(VoiceId),
    Classes([f32; 5]),
}

#[derive(Default)]
struct Shared {
    cmds: Mutex<Vec<Cmd>>,
    next: AtomicU64,
    /// voices playing (not fading out) after the last block
    live: AtomicU32,
    /// voices stolen by the channel limit (total)
    culled: AtomicU64,
    /// largest bus peak before the limiter in the last block (x1000)
    peak_milli: AtomicU32,
    /// largest limiter gain reduction in the last block (dB x100)
    gr_centi_db: AtomicU32,
    /// bus samples that would have clipped (|x| >= 1) without the limiter (total)
    over: AtomicU64,
    /// a mixer is rendering (the output stream exists)
    running: AtomicBool,
}

/// Main-thread side of the mixer.
#[derive(Clone, Default)]
pub struct MixerHandle(Arc<Shared>);

impl MixerHandle {
    fn send(&self, c: Cmd) {
        if let Ok(mut q) = self.0.cmds.lock() {
            q.push(c);
        }
    }

    /// Start a voice. `gain` = data volume (the class factor `cat` is applied by the mixer).
    pub fn play(&self, wave: Wave, gain: f32, cat: u8, speed: f32, looped: bool) -> VoiceId {
        let id = self.0.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.send(Cmd::Play { id, wave, gain, cat, speed, looped });
        id
    }

    pub fn set_gain(&self, id: VoiceId, gain: f32) {
        self.send(Cmd::Gain(id, gain));
    }

    pub fn set_speed(&self, id: VoiceId, speed: f32) {
        self.send(Cmd::Speed(id, speed));
    }

    /// Fade out and drop a voice.
    pub fn stop(&self, id: VoiceId) {
        self.send(Cmd::Stop(id));
    }

    /// Options volume factors per class (0 sound, 1 music, 2 voice, 3 voip, 4 master only).
    pub fn set_classes(&self, c: [f32; 5]) {
        self.send(Cmd::Classes(c));
    }

    pub fn live(&self) -> u32 {
        self.0.live.load(Ordering::Relaxed)
    }

    pub fn culled(&self) -> u64 {
        self.0.culled.load(Ordering::Relaxed)
    }

    /// (bus peak before the limiter, limiter gain reduction dB) of the last block
    pub fn levels(&self) -> (f32, f32) {
        (self.0.peak_milli.load(Ordering::Relaxed) as f32 / 1000.0, self.0.gr_centi_db.load(Ordering::Relaxed) as f32 / 100.0)
    }

    pub fn over(&self) -> u64 {
        self.0.over.load(Ordering::Relaxed)
    }

    pub fn running(&self) -> bool {
        self.0.running.load(Ordering::Relaxed)
    }

    /// The audio-thread side, rendering stereo at `rate`.
    pub fn mixer(&self, rate: u32) -> Mixer {
        Mixer::new(self.clone(), rate)
    }
}

struct Voice {
    id: VoiceId,
    wave: Wave,
    cat: u8,
    looped: bool,
    pos: f64,
    gain: f32,
    gain_to: f32,
    speed: f32,
    speed_to: f32,
    /// samples of the start fade done (ATTACK)
    attack: u32,
    /// fading out: gain decrement per sample
    release: Option<f32>,
    waited: f32,
    serial: u64,
}

/// Audio-thread side: renders interleaved stereo blocks.
pub struct Mixer {
    h: MixerHandle,
    rate: u32,
    voices: Vec<Voice>,
    classes: [f32; 5],
    k_smooth: f32,
    attack_len: u32,
    release_len: f32,
    serial: u64,
    lim: Limiter,
    pending: Vec<Cmd>,
}

impl Mixer {
    pub fn new(h: MixerHandle, rate: u32) -> Mixer {
        let r = rate as f32;
        h.0.running.store(true, Ordering::Relaxed);
        Mixer {
            h,
            rate,
            voices: Vec::with_capacity(MAX_CHANNELS * 2),
            classes: [1.0; 5],
            k_smooth: 1.0 - (-1.0 / (SMOOTH_S * r)).exp(),
            attack_len: ((ATTACK_S * r) as u32).max(1),
            release_len: (RELEASE_S * r).max(1.0),
            serial: 0,
            lim: Limiter::new(rate),
            pending: Vec::new(),
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    fn target(&self, gain: f32, cat: u8) -> f32 {
        (gain * self.classes[(cat as usize).min(4)]).clamp(0.0, MAX_VOICE_GAIN)
    }

    fn apply(&mut self) {
        // never block the audio thread: if the main thread holds the queue, take it next block
        if let Ok(mut q) = self.h.0.cmds.try_lock() {
            std::mem::swap(&mut *q, &mut self.pending);
        }
        for c in std::mem::take(&mut self.pending) {
            match c {
                Cmd::Play { id, wave, gain, cat, speed, looped } => {
                    // channel limit: steal the least important playing voice (one-shots before
                    // loops, then the quietest, then the oldest) with a fade
                    let live: Vec<usize> = (0..self.voices.len()).filter(|&i| self.voices[i].release.is_none()).collect();
                    if live.len() >= MAX_CHANNELS {
                        let v = live.into_iter().min_by(|&a, &b| {
                            let (x, y) = (&self.voices[a], &self.voices[b]);
                            x.looped.cmp(&y.looped).then(x.gain_to.partial_cmp(&y.gain_to).unwrap_or(std::cmp::Ordering::Equal)).then(x.serial.cmp(&y.serial))
                        });
                        if let Some(v) = v {
                            let g = self.voices[v].gain.max(1e-6);
                            self.voices[v].release = Some(g / self.release_len);
                            self.h.0.culled.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    self.serial += 1;
                    let t = self.target(gain, cat);
                    self.voices.push(Voice {
                        id,
                        wave,
                        cat,
                        looped,
                        pos: 0.0,
                        gain: t,
                        gain_to: gain,
                        speed: speed.clamp(0.25, 4.0),
                        speed_to: speed.clamp(0.25, 4.0),
                        attack: 0,
                        release: None,
                        waited: 0.0,
                        serial: self.serial,
                    });
                }
                Cmd::Gain(id, g) => {
                    if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
                        v.gain_to = g;
                    }
                }
                Cmd::Speed(id, s) => {
                    if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
                        v.speed_to = s.clamp(0.25, 4.0);
                    }
                }
                Cmd::Stop(id) => {
                    let rl = self.release_len;
                    if let Some(v) = self.voices.iter_mut().find(|v| v.id == id && v.release.is_none()) {
                        v.release = Some(v.gain.max(1e-6) / rl);
                    }
                }
                Cmd::Classes(c) => self.classes = c,
            }
        }
    }

    /// Mix the next `out.len() / 2` stereo frames into `out` (overwritten).
    pub fn render(&mut self, out: &mut [f32]) {
        self.apply();
        out.iter_mut().for_each(|x| *x = 0.0);
        let n = out.len() / 2;
        let block_s = n as f32 / self.rate as f32;
        let (k, al) = (self.k_smooth, self.attack_len);
        let classes = self.classes;
        let out_rate = self.rate as f64;
        self.voices.retain_mut(|v| {
            let Some(p) = v.wave.get() else {
                v.waited += block_s;
                return v.waited < MAX_WAIT_S && v.release.is_none();
            };
            if p.frames == 0 {
                return false;
            }
            let gt = (v.gain_to * classes[(v.cat as usize).min(4)]).clamp(0.0, MAX_VOICE_GAIN);
            let step = p.rate as f64 / out_rate;
            let fr = p.frames;
            let ch = p.ch as usize;
            let d = &p.data;
            for f in 0..n {
                match v.release {
                    Some(dec) => {
                        v.gain -= dec;
                        if v.gain <= 0.0 {
                            return false;
                        }
                    }
                    None => v.gain += (gt - v.gain) * k,
                }
                v.speed += (v.speed_to - v.speed) * k;
                let mut i = v.pos as usize;
                if i >= fr {
                    if !v.looped {
                        return false;
                    }
                    v.pos -= fr as f64 * (v.pos / fr as f64).floor();
                    i = (v.pos as usize).min(fr - 1);
                }
                let t = (v.pos - i as f64) as f32;
                let j = i + 1;
                let j = if j < fr { Some(j) } else if v.looped { Some(0) } else { None };
                let mut g = v.gain;
                if v.attack < al {
                    v.attack += 1;
                    g *= v.attack as f32 / al as f32;
                }
                if ch == 1 {
                    let a = d[i];
                    let b = j.map(|j| d[j]).unwrap_or(0.0);
                    let s = (a + (b - a) * t) * g;
                    out[2 * f] += s;
                    out[2 * f + 1] += s;
                } else {
                    let (a0, a1) = (d[i * ch], d[i * ch + 1]);
                    let (b0, b1) = j.map(|j| (d[j * ch], d[j * ch + 1])).unwrap_or((0.0, 0.0));
                    out[2 * f] += (a0 + (b0 - a0) * t) * g;
                    out[2 * f + 1] += (a1 + (b1 - a1) * t) * g;
                }
                v.pos += step * v.speed as f64;
            }
            true
        });
        out.iter_mut().for_each(|x| *x *= BUS_GAIN);
        let (peak, gr, over) = self.lim.process(out);
        let s = &self.h.0;
        s.live.store(self.voices.iter().filter(|v| v.release.is_none()).count() as u32, Ordering::Relaxed);
        s.peak_milli.store((peak * 1000.0) as u32, Ordering::Relaxed);
        s.gr_centi_db.store((gr * 100.0) as u32, Ordering::Relaxed);
        s.over.fetch_add(over, Ordering::Relaxed);
    }
}

/// Look-ahead peak limiter: the gain is the moving average (look-ahead length) of the sliding
/// minimum of the required gain, so it is down before a peak leaves the delay line; release is a
/// one-pole glide. A soft clipper above CLIP_KNEE keeps the output below 1.0 in any case.
struct Limiter {
    d: usize,
    delay: Vec<[f32; 2]>,
    /// sliding minimum of the required gain over the last d + 1 samples (the window must reach
    /// back to the sample that leaves the delay line now): monotonic queue (sample no., gain)
    want: std::collections::VecDeque<(u64, f32)>,
    n: u64,
    hold: Vec<f32>,
    hold_sum: f32,
    idx: usize,
    g: f32,
    k_rel: f32,
}

impl Limiter {
    fn new(rate: u32) -> Limiter {
        let d = ((LIM_LOOKAHEAD_S * rate as f32) as usize).max(4);
        Limiter { d, delay: vec![[0.0; 2]; d], want: std::collections::VecDeque::with_capacity(d + 2), n: 0, hold: vec![1.0; d], hold_sum: d as f32, idx: 0, g: 1.0, k_rel: 1.0 - (-1.0 / (LIM_RELEASE_S * rate as f32)).exp() }
    }

    /// Returns (input peak, max gain reduction dB, samples >= 1.0 at the input).
    fn process(&mut self, buf: &mut [f32]) -> (f32, f32, u64) {
        let mut peak = 0.0f32;
        let mut gmin = 1.0f32;
        let mut over = 0u64;
        for fr in buf.chunks_exact_mut(2) {
            let p = fr[0].abs().max(fr[1].abs());
            peak = peak.max(p);
            if fr[0].abs() >= 1.0 {
                over += 1;
            }
            if fr[1].abs() >= 1.0 {
                over += 1;
            }
            let w = if p > LIM_THRESHOLD { LIM_THRESHOLD / p } else { 1.0 };
            let i = self.idx;
            while self.want.back().is_some_and(|b| b.1 >= w) {
                self.want.pop_back();
            }
            self.want.push_back((self.n, w));
            while self.want.front().is_some_and(|f| f.0 + self.d as u64 + 1 <= self.n) {
                self.want.pop_front();
            }
            self.n += 1;
            let h = self.want.front().map(|f| f.1).unwrap_or(1.0);
            self.hold_sum += h - self.hold[i];
            self.hold[i] = h;
            let ga = (self.hold_sum / self.d as f32).min(1.0);
            self.g = if ga < self.g { ga } else { self.g + (ga - self.g) * self.k_rel };
            gmin = gmin.min(self.g);
            let out = self.delay[i];
            self.delay[i] = [fr[0], fr[1]];
            self.idx = (i + 1) % self.d;
            fr[0] = soft_clip(out[0] * self.g);
            fr[1] = soft_clip(out[1] * self.g);
        }
        // the running sum drifts with float error: rebuild it now and then
        self.hold_sum = self.hold.iter().sum();
        (peak, -20.0 * gmin.max(1e-6).log10(), over)
    }
}

fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= CLIP_KNEE {
        return x;
    }
    let r = 1.0 - CLIP_KNEE;
    x.signum() * (CLIP_KNEE + r * ((a - CLIP_KNEE) / r).tanh())
}
