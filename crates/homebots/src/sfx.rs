//! Sound effects: SoundCues from the game's audio packages (upk::sound), played with bevy_audio.
//!
//! Cues are requested by object path ("A2_Weapons_C.M4.SC__M4_ReloadRootA_000"); packages are
//! opened on first use. A cue graph is evaluated once per play (random picks, modulation,
//! delays); 3D cues are attenuated from the camera with the Attenuation node's MinRadius /
//! MaxRadius / DistanceModel (UE units). Automatic weapon fire cues (GCSoundNodeSwitch
//! PlayFireRelease) loop while shots come and play their release branch after the last one.
//! All voices are mixed by crate::mixer (one bevy_audio player for the whole bus, see there).
use crate::mixer::{self, MixerHandle, VoiceId, Wave};
use bevy::audio::{AddAudioSource, Decodable, PlaybackSettings};
use bevy::prelude::*;
use std::collections::HashMap;
use upk::sound::{SoundCueData, SoundNode};

pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SfxQueue>().init_resource::<Mix>().add_audio_source::<MixerBus>().add_systems(PostUpdate, play_sounds);
    }
}

/// The sound mixer's main-thread handle.
#[derive(Resource, Default, Clone)]
pub struct Mix(pub MixerHandle);

/// The mixer bus as a bevy_audio source: one AudioPlayer plays all in-game sounds.
#[derive(Asset, TypePath)]
pub struct MixerBus(MixerHandle, u32);

/// The bus as a rodio source (stereo f32 at the device rate; renders blocks of BUS_BLOCK frames).
pub struct BusSource {
    m: mixer::Mixer,
    buf: Vec<f32>,
    i: usize,
}

/// Frames per mixer block (~5 ms). NOT from data.
const BUS_BLOCK: usize = 256;

impl Iterator for BusSource {
    type Item = f32;
    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.i >= self.buf.len() {
            let mut b = std::mem::take(&mut self.buf);
            self.m.render(&mut b);
            self.buf = b;
            self.i = 0;
        }
        self.i += 1;
        Some(self.buf[self.i - 1])
    }
}

impl bevy::audio::Source for BusSource {
    fn current_frame_len(&self) -> Option<usize> {
        // the format never changes; rodio rebuilds its (pass-through) converter per "frame"
        Some(32768)
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.m.rate()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

impl Decodable for MixerBus {
    type DecoderItem = f32;
    type Decoder = BusSource;
    fn decoder(&self) -> BusSource {
        BusSource { m: self.0.mixer(self.1), buf: vec![0.0; BUS_BLOCK * 2], i: BUS_BLOCK * 2 }
    }
}

/// The output device's sample rate (the rate rodio's stream runs at: its default output config),
/// so the bus needs no resampling. 48 kHz if there is no device.
fn device_rate() -> u32 {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host().default_output_device().and_then(|d| d.default_output_config().ok()).map(|c| c.sample_rate().0).unwrap_or(48000)
}

/// Game install root (set by main).
#[derive(Resource, Clone)]
pub struct GameRoot(pub std::path::PathBuf);

#[derive(Clone, Debug)]
pub struct SfxReq {
    pub cue: String,
    /// world position (UE units); None = 2D (first person, UI)
    pub at: Option<Vec3>,
    pub volume: f32,
    /// a weapon shot: at MaxConcurrentPlayCount the oldest instance is cut and the new one plays
    /// (the game plays a weapon's fire cue on the weapon's own AudioComponent, which restarts;
    /// dropping the new one silenced fast M9 shots: SC_M9_Fire1P MaxConcurrentPlayCount 2)
    pub restart: bool,
}

/// A looping cue requested this frame: GCSoundNodeSwitch parameters (e.g. the vehicle engine's
/// "Gas") and a pitch factor applied to its looping layers.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopReq {
    pub cue: String,
    pub at: Option<Vec3>,
    pub params: Vec<(String, usize)>,
    pub pitch: f32,
    /// volume factors of looping layers by wave name substring (vehicle engines: the tyre and
    /// throttle loops follow speed / throttle; 1.0 for layers not listed)
    pub gains: Vec<(String, f32)>,
}

/// A sound wave played directly (no cue): "Package.Group.Name" path, volume (sound slider),
/// playback speed, start delay (s).
#[derive(Clone, Debug)]
pub struct WaveReq {
    pub wave: String,
    pub volume: f32,
    pub pitch: f32,
    pub delay: f32,
}

#[derive(Resource, Default)]
pub struct SfxQueue(pub Vec<SfxReq>, pub std::collections::HashMap<String, LoopReq>, pub Vec<WaveReq>, pub std::collections::HashMap<String, WaveReq>);

impl SfxQueue {
    /// Play a wave directly (2D).
    pub fn wave(&mut self, wave: &str, volume: f32, pitch: f32, delay: f32) {
        self.2.push(WaveReq { wave: wave.to_string(), volume, pitch, delay });
    }

    /// Decode a wave now without playing it (long external waves before they are needed).
    pub fn preload(&mut self, wave: &str) {
        self.2.push(WaveReq { wave: wave.to_string(), volume: -1.0, pitch: 1.0, delay: 0.0 });
    }

    /// Keep a wave looping this frame under `key` (2D; volume and pitch follow every frame, the
    /// loop stops in the first frame that does not request it).
    pub fn wave_loop(&mut self, key: &str, wave: &str, volume: f32, pitch: f32) {
        self.3.insert(key.to_string(), WaveReq { wave: wave.to_string(), volume, pitch, delay: 0.0 });
    }

    pub fn play(&mut self, cue: &str, at: Option<Vec3>) {
        self.play_volume(cue, at, 1.0);
    }

    /// `play` with a volume factor.
    pub fn play_volume(&mut self, cue: &str, at: Option<Vec3>, volume: f32) {
        if !cue.is_empty() {
            self.0.push(SfxReq { cue: cue.to_string(), at, volume, restart: false });
        }
    }

    /// Keep a looping cue playing this frame under `key` (a loop stops in the first frame that
    /// does not request it again), e.g. vehicle engines, looping weapon fire.
    pub fn looping(&mut self, key: &str, cue: &str, at: Option<Vec3>) {
        self.looping_params(key, cue, at, &[], 1.0);
    }

