//! Day/night cycle and weather (user request 2026-10-21). NOT in the original game: every
//! Homefront level has one fixed sun, sky light and sky dome, so all tuning values here are
//! NOT from data (each marked). Off by default: the level's own look stays the default.
//!
//! The renderer lights the level with the game's own constants (gamemat::Globals: sun
//! direction / colour, upper / lower sky light, ambient) from a storage buffer; this module
//! patches those constants on the GPU every frame (queue.write_buffer, like crater.rs /
//! treewind.rs) starting from the level's values, which stay untouched on the CPU side:
//!
//! * day/night: the sun moves on a circle through the level's own sun direction (at the level's
//!   "base hour" the light is exactly the level's), dims and warms near the horizon; at night a
//!   dim blue moon light opposite the sun takes over (sun shadow follows: the DirectionalLight
//!   is turned too). Sky light darkens / tints, the sky dome and fog dome (objects[].z flag,
//!   main.rs) darken and get stars (gamemat.rs hf_weather_sky). Grass (fixed light constants
//!   in its shader) gets a light multiplier.
//! * weather: overcast (sun blocked, grey sky light, grey dark sky), rain / storm (rain sheets
//!   with the game's KFX_Impacts Textures.Imp_FallingDrops, camera-relative wrapped volume drawn
//!   on the GPU, wind loop A2_AmbientAudio_S_2_oldFL1.Wind.S_Fencewind*), storm lightning
//!   (light flash from the bolt direction with its shadows, sky flash, the game's
//!   KFX_Atmosphere Storm.FX_Lightning sky glow, thunder = A2_Explosions_S Ambient.AmbientLOD_*
//!   distant rumbles pitched down, delayed by distance / 343 m/s), snow (KFX_Atmosphere
//!   Textures.PCloudDot flakes, colder brighter sky light), fog.
//! * fog: exponential height fog as a full-screen pass after the opaque pass (depth buffer),
//!   with a sun in-scatter lobe; optional Bevy VolumetricFog light shafts (HB_VOLUMETRIC=1).
//!
//! Switches: HB_WEATHER=clear|rain|storm|snow|fog, HB_DAYNIGHT=<minutes per 24 h>,
//! HB_TIME=<hour 0-24 at start>, HB_VOLUMETRIC=1; debug menu (F1) tab "Werkzeuge".
//!
//! Random weather per round (user 2026-10-22 16:21, option "Zufälliges Wetter/Tageszeit",
//! settings key RANDOM_KEY, default on; round_weather below): every match start and every new
//! round of a match switches the simulation on with a random kind (ROUND_KINDS) and a random
//! hour (round_hour: mostly day). At the match start it is set at once (while the level is
//! built), a later round blends over TRANSITION_S and the sun moves forward to the new hour.
//! Back in the front end the simulation is off again. Any of HB_WEATHER / HB_TIME / HB_DAYNIGHT
//! keeps the fixed test values (no randomisation). All weights NOT from data.

use bevy::{
    core_pipeline::{
        core_3d::graph::{Core3d, Node3d},
        fullscreen_vertex_shader::fullscreen_shader_vertex_state,
    },
    ecs::query::QueryItem,
    pbr::{FogVolume, MaterialPipeline, MaterialPipelineKey, NotShadowCaster, NotShadowReceiver, VolumetricFog, VolumetricLight},
    prelude::*,
    render::{
        extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin},
        mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
        render_asset::{RenderAssetUsages, RenderAssets},
        render_graph::{NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner},
        render_resource::{
            binding_types::{texture_2d, texture_depth_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
        view::{NoFrustumCulling, ViewDepthTexture, ViewTarget},
        Render, RenderApp, RenderSet,
    },
};
use std::f32::consts::PI;
use std::sync::Mutex;

// ---------------------------------------------------------------------------------------------
// Tuning (all NOT from data)
// ---------------------------------------------------------------------------------------------

/// highest sun elevation of the day: at least 60 deg, or the level's own + 8.6 deg (NOT from data)
const NOON_MIN_ELEV: f32 = 1.047;
const NOON_OVER_BASE: f32 = 0.15;
/// sin(elevation) below which the light vector used for shading / shadows is held (very long
/// shadows and the terrain-sky offset 1/z blow up near the horizon) (NOT from data)
const MIN_LIGHT_Z: f32 = 0.12;
/// warm sun colour multiplier at the horizon (NOT from data)
const SUN_WARM: Vec3 = Vec3::new(1.0, 0.52, 0.26);
/// moon light: fraction of the sun's luminance, colour (NOT from data). 2026-10-21 (user: houses
/// too black, single areas still lit at night): was 0.07 with sky 0.06, i.e. the day's
/// sun : sky contrast (~5 : 1) at night - with the game's eye adaptation (HDRLumHighClamp 3.5)
/// moonlit faces looked like daylight next to black shadows. Now ~1.5 : 1 (moon halved, sky
/// light doubled).
const MOON_I: f32 = 0.035;
const MOON_COL: Vec3 = Vec3::new(0.55, 0.65, 1.0);
/// sky light at night (fraction) and its tints at night / dusk (NOT from data)
const NIGHT_AMB: f32 = 0.12;
const NIGHT_TINT: Vec3 = Vec3::new(0.45, 0.55, 1.0);
const DUSK_TINT: Vec3 = Vec3::new(1.0, 0.72, 0.55);
/// sky dome brightness at night (fraction), tints at night / dusk (NOT from data)
const SKY_NIGHT: f32 = 0.025;
const SKY_NIGHT_TINT: Vec3 = Vec3::new(0.45, 0.55, 1.0);
const SKY_DUSK_TINT: Vec3 = Vec3::new(1.0, 0.55, 0.35);
/// overcast: share of direct sun blocked, sky light desaturation / dimming, sky dome
/// darkening / desaturation (NOT from data)
const OVC_SUN: f32 = 0.85;
const OVC_SKY_DESAT: f32 = 0.7;
const OVC_SKY_DIM: f32 = 0.35;
const OVC_DOME_DARK: f32 = 0.55;
const OVC_DOME_DESAT: f32 = 0.85;
/// snow: upper / lower sky light multipliers (cold, bright ground bounce) (NOT from data)
const SNOW_UPPER: Vec3 = Vec3::new(1.15, 1.2, 1.3);
const SNOW_LOWER: Vec3 = Vec3::new(1.6, 1.7, 1.9);
/// seconds for a weather change to settle (NOT from data)
const TRANSITION_S: f32 = 8.0;
/// lightning: seconds between strikes, bolt distance range (m), flash light / sky / ambient
/// (NOT from data)
const STRIKE_MIN_S: f32 = 5.0;
const STRIKE_RAND_S: f32 = 11.0;
const BOLT_DIST: (f32, f32) = (400.0, 4000.0);
const BOLT_COL: Vec3 = Vec3::new(0.75, 0.8, 1.0);
const BOLT_I: f32 = 9.0;
const BOLT_SKY: Vec3 = Vec3::new(0.5, 0.55, 0.75);
const BOLT_SKY_I: f32 = 2.5;
const BOLT_AMB: f32 = 0.35;
/// flash pulses: (start s, amplitude, decay s) (NOT from data)
const PULSES: [(f32, f32, f32); 3] = [(0.0, 1.0, 0.05), (0.13, 0.55, 0.04), (0.27, 0.8, 0.09)];
const SPEED_OF_SOUND: f32 = 343.0;
/// thunder: game waves (distant explosion rumbles, there is no thunder in the game data),
/// played slower / deeper (NOT from data)
const THUNDER: [&str; 6] = [
    "A2_Explosions_S.Ambient.AmbientLOD_03",
    "A2_Explosions_S.Ambient.AmbientLOD_05",
    "A2_Explosions_S.Ambient.AmbientLOD_08",
    "A2_Explosions_S.Ambient.AmbientLOD_12",
    "A2_Explosions_S.Ambient.AmbientLOD_16",
    "A2_Explosions_S.Ambient.AmbientLOD_20",
];
const THUNDER_PITCH: (f32, f32) = (0.6, 0.8);
const THUNDER_VOL: f32 = 1.0;
/// storm wind loop (game wave of the Crossroads / Overpass audio sublevel) and its volume
const WIND_WAVE: &str = "A2_AmbientAudio_S_2_oldFL1.Wind.S_Fencewind1_mono";
const WIND_VOL: f32 = 0.45;
/// the game's storm sky glow (EmitterDuration 15 s), placed this far above the camera (UE)
pub const LIGHTNING_FX: &str = "KFX_Atmosphere.Storm.FX_Lightning";
const LIGHTNING_FX_UP: f32 = 15000.0;
const LIGHTNING_FX_EVERY: f32 = 15.0;
/// fog: sky light -> fog colour, light -> fog colour, sun in-scatter, height falloff (1/m), max
/// opacity, distance for the empty sky (m) (NOT from data)
const FOG_SKY_K: f32 = 0.9;
const FOG_LIGHT_K: f32 = 0.03;
const FOG_SUN_K: f32 = 0.15;
const FOG_FALLOFF: f32 = 0.01;
const FOG_MAX: f32 = 0.97;
const FOG_SKY_DIST: f32 = 3000.0;
/// precipitation volume around the camera: horizontal size, height, near fade (m) (NOT from data)
const RAIN_BOX: (f32, f32, f32) = (36.0, 16.0, 1.5);
const RAIN_SHEETS: u32 = 2400;
/// rain sheet quad (a 4-frame cluster of drop streaks): width, height (m), fall speed (m/s),
/// opacity, colour from the sky light / sun (NOT from data)
const RAIN_SIZE: (f32, f32) = (1.6, 3.2);
const RAIN_FALL: f32 = 9.0;
const RAIN_ALPHA: f32 = 0.55;
const RAIN_SKY_K: f32 = 0.35;
const RAIN_LIGHT_K: f32 = 0.05;
const SNOW_BOX: (f32, f32, f32) = (30.0, 14.0, 0.6);
const SNOW_FLAKES: u32 = 6000;
/// snowflake size (m), fall speed (m/s), sway (m), opacity, colour factors (NOT from data)
const SNOW_SIZE: f32 = 0.045;
const SNOW_FALL: f32 = 1.1;
const SNOW_SWAY: f32 = 0.35;
const SNOW_ALPHA: f32 = 0.9;
const SNOW_SKY_K: f32 = 0.8;
const SNOW_LIGHT_K: f32 = 0.2;
/// wind (m/s, Bevy x / z) at full storm (NOT from data)
const WIND: Vec2 = Vec2::new(3.0, 1.2);
/// Bevy VolumetricFog light shafts: box around the camera (m), density, steps (NOT from data)
const VOL_BOX: Vec3 = Vec3::new(1200.0, 300.0, 1200.0);
const VOL_DENSITY: f32 = 0.0015;
const VOL_STEPS: u32 = 32;

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    Clear,
    Rain,
    Storm,
    Snow,
    Fog,
}

