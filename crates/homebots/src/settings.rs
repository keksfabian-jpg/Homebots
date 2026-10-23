//! Player settings of the options screens (options.rs), stored by the option widgets'
//! data-store markup ("GraphicsSettings:Gamma", "PlayerDetails:Minimap",
//! "InputActionToKey:Infantry.MouseSensitivityValue", ...) and saved to homebots_settings.ini in
//! the working directory. Key bindings live in input.rs.
//!
//! Defaults (source per entry in DEFAULTS):
//! - GCProfileSettings DefaultSettings (GCGame.u): HUD flags, volumes 1.0, Display Gamma 2.2,
//!   Push To Talk 1, Mute Voice 0, Invert Mouse Y 0
//! - the user's GCEngine.ini [SystemSettings]: MotionBlur, DepthOfField, AmbientOcclusion False,
//!   Bloom, Distortion, UseVsync False, UseFPSSmoothing, Fullscreen 2, AllowD3D10 False,
//!   DetailMode 2, DrawDistance 1, MaxAnisotropy 8, MaxMultisamples 2,
//!   FoliageDrawRadiusMultiplier 1, ResX 1366, ResY 768
//! - the widgets' own SliderValue CurrentValue where nothing else gives one
//! NOT from data: Master volume 80 (user's wish of -20 %), mouse sliders 50 (= the ini's
//! sensitivity factors unchanged, see input.rs), difficulty 2 (HART = bot strength "high" for
//! both teams, user 2026-10-22 "make all bots more skilled"), FieldOfView 0 = the measured in-game view (trace t3).
use std::collections::BTreeMap;
use std::sync::RwLock;

pub const FILE: &str = "homebots_settings.ini";

/// (markup key, default)
const DEFAULTS: &[(&str, f32)] = &[
    // GraphicsSettings (GCEngine.ini [SystemSettings] of the user)
    ("GraphicsSettings:AmbientOcclusion", 0.0),
    ("GraphicsSettings:Distortion", 1.0),
    ("GraphicsSettings:Bloom", 1.0),
    ("GraphicsSettings:DepthOfField", 1.0),
    ("GraphicsSettings:MotionBlur", 1.0),
    ("GraphicsSettings:FoliageDetail", 1.0),
    ("GraphicsSettings:DrawDistance", 1.0),
    ("GraphicsSettings:LockToVSync", 0.0),
    ("GraphicsSettings:FPSSmoothing", 1.0),
    ("GraphicsSettings:Gamma", 2.2),
    ("GraphicsSettings:AllowD3D10", 0.0),
    // combo indices into the loc item lists (gcuidataprovider_*)
    // texture quality: VeryLow..VeryHigh. NOT from data: Medium = the 1024 cap HomeBots used so
    // far (the user's TEXTUREGROUPs have LODBias 0, i.e. full size)
    ("GraphicsSettings:TextureQuality", 2.0),
    // ShadowQuality: MaxShadowResolution 1024 (NOT from data: index 3 = High for 1024)
    ("GraphicsSettings:ShadowQuality", 3.0),
    ("GraphicsSettings:ShaderQuality", 4.0),
    // MaxMultisamples 2 -> "2x"
    ("GraphicsSettings:MultiSampleAntiAliasing", 1.0),
    // DetailMode 2 -> High (VeryLow..VeryHigh, NOT from data: 2 -> index 3)
    ("GraphicsSettings:WorldDetail", 3.0),
    // MaxAnisotropy 8 -> Off, 2x, 4x, 8x, 16x index 3
    ("GraphicsSettings:Anisotropy", 3.0),
    // Windowed, Fullscreen, WindowedFullscreen. NOT from data: windowed as HomeBots ran so far
    // (the user's GCEngine.ini has Fullscreen=2)
    ("GraphicsSettings:FullscreenMode", 0.0),
    // ScreenAspectRatio 1.7786 -> 16:9 (16:9, 16:10, 4:3, 5:4)
    ("GraphicsSettings:ScreenAspectRatios", 0.0),
    // ScreenResolution: the chosen size (0: the window's start size)
    ("GraphicsSettings:ResX", 0.0),
    ("GraphicsSettings:ResY", 0.0),
    // OverallQuality: VeryLow..VeryHigh, Custom
    ("GraphicsSettings:OverallQuality", 5.0),
    // AudioSettings (profile: 1.0 = 100 %)
    ("AudioSettings:MasterVolumePct", 80.0),
    ("AudioSettings:MusicVolumePct", 100.0),
    ("AudioSettings:SoundVolumePct", 100.0),
    ("AudioSettings:VoiceVolumePct", 100.0),
    ("AudioSettings:VOIPVolumePct", 100.0),
    ("AudioSettings:MuteVoice", 0.0),
    // PlayerDetails (GCProfileSettings DefaultSettings)
    ("PlayerDetails:pushToTalk", 1.0),
    ("PlayerDetails:FieldOfView", 0.0),
    ("PlayerDetails:Minimap", 1.0),
    ("PlayerDetails:FriendlyIcons", 1.0),
    ("PlayerDetails:FriendlyNames", 1.0),
    ("PlayerDetails:WeaponReticules", 1.0),
    ("PlayerDetails:ObjectiveIndicators", 1.0),
    ("PlayerDetails:EnemyNames", 1.0),
    ("PlayerDetails:ChatText", 1.0),
    ("PlayerDetails:AmmoMeter", 1.0),
    ("PlayerDetails:GameScore", 1.0),
    ("PlayerDetails:SpecialWeapons", 1.0),
    ("PlayerDetails:BattlePoints", 1.0),
    ("PlayerDetails:GameHints", 1.0),
    ("PlayerDetails:FrameRateToggle", 0.0),
    // Subtitles: not a profile setting; GCEngine.ini [Engine.Engine] bSubtitlesEnabled True
    ("PlayerDetails:Subtitles", 1.0),
    ("PlayerDetails:LogitechLCD", 0.0),
    ("PlayerDetails:Difficulty", 2.0),
    // HomeBots option (NOT from data, user 2026-10-22): random weather / time of day per round
    // (weather.rs RANDOM_KEY; shown in the Logitech LCD row of U_HUDPrefs, options.rs)
    ("HomeBots:RandomWeather", 1.0),
    // InputActionToKey (sliders 0..100)
    ("InputActionToKey:Infantry.MouseSensitivityValue", 50.0),
    ("InputActionToKey:Infantry.MouseYawFactor", 50.0),
    ("InputActionToKey:Infantry.MousePitchFactor", 50.0),
    ("InputActionToKey:Infantry.ZoomSensitivityValue", 50.0),
    ("InputActionToKey:Infantry.InvertMouse", 0.0),
    // GCInput.ini bEnableMouseSmoothing=true
    ("InputActionToKey:Infantry.ToggleMouseSmoothing", 1.0),
    ("InputActionToKey:Infantry.ToggleMouseAcceleration", 0.0),
    ("InputActionToKey:Infantry.JoySensitivityValue", 50.0),
    ("InputActionToKey:Infantry.JoyYawFactor", 50.0),
    ("InputActionToKey:Infantry.JoyPitchFactor", 50.0),
    ("InputActionToKey:Infantry.InvertJoy", 0.0),
    ("InputActionToKey:Global.FlipGamepadSticks", 0.0),
    ("InputActionToKey:Global.DefaultJoystickToGamepad", 1.0),
    ("InputActionToKey:Global.Vibration", 1.0),
    // GCInput.ini [GCGame.GCPlayerInput] ToggleZoom=true
    ("InputActionToKey:ToggleADS", 0.0),
    ("InputActionToKey:Global.SaveKeyMapToSteamCloud", 0.0),
];