    /// `looping` with switch parameters (a change plays the one-shot layers that the new
    /// values add, e.g. a rev when "Gas" goes 0 -> 1) and a pitch factor for the loops.
    pub fn looping_params(&mut self, key: &str, cue: &str, at: Option<Vec3>, params: &[(&str, usize)], pitch: f32) {
        if !cue.is_empty() {
            self.1.insert(key.to_string(), LoopReq { cue: cue.to_string(), at, params: params.iter().map(|(n, v)| (n.to_string(), *v)).collect(), pitch, gains: Vec::new() });
        }
    }

    /// `looping_params` with per-layer volume factors (see LoopReq::gains).
    pub fn looping_layers(&mut self, key: &str, cue: &str, at: Option<Vec3>, params: &[(&str, usize)], pitch: f32, gains: &[(&str, f32)]) {
        if !cue.is_empty() {
            self.1.insert(
                key.to_string(),
                LoopReq { cue: cue.to_string(), at, params: params.iter().map(|(n, v)| (n.to_string(), *v)).collect(), pitch, gains: gains.iter().map(|(n, v)| (n.to_string(), *v)).collect() },
            );
        }
    }
}

/// Channel limit: crate::mixer::MAX_CHANNELS ([XAudio2.XAudio2Device] MaxChannels=64); the mixer
/// steals the least important voice with a fade (user 2026-10-18: "after playing for a few minutes
/// the sound gets scratchy").
#[derive(Default)]
struct SfxState {
    lib: Option<upk::library::Library>,
    pkgs: HashMap<String, Option<upk::Package>>,
    wave_lens: HashMap<String, f32>,
    /// listener distance (UE units) of the sound being evaluated (SoundNodeDistanceCrossFade);
    /// 0 for 2D sounds
    dist: f32,
    /// packages cooked into maps (see `cooked_package`)
    cooked: HashMap<String, Option<std::sync::Arc<upk::Package>>>,
    cues: HashMap<String, Option<SoundCueData>>,
    waves: HashMap<String, Option<(Wave, f32)>>,
    /// end times of the playing instances per cue (MaxConcurrentPlayCount)
    active: HashMap<String, Vec<(f64, Vec<VoiceId>)>>,
    /// weapon sound infos: GCSoundInfoWeapon path -> (Fire1P, Fire3P, ADSIn, ADSOut)
    infos: HashMap<String, [String; 4]>,
    delayed: Vec<(f32, Wave, f32, f32, u8)>,
    seed: u32,
    /// running loops: key -> (request, looping audio entities with base pitch, base volume, wave)
    loops: HashMap<String, (LoopReq, Vec<(VoiceId, f32, f32, String, u8)>)>,
    /// switch parameter values for the cue being evaluated
    params: Vec<(String, usize)>,
    /// GCSoundNodeSwitch "BehindView" (the class default parameter) for the cue being
    /// evaluated: 0 for the local player's own sounds (2D), 1 for sounds of other pawns placed in
    /// the world (footsteps: the 3P branch with its attenuation)
    behind_view: usize,
    /// automatic fire per shooter key
    fire: HashMap<String, FireLoop>,
    /// sound group volumes (lowercase name -> product along the parent chain)
    groups: HashMap<String, f32>,
    /// sound packages opened in the background (the first shot used to open and decompress
    /// A2_Weapons_S on the frame it was fired)
    preload: Option<std::sync::Arc<std::sync::Mutex<Vec<(String, upk::Package)>>>>,
    /// direct waves not found (warned once)
    missing: std::collections::HashSet<String>,
    /// direct wave loops (SfxQueue::wave_loop): key -> (entity, wave)
    wave_loops: HashMap<String, (VoiceId, String)>,
    /// Ogg -> PCM worker (fills the wave's slot)
    decoder: Option<std::sync::mpsc::Sender<(Wave, Vec<u8>)>>,
    /// waves handed to the worker
    decoding: std::collections::HashSet<String>,
    /// the mixer bus player
    bus: Option<Entity>,
}

/// In-game sound packages opened ahead of use (weapons, impacts, explosions, UI, vehicles).
const PRELOAD: &[&str] = &[
    "A2_Weapons_C", "A2_Weapons_S", "A2_ProjctileImpact_C", "A2_ProjctileImpact_S", "A2_Explosions_C", "A2_Explosions_S", "A2_UI_C", "A2_UI_S",
    "A2_ProjctileFlight_C", "A2_ProjctileFlight_S", "A2_CharacterMovement_C", "A2_CharacterMovement_S", "A2_M1114_C", "A2_M1114_S", "A2_Droids_C", "A2_Droids_S",
];

/// [Engine.AudioDevice] SoundGroups of DefaultEngine.ini: each group's volume times its
/// parents'. The common in-game chain DistortionMaster (.3) x FL2SubMaster (.4) is divided out
/// (it scales every in-game sound alike, i.e. it is the master level).
fn sound_groups(root: &std::path::Path) -> HashMap<String, f32> {
    let mut own: HashMap<String, f32> = HashMap::new();
    let mut parent: HashMap<String, String> = HashMap::new();
    let Ok(text) = std::fs::read_to_string(root.join("GCGame/Config/DefaultEngine.ini")) else { return HashMap::new() };
    for l in text.lines() {
        let l = l.trim();
        let Some(rest) = l.strip_prefix("+SoundGroups=(") else { continue };
        let field = |k: &str| rest.find(k).map(|i| &rest[i + k.len()..]);
        let Some(name) = field("GroupName=").map(|x| x.split([',', ')']).next().unwrap_or("").to_ascii_lowercase()) else { continue };
        let vol = field("Volume=").and_then(|x| x.split([',', ')']).next()).and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
        own.insert(name.clone(), vol);
        if let Some(c) = field("ChildGroupNames=(") {
            for ch in c.split(')').next().unwrap_or("").split(',') {
                let ch = ch.trim().to_ascii_lowercase();
                if !ch.is_empty() {
                    parent.entry(ch).or_insert_with(|| name.clone());
                }
            }
        }
    }
    let chain = |g: &str| {
        let mut v = 1.0;
        let mut cur = Some(g.to_string());
        let mut n = 0;
        while let Some(c) = cur {
            v *= own.get(&c).copied().unwrap_or(1.0);
            cur = parent.get(&c).cloned();
            n += 1;
            if n > 16 {
                break;
            }
        }
        v
    };
    let base = chain("fl2submaster");
    let mut out: HashMap<String, f32> = own.keys().map(|k| (k.clone(), chain(k) / base.max(1e-6))).collect();
    // options sliders by the group's branch: MusicUI (Music), SFXUI (Sound), Voice, VoiceChat
    // (VOIP), all under User_Master (Master). Stored as "cat:<group>" = 0 sound, 1 music,
    // 2 voice, 3 voip, 4 master only
    for k in own.keys() {
        let mut cur = Some(k.clone());
        let mut cat = 4.0;
        let mut n = 0;
        while let Some(c) = cur {
            match c.as_str() {
                "musicui" => {
                    cat = 1.0;
                    break;
                }
                "sfxui" => {
                    cat = 0.0;
                    break;
                }
                "voice" => {
                    cat = 2.0;
                    break;
                }
                "voicechat" => {
                    cat = 3.0;
                    break;
                }
                _ => {}
            }
            cur = parent.get(&c).cloned();
            n += 1;
            if n > 16 {
                break;
            }
        }
        out.insert(format!("cat:{k}"), cat);
    }
    out
}

impl SfxState {
    /// Data volume of a sound group and its options class.
    fn group_volume(&self, g: &str) -> (f32, u8) {
        if g.is_empty() || g.eq_ignore_ascii_case("None") {
            return (1.0, 0);
        }
        let g = g.to_ascii_lowercase();
        let cat = self.groups.get(&format!("cat:{g}")).copied().unwrap_or(0.0) as u8;
        (self.groups.get(&g).copied().unwrap_or(1.0), cat)
    }
}

/// Options volume factor of a class: 0 sound, 1 music, 2 voice, 3 voip, 4 master only.
fn class_volume(cat: u8) -> f32 {
    crate::settings::volume(match cat {
        1 => "music",
        2 => "voice",
        3 => "voip",
        4 => "master",
        _ => "sound",
    })
}

/// Opened map / localisation files with the first path components of their exports, and which
/// file carries a cooked-in package.
type Cooked = (Vec<(std::path::PathBuf, std::sync::Arc<upk::Package>, std::collections::HashSet<String>)>, HashMap<String, Option<usize>>, bool);
static COOKED: std::sync::LazyLock<std::sync::Mutex<Cooked>> = std::sync::LazyLock::new(Default::default);

/// A package that has no file of its own but is cooked into the maps (A2_Droids_C,
/// A2_Airstrike_C, A2_PreditorDrone_C) or the localisation packages (A2_Airstrike_S voice
/// waves in FL-<map>_LOC_<lang>.upk). The returned package's object paths start with `name`.
/// The files are opened once.
/// Every cooked file that holds objects of package `name` (see `cooked_package`).
pub fn cooked_packages(lib: &upk::library::Library, name: &str) -> Vec<std::sync::Arc<upk::Package>> {
    let _ = cooked_package(lib, name);
    let key = name.to_ascii_lowercase();
    let Ok(g) = COOKED.lock() else { return Vec::new() };
    g.0.iter().filter(|(_, _, tops)| tops.contains(&key)).map(|x| x.1.clone()).collect()
}

pub fn cooked_package(lib: &upk::library::Library, name: &str) -> Option<std::sync::Arc<upk::Package>> {
    let key = name.to_ascii_lowercase();
    let mut g = COOKED.lock().ok()?;
    if !g.2 {
        g.2 = true;
        let lang = crate::menu::language(&lib.root);
        for path in lib.cooked_files(&lang) {
            let Ok(p) = upk::Package::open(&path) else { continue };
            let tops: std::collections::HashSet<String> = (0..p.exports.len()).map(|i| p.object_path(i as i32 + 1).split('.').next().unwrap_or("").to_ascii_lowercase()).collect();
            g.0.push((path, std::sync::Arc::new(p), tops));
        }
        info!("sfx: {} map / localisation packages indexed", g.0.len());
    }
    if let Some(found) = g.1.get(&key) {
        return found.map(|i| g.0[i].1.clone());
    }
    let hit = g.0.iter().position(|(_, _, tops)| tops.contains(&key));
    info!("sfx: cooked package {name}: {:?}", hit.map(|i| g.0[i].0.file_name().map(|f| f.to_string_lossy().to_string())));
    g.1.insert(key, hit);
    hit.map(|i| g.0[i].1.clone())
}

impl SfxState {
    fn package(&mut self, name: &str) -> Option<&upk::Package> {
        let key = name.to_ascii_lowercase();
        if !self.pkgs.contains_key(&key) {
            let p = self.lib.as_ref().and_then(|l| l.open(name));
            if p.is_none() {
                warn!("sfx: package {name} not found");
            }
            self.pkgs.insert(key.clone(), p);
        }
        self.pkgs.get(&key).and_then(|p| p.as_ref())
    }