impl Kind {
    pub const ALL: [(Kind, &'static str); 5] = [(Kind::Clear, "Klar"), (Kind::Rain, "Regen"), (Kind::Storm, "Gewitter"), (Kind::Snow, "Schnee"), (Kind::Fog, "Nebel")];

    fn parse(s: &str) -> Option<Kind> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "clear" | "klar" | "" => Kind::Clear,
            "rain" | "regen" => Kind::Rain,
            "storm" | "thunderstorm" | "gewitter" => Kind::Storm,
            "snow" | "schnee" => Kind::Snow,
            "fog" | "nebel" => Kind::Fog,
            _ => return None,
        })
    }

    /// target weather values (NOT from data)
    fn preset(self) -> Wx {
        match self {
            Kind::Clear => Wx::default(),
            Kind::Rain => Wx { overcast: 0.75, rain: 0.6, snow: 0.0, fog: 0.0025, wind: 0.3, lightning: 0.0 },
            Kind::Storm => Wx { overcast: 1.0, rain: 1.0, snow: 0.0, fog: 0.004, wind: 1.0, lightning: 1.0 },
            Kind::Snow => Wx { overcast: 0.7, rain: 0.0, snow: 1.0, fog: 0.006, wind: 0.2, lightning: 0.0 },
            Kind::Fog => Wx { overcast: 0.5, rain: 0.0, snow: 0.0, fog: 0.012, wind: 0.0, lightning: 0.0 },
        }
    }
}

/// Weather values, blended towards the preset of the chosen kind.
#[derive(Clone, Copy, Default, Debug)]
struct Wx {
    overcast: f32,
    rain: f32,
    snow: f32,
    /// fog density at the camera height (1/m)
    fog: f32,
    wind: f32,
    lightning: f32,
}

impl Wx {
    fn lerp(self, o: Wx, t: f32) -> Wx {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Wx { overcast: l(self.overcast, o.overcast), rain: l(self.rain, o.rain), snow: l(self.snow, o.snow), fog: l(self.fog, o.fog), wind: l(self.wind, o.wind), lightning: l(self.lightning, o.lightning) }
    }
}

#[derive(Resource)]
pub struct Weather {
    pub enabled: bool,
    pub kind: Kind,
    /// time of day (h); None = the level's own sun position (set on the first frame)
    pub hour: Option<f32>,
    /// real minutes per 24 h; 0 = time stands still
    pub day_minutes: f32,
    /// Bevy VolumetricFog light shafts
    pub volumetric: bool,
    /// debug menu: strike now
    pub strike_request: bool,
    cur: Wx,
    seed: u32,
    next_strike: f32,
    /// seconds since the last strike and its brightness scale
    strike_t: f32,
    strike_scale: f32,
    /// UE direction towards the bolt
    bolt_dir: Vec3,
    fx_timer: f32,
    /// frames left that write the level's own values back after switching off
    restore: u8,
    /// level base hour (h) of the primary buffer, for the debug menu
    pub base_hour: f32,
    /// precipitation drift offsets (m, wrapped)
    rain_off: Vec3,
    snow_off: Vec3,
    sim_time: f32,
    /// a new round's hour the sun moves forward to (h) and the rate (h/s)
    hour_glide: Option<(f32, f32)>,
}

impl Default for Weather {
    fn default() -> Self {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let mut w = Weather {
            enabled: false,
            kind: Kind::Clear,
            hour: None,
            day_minutes: 0.0,
            volumetric: false,
            strike_request: false,
            cur: Wx::default(),
            seed: 0x2545_f491,
            next_strike: 3.0,
            strike_t: 99.0,
            strike_scale: 0.0,
            bolt_dir: Vec3::Z,
            fx_timer: 0.0,
            restore: 0,
            base_hour: 12.0,
            rain_off: Vec3::ZERO,
            snow_off: Vec3::ZERO,
            sim_time: 0.0,
            hour_glide: None,
        };
        if let Some(v) = env("HB_WEATHER") {
            match Kind::parse(&v) {
                Some(k) => {
                    w.enabled = true;
                    w.kind = k;
                    // start in the chosen weather (no fade-in from clear)
                    w.cur = k.preset();
                }
                None => warn!("HB_WEATHER={v}: unknown (clear|rain|storm|snow|fog)"),
            }
        }
        if let Some(m) = env("HB_DAYNIGHT").and_then(|v| v.parse::<f32>().ok()) {
            w.enabled = true;
            w.day_minutes = m.max(0.0);
        }
        if let Some(h) = env("HB_TIME").and_then(|v| v.parse::<f32>().ok()) {
            w.enabled = true;
            w.hour = Some(h.rem_euclid(24.0));
        }
        if env("HB_VOLUMETRIC").is_some_and(|v| v != "0") {
            w.enabled = true;
            w.volumetric = true;
        }
        if w.enabled {
            info!("weather: on ({:?}, day {} min, hour {:?}, volumetric {})", w.kind, w.day_minutes, w.hour, w.volumetric);
        }
        w
    }
}

impl Weather {
    fn rand(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / 16_777_216.0
    }

    /// lightning flash brightness now (0..)
    fn flash(&self) -> f32 {
        let t = self.strike_t;
        let mut f = 0.0;
        for (t0, a, tau) in PULSES {
            if t >= t0 {
                f += a * (-(t - t0) / tau).exp();
            }
        }
        f * self.strike_scale
    }

    /// Current rain strength 0..1 (0 while the weather is off; read by screenrain.rs).
    pub fn rain_amount(&self) -> f32 {
        if self.enabled { self.cur.rain } else { 0.0 }
    }

    /// A new round: random kind and hour, the simulation on. `instant`: no blend (match start).
    pub fn new_round(&mut self, seed: u32, instant: bool) -> (Kind, f32) {
        self.seed = seed | 1;
        for _ in 0..4 {
            self.rand();
        }
        let r = self.rand();
        let total: f32 = ROUND_KINDS.iter().map(|k| k.1).sum();
        let mut acc = 0.0;
        let mut kind = Kind::Clear;
        for (k, wgt) in ROUND_KINDS {
            acc += wgt / total;
            if r < acc {
                kind = k;
                break;
            }
        }
        let (a, b) = (self.rand(), self.rand());
        let hour = round_hour(a, b);
        self.enabled = true;
        self.kind = kind;
        self.next_strike = 3.0;
        if instant || self.hour.is_none() {
            self.cur = kind.preset();
            self.hour = Some(hour);
            self.hour_glide = None;
        } else {
            let from = self.hour.unwrap_or(self.base_hour);
            let ahead = (hour - from).rem_euclid(24.0);
            self.hour_glide = Some((hour, ahead / TRANSITION_S));
        }
        (kind, hour)
    }
}

/// Settings key of the option "Zufälliges Wetter/Tageszeit" (options.rs, settings.rs; default 1).
pub const RANDOM_KEY: &str = "HomeBots:RandomWeather";

/// Kinds of the random round weather and their weights (NOT from data: clear is the common one).
const ROUND_KINDS: [(Kind, f32); 5] = [(Kind::Clear, 0.5), (Kind::Rain, 0.15), (Kind::Storm, 0.1), (Kind::Snow, 0.1), (Kind::Fog, 0.15)];

/// Random hour from two uniform numbers (NOT from data): 70 % day 8-17 h, 15 % dawn / dusk
/// (5-8 h or 17-20 h), 15 % night (20-5 h).
fn round_hour(a: f32, b: f32) -> f32 {
    let h = if a < 0.7 {
        8.0 + 9.0 * b
    } else if a < 0.85 {
        if b < 0.5 { 5.0 + 6.0 * b } else { 17.0 + 6.0 * (b - 0.5) }
    } else {
        20.0 + 9.0 * b
    };
    h.rem_euclid(24.0)
}

/// Test values in the environment: the random round weather stays off.
fn env_fixed() -> bool {
    ["HB_WEATHER", "HB_TIME", "HB_DAYNIGHT"].iter().any(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()))
}