struct Store {
    vals: BTreeMap<String, f32>,
    version: u64,
}

static STORE: RwLock<Option<Store>> = RwLock::new(None);

pub fn default_of(key: &str) -> Option<f32> {
    DEFAULTS.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|x| x.1)
}

fn with<R>(f: impl FnOnce(&Store) -> R) -> R {
    {
        let g = STORE.read().unwrap();
        if let Some(s) = g.as_ref() {
            return f(s);
        }
    }
    load();
    f(STORE.read().unwrap().as_ref().unwrap())
}

/// Current value (the stored one, else the default, else 0).
pub fn get(key: &str) -> f32 {
    with(|s| s.vals.get(key).copied()).or_else(|| default_of(key)).unwrap_or_else(|| {
        // the vehicle / air variants of the mouse and joystick sliders (Land., Heli., Jet.):
        // 50 = the ini's factors unchanged, like the infantry ones
        if key.starts_with("InputActionToKey:") && (key.ends_with("Value") || key.ends_with("Factor")) {
            50.0
        } else {
            0.0
        }
    })
}

pub fn flag(key: &str) -> bool {
    get(key) > 0.5
}

pub fn set(key: &str, v: f32) {
    let _ = with(|_| ());
    let mut g = STORE.write().unwrap();
    let s = g.as_mut().unwrap();
    if s.vals.get(key) != Some(&v) {
        s.vals.insert(key.to_string(), v);
        s.version += 1;
    }
}

/// Marks a change made elsewhere (key bindings) so it is saved.
pub fn touch() {
    let _ = with(|_| ());
    if let Some(s) = STORE.write().unwrap().as_mut() {
        s.version += 1;
    }
}