    fn find(&mut self, path: &str, class: &str) -> Option<(String, usize)> {
        let (pk, rest) = path.split_once('.')?;
        if self.package(pk).is_some() {
            let p = self.package(pk)?;
            let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == class && p.object_path(i as i32 + 1).eq_ignore_ascii_case(rest))?;
            return Some((pk.to_ascii_lowercase(), i));
        }
        // cooked into a map / localisation package: full path, kept under "cooked:<package>"
        let key = format!("cooked:{}", pk.to_ascii_lowercase());
        if !self.cooked.contains_key(&key) {
            let c = self.lib.as_ref().and_then(|l| cooked_package(l, pk));
            self.cooked.insert(key.clone(), c);
        }
        if let Some(p) = self.cooked.get(&key).and_then(|x| x.as_ref()) {
            if let Some(i) = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == class && p.object_path(i as i32 + 1).eq_ignore_ascii_case(path)) {
                return Some((key, i));
            }
        }
        // a package's objects can be spread over several cooked files (A2_AmbientAudio_S_2_oldFL1:
        // a few in Overpass_bcsettings_LOC_deu, the wind waves in Crossroads_audio.map)
        let all = self.lib.as_ref().map(|l| cooked_packages(l, pk)).unwrap_or_default();
        for (k, p) in all.into_iter().enumerate() {
            if let Some(i) = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == class && p.object_path(i as i32 + 1).eq_ignore_ascii_case(path)) {
                let kk = format!("{key}#{k}");
                self.cooked.insert(kk.clone(), Some(p));
                return Some((kk, i));
            }
        }
        None
    }

    /// An opened package by `find`'s key.
    fn pkg(&self, key: &str) -> Option<&upk::Package> {
        if key.starts_with("cooked:") {
            return self.cooked.get(key)?.as_deref();
        }
        self.pkgs.get(key)?.as_ref()
    }

    fn cue(&mut self, path: &str) -> Option<SoundCueData> {
        if let Some(c) = self.cues.get(path) {
            return c.clone();
        }
        let c = self.find(path, "SoundCue").and_then(|(pk, i)| {
            let name = if pk.starts_with("cooked:") { String::new() } else { path.split('.').next().unwrap_or("").to_string() };
            self.pkg(&pk)?.read_sound_cue(i, &name)
        });
        // a voice line instance (GCSoundCueInstance): its master cue with the instance's waves in
        // the SoundNodeWaveParam slots, the instance's volume and sound group
        let c = c.or_else(|| {
            let (pk, i) = self.find(path, "GCSoundCueInstance")?;
            let p = self.pkg(&pk)?;
            let (cue, params, vol, group) = p.read_cue_instance(i)?;
            let master = p.object_path(cue);
            let master = if pk.starts_with("cooked:") { master } else { format!("{}.{master}", path.split('.').next().unwrap_or("")) };
            let mut m = self.cue(&master)?;
            fn fill(n: &mut SoundNode, params: &[(String, String)]) {
                match n {
                    SoundNode::WaveParam { name, child } => match params.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
                        Some((_, w)) => *n = SoundNode::Wave(w.clone()),
                        None => fill(child, params),
                    },
                    SoundNode::Random(c) | SoundNode::Mixer(c) | SoundNode::Concat(c) => c.iter_mut().for_each(|x| fill(&mut x.1, params)),
                    SoundNode::Modulator { child, .. } | SoundNode::Delay { child, .. } | SoundNode::Attenuation { child, .. } | SoundNode::Looping { child, .. } => fill(child, params),
                    SoundNode::Switch { children, .. } => children.iter_mut().for_each(|x| fill(x, params)),
                    SoundNode::CrossFade(c) => c.iter_mut().for_each(|x| fill(&mut x.3, params)),
                    _ => {}
                }
            }
            fill(&mut m.root, &params);
            // the master's Random nodes over unfilled WaveParam slots (Dry3.. without a wave)
            // stay empty: they play nothing
            fn prune(n: &mut SoundNode) {
                match n {
                    SoundNode::WaveParam { child, .. } => {
                        prune(child);
                        *n = std::mem::replace(child.as_mut(), SoundNode::Empty);
                    }
                    SoundNode::Random(c) => {
                        c.iter_mut().for_each(|x| prune(&mut x.1));
                        c.retain(|x| !matches!(x.1, SoundNode::Empty));
                    }
                    SoundNode::Mixer(c) | SoundNode::Concat(c) => c.iter_mut().for_each(|x| prune(&mut x.1)),
                    SoundNode::Modulator { child, .. } | SoundNode::Delay { child, .. } | SoundNode::Attenuation { child, .. } | SoundNode::Looping { child, .. } => prune(child),
                    SoundNode::Switch { children, .. } => children.iter_mut().for_each(prune),
                    SoundNode::CrossFade(c) => c.iter_mut().for_each(|x| prune(&mut x.3)),
                    _ => {}
                }
            }
            prune(&mut m.root);
            m.volume *= vol;
            if !group.is_empty() && !group.eq_ignore_ascii_case("None") {
                m.group = group;
            }
            Some(m)
        });
        if c.is_none() {
            warn!("sfx: cue {path} not readable");
        }
        self.cues.insert(path.to_string(), c.clone());
        c
    }

    /// `sync`: decoded before it plays (loops, external files). Waves are decoded to f32 PCM once
    /// (decoding per play cost the main thread up to 11.7 ms p95 / the audio thread a Vorbis
    /// decoder per voice, profiles 2026-10-18/19). One-shots are decoded on a worker thread; a
    /// voice of a wave still decoding waits silently in the mixer and starts when it is ready
    /// (decoding a long wave on the frame it was first played: sfx up to 44.8 ms, 2026-10-19).
    /// Small waves (< SYNC_OGG bytes, ~ms to decode) are decoded right away so the first shot of
    /// a weapon is on time.
    fn wave(&mut self, path: &str, sync: bool) -> Option<(Wave, f32)> {
        /// NOT from data
        const SYNC_OGG: usize = 48 * 1024;
        if let Some(w) = self.waves.get(path) {
            return w.clone();
        }
        // "ext:<file>": an external Ogg file (not game data; working directory = project), e.g.
        // the nuke's downloaded sounds in assets/nuke (credits there), decoded right away
        if let Some(file) = path.strip_prefix("ext:") {
            let h = std::fs::read(file).ok().and_then(|ogg| mixer::ogg_to_pcm(&ogg)).map(|p| {
                let d = p.duration();
                (mixer::ready(p), d)
            });
            if h.is_none() {
                warn!("sfx: external sound {file} not found or not Ogg Vorbis");
            }
            self.waves.insert(path.to_string(), h.clone());
            return h;
        }
        let Some((ogg, d)) = self.find(path, "SoundNodeWave").and_then(|(pk, i)| self.pkg(&pk)?.read_wave_ogg(i)) else {
            self.waves.insert(path.to_string(), None);
            return None;
        };
        if sync || ogg.len() < SYNC_OGG {
            let _perf = crate::perf::scope("sfx_decode");
            let h = mixer::ogg_to_pcm(&ogg).map(|p| (mixer::ready(p), d));
            self.waves.insert(path.to_string(), h.clone());
            return h;
        }
        let tx = self.decoder.get_or_insert_with(|| {
            let (jt, jr) = std::sync::mpsc::channel::<(Wave, Vec<u8>)>();
            std::thread::spawn(move || {
                while let Ok((w, ogg)) = jr.recv() {
                    let _ = w.set(mixer::ogg_to_pcm(&ogg).unwrap_or_else(mixer::Pcm::empty));
                }
            });
            jt
        });
        let w: Wave = std::sync::Arc::new(std::sync::OnceLock::new());
        let _ = tx.send((w.clone(), ogg));
        self.decoding.insert(path.to_string());
        let h = Some((w, d));
        self.waves.insert(path.to_string(), h.clone());
        h
    }

    /// Duration of a wave (seconds), for SoundNodeConcatenator.
    fn wave_len(&mut self, path: &str) -> f32 {
        if let Some(d) = self.wave_lens.get(path) {
            return *d;
        }
        let d = self.find(path, "SoundNodeWave").and_then(|(pk, i)| self.pkg(&pk)?.wave_duration(i)).unwrap_or(0.0);
        self.wave_lens.insert(path.to_string(), d);
        d
    }

    fn rnd(&mut self) -> f32 {
        if self.seed == 0 {
            self.seed = 0x1234_5677;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 1_000_000) as f32 / 1_000_000.0
    }

    /// Flatten a cue graph into plays. `release`: the PlayFireRelease switch parameter
    /// (0 while a weapon fires, 1 once the trigger is let go); other switch parameters
    /// ("Inside") take child 0.
    fn eval(&mut self, n: &SoundNode, ctx: Ctx, out: &mut Vec<Play>) {
        match n {
            SoundNode::Wave(p) => {
                if !out.iter().any(|o| &o.wave == p) {
                    out.push(Play { wave: p.clone(), vol: ctx.vol, pitch: ctx.pitch, delay: ctx.delay, att: ctx.att, looped: ctx.looped, cat: ctx.cat });
                }
            }
            SoundNode::Mixer(c) => {
                for (v, ch) in c {
                    self.eval(ch, Ctx { vol: ctx.vol * v, ..ctx }, out);
                }
            }
            SoundNode::Random(c) => {
                let total: f32 = c.iter().map(|x| x.0.max(0.0)).sum();
                if total <= 0.0 || c.is_empty() {
                    return;
                }
                let mut r = self.rnd() * total;
                for (w, ch) in c {
                    if r <= *w {
                        self.eval(ch, ctx, out);
                        return;
                    }
                    r -= w;
                }
                let last = &c[c.len() - 1].1;
                self.eval(last, ctx, out);
            }
            SoundNode::Modulator { volume, pitch: p, child } => {
                let v = volume.0 + (volume.1 - volume.0) * self.rnd();
                let q = p.0 + (p.1 - p.0) * self.rnd();
                self.eval(child, Ctx { vol: ctx.vol * v, pitch: ctx.pitch * q, ..ctx }, out);
            }
            SoundNode::Delay { delay: d, child } => {
                let x = d.0 + (d.1 - d.0) * self.rnd();
                self.eval(child, Ctx { delay: ctx.delay + x, ..ctx }, out);
            }
            SoundNode::Attenuation { radius_min, radius_max, model, db_at_max, child } => self.eval(child, Ctx { att: Some(Att { min: *radius_min, max: *radius_max, model: *model, db: *db_at_max }), ..ctx }, out),
            SoundNode::Looping { forever, count, child } => self.eval(child, Ctx { looped: ctx.looped || *forever || *count > 1.0, ..ctx }, out),
            SoundNode::Switch { param, children } => {
                let k = if param.eq_ignore_ascii_case("PlayFireRelease") {
                    ctx.release as usize
                } else {
                    self.params.iter().find(|(n, _)| n.eq_ignore_ascii_case(param)).map(|x| x.1).unwrap_or_else(|| {
                        // "Multiplayer": this is a multiplayer match; "BehindView": see behind_view
                        if param.eq_ignore_ascii_case("Multiplayer") {
                            1
                        } else if param.eq_ignore_ascii_case("BehindView") {
                            self.behind_view
                        } else {
                            0
                        }
                    })
                };
                if let Some(c) = children.get(k.min(children.len().saturating_sub(1))) {
                    self.eval(c, ctx, out);
                }
            }
            SoundNode::Concat(c) => {
                // one after another: each child starts when the previous one's waves end
                let mut t = 0.0f32;
                for (v, ch) in c {
                    let mut part = Vec::new();
                    self.eval(ch, Ctx { vol: ctx.vol * v, delay: ctx.delay + t, ..ctx }, &mut part);
                    let mut len = 0.0f32;
                    for p in &part {
                        len = len.max(p.delay - (ctx.delay + t) + self.wave_len(&p.wave) / p.pitch.max(0.1));
                    }
                    t += len;
                    for p in part {
                        if !out.iter().any(|o| o.wave == p.wave) {
                            out.push(p);
                        }
                    }
                }
            }
            SoundNode::WaveParam { child, .. } => self.eval(child, ctx, out),
            SoundNode::CrossFade(c) => {
                // SoundNodeDistanceCrossFade::ParseNodes: each layer's volume by the listener
                // distance (UE3 takes the min / max of the distributions as the fade range)
                let d = self.dist;
                for (fi, fo, v, ch) in c {
                    let g = if d < fi.0 {
                        0.0
                    } else if d < fi.1 {
                        (d - fi.0) / (fi.1 - fi.0).max(1e-3)
                    } else if d <= fo.0 {
                        1.0
                    } else if d < fo.1 {
                        1.0 - (d - fo.0) / (fo.1 - fo.0).max(1e-3)
                    } else {
                        0.0
                    };
                    if g * v > 0.0 {
                        self.eval(ch, Ctx { vol: ctx.vol * g * v, ..ctx }, out);
                    }
                }
            }
            SoundNode::Empty => {}
        }
    }
}