#[derive(Default)]
struct RoundWx {
    prev: Option<crate::menu::Phase>,
    /// a match is being created: randomise once its level is built
    pending: bool,
    /// this module switched the simulation on for the running match
    ours: bool,
    rounds: i32,
    serial: u32,
}

/// Random weather per round (see the module doc).
fn round_weather(mut w: ResMut<Weather>, flow: Res<crate::menu::MatchFlow>, obj: Res<crate::objectives::Objectives>, mut st: Local<RoundWx>) {
    use crate::menu::Phase;
    let phase = flow.phase;
    let entered = st.prev != Some(phase);
    st.prev = Some(phase);
    let on = crate::settings::flag(RANDOM_KEY) && !env_fixed();
    if entered && phase == Phase::Creating {
        st.pending = true;
    }
    // back to the front end: the level's own look
    if matches!(phase, Phase::Leaving | Phase::Front) {
        st.pending = false;
        if st.ours {
            st.ours = false;
            w.enabled = false;
            w.hour_glide = None;
            info!("round weather: off (front end)");
        }
        return;
    }
    let rounds = flow.hud.rounds[0] + flow.hud.rounds[1];
    let seed = || {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9e37);
        (n ^ (n >> 29)) as u32
    };
    if st.pending && phase != Phase::Creating {
        // the match level is built (Loading / Movie / Spawn): the first round's weather at once
        st.pending = false;
        st.rounds = rounds;
        if on {
            st.serial = st.serial.wrapping_add(1);
            let (k, h) = w.new_round(seed() ^ st.serial, true);
            st.ours = true;
            info!("round weather: match start: {k:?}, {:02}:{:02} h", h as u32, ((h.fract()) * 60.0) as u32);
        }
        return;
    }
    if matches!(phase, Phase::Spawn | Phase::Playing) && rounds != st.rounds {
        st.rounds = rounds;
        // (the match-deciding round ends the match: no new weather for the end banner)
        if on && !obj.match_over {
            st.serial = st.serial.wrapping_add(1);
            let (k, h) = w.new_round(seed() ^ st.serial, false);
            st.ours = true;
            info!("round weather: round {}: {k:?}, {:02}:{:02} h", rounds + 1, h as u32, ((h.fract()) * 60.0) as u32);
        }
    }
}

/// Sky flag of a placed mesh for objects[].z (gamemat.rs hf_weather_sky): 1 sky dome
/// (GCSkyboxActor), 2 fog dome (FogDome.FogDome of the GCFogBoxActor), 3 backdrop (meshes of
/// the game's SKY_Elements package: the San Francisco / Tiburon / Alcatraz cards, ships and tree
/// cards of the *_sky sublevels; only their unlit materials are darkened, gamemat.rs
/// hf_weather_backdrop, lit ones already follow the light), 0 everything else.
pub fn sky_flag(sky: bool, mesh_path: &str) -> f32 {
    if std::env::var_os("HB_NOSKYFLAG").is_some() {
        return 0.0;
    }
    if sky {
        1.0
    } else if mesh_path.contains("FogDome") {
        2.0
    } else if mesh_path.starts_with("SKY_Elements.") {
        3.0
    } else {
        0.0
    }
}

/// The storm sky glow is pooled with the level's effects (one instance, dormant until a storm).
pub fn add_pools(pools: &mut Vec<(String, usize)>) {
    // only when a storm can happen (HB_WEATHER set; the debug menu needs it too): the pooled
    // sky glow suspected of the white sky in tests 2026-10-21 (HB_LIGHTNINGPOOL=1 forces it)
    if std::env::var_os("HB_WEATHER").is_none() && std::env::var_os("HB_LIGHTNINGPOOL").is_none() {
        return;
    }
    if !pools.iter().any(|p| p.0 == LIGHTNING_FX) {
        pools.push((LIGHTNING_FX.to_string(), 1));
    }
}

// ---------------------------------------------------------------------------------------------
// GPU constants upload (main world -> render world without per-frame allocation)
// ---------------------------------------------------------------------------------------------

const MAX_BUFS: usize = 8;
/// gamemat::Globals is 12 vec4; the game-shader globals add MaterialUpper/LowerSkyColor and
/// CheapAmbientColor (main.rs)
const GLOBALS_SIZE: usize = std::mem::size_of::<crate::gamemat::Globals>();
const GS_SIZE: usize = GLOBALS_SIZE + 48;

struct Upload {
    n: usize,
    ids: [Option<AssetId<ShaderStorageBuffer>>; MAX_BUFS],
    /// upper, lower, light colour, light dir, ambient (offset 0)
    main: [[[f32; 4]; 5]; MAX_BUFS],
    /// sky_fx, sky_tint (offset 112)
    sky: [[[f32; 4]; 2]; MAX_BUFS],
    /// game-shader buffers: MaterialUpper/LowerSkyColor (offset 144)
    gs: [Option<[[f32; 4]; 2]>; MAX_BUFS],
    /// Globals.fog (translucent surfaces), the same for all buffers
    fog: [[f32; 4]; 3],
}

static UPLOAD: Mutex<Upload> = Mutex::new(Upload { n: 0, ids: [None; MAX_BUFS], main: [[[0.0; 4]; 5]; MAX_BUFS], sky: [[[0.0; 4]; 2]; MAX_BUFS], gs: [None; MAX_BUFS], fog: [[0.0; 4]; 3] });

fn upload(buffers: Res<RenderAssets<GpuShaderStorageBuffer>>, queue: Res<RenderQueue>) {
    let Ok(u) = UPLOAD.lock() else { return };
    for k in 0..u.n {
        let Some(id) = u.ids[k] else { continue };
        let Some(b) = buffers.get(id) else { continue };
        // a missing COPY_DST would be a fatal wgpu validation error
        if b.buffer.size() < GLOBALS_SIZE as u64 || !b.buffer.usage().contains(BufferUsages::COPY_DST) {
            continue;
        }
        queue.write_buffer(&b.buffer, 0, bytemuck::cast_slice(&u.main[k]));
        queue.write_buffer(&b.buffer, 112, bytemuck::cast_slice(&u.sky[k]));
        queue.write_buffer(&b.buffer, std::mem::offset_of!(crate::gamemat::Globals, fog) as u64, bytemuck::cast_slice(&u.fog));
        if let Some(g) = &u.gs[k] {
            if b.buffer.size() >= GS_SIZE as u64 {
                queue.write_buffer(&b.buffer, GLOBALS_SIZE as u64, bytemuck::cast_slice(g));
            }
        }
    }
}

/// The level-constant buffers in use (from the terrain / game-shader materials, as crater.rs).
#[derive(Resource, Default)]
struct Targets(Vec<AssetId<ShaderStorageBuffer>>);