/// Bumped on every change: systems that apply a setting compare it with what they applied.
pub fn version() -> u64 {
    with(|s| s.version)
}

/// Reads homebots_settings.ini ("key=value" lines; [Bindings] lines go to input.rs).
pub fn load() {
    let mut vals = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(FILE) {
        let mut section = String::new();
        for l in text.lines() {
            let l = l.trim();
            if l.starts_with('[') {
                section = l.trim_matches(['[', ']']).to_string();
                continue;
            }
            if section != "Settings" {
                continue;
            }
            if let Some((k, v)) = l.split_once('=') {
                if let Ok(v) = v.trim().parse::<f32>() {
                    vals.insert(k.trim().to_string(), v);
                }
            }
        }
    }
    *STORE.write().unwrap() = Some(Store { vals, version: 1 });
}

/// Writes the changed values and the key bindings.
pub fn save() {
    let mut out = String::from("; HomeBots settings (options menu). Delete this file to reset.\n[Settings]\n");
    with(|s| {
        for (k, v) in &s.vals {
            out.push_str(&format!("{k}={v}\n"));
        }
    });
    out.push_str("[Bindings]\n");
    out.push_str(&crate::input::save_lines());
    if let Err(e) = std::fs::write(FILE, out) {
        bevy::log::warn!("settings: {FILE} not written: {e}");
    }
}

/// Back to the defaults (ResetToDefaults) for these keys.
pub fn reset_keys(keys: &[String]) {
    let _ = with(|_| ());
    let mut g = STORE.write().unwrap();
    let s = g.as_mut().unwrap();
    s.vals.retain(|k, _| !keys.contains(k));
    s.version += 1;
}

static FPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Smoothed frames per second (kept by options.rs) for the FrameRateToggle display.
pub fn set_fps(f: f32) {
    FPS.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub fn fps() -> f32 {
    f32::from_bits(FPS.load(std::sync::atomic::Ordering::Relaxed))
}

static ASPECT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3FE3_8E39);