/// SoundNodeAttenuation: radii (UE units), DistanceModel, dBAttenuationAtMax.
#[derive(Clone, Copy, Debug)]
struct Att {
    min: f32,
    max: f32,
    model: u8,
    db: f32,
}

impl Att {
    /// cues without an Attenuation node, played at a position (not from data)
    const FALLBACK: Att = Att { min: 400.0, max: 5000.0, model: 0, db: -60.0 };

    /// UE3 SoundNodeAttenuation::ParseNodes volume factor at distance `d`.
    fn gain(&self, d: f32) -> f32 {
        if d <= self.min {
            return 1.0;
        }
        if d >= self.max {
            return 0.0;
        }
        let x = (d - self.min) / (self.max - self.min).max(1.0);
        let g = match self.model {
            1 => -0.5 * x.max(1e-6).ln(),
            2 => 0.02 / x.max(1e-6),
            3 => 1.0 + 0.5 * (1.0 - x).max(1e-6).ln(),
            4 => 10f32.powf(x * self.db.min(0.0) / 20.0),
            _ => 1.0 - x,
        };
        g.clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy)]
struct Ctx {
    vol: f32,
    pitch: f32,
    delay: f32,
    att: Option<Att>,
    looped: bool,
    release: bool,
    /// options volume class of the cue's sound group
    cat: u8,
}