fn collect_targets(mut ev: EventReader<AssetEvent<crate::terrainemu::TerrainEmu>>, mats: Res<Assets<crate::terrainemu::TerrainEmu>>, mut t: ResMut<Targets>) {
    if !ev.read().any(|e| matches!(e, AssetEvent::Added { .. } | AssetEvent::Removed { .. })) {
        return;
    }
    t.0.clear();
    for (_, m) in mats.iter() {
        let id = m.globals.id();
        if !t.0.contains(&id) && t.0.len() < MAX_BUFS {
            t.0.push(id);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Lighting model
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Base {
    upper: [f32; 4],
    lower: [f32; 4],
    color: [f32; 4],
    dir: [f32; 4],
    amb: [f32; 4],
    mat_sky: Option<[[f32; 4]; 2]>,
}

fn read_base(data: &[u8]) -> Option<Base> {
    if data.len() < GLOBALS_SIZE {
        return None;
    }
    let v = |o: usize| bytemuck::pod_read_unaligned::<[f32; 4]>(&data[o..o + 16]);
    Some(Base {
        upper: v(0),
        lower: v(16),
        color: v(32),
        dir: v(48),
        amb: v(64),
        mat_sky: (data.len() == GS_SIZE).then(|| [v(GLOBALS_SIZE), v(GLOBALS_SIZE + 16)]),
    })
}

fn v3(a: [f32; 4]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn luma(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.3, 0.59, 0.11))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Sun circle through the level's sun direction `l0` (UE, towards the sun): equinox path at a
/// latitude that gives the noon elevation, turned so that `base_hour(l0)` gives `l0` exactly.
struct SunPath {
    phi: f32,
    rot: f32,
    h0: f32,
}

impl SunPath {
    fn new(l0: Vec3) -> SunPath {
        let e0 = l0.z.clamp(-0.99, 0.99).asin();
        let emax = (e0 + NOON_OVER_BASE).max(NOON_MIN_ELEV).min(1.5);
        let phi = PI * 0.5 - emax;
        let h0 = (e0.sin() / phi.cos()).clamp(-1.0, 1.0).acos();
        let v0 = Self::local(phi, h0);
        let rot = l0.y.atan2(l0.x) - v0.y.atan2(v0.x);
        SunPath { phi, rot, h0 }
    }

    /// (east, north, up) at hour angle h, declination 0
    fn local(phi: f32, h: f32) -> Vec3 {
        Vec3::new(-h.sin(), -phi.sin() * h.cos(), phi.cos() * h.cos())
    }

    fn base_hour(&self) -> f32 {
        12.0 + self.h0 * 12.0 / PI
    }

    fn at(&self, hour: f32) -> Vec3 {
        let s = Self::local(self.phi, (hour - 12.0) / 12.0 * PI);
        let (sn, cs) = self.rot.sin_cos();
        Vec3::new(s.x * cs - s.y * sn, s.x * sn + s.y * cs, s.z)
    }
}

/// Everything derived for one level buffer this frame.
#[derive(Clone, Copy, Default)]
struct Lit {
    color: Vec3,
    /// UE, towards the light
    dir: Vec3,
    upper: Vec3,
    lower: Vec3,
    amb_add: Vec3,
    sky_fx: Vec4,
    sky_tint: Vec3,
    /// brightness of unlit backdrop cards (sky flag 3) and of the global MaterialTintColor
    /// (1 = the level's look): the sky-light fraction lit surfaces get (sky_tint.w = 1 - this)
    backdrop: f32,
    mat_upper: Vec3,
    mat_lower: Vec3,
}

struct Frame {
    hour: f32,
    wx: Wx,
    flash: f32,
    bolt_dir: Vec3,
}

fn light(b: &Base, f: &Frame) -> Lit {
    let l0 = v3(b.dir).normalize_or_zero();
    let bc = v3(b.color);
    let path = SunPath::new(l0);
    let s = path.at(f.hour);
    let se = s.z;
    let rel = |x: f32| smoothstep(-0.02, 0.25, x);
    let sun_i = if rel(l0.z) > 1e-3 { (rel(se) / rel(l0.z)).min(1.25) } else { 0.0 };
    let warm = SUN_WARM.lerp(Vec3::ONE, smoothstep(0.0, 0.3, se));
    let (mut dir, mut color) = if se > -0.02 { (s, bc * warm * sun_i) } else { (-s, MOON_COL * luma(bc) * MOON_I * smoothstep(0.0, 0.15, -se)) };
    let day_k = smoothstep(-0.25, 0.25, se);
    DAY_K.store(day_k.to_bits(), std::sync::atomic::Ordering::Relaxed);
    let dusk = 1.0 - smoothstep(0.0, 0.3, (se - 0.02).abs());
    let amb = NIGHT_AMB + (1.0 - NIGHT_AMB) * day_k;
    let tint = Vec3::ONE.lerp(NIGHT_TINT, 1.0 - day_k) * Vec3::ONE.lerp(DUSK_TINT, dusk * 0.6);
    let mut sky_mul = SKY_NIGHT + (1.0 - SKY_NIGHT) * smoothstep(-0.2, 0.15, se);
    let sky_tint = Vec3::ONE.lerp(SKY_NIGHT_TINT, 1.0 - day_k) * Vec3::ONE.lerp(SKY_DUSK_TINT, dusk);
    let mut stars = 1.0 - smoothstep(-0.15, 0.02, se);
    // weather
    let w = f.wx;
    color *= 1.0 - OVC_SUN * w.overcast;
    let sky_light = |c: Vec3, snow_k: Vec3| -> Vec3 {
        let c = c * amb * tint;
        let c = c.lerp(Vec3::splat(luma(c)), OVC_SKY_DESAT * w.overcast) * (1.0 - OVC_SKY_DIM * w.overcast);
        c * Vec3::ONE.lerp(snow_k, w.snow)
    };
    let mut upper = sky_light(v3(b.upper), SNOW_UPPER);
    let lower = sky_light(v3(b.lower), SNOW_LOWER);
    let (mut mat_upper, mat_lower) = match b.mat_sky {
        Some(m) => (sky_light(v3(m[0]), SNOW_UPPER), sky_light(v3(m[1]), SNOW_LOWER)),
        None => (Vec3::ZERO, Vec3::ZERO),
    };
    sky_mul *= 1.0 - OVC_DOME_DARK * w.overcast * (1.0 - 0.5 * w.snow);
    stars *= 1.0 - w.overcast;
    // unlit backdrop cards / MaterialTintColor: as dark as the sky light on lit surfaces
    // (night fraction, overcast dimming; tint = sky_tint) (NOT from data)
    let mut backdrop = amb * (1.0 - OVC_SKY_DIM * w.overcast);
    // lightning: light from the bolt (its shadows) while it outshines the sun
    let mut amb_add = Vec3::ZERO;
    if f.flash > 1e-3 {
        let bolt = BOLT_COL * BOLT_I * f.flash;
        if luma(bolt) > luma(color) {
            dir = f.bolt_dir;
        }
        color += bolt;
        upper += BOLT_SKY * BOLT_SKY_I * f.flash;
        mat_upper += BOLT_SKY * BOLT_SKY_I * f.flash;
        amb_add = BOLT_SKY * BOLT_AMB * f.flash;
        backdrop += BOLT_AMB * f.flash;
    }
    // the shading / shadow light vector stays above MIN_LIGHT_Z
    if dir.z < MIN_LIGHT_Z {
        let h = Vec2::new(dir.x, dir.y).normalize_or(Vec2::X) * (1.0 - MIN_LIGHT_Z * MIN_LIGHT_Z).sqrt();
        dir = Vec3::new(h.x, h.y, MIN_LIGHT_Z);
    }
    Lit {
        color,
        dir: dir.normalize(),
        upper,
        lower,
        amb_add,
        sky_fx: Vec4::new(1.0 - sky_mul, stars, OVC_DOME_DESAT * w.overcast, f.flash * 1.2),
        sky_tint,
        backdrop: backdrop.clamp(0.0, 1.0),
        mat_upper,
        mat_lower,
    }
}

/// Values of the primary (shown) level for the light entity, fog, grass and precipitation.
#[derive(Resource, Default)]
struct Primary {
    valid: bool,
    base: Option<Base>,
    lit: Lit,
}

// ---------------------------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------------------------

/// Daylight factor of the current frame (1 day .. 0 night; 1 while the weather is off: the
/// levels are lit for day). Read by dynlights.rs for the tracer light dimming.
static DAY_K: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

pub fn daylight() -> f32 {
    f32::from_bits(DAY_K.load(std::sync::atomic::Ordering::Relaxed))
}

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
        let _ = shaders.insert(PRECIP_SHADER.id(), Shader::from_wgsl(PRECIP_WGSL, "hb_precip.wgsl"));
        let _ = shaders.insert(FOG_SHADER.id(), Shader::from_wgsl(FOG_WGSL, "hb_weather_fog.wgsl"));
        app.init_resource::<Weather>()
            .init_resource::<Targets>()
            .init_resource::<Primary>()
            .init_resource::<PrecipAssets>()
            .init_resource::<LightBase>()
            .add_plugins(MaterialPlugin::<PrecipMat> { prepass_enabled: false, shadows_enabled: false, ..default() })
            .add_plugins((ExtractComponentPlugin::<WeatherFog>::default(), UniformComponentPlugin::<WeatherFog>::default()))
            .add_systems(Update, (round_weather, collect_targets, simulate, (apply_light, apply_fog, apply_grass, apply_precip, update_precip_occlusion, apply_volumetric)).chain());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(Render, upload.in_set(RenderSet::PrepareResources))
            .add_render_graph_node::<ViewNodeRunner<FogNode>>(Core3d, WeatherFogLabel)
            .add_render_graph_edges(Core3d, (Node3d::MainOpaquePass, WeatherFogLabel, Node3d::MainTransmissivePass))
            .add_render_graph_edges(Core3d, (WeatherFogLabel, crate::scenecopy::SceneCopyLabel));
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<FogPipeline>();
    }
}

#[allow(clippy::too_many_arguments)]
fn simulate(
    time: Res<Time>,
    mut w: ResMut<Weather>,
    targets: Res<Targets>,
    buffers: Res<Assets<ShaderStorageBuffer>>,
    lights: Query<&DirectionalLight>,
    mut prim: ResMut<Primary>,
    (mut sfx, mut fx): (ResMut<crate::sfx::SfxQueue>, ResMut<crate::particles::Particles>),
    cams: Query<&GlobalTransform, (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    light_base: Res<LightBase>,
) {
    let dt = time.delta_secs().min(0.25);
    let Ok(mut up) = UPLOAD.lock() else { return };
    if !w.enabled {
        prim.valid = false;
        if w.restore == 0 {
            up.n = 0;
            return;
        }
        // write the level's own values back
        w.restore -= 1;
        up.n = 0;
        for &id in &targets.0 {
            let Some(b) = buffers.get(id).and_then(|b| b.data.as_deref()).and_then(read_base) else { continue };
            let k = up.n;
            up.ids[k] = Some(id);
            up.main[k] = [b.upper, b.lower, b.color, b.dir, b.amb];
            up.sky[k] = [[0.0; 4]; 2];
            up.gs[k] = b.mat_sky;
            up.fog = [[0.0; 4]; 3];
            up.n += 1;
        }
        return;
    }
    w.restore = 3;
    w.sim_time += dt;
    // weather blend
    let target = w.kind.preset();
    let k = 1.0 - (-dt / TRANSITION_S).exp();
    w.cur = w.cur.lerp(target, k);
    // lightning
    let storm = w.cur.lightning > 0.5;
    w.strike_t += dt;
    w.next_strike -= dt;
    if (storm && w.next_strike <= 0.0) || std::mem::take(&mut w.strike_request) {
        let dist = BOLT_DIST.0 + (BOLT_DIST.1 - BOLT_DIST.0) * w.rand();
        let az = w.rand() * std::f32::consts::TAU;
        w.bolt_dir = Vec3::new(az.cos() * 0.45, az.sin() * 0.45, 1.0).normalize();
        w.strike_t = 0.0;
        w.strike_scale = (1.25 - dist / 3500.0).clamp(0.35, 1.0);
        w.next_strike = STRIKE_MIN_S + STRIKE_RAND_S * w.rand();
        let wave = THUNDER[((w.rand() * THUNDER.len() as f32) as usize).min(THUNDER.len() - 1)];
        let pitch = THUNDER_PITCH.0 + (THUNDER_PITCH.1 - THUNDER_PITCH.0) * w.rand();
        let vol = THUNDER_VOL * (1.1 - dist / 5000.0).clamp(0.35, 1.0);
        sfx.wave(wave, vol, pitch, dist / SPEED_OF_SOUND);
    }
    let cam_ue = cams.iter().next().map(|g| {
        let p = g.translation();
        Vec3::new(p.x, p.z, p.y) * 100.0
    });
    // the game's storm sky glow (FX_Lightning, 15 s emitter), restarted over the camera
    if storm {
        w.fx_timer -= dt;
        if w.fx_timer <= 0.0 {
            w.fx_timer = LIGHTNING_FX_EVERY;
            if let Some(c) = cam_ue {
                fx.fire(LIGHTNING_FX, c + Vec3::new(0.0, 0.0, LIGHTNING_FX_UP), Quat::IDENTITY);
            }
        }
    } else {
        w.fx_timer = 0.0;
    }
    if w.cur.wind > 0.05 {
        sfx.wave_loop("weather_wind", WIND_WAVE, WIND_VOL * w.cur.wind, 1.0);
    }
    // precipitation drift (wrapped by the shader's volume)
    let wind = WIND * w.cur.wind;
    let rain_v = Vec3::new(wind.x, -RAIN_FALL, wind.y);
    let snow_v = Vec3::new(wind.x * 0.5, -SNOW_FALL, wind.y * 0.5);
    let rb = Vec3::new(RAIN_BOX.0, RAIN_BOX.1, RAIN_BOX.0);
    let sb = Vec3::new(SNOW_BOX.0, SNOW_BOX.1, SNOW_BOX.0);
    w.rain_off = (w.rain_off + rain_v * dt).rem_euclid(rb);
    w.snow_off = (w.snow_off + snow_v * dt).rem_euclid(sb);

    // primary level buffer: the one whose sun is the DirectionalLight's (spawn_lighting)
    let light_travel = light_base.0.map(|(_, r, _, _)| r * Vec3::NEG_Z).or_else(|| lights.iter().next().map(|_| Vec3::NEG_Y));
    let mut best: Option<(f32, Base)> = None;
    let mut bases: [Option<Base>; MAX_BUFS] = [None; MAX_BUFS];
    for (i, &id) in targets.0.iter().enumerate().take(MAX_BUFS) {
        let Some(b) = buffers.get(id).and_then(|b| b.data.as_deref()).and_then(read_base) else { continue };
        bases[i] = Some(b);
        if b.mat_sky.is_some() || v3(b.dir).length_squared() < 0.5 {
            continue;
        }
        let d = v3(b.dir);
        let score = light_travel.map(|t| -t.dot(Vec3::new(d.x, d.z, d.y))).unwrap_or(0.0);
        if best.as_ref().is_none_or(|x| score > x.0) {
            best = Some((score, b));
        }
    }
    if let Some((_, b)) = &best {
        let bh = SunPath::new(v3(b.dir).normalize()).base_hour();
        w.base_hour = bh;
        if w.hour.is_none() {
            w.hour = Some(bh);
        }
    }
    // a new round's hour: the sun moves forward to it (round_weather)
    if let Some((to, rate)) = w.hour_glide {
        let cur = w.hour.unwrap_or(w.base_hour);
        let left = (to - cur).rem_euclid(24.0);
        if left <= rate * dt || rate <= 0.0 || left > 23.99 {
            w.hour = Some(to);
            w.hour_glide = None;
        } else {
            w.hour = Some((cur + rate * dt).rem_euclid(24.0));
        }
    }
    if w.day_minutes > 0.0 {
        let h = w.hour.unwrap_or(w.base_hour) + dt * 24.0 / (w.day_minutes * 60.0);
        w.hour = Some(h.rem_euclid(24.0));
    }
    let frame = Frame { hour: w.hour.unwrap_or(w.base_hour), wx: w.cur, flash: w.flash(), bolt_dir: w.bolt_dir };
    up.n = 0;
    for (i, &id) in targets.0.iter().enumerate().take(MAX_BUFS) {
        let Some(b) = bases[i] else { continue };
        if v3(b.dir).length_squared() < 0.5 {
            continue;
        }
        let l = light(&b, &frame);
        let k = up.n;
        up.ids[k] = Some(id);
        let d = l.dir;
        // lower_sky.w = SkyLightComponent.LowerTerrainBrightness: the shaders use it as
        // GrassColorTexture * w for the lower hemisphere near the terrain (gamemat.rs lit path,
        // the game shaders' LowerSkyColor.w, terrainemu). It was left at the day value, so at
        // night everything within TerrainLightingFalloffDistance (80-100 uu) above the terrain
        // height under the object's origin - the lower part of every house / container wall -
        // kept a daylight ground bounce (Angel Island SkyLight_1: w 6.0) while the rest was at
        // 12 % ("lower half of the houses still slightly bright", 2026-10-21 18:27).
        // NOT from data: scaled with the lower sky colour's luminance ratio (night, overcast,
        // snow, lightning follow the sky light).
        let lk = if luma(v3(b.lower)) > 1e-5 { luma(l.lower) / luma(v3(b.lower)) } else { l.backdrop };
        up.main[k] = [
            [l.upper.x, l.upper.y, l.upper.z, b.upper[3]],
            [l.lower.x, l.lower.y, l.lower.z, b.lower[3] * lk],
            [l.color.x, l.color.y, l.color.z, b.color[3]],
            [d.x, d.y, d.z, if d.z.abs() > 1e-4 { 1.0 / d.z } else { 0.0 }],
            [b.amb[0] + l.amb_add.x, b.amb[1] + l.amb_add.y, b.amb[2] + l.amb_add.z, b.amb[3]],
        ];
        up.sky[k] = [l.sky_fx.to_array(), [1.0 - l.sky_tint.x, 1.0 - l.sky_tint.y, 1.0 - l.sky_tint.z, 1.0 - l.backdrop]];
        up.gs[k] = b.mat_sky.map(|m| [[l.mat_upper.x, l.mat_upper.y, l.mat_upper.z, m[0][3]], [l.mat_lower.x, l.mat_lower.y, l.mat_lower.z, m[1][3]]]);
        up.n += 1;
    }
    match best {
        Some((_, b)) => {
            prim.valid = true;
            prim.base = Some(b);
            prim.lit = light(&b, &frame);
        }
        None => prim.valid = false,
    }
    up.fog = translucent_fog(&w, &prim);
}

/// The level's DirectionalLight as spawned (entity, rotation, colour, illuminance), restored
/// when the weather is switched off.
#[derive(Resource, Default)]
struct LightBase(Option<(Entity, Quat, Color, f32)>);

fn apply_light(w: Res<Weather>, prim: Res<Primary>, mut base: Local<Option<(Entity, Quat, Color, f32)>>, mut lights: Query<(Entity, &mut DirectionalLight, &mut Transform)>, mut lb: ResMut<LightBase>) {
    let Some((e, mut dl, mut tf)) = lights.iter_mut().next() else {
        *base = None;
        lb.0 = None;
        return;
    };
    if base.is_none_or(|b| b.0 != e) {
        *base = Some((e, tf.rotation, dl.color, dl.illuminance));
    }
    let b = base.unwrap();
    lb.0 = Some(b);
    if !w.enabled || !prim.valid {
        if tf.rotation != b.1 {
            tf.rotation = b.1;
            dl.color = b.2;
            dl.illuminance = b.3;
        }
        return;
    }
    let l = prim.lit;
    // UE towards-light -> Bevy travel direction (spawn_lighting)
    let travel = -Vec3::new(l.dir.x, l.dir.z, l.dir.y);
    let up = if travel.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
    tf.rotation = Transform::IDENTITY.looking_to(travel, up).rotation;
    let m = l.color.max_element().max(1e-4);
    dl.color = Color::linear_rgb(l.color.x / m, l.color.y / m, l.color.z / m);
    dl.illuminance = m * PI * 1.2;
}

fn apply_grass(w: Res<Weather>, prim: Res<Primary>, mut grass: ResMut<Assets<crate::grass::GrassMat>>, mut last: Local<Vec3>, mut ev: EventReader<AssetEvent<crate::grass::GrassMat>>) {
    let added = ev.read().any(|e| matches!(e, AssetEvent::Added { .. }));
    let v = match (&prim.base, w.enabled && prim.valid) {
        (Some(b), true) => {
            let l = &prim.lit;
            let num = l.color + l.upper;
            let den = v3(b.color) + v3(b.upper);
            let r = Vec3::new(num.x / den.x.max(1e-4), num.y / den.y.max(1e-4), num.z / den.z.max(1e-4)).clamp(Vec3::ZERO, Vec3::splat(3.0));
            Vec3::ONE - r
        }
        _ => Vec3::ZERO,
    };
    if (v - *last).abs().max_element() < 0.004 && !added {
        return;
    }
    *last = v;
    let ids: Vec<_> = grass.ids().collect();
    for id in ids {
        if let Some(m) = grass.get_mut(id) {
            m.params.weather = v.extend(0.0);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Fog (full-screen pass after the opaque pass)
// ---------------------------------------------------------------------------------------------

/// Per-camera fog constants (Bevy world, metres).
#[derive(Component, Clone, Copy, Default, ShaderType)]
pub struct WeatherFog {
    /// camera position, w = near plane
    cam_pos: Vec4,
    /// view right * tan(fov_x / 2)
    cam_right: Vec4,
    /// view up * tan(fov_y / 2)
    cam_up: Vec4,
    /// view forward
    cam_fwd: Vec4,
    /// rgb fog colour, w density (1/m) at the camera height
    fog_color: Vec4,
    /// direction towards the light (Bevy), w height falloff (1/m)
    sun_dir: Vec4,
    /// rgb sun in-scatter colour, w max opacity
    sun_color: Vec4,
    /// x distance used for the empty sky (m)
    misc: Vec4,
}

impl ExtractComponent for WeatherFog {
    type QueryData = &'static WeatherFog;
    type QueryFilter = ();
    type Out = WeatherFog;

    fn extract_component(item: QueryItem<'_, Self::QueryData>) -> Option<Self::Out> {
        (item.fog_color.w > 0.0).then_some(*item)
    }
}

/// Globals.fog: the constants of the full-screen fog (apply_fog) for translucent surfaces;
/// zero when that pass is off.
fn translucent_fog(w: &Weather, prim: &Primary) -> [[f32; 4]; 3] {
    if !(w.enabled && prim.valid && w.cur.fog > 1e-5) {
        return [[0.0; 4]; 3];
    }
    let l = &prim.lit;
    let fc = l.upper * FOG_SKY_K + l.color * FOG_LIGHT_K;
    let sun = Vec3::new(l.dir.x, l.dir.z, l.dir.y);
    let sc = l.color * FOG_SUN_K * (1.0 - 0.5 * w.cur.overcast);
    [fc.extend(w.cur.fog).to_array(), sun.extend(FOG_FALLOFF).to_array(), sc.extend(FOG_MAX).to_array()]
}

fn apply_fog(mut commands: Commands, w: Res<Weather>, prim: Res<Primary>, mut cams: Query<(Entity, &GlobalTransform, &Projection, Option<&mut WeatherFog>), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>) {
    for (e, gt, proj, fog) in &mut cams {
        let on = w.enabled && prim.valid && w.cur.fog > 1e-5;
        let Some(mut fog) = fog else {
            if on {
                commands.entity(e).insert(WeatherFog::default());
            }
            continue;
        };
        if !on {
            if fog.fog_color.w != 0.0 {
                fog.fog_color.w = 0.0;
            }
            continue;
        }
        let Projection::Perspective(p) = proj else { continue };
        let ty = (p.fov * 0.5).tan();
        let tx = ty * p.aspect_ratio;
        let (_, r, t) = gt.to_scale_rotation_translation();
        // the same constants as the translucent surfaces' fog (Globals.fog)
        let [fc, sun, sc] = translucent_fog(&w, &prim).map(Vec4::from_array);
        *fog = WeatherFog {
            cam_pos: t.extend(p.near),
            cam_right: (r * Vec3::X * tx).extend(0.0),
            cam_up: (r * Vec3::Y * ty).extend(0.0),
            cam_fwd: (r * Vec3::NEG_Z).extend(0.0),
            fog_color: fc,
            sun_dir: sun,
            sun_color: sc,
            misc: Vec4::new(FOG_SKY_DIST, 0.0, 0.0, 0.0),
        };
    }
}

/// The fog of FOG_WGSL for translucent surfaces, evaluated in their own shaders (they are
/// drawn after the full-screen pass; unfogged road edges stood out dark against the fogged
/// terrain). Globals.fog (gamemat.rs) holds the constants; UE world position in, (fog colour,
/// opacity) out; density 0 = no fog.
pub const TRANSLUCENT_FOG_WGSL: &str = r#"
fn hf_translucent_fog(wue: vec3<f32>) -> vec4<f32> {
    let dens = hfg.fog_color.w;
    if (dens <= 0.0) { return vec4<f32>(0.0); }
    let ray = vec3<f32>(wue.x, wue.z, wue.y) * 0.01 - view.world_position;
    let dist = length(ray);
    let kd = hfg.fog_sun_dir.w * ray.y;
    var od = dens * dist;
    if (abs(kd) > 0.0001) {
        od = dens * dist * (1.0 - exp(-kd)) / kd;
    }
    let fog = min(1.0 - exp(-od), hfg.fog_sun.w);
    let mu = max(dot(ray / max(dist, 0.0001), hfg.fog_sun_dir.xyz), 0.0);
    let mu2 = mu * mu;
    return vec4<f32>(hfg.fog_color.rgb + hfg.fog_sun.rgb * (mu2 * mu2 * mu2 * mu2), fog);
}
"#;

const FOG_SHADER: Handle<Shader> = bevy::asset::weak_handle!("b7e1c2d4-5a6f-4e3b-9c8d-1f2e3a4b5c6d");

const FOG_WGSL: &str = r#"
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct WeatherFog {
    cam_pos: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    cam_fwd: vec4<f32>,
    fog_color: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    misc: vec4<f32>,
}

@group(0) @binding(0) var color: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var<uniform> f: WeatherFog;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.position.xy);
    let c = textureLoad(color, p, 0);
    let d = textureLoad(depth, p, 0);
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    // view ray with forward component 1; reverse-Z infinite projection: view z = near / depth
    let ray1 = f.cam_fwd.xyz + ndc.x * f.cam_right.xyz + ndc.y * f.cam_up.xyz;
    let vz = select(f.misc.x, min(f.cam_pos.w / d, f.misc.x), d > 0.0);
    let ray = ray1 * vz;
    let dist = length(ray);
    // exponential height fog, analytic along the ray (density at the camera height)
    let k = f.sun_dir.w;
    let kd = k * ray.y;
    var od = f.fog_color.w * dist;
    if (abs(kd) > 0.0001) {
        od = f.fog_color.w * dist * (1.0 - exp(-kd)) / kd;
    }
    let fog = min(1.0 - exp(-od), f.sun_color.w);
    let mu = max(dot(ray / max(dist, 0.0001), f.sun_dir.xyz), 0.0);
    let mu2 = mu * mu;
    let fc = f.fog_color.rgb + f.sun_color.rgb * (mu2 * mu2 * mu2 * mu2);
    return vec4<f32>(mix(c.rgb, fc, fog), c.a);
}
"#;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct WeatherFogLabel;

#[derive(Resource)]
struct FogPipeline {
    layout: BindGroupLayout,
    pipeline: CachedRenderPipelineId,
}

impl FromWorld for FogPipeline {
    fn from_world(world: &mut World) -> Self {
        let dev = world.resource::<RenderDevice>();
        let layout = dev.create_bind_group_layout(
            "hb_weather_fog_layout",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_2d(TextureSampleType::Float { filterable: false }), texture_depth_2d(), uniform_buffer::<WeatherFog>(true))),
        );
        let cache = world.resource_mut::<PipelineCache>();
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("hb_weather_fog".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader_vertex_state(),
            fragment: Some(FragmentState {
                shader: FOG_SHADER,
                shader_defs: vec![],
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState { format: ViewTarget::TEXTURE_FORMAT_HDR, blend: None, write_mask: ColorWrites::ALL })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });
        FogPipeline { layout, pipeline }
    }
}

#[derive(Default)]
struct FogNode;

impl ViewNode for FogNode {
    type ViewQuery = (&'static ViewTarget, &'static ViewDepthTexture, &'static DynamicUniformIndex<WeatherFog>);

    fn run(&self, _graph: &mut RenderGraphContext, ctx: &mut RenderContext, (target, depth, index): QueryItem<Self::ViewQuery>, world: &World) -> Result<(), NodeRunError> {
        // the HDR target only (the fog pipeline's format); HB_NOFOGPASS=1: pass off (diagnosis)
        if !target.is_hdr() || crate::diag("HB_NOFOGPASS") {
            return Ok(());
        }
        let pipe = world.resource::<FogPipeline>();
        let Some(p) = world.resource::<PipelineCache>().get_render_pipeline(pipe.pipeline) else { return Ok(()) };
        let Some(ubind) = world.resource::<ComponentUniforms<WeatherFog>>().uniforms().binding() else { return Ok(()) };
        let post = target.post_process_write();
        let bg = ctx.render_device().create_bind_group("hb_weather_fog", &pipe.layout, &BindGroupEntries::sequential((post.source, depth.view(), ubind)));
        let mut rp = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("hb_weather_fog"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, resolve_target: None, ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_render_pipeline(p);
        rp.set_bind_group(0, &bg, &[index.index()]);
        rp.draw(0..3, 0..1);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Bevy VolumetricFog light shafts (optional)
// ---------------------------------------------------------------------------------------------

#[derive(Component)]
struct ShaftVolume;

#[allow(clippy::type_complexity)]
fn apply_volumetric(
    mut commands: Commands,
    w: Res<Weather>,
    prim: Res<Primary>,
    cams: Query<(Entity, &GlobalTransform, Has<VolumetricFog>), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    lights: Query<(Entity, Has<VolumetricLight>), With<DirectionalLight>>,
    mut vols: Query<(Entity, &mut Transform, &mut FogVolume), With<ShaftVolume>>,
) {
    let on = w.enabled && w.volumetric && prim.valid;
    for (e, gt, has) in &cams {
        if on && !has {
            commands.entity(e).insert(VolumetricFog { ambient_intensity: 0.0, step_count: VOL_STEPS, jitter: 0.0, ..default() });
        } else if !on && has {
            commands.entity(e).remove::<VolumetricFog>();
        }
        if on {
            match vols.iter_mut().next() {
                Some((_, mut tf, mut fv)) => {
                    tf.translation = gt.translation();
                    // denser in fog / rain, thin when overcast hides the sun anyway
                    fv.density_factor = VOL_DENSITY * (1.0 + 2.0 * w.cur.fog / 0.012);
                }
                None => {
                    commands.spawn((ShaftVolume, FogVolume { density_factor: VOL_DENSITY, absorption: 0.3, scattering: 0.3, ..default() }, Transform::from_translation(gt.translation()).with_scale(VOL_BOX)));
                }
            }
        }
    }
    for (e, has) in &lights {
        if on && !has {
            commands.entity(e).insert(VolumetricLight);
        } else if !on && has {
            commands.entity(e).remove::<VolumetricLight>();
        }
    }
    if !on {
        for (e, _, _) in &vols {
            commands.entity(e).despawn();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Precipitation: camera-relative wrapped volume of quads animated in the vertex shader
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct PrecipParams {
    /// x horizontal size, y height, z near fade (m), w drawn fraction
    box_size: Vec4,
    /// rgb colour, a opacity
    color: Vec4,
    /// xyz drift offset (m), w unused
    motion: Vec4,
    /// x mode (0 rain sheets, 1 snow flakes), y width, z height (m)
    shape: Vec4,
    /// xyz fall direction (normalised), w snow sway (m)
    fall: Vec4,
    /// x time (s)
    misc: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PrecipMat {
    #[uniform(0)]
    params: PrecipParams,
    #[texture(1)]
    #[sampler(2)]
    tex: Option<Handle<Image>>,
    /// roof heights around the camera (OCC_N x OCC_N cells, toroidal; rgba32f: top surface
    /// height (m), cell x, cell z, 1 = valid)
    #[texture(3, sample_type = "float", filterable = false)]
    occ: Option<Handle<Image>>,
}

const PRECIP_SHADER: Handle<Shader> = bevy::asset::weak_handle!("3c9d8e7f-2b1a-4c5d-8e6f-7a8b9c0d1e2f");

impl Material for PrecipMat {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(PRECIP_SHADER)
    }
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(PRECIP_SHADER)
    }
    // premultiplied blend: alpha 0 = additive (rain), alpha a = over (snow)
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn specialize(_p: &MaterialPipeline<Self>, d: &mut RenderPipelineDescriptor, layout: &MeshVertexBufferLayoutRef, _k: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        let vb = layout.0.get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0), Mesh::ATTRIBUTE_UV_0.at_shader_location(1), Mesh::ATTRIBUTE_UV_1.at_shader_location(2)])?;
        d.vertex.buffers = vec![vb];
        d.primitive.cull_mode = None;
        Ok(())
    }
}

const PRECIP_WGSL: &str = r#"
#import bevy_pbr::mesh_view_bindings::view

const OCC_N: i32 = 64;
const OCC_CELL: f32 = 1.0;

struct PrecipParams {
    box_size: vec4<f32>,
    color: vec4<f32>,
    motion: vec4<f32>,
    shape: vec4<f32>,
    fall: vec4<f32>,
    misc: vec4<f32>,
}

@group(2) @binding(0) var<uniform> p: PrecipParams;
@group(2) @binding(1) var tex: texture_2d<f32>;
@group(2) @binding(2) var smp: sampler;
@group(2) @binding(3) var occ: texture_2d<f32>;

struct VIn {
    // random position in the unit volume
    @location(0) seed: vec3<f32>,
    // quad corner: x -1 / 1, y 0 bottom / 1 top
    @location(1) corner: vec2<f32>,
    // x: drawn below this intensity, y: variation
    @location(2) rnd: vec2<f32>,
}

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fade: f32,
}

@vertex
fn vertex(v: VIn) -> VOut {
    var o: VOut;
    o.uv = vec2<f32>(0.0);
    o.fade = 0.0;
    if (v.rnd.x >= p.box_size.w) {
        o.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return o;
    }
    let size = vec3<f32>(p.box_size.x, p.box_size.y, p.box_size.x);
    let cam = view.world_position;
    let w = v.seed * size + p.motion.xyz;
    let rel = (fract((w - cam) / size) - vec3<f32>(0.5)) * size;
    var c = cam + rel;
    let snow = p.shape.x > 0.5;
    let t = p.misc.x;
    if (snow) {
        c += vec3<f32>(sin(t * 0.8 + v.rnd.y * 40.0), 0.0, cos(t * 0.6 + v.rnd.y * 23.0)) * p.fall.w;
    }
    // under a roof / any cover: the top surface of the drop's 1 m column (weather.rs
    // update_precip_occlusion) is above the drop -> not drawn (user 2026-10-22 21:11)
    let cell = vec2<i32>(floor(c.xz / OCC_CELL));
    let ix = ((cell % vec2<i32>(OCC_N)) + vec2<i32>(OCC_N)) % vec2<i32>(OCC_N);
    let oc = textureLoad(occ, ix, 0);
    if (oc.a > 0.5 && i32(oc.g) == cell.x && i32(oc.b) == cell.y && c.y < oc.r) {
        o.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return o;
    }
    let to_cam = cam - c;
    let dist = length(to_cam);
    var pos: vec3<f32>;
    if (snow) {
        let right = view.world_from_view[0].xyz;
        let up = view.world_from_view[1].xyz;
        let s = p.shape.y * (0.6 + 0.8 * fract(v.rnd.y * 7.0));
        pos = c + (right * v.corner.x + up * (v.corner.y * 2.0 - 1.0)) * s * 0.5;
        o.uv = vec2<f32>(v.corner.x * 0.5 + 0.5, 1.0 - v.corner.y);
    } else {
        // sheet turned to the camera around the fall axis
        let axis = -p.fall.xyz;
        var right = cross(axis, to_cam);
        let rl = length(right);
        right = select(vec3<f32>(1.0, 0.0, 0.0), right / max(rl, 0.0001), rl > 0.0001);
        pos = c + right * (v.corner.x * p.shape.y * 0.5) + axis * ((v.corner.y - 0.5) * p.shape.z);
        // one of the texture's 4 drop clusters
        let frame = floor(fract(v.rnd.y * 13.0) * 4.0);
        o.uv = vec2<f32>((frame + v.corner.x * 0.5 + 0.5) * 0.25, 1.0 - v.corner.y);
    }
    let edge = 1.0 - smoothstep(0.32 * size.x, 0.5 * size.x, length(rel.xz));
    let vert = 1.0 - smoothstep(0.32 * size.y, 0.5 * size.y, abs(rel.y));
    o.fade = smoothstep(p.box_size.z * 0.4, p.box_size.z, dist) * edge * vert;
    o.clip = view.clip_from_world * vec4<f32>(pos, 1.0);
    return o;
}

@fragment
fn fragment(i: VOut) -> @location(0) vec4<f32> {
    let s = textureSample(tex, smp, i.uv);
    let l = max(max(s.r, s.g), s.b);
    let a = clamp(l * p.color.a * i.fade, 0.0, 1.0);
    if (p.shape.x > 0.5) {
        return vec4<f32>(p.color.rgb * a, a);
    }
    return vec4<f32>(p.color.rgb * a, 0.0);
}
"#;

#[derive(Resource, Default)]
struct PrecipAssets {
    tried: bool,
    rain: Option<(Entity, Handle<PrecipMat>)>,
    snow: Option<(Entity, Handle<PrecipMat>)>,
    /// roof-height grid (update_precip_occlusion)
    occ: Option<Handle<Image>>,
    /// per cell: (cell x, cell z) it holds, top height (m); None = not computed
    occ_cells: Vec<Option<(i32, i32, f32)>>,
    /// collision triangle count the grid was built with (level change -> rebuilt)
    occ_tris: usize,
}

/// Roof grid: OCC_N x OCC_N cells of OCC_CELL m around the camera (covers the rain / snow boxes,
/// 36 / 30 m). NOT from data (the game has no rain).
const OCC_N: i32 = 64;
const OCC_CELL: f32 = 1.0;
/// cells (rays) computed per frame
const OCC_RAYS_PER_FRAME: usize = 384;

fn occ_image() -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut img = Image::new_fill(Extent3d { width: OCC_N as u32, height: OCC_N as u32, depth_or_array_layers: 1 }, TextureDimension::D2, &[0u8; 16], TextureFormat::Rgba32Float, RenderAssetUsages::default());
    img.sampler = bevy::image::ImageSampler::nearest();
    img
}

/// The highest surface (shot collision: what is drawn - roofs, bridges, awnings) of each 1 m
/// column around the camera, by a ray down from 100 m above the camera. A drop below it is not
/// drawn (precipitation shader). Cells are computed nearest first, OCC_RAYS_PER_FRAME per frame,
/// in a grid that scrolls with the camera.
fn update_precip_occlusion(
    w: Res<Weather>,
    prim: Res<Primary>,
    col: Res<crate::player::CollisionWorld>,
    cams: Query<&GlobalTransform, (With<Camera3d>, With<crate::hfpost::HfPost>, Without<crate::minimap::MinimapCam>)>,
    mut pa: ResMut<PrecipAssets>,
    mut images: ResMut<Assets<Image>>,
    mut buf: Local<Vec<u32>>,
) {
    let on = w.enabled && prim.valid && (w.cur.rain > 0.01 || w.cur.snow > 0.01);
    if !on || pa.occ.is_none() {
        return;
    }
    let Some(cam) = cams.iter().next().map(|t| t.translation()) else { return };
    let n = OCC_N as usize;
    if pa.occ_cells.len() != n * n || pa.occ_tris != col.tris.len() {
        pa.occ_cells = vec![None; n * n];
        pa.occ_tris = col.tris.len();
    }
    let cx = (cam.x / OCC_CELL).floor() as i32;
    let cz = (cam.z / OCC_CELL).floor() as i32;
    let half = OCC_N / 2;
    // cells of the window that do not hold their column yet, nearest first
    let mut todo: Vec<(i32, i32, i32)> = Vec::new();
    for dz in -half..half {
        for dx in -half..half {
            let (x, z) = (cx + dx, cz + dz);
            let i = (z.rem_euclid(OCC_N) * OCC_N + x.rem_euclid(OCC_N)) as usize;
            if pa.occ_cells[i].is_none_or(|c| c.0 != x || c.1 != z) {
                todo.push((dx * dx + dz * dz, x, z));
            }
        }
    }
    if todo.is_empty() {
        return;
    }
    todo.sort_unstable_by_key(|t| t.0);
    let top = cam.y + 100.0;
    for &(_, x, z) in todo.iter().take(OCC_RAYS_PER_FRAME) {
        let o = Vec3::new((x as f32 + 0.5) * OCC_CELL, top, (z as f32 + 0.5) * OCC_CELL);
        let h = col.raycast_hit(o, Vec3::NEG_Y, 400.0, &mut buf, crate::player::COL_TRACE).map(|(t, _, _)| top - t).unwrap_or(-1.0e6);
        let i = (z.rem_euclid(OCC_N) * OCC_N + x.rem_euclid(OCC_N)) as usize;
        pa.occ_cells[i] = Some((x, z, h));
    }
    let Some(img) = pa.occ.as_ref().and_then(|h| images.get_mut(h)) else { return };
    let Some(data) = img.data.as_mut() else { return };
    for (i, c) in pa.occ_cells.iter().enumerate() {
        let v: [f32; 4] = match c {
            Some((x, z, h)) => [*h, *x as f32, *z as f32, 1.0],
            None => [0.0, 0.0, 0.0, 0.0],
        };
        for k in 0..4 {
            data[i * 16 + k * 4..i * 16 + k * 4 + 4].copy_from_slice(&v[k].to_le_bytes());
        }
    }
}

/// `n` quads with random seeds in the unit volume (RENDER_WORLD only: no CPU copy kept).
fn precip_mesh(n: u32, seed: u32) -> Mesh {
    let mut s = seed;
    let mut r = || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        (s >> 8) as f32 / 16_777_216.0
    };
    let mut pos = Vec::with_capacity(n as usize * 4);
    let mut uv0 = Vec::with_capacity(n as usize * 4);
    let mut uv1 = Vec::with_capacity(n as usize * 4);
    let mut idx = Vec::with_capacity(n as usize * 6);
    for q in 0..n {
        let p = [r(), r(), r()];
        let rnd = [r(), r()];
        for c in [[-1.0f32, 0.0f32], [1.0, 0.0], [1.0, 1.0], [-1.0, 1.0]] {
            pos.push(p);
            uv0.push(c);
            uv1.push(rnd);
        }
        let b = q * 4;
        idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv0);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
    m.insert_indices(Indices::U32(idx));
    m
}

/// Game textures for the precipitation (loaded once, on first use).
fn load_precip_textures(root: &std::path::Path, images: &mut Assets<Image>) -> (Option<Handle<Image>>, Option<Handle<Image>>) {
    let Ok(lib) = upk::library::Library::scan(root) else { return (None, None) };
    let load = |pkg: &str, path: &str, images: &mut Assets<Image>| -> Option<Handle<Image>> {
        let p = lib.open(pkg)?;
        let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "Texture2D" && p.object_path(i as i32 + 1) == path)?;
        let img = crate::gamemat::load_texture(&p, &lib, i as i32 + 1, [upk::matcompile::Address::Clamp; 2], 512)?;
        Some(images.add(img))
    };
    let rain = load("KFX_Impacts", "Textures.Imp_FallingDrops", images);
    let snow = load("KFX_Atmosphere", "Textures.PCloudDot", images);
    info!("weather: rain texture {}, snow texture {}", rain.is_some(), snow.is_some());
    (rain, snow)
}