/// Window aspect (width / height), kept by player.rs.
pub fn set_aspect(a: f32) {
    if a.is_finite() && a > 0.1 {
        ASPECT.store(a.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// The player's unzoomed vertical field of view (degrees) at the current window aspect.
pub fn vfov() -> f32 {
    base_vfov(f32::from_bits(ASPECT.load(std::sync::atomic::Ordering::Relaxed)))
}

/// Mouse look factors (x, y) on top of DefaultMouseSensitivity 45:
/// - MouseSensitivityValue 0..100 over GCPlayerInput MouseSensitivityScaleLow 0.25 /
///   ScaleMed 1 / ScaleHigh 4 (GCInput.ini); NOT from data: 0 / 50 / 100 on those points,
///   linear between
/// - MouseYawFactor / MousePitchFactor: 50 = DefaultMouseYawFactor / DeafultMousePitchFactor
///   1.0; NOT from data: linear 0..2 over the slider
/// - zoomed (ADS): m_bUseZoomLookScale true, m_fZoomLookScale 0.40 (GCInput.ini) times
///   ZoomSensitivityValue (50 = 1, linear 0..2, NOT from data)
/// - InvertMouse flips y; ToggleMouseAcceleration NOT from data: up to x2 at fast motion
pub fn mouse_scale(delta: bevy::math::Vec2, zoomed: bool) -> (f32, f32) {
    mouse_scale_for("Infantry", delta, zoomed)
}

/// `mouse_scale` with the sliders of a PC_U_KeyBindings tab: Infantry, Land (vehicles, the
/// ground drone), Heli (air; the Hellfire UAV is INCFG_Heli). Smoothing / acceleration are the
/// common tab's.
pub fn mouse_scale_for(set: &str, delta: bevy::math::Vec2, zoomed: bool) -> (f32, f32) {
    let k = |n: &str| format!("InputActionToKey:{set}.{n}");
    let v = get(&k("MouseSensitivityValue")).clamp(0.0, 100.0);
    let sens = if v <= 50.0 { 0.25 + (1.0 - 0.25) * v / 50.0 } else { 1.0 + (4.0 - 1.0) * (v - 50.0) / 50.0 };
    let yaw = get(&k("MouseYawFactor")).clamp(0.0, 100.0) / 50.0;
    let pitch = get(&k("MousePitchFactor")).clamp(0.0, 100.0) / 50.0;
    let zoom = if zoomed { 0.40 * get("InputActionToKey:Infantry.ZoomSensitivityValue").clamp(0.0, 100.0) / 50.0 } else { 1.0 };
    let accel = if flag("InputActionToKey:Infantry.ToggleMouseAcceleration") { 1.0 + (delta.length() / 40.0).min(1.0) } else { 1.0 };
    let inv = if flag(&k("InvertMouse")) { -1.0 } else { 1.0 };
    (sens * yaw * zoom * accel, sens * pitch * zoom * accel * inv)
}

/// Vertical field of view in degrees for the player camera: FieldOfView 0 = the measured view
/// (trace t3: vertical 52.5 deg); else the slider as UE3's horizontal FOV at the window aspect.
pub fn base_vfov(aspect: f32) -> f32 {
    let h = get("PlayerDetails:FieldOfView");
    if h < 1.0 {
        return crate::player::VFOV_DEG;
    }
    2.0 * ((h.to_radians() * 0.5).tan() / aspect.max(0.1)).atan().to_degrees()
}

/// Volume factor of a sound class: Master x the class slider (percent).
pub fn volume(class: &str) -> f32 {
    let master = get("AudioSettings:MasterVolumePct") / 100.0;
    let c = match class {
        "music" => get("AudioSettings:MusicVolumePct"),
        "voice" => get("AudioSettings:VoiceVolumePct"),
        "voip" => {
            if flag("AudioSettings:MuteVoice") {
                0.0
            } else {
                get("AudioSettings:VOIPVolumePct")
            }
        }
        "master" => 100.0,
        _ => get("AudioSettings:SoundVolumePct"),
    } / 100.0;
    (master * c).max(0.0)
}

/// KVA bot skill by difficulty (EINFACH / NORMAL / HART / GUERILLA). NOT from data: Normal =
/// 1.25 (the user's earlier wish), the steps around it 0.25.
pub fn kva_skill() -> f32 {
    [1.0, 1.25, 1.5, 1.75][(get("PlayerDetails:Difficulty").round() as usize).min(3)]
}

/// Bot strength 0..1 for both teams (bots/brain.rs level) by difficulty EINFACH / NORMAL / HART /
/// GUERILLA: 0.35 / 0.6 / 0.8 / 1.0. NOT from data (the SP difficulty objects FL2SPDifficulty*
/// are not in the MP packages).
pub fn bot_level() -> f32 {
    [0.35, 0.6, 0.8, 1.0][(get("PlayerDetails:Difficulty").round().max(0.0) as usize).min(3)]
}

/// Largest texture side loaded (TextureQuality VeryLow..VeryHigh). NOT from data: UE3 drops mips
/// by the texture groups' LODBias; here a cap of 256 / 512 / 1024 / 2048 / 4096.
pub fn max_tex() -> u32 {
    [256, 512, 1024, 2048, 4096][(get("GraphicsSettings:TextureQuality").round().max(0.0) as usize).min(4)]
}

/// Sampler anisotropy (Anisotropy Off, 2x, 4x, 8x, 16x; the user's MaxAnisotropy 8).
pub fn anisotropy() -> u16 {
    [1, 2, 4, 8, 16][(get("GraphicsSettings:Anisotropy").round().max(0.0) as usize).min(4)]
}

static WINDOW_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Window mode / resolution are applied with "Übernehmen" (ApplyChangesButton) and at start.
pub fn apply_window() {
    WINDOW_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn window_epoch() -> u64 {
    WINDOW_EPOCH.load(std::sync::atomic::Ordering::Relaxed)
}

/// Video modes of the window's monitor (physical sizes, refresh rates), kept by options.rs.
pub static MODES: RwLock<Vec<bevy::window::VideoMode>> = RwLock::new(Vec::new());

/// Aspect of the ScreenAspectRatios entry (16:9, 16:10, 4:3, 5:4).
pub fn aspect_choice() -> f32 {
    [16.0 / 9.0, 16.0 / 10.0, 4.0 / 3.0, 5.0 / 4.0][(get("GraphicsSettings:ScreenAspectRatios").round().max(0.0) as usize).min(3)]
}

/// Distinct monitor sizes of the chosen aspect (largest last); fallback list NOT from data.
pub fn resolutions() -> Vec<(u32, u32)> {
    let a = aspect_choice();
    let mut v: Vec<(u32, u32)> = MODES.read().unwrap().iter().map(|m| (m.physical_size.x, m.physical_size.y)).filter(|(w, h)| *h > 0 && ((*w as f32 / *h as f32) - a).abs() < 0.03 && *w >= 640).collect();
    if v.is_empty() {
        let all = [(1280, 720), (1366, 768), (1600, 900), (1920, 1080), (2560, 1440), (1280, 800), (1440, 900), (1680, 1050), (1920, 1200), (1024, 768), (1280, 960), (1600, 1200), (1280, 1024)];
        v = all.into_iter().filter(|(w, h)| ((*w as f32 / *h as f32) - a).abs() < 0.03).collect();
    }
    v.sort();
    v.dedup();
    v
}