impl Ctx {
    fn root(cue: &SoundCueData, vol: f32, release: bool, group: (f32, u8)) -> Ctx {
        Ctx { vol: cue.volume * vol * group.0, pitch: cue.pitch, delay: 0.0, att: None, looped: false, release, cat: group.1 }
    }
}

struct Play {
    wave: String,
    vol: f32,
    pitch: f32,
    delay: f32,
    att: Option<Att>,
    looped: bool,
    cat: u8,
}

/// Whether a cue has a PlayFireRelease switch (looping automatic fire with a release tail).
fn has_release(n: &SoundNode) -> bool {
    match n {
        SoundNode::Switch { param, children } => param.eq_ignore_ascii_case("PlayFireRelease") || children.iter().any(has_release),
        SoundNode::Mixer(c) | SoundNode::Random(c) | SoundNode::Concat(c) => c.iter().any(|x| has_release(&x.1)),
        SoundNode::WaveParam { child, .. } => has_release(child),
        SoundNode::CrossFade(c) => c.iter().any(|x| has_release(&x.3)),
        SoundNode::Modulator { child, .. } | SoundNode::Delay { child, .. } | SoundNode::Attenuation { child, .. } | SoundNode::Looping { child, .. } => has_release(child),
        _ => false,
    }
}

/// A weapon's fire cue held while the trigger is down (keyed per shooter).
struct FireLoop {
    cue: String,
    at: Option<Vec3>,
    /// the shot keeps the loop alive until this time
    until: f64,
    /// audio entities with their base volume and attenuation (positional loops follow the listener)
    es: Vec<(VoiceId, f32, Option<Att>, u8)>,
}