#[allow(clippy::too_many_arguments)]
fn apply_precip(
    mut commands: Commands,
    w: Res<Weather>,
    prim: Res<Primary>,
    mut pa: ResMut<PrecipAssets>,
    root: Option<Res<crate::sfx::GameRoot>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<PrecipMat>>,
    mut vis: Query<&mut Visibility>,
) {
    let on = w.enabled && prim.valid;
    let rain_i = if on { w.cur.rain } else { 0.0 };
    let snow_i = if on { w.cur.snow } else { 0.0 };
    if !pa.tried && (rain_i > 0.01 || snow_i > 0.01) {
        let Some(root) = root else { return };
        pa.tried = true;
        let (rt, st) = load_precip_textures(&root.0, &mut images);
        let occ = images.add(occ_image());
        pa.occ = Some(occ.clone());
        let mut mk = |n: u32, seed: u32, tex: Option<Handle<Image>>| -> Option<(Entity, Handle<PrecipMat>)> {
            let tex = tex?;
            let mat = mats.add(PrecipMat { params: PrecipParams::default(), tex: Some(tex), occ: Some(occ.clone()) });
            let e = commands
                .spawn((Mesh3d(meshes.add(precip_mesh(n, seed))), MeshMaterial3d(mat.clone()), Transform::IDENTITY, Visibility::Hidden, NoFrustumCulling, NotShadowCaster, NotShadowReceiver))
                .id();
            Some((e, mat))
        };
        pa.rain = mk(RAIN_SHEETS, 0x1234_5678, rt);
        pa.snow = mk(SNOW_FLAKES, 0x8765_4321, st);
    }
    let l = prim.lit;
    let wind = WIND * w.cur.wind;
    let entries = [
        (pa.rain.clone(), rain_i, false),
        (pa.snow.clone(), snow_i, true),
    ];
    for (entry, amount, snow) in entries {
        let Some((e, h)) = entry else { continue };
        let shown = amount > 0.01;
        if let Ok(mut v) = vis.get_mut(e) {
            let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
        if !shown {
            continue;
        }
        let Some(m) = mats.get_mut(&h) else { continue };
        m.params = if snow {
            PrecipParams {
                box_size: Vec4::new(SNOW_BOX.0, SNOW_BOX.1, SNOW_BOX.2, amount),
                color: (l.upper * SNOW_SKY_K + l.color * SNOW_LIGHT_K).extend(SNOW_ALPHA),
                motion: w.snow_off.extend(0.0),
                shape: Vec4::new(1.0, SNOW_SIZE, SNOW_SIZE, 0.0),
                fall: Vec3::new(wind.x * 0.5, -SNOW_FALL, wind.y * 0.5).normalize().extend(SNOW_SWAY),
                misc: Vec4::new(w.sim_time % 3600.0, 0.0, 0.0, 0.0),
            }
        } else {
            PrecipParams {
                box_size: Vec4::new(RAIN_BOX.0, RAIN_BOX.1, RAIN_BOX.2, amount),
                color: (l.upper * RAIN_SKY_K + l.color * RAIN_LIGHT_K).extend(RAIN_ALPHA),
                motion: w.rain_off.extend(0.0),
                shape: Vec4::new(0.0, RAIN_SIZE.0, RAIN_SIZE.1, 0.0),
                fall: Vec3::new(wind.x, -RAIN_FALL, wind.y).normalize().extend(0.0),
                misc: Vec4::new(w.sim_time % 3600.0, 0.0, 0.0, 0.0),
            }
        };
    }
}

// ---------------------------------------------------------------------------------------------
// Debug menu (debug.rs, tab "Werkzeuge")
// ---------------------------------------------------------------------------------------------

pub fn debug_ui(ui: &mut bevy_egui::egui::Ui, w: &mut Weather) {
    use bevy_egui::egui;
    ui.checkbox(&mut w.enabled, "Tageszeit + Wetter an (nicht im Originalspiel)");
    ui.horizontal_wrapped(|ui| {
        for (k, name) in Kind::ALL {
            if ui.selectable_label(w.kind == k, name).clicked() {
                w.kind = k;
                w.enabled = true;
            }
        }
    });
    let mut h = w.hour.unwrap_or(w.base_hour);
    ui.horizontal(|ui| {
        ui.label("Uhrzeit");
        if ui.add(egui::Slider::new(&mut h, 0.0..=24.0).step_by(0.05).suffix(" h")).changed() {
            w.hour = Some(h);
            w.enabled = true;
        }
        if ui.button("Level-Zeit").clicked() {
            w.hour = Some(w.base_hour);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Tag dauert");
        ui.add(egui::Slider::new(&mut w.day_minutes, 0.0..=60.0).step_by(0.5).suffix(" min"));
        ui.label(egui::RichText::new("(0 = Zeit steht)").small());
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut w.volumetric, "Lichtstrahlen (Bevy VolumetricFog)");
        if ui.button("Blitz jetzt").clicked() {
            w.strike_request = true;
            w.enabled = true;
        }
    });
}