/// Fire / ADS cues of a GCSoundInfoWeapon (A2_Weapons_C SI_*).
fn weapon_sounds(state: &mut SfxState, si: &str) -> [String; 4] {
    if let Some(x) = state.infos.get(si) {
        return x.clone();
    }
    let mut out: [String; 4] = Default::default();
    if let Some((pk, i)) = state.find(si, "GCSoundInfoWeapon") {
        let pname = si.split('.').next().unwrap_or("").to_string();
        if let Some(p) = state.pkgs.get(&pk).and_then(|p| p.as_ref()) {
            if let Some(pr) = p.export_props(i) {
                for (k, n) in ["Fire1P", "Fire3P", "ADSIn", "ADSOut"].iter().enumerate() {
                    if let Some(o) = pr.object(n).filter(|&o| o != 0) {
                        out[k] = if o > 0 { format!("{pname}.{}", p.object_path(o)) } else { p.object_path(o) };
                    }
                }
            }
        }
    }
    state.infos.insert(si.to_string(), out.clone());
    out
}

/// Weapon sound requests by sound-info path and kind (0 Fire1P, 1 Fire3P, 2 ADSIn, 3 ADSOut);
/// `.1`: shots (shooter key, sound info, kind, position, seconds the shot keeps an automatic
/// fire loop alive).
#[derive(Resource, Default)]
pub struct WeaponSfx(pub Vec<(String, usize, Option<Vec3>)>, pub Vec<(String, String, usize, Option<Vec3>, f32)>, pub Vec<(String, String, Option<Vec3>, f32)>);

impl WeaponSfx {
    /// One shot of `key`'s weapon. Cues with a PlayFireRelease switch loop while shots keep
    /// coming and play their release tail after the last one; other cues play once per shot.
    pub fn shot(&mut self, key: &str, si: &str, kind: usize, at: Option<Vec3>, hold: f32) {
        self.1.push((key.to_string(), si.to_string(), kind, at, hold));
    }

    /// The same for a fire cue given directly (vehicle guns: GCVehicleAudio SCFireSound).
    pub fn shot_cue(&mut self, key: &str, cue: &str, at: Option<Vec3>, hold: f32) {
        if !cue.is_empty() {
            self.2.push((key.to_string(), cue.to_string(), at, hold));
        }
    }
}

/// Distance gain of a play at `at` (None = 2D).
fn gain(att: Option<Att>, at: Option<Vec3>, listener: Option<Vec3>) -> f32 {
    match (at, listener) {
        (Some(at), Some(l)) => att.unwrap_or(Att::FALLBACK).gain(at.distance(l)),
        _ => 1.0,
    }
}

fn play_sounds(
    mut commands: Commands,
    time: Res<Time>,
    root: Option<Res<GameRoot>>,
    mut queue: ResMut<SfxQueue>,
    weapon: Option<ResMut<WeaponSfx>>,
    mix: Res<Mix>,
    cams: Query<&GlobalTransform, (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    mut buses: ResMut<Assets<MixerBus>>,
    exists: Query<()>,
    mut st: Local<SfxState>,
    (mut last_ver, mut last_culled): (Local<u64>, Local<u64>),
) {
    let _perf = crate::perf::scope("sfx");
    let mix = &mix.0;
    if st.lib.is_none() {
        let Some(root) = root else { return };
        st.lib = upk::library::Library::scan(&root.0).ok();
        if st.lib.is_none() {
            return;
        }
        st.groups = sound_groups(&root.0);
        info!("sfx: {} sound groups", st.groups.len());
        let paths: Vec<(String, std::path::PathBuf)> = PRELOAD.iter().filter_map(|n| st.lib.as_ref()?.path_of(n).cloned().map(|p| (n.to_ascii_lowercase(), p))).collect();
        let slot = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        st.preload = Some(slot.clone());
        std::thread::spawn(move || {
            for (n, p) in paths {
                if let Ok(pk) = upk::Package::open(&p) {
                    if let Ok(mut g) = slot.lock() {
                        g.push((n, pk));
                    }
                }
            }
        });
    }
    let st = &mut *st;
    // the bus player (again if something despawned it)
    if st.bus.is_none_or(|e| exists.get(e).is_err()) {
        let rate = device_rate();
        info!("sfx: mixer bus at {rate} Hz");
        let h = buses.add(MixerBus(mix.clone(), rate));
        st.bus = Some(commands.spawn((AudioPlayer(h), PlaybackSettings::ONCE)).id());
        *last_ver = u64::MAX;
    }
    if let Some(slot) = st.preload.as_ref() {
        let done: Vec<(String, upk::Package)> = slot.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
        for (n, p) in done {
            st.pkgs.entry(n).or_insert(Some(p));
        }
    }
    let listener = cams.iter().next().map(|g| {
        let t = g.translation();
        Vec3::new(t.x, t.z, t.y) * 100.0
    });
    let now = time.elapsed_secs_f64();
    // options volume factors per class (the mixer applies them to every voice, smoothed)
    let ver = crate::settings::version();
    if ver != *last_ver {
        *last_ver = ver;
        mix.set_classes([class_volume(0), class_volume(1), class_volume(2), class_volume(3), class_volume(4)]);
    }
    crate::perf::count("audio_voices", mix.live() as u64);
    let culled = mix.culled();
    crate::perf::count("audio_voices_culled", culled - (*last_culled).min(culled));
    *last_culled = culled;
    let mut start: Vec<(Wave, f32, f32, u8)> = Vec::new();
    if let Some(mut w) = weapon {
        for (si, k, at) in std::mem::take(&mut w.0) {
            let c = weapon_sounds(st, &si);
            if !c[k].is_empty() {
                queue.0.push(SfxReq { cue: c[k].clone(), at, volume: 1.0, restart: false });
            }
        }
        let mut shots: Vec<(String, String, Option<Vec3>, f32)> = Vec::new();
        for (key, si, k, at, hold) in std::mem::take(&mut w.1) {
            let c = weapon_sounds(st, &si);
            shots.push((key, c[k].clone(), at, hold));
        }
        shots.extend(std::mem::take(&mut w.2));
        for (key, path, at, hold) in shots {
            if path.is_empty() {
                continue;
            }
            let Some(cue) = st.cue(&path) else { continue };
            if !has_release(&cue.root) {
                queue.0.push(SfxReq { cue: path, at, volume: 1.0, restart: true });
                continue;
            }
            if let Some(f) = st.fire.get_mut(&key).filter(|f| f.cue == path) {
                f.until = now + hold as f64;
                f.at = at;
                continue;
            }
            if let Some(old) = st.fire.remove(&key) {
                for (e, ..) in old.es {
                    mix.stop(e);
                }
            }
            // trigger down: the PlayFireRelease = 0 branch (loops plus the one-shot layers)
            let mut plays = Vec::new();
            st.params.clear();
            st.behind_view = at.is_some() as usize;
            st.dist = match (at, listener) { (Some(a), Some(l)) => a.distance(l), _ => 0.0 };
            st.eval(&cue.root, Ctx::root(&cue, 1.0, false, st.group_volume(&cue.group)), &mut plays);
            let mut es = Vec::new();
            for pl in plays {
                let v = pl.vol * gain(pl.att, at, listener);
                let Some((h, _)) = st.wave(&pl.wave, pl.looped) else { continue };
                if pl.looped {
                    let e = mix.play(h, v, pl.cat, pl.pitch, true);
                    es.push((e, pl.vol, pl.att, pl.cat));
                } else if v > 0.01 {
                    if pl.delay > 0.0 {
                        st.delayed.push((pl.delay, h, v, pl.pitch, pl.cat));
                    } else {
                        start.push((h, v, pl.pitch, pl.cat));
                    }
                }
            }
            st.fire.insert(key, FireLoop { cue: path, at, until: now + hold as f64, es });
        }
    }
    // trigger released: stop the loop, play the PlayFireRelease = 1 branch
    let done: Vec<String> = st.fire.iter().filter(|(_, f)| now > f.until).map(|(k, _)| k.clone()).collect();
    for k in done {
        let Some(f) = st.fire.remove(&k) else { continue };
        for (e, ..) in &f.es {
            mix.stop(*e);
        }
        let Some(cue) = st.cue(&f.cue) else { continue };
        let mut plays = Vec::new();
        st.params.clear();
        st.behind_view = f.at.is_some() as usize;
        st.dist = match (f.at, listener) { (Some(a), Some(l)) => a.distance(l), _ => 0.0 };
        st.eval(&cue.root, Ctx::root(&cue, 1.0, true, st.group_volume(&cue.group)), &mut plays);
        for pl in plays.into_iter().filter(|p| !p.looped) {
            let v = pl.vol * gain(pl.att, f.at, listener);
            if v <= 0.01 {
                continue;
            }
            let Some((h, _)) = st.wave(&pl.wave, false) else { continue };
            if pl.delay > 0.0 {
                st.delayed.push((pl.delay, h, v, pl.pitch, pl.cat));
            } else {
                start.push((h, v, pl.pitch, pl.cat));
            }
        }
    }
    // positional fire loops follow the listener distance
    for f in st.fire.values() {
        if f.at.is_none() {
            continue;
        }
        for (e, base, att, _) in &f.es {
            mix.set_gain(*e, base * gain(*att, f.at, listener));
        }
    }
    let dt = time.delta_secs();
    for d in st.delayed.iter_mut() {
        d.0 -= dt;
    }
    let mut keep = Vec::new();
    for d in std::mem::take(&mut st.delayed) {
        if d.0 <= 0.0 {
            start.push((d.1, d.2, d.3, d.4));
        } else {
            keep.push(d);
        }
    }
    st.delayed = keep;
    for req in std::mem::take(&mut queue.0) {
        let Some(cue) = st.cue(&req.cue) else { continue };
        let act = st.active.entry(req.cue.clone()).or_default();
        act.retain(|e| e.0 > now);
        if cue.max_concurrent > 0 && act.len() >= cue.max_concurrent as usize {
            if !req.restart {
                continue;
            }
            // cut the oldest instance
            let k = act.iter().enumerate().min_by(|a, b| a.1.0.partial_cmp(&b.1.0).unwrap()).map(|x| x.0).unwrap_or(0);
            let old = act.remove(k);
            old.1.into_iter().for_each(|e| mix.stop(e));
        }
        let mut mine: Vec<VoiceId> = Vec::new();
        let mut plays = Vec::new();
        st.params.clear();
        st.behind_view = req.at.is_some() as usize;
        st.dist = match (req.at, listener) { (Some(a), Some(l)) => a.distance(l), _ => 0.0 };
        st.eval(&cue.root, Ctx::root(&cue, req.volume, false, st.group_volume(&cue.group)), &mut plays);
        // HB_SFXDBG: every cue with its sound group and options slider (0 sound, 1 music,
        // 2 voice, 3 voip, 4 master only)
        if std::env::var_os("HB_SFXDBG").is_some() {
            let (gv, cat) = st.group_volume(&cue.group);
            info!("sfx: {} group '{}' ({gv:.2}) slider {cat}, {} waves", req.cue, cue.group, plays.len());
        }
        let mut longest = 0.0f32;
        for pl in plays {
            if req.at.is_some() && listener.is_some() && pl.att.unwrap_or(Att::FALLBACK).max < req.at.unwrap().distance(listener.unwrap()) {
                continue;
            }
            let v = pl.vol * gain(pl.att, req.at, listener);
            if v <= 0.01 {
                continue;
            }
            let Some((h, dur)) = st.wave(&pl.wave, false) else { continue };
            longest = longest.max(pl.delay + dur / pl.pitch.max(0.1));
            if pl.delay > 0.0 {
                st.delayed.push((pl.delay, h, v, pl.pitch, pl.cat));
            } else {
                mine.push(mix.play(h, v, pl.cat, pl.pitch, false));
            }
        }
        st.active.entry(req.cue.clone()).or_default().push((now + longest as f64, mine));
    }
    for (h, v, pitch, cat) in start {
        mix.play(h, v, cat, pitch, false);
    }
    // direct waves (nuke.rs)
    for w in std::mem::take(&mut queue.2) {
        let Some((h, _)) = st.wave(&w.wave, false) else {
            if st.missing.insert(w.wave.clone()) {
                warn!("sfx: wave {} not found", w.wave);
            }
            continue;
        };
        if w.volume < 0.0 {
            continue;
        }
        if w.delay > 0.0 {
            st.delayed.push((w.delay, h, w.volume, w.pitch, 0));
        } else {
            mix.play(h, w.volume, 0, w.pitch, false);
        }
    }
    let want_w = std::mem::take(&mut queue.3);
    let gone: Vec<String> = st.wave_loops.iter().filter(|(k, (_, wv))| want_w.get(*k).is_none_or(|w| &w.wave != wv)).map(|(k, _)| k.clone()).collect();
    for k in gone {
        if let Some((e, _)) = st.wave_loops.remove(&k) {
            mix.stop(e);
        }
    }
    for (k, w) in want_w {
        if let Some((e, _)) = st.wave_loops.get(&k) {
            mix.set_gain(*e, w.volume);
            mix.set_speed(*e, w.pitch);
            continue;
        }
        let Some((h, _)) = st.wave(&w.wave, true) else {
            if st.missing.insert(w.wave.clone()) {
                warn!("sfx: wave {} not found", w.wave);
            }
            continue;
        };
        let e = mix.play(h, w.volume, 0, w.pitch, true);
        st.wave_loops.insert(k, (e, w.wave));
    }
    // loops: start requested ones, stop the rest (the cue graph is evaluated once per start).
    // Only SoundNodeLooping layers loop; the cue's one-shot layers (engine start, revs) play
    // once. A change of the switch parameters plays the one-shot layers the new values add.
    let want = std::mem::take(&mut queue.1);
    let stop: Vec<String> = st.loops.iter().filter(|(k, (r, _))| want.get(*k).is_none_or(|w| w.cue != r.cue)).map(|(k, _)| k.clone()).collect();
    for k in stop {
        if let Some((_, es)) = st.loops.remove(&k) {
            for (e, ..) in es {
                mix.stop(e);
            }
        }
    }
    let mut once: Vec<(Wave, f32, f32, f32, u8)> = Vec::new();
    for (k, req) in want {
        let Some(cue) = st.cue(&req.cue) else { continue };
        let group = st.group_volume(&cue.group);
        if let Some((old, es)) = st.loops.get(&k).cloned() {
            if old.params != req.params {
                // same random picks for both evaluations, then the difference
                let seed = st.seed;
                let mut before = Vec::new();
                st.params = old.params.clone();
                st.behind_view = req.at.is_some() as usize;
                st.dist = match (req.at, listener) { (Some(a), Some(l)) => a.distance(l), _ => 0.0 };
                st.eval(&cue.root, Ctx::root(&cue, 1.0, false, group), &mut before);
                st.seed = seed;
                let mut after = Vec::new();
                st.params = req.params.clone();
                st.behind_view = req.at.is_some() as usize;
                st.dist = match (req.at, listener) { (Some(a), Some(l)) => a.distance(l), _ => 0.0 };
                st.eval(&cue.root, Ctx::root(&cue, 1.0, false, group), &mut after);
                for pl in after.into_iter().filter(|p| !p.looped && !before.iter().any(|b| b.wave == p.wave)) {
                    let v = pl.vol * gain(pl.att, req.at, listener);
                    if let Some((h, _)) = st.wave(&pl.wave, false) {
                        once.push((h, v, pl.pitch, pl.delay, pl.cat));
                    }
                }
            }
            for (e, base, vol, wave, _) in &es {
                if base * req.pitch != base * old.pitch {
                    mix.set_speed(*e, base * req.pitch);
                }
                if !req.gains.is_empty() || !old.gains.is_empty() {
                    mix.set_gain(*e, vol * layer_gain(&req.gains, wave));
                }
            }
            if let Some(l) = st.loops.get_mut(&k) {
                l.0 = req;
            }
            continue;
        }
        let mut plays = Vec::new();
        st.params = req.params.clone();
        st.behind_view = req.at.is_some() as usize;
        st.dist = match (req.at, listener) { (Some(a), Some(l)) => a.distance(l), _ => 0.0 };
        st.eval(&cue.root, Ctx::root(&cue, 1.0, false, group), &mut plays);
        let mut es = Vec::new();
        for pl in plays {
            let v = pl.vol * gain(pl.att, req.at, listener);
            let Some((h, _)) = st.wave(&pl.wave, pl.looped) else { continue };
            if pl.looped {
                let g = layer_gain(&req.gains, &pl.wave);
                let e = mix.play(h, v * g, pl.cat, pl.pitch * req.pitch, true);
                es.push((e, pl.pitch, v, pl.wave.clone(), pl.cat));
            } else {
                once.push((h, v, pl.pitch, pl.delay, pl.cat));
            }
        }
        st.loops.insert(k, (req, es));
    }
    for (h, v, pitch, delay, cat) in once {
        if v <= 0.01 {
            continue;
        }
        if delay > 0.0 {
            st.delayed.push((delay, h, v, pitch, cat));
        } else {
            mix.play(h, v, cat, pitch, false);
        }
    }
}

/// Volume factor of a looping layer (LoopReq::gains, matched by wave name substring).
fn layer_gain(gains: &[(String, f32)], wave: &str) -> f32 {
    let w = wave.to_ascii_lowercase();
    gains.iter().find(|(n, _)| w.contains(&n.to_ascii_lowercase())).map(|g| g.1).unwrap_or(1.0)
}
