//! NUKE killstreak (user request; the game has no such killstreak, so everything here is NOT from
//! data unless a line says otherwise). Modelled on the user's reference video (Modern Warfare
//! tactical nuke, 2026-10-19): call on the phone / detonator, "inbound" countdown, a Fulton
//! balloon goes up, a transport plane comes in (the view is held on it) and snags the line, the
//! player is yanked off the ground and towed away, the warhead comes down, flash, fireball,
//! white-out, then the mushroom cloud seen from far away, match won.
//!
//! Sequence (seconds from the slot key):
//! - 0: the C4 detonator (CFG_EXP_C4Detonator, loadout ATOMSCHLAG carries C4) is drawn and
//!   pressed; countdown to IMPACT in the HUD banner (GCUIBannerMessageManager) with the game's
//!   A2_UI_S CountdownBeep
//! - DEPLOY: the Fulton balloon rises on its line (BALLOON_H above the player); the view is held
//!   on the balloon, then on the incoming plane (procedural C-130 shape, the game has no such
//!   aircraft) until it snags the line at CATCH
//! - CATCH: the yank; the player trails below and behind the climbing plane (line angle swings
//!   back), wind and engines; the view turns back towards ground zero
//! - IMPACT - WARHEAD: the warhead comes down with the ST_Hellfire trail (data template) and the
//!   game's RocketFlyby
//! - IMPACT: flash, GCExplosionLight_Nuke (data: GCGame class), the volumetric cloud
//!   (nukecloud.rs: fireball growing into the rolling cap, stem, base surge, condensation shell),
//!   game explosions at the base and dust bursts along the shock front (data templates), every
//!   bot inside the shock radius dies, the boom with the speed of sound (game explosion waves,
//!   pitched down), white-out, then the cloud from far away, the match is won.
//!
//! End-of-match killstreak (user 2026-10-22 16:22, objectives.rs Endgame): no loadout carries it
//! any more. The team that decides the match gets it: the player calls it with the first special
//! slot key (HUD prompt), or a bot of the winning team calls it ("bot mode": the countdown, the
//! siren, the warhead, the detonation, the cloud and the blast as above, seen from the player's
//! own view; the blast kills the player too; the far view after the white-out).
//!
//! Bot mode pickup (user 2026-10-22 22:08): the calling bot is picked up like the player (same
//! timings, balloon, plane, line, catch): it stops (no AI, no target, takes no damage: bots.rs /
//! combat.rs ask `held`), the balloon rises over it, the plane snags the line and the bot's own
//! body hangs on it below and behind the plane (bots.rs render_bots draws it with `towed_pose`:
//! the game's jump_up / jump_dn sequences, the body along the line; NOT from data). The player's
//! view is turned to the balloon, the plane and the snag from DEPLOY until CATCH + BOT_VIEW_AFTER
//! (pawn yaw / pitch, his position stays), then it is his again. The pickup sounds are heard from
//! the distance (plain waves, a distance gain; NOT from data).
//! The team scores stay frozen, the match ends with the caller's team when the sequence is over.
//!
//! Tests: HB_NUKETEST=<s>[,<t0>] calls the nuke s seconds after the spawn (starting at t0);
//! HB_NUKE_AT=x,y,z ground zero (UE); HB_ENDNUKE=<s>[,<team>] awards it after s s of play
//! (objectives.rs).

use bevy::{
    core_pipeline::{core_3d::graph::{Core3d, Node3d}, fullscreen_vertex_shader::fullscreen_shader_vertex_state},
    ecs::query::QueryItem,
    pbr::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner},
        render_resource::{binding_types::{sampler, texture_2d, uniform_buffer_sized}, *},
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
        RenderApp,
    },
};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Mutex;

pub const SHADER: &str = "embedded://homebots/nukefx.wgsl";

/// Item class of the killstreak (debug menu grant) and its name (NOT from data).
pub const ITEM: &str = "CFG_Nuke";
pub const NAME: &str = "ATOMSCHLAG";

// ---- timeline (seconds from the call; NOT from data, after the reference video)
/// own first-person animations (vmanim.rs, made in Blender on the game's 1P arms rig, NOT from
/// data): hooking the Fulton line's carabiner into the harness ring, the middle finger while towed
const HOOK_AT: f32 = 1.2;
/// the balloon goes up when the line pulls away at the end of the hook-up
const DEPLOY: f32 = HOOK_AT + 1.45;
const BALLOON_RISE: f32 = 2.4;
const PLANE_SEEN: f32 = 4.2;
const CATCH: f32 = 7.2;
const IMPACT: f32 = 14.0;
/// middle finger up 2 s before the detonation (user 2026-10-20 03:12), until the white-out is full
const FINGER_AT: f32 = IMPACT - 2.0;
/// hold part of finger.hba (frames 20..74 at 30 fps, after the jab): the hand is up, small shakes
const FINGER_HOLD: (f32, f32) = (20.0 / 30.0, 74.0 / 30.0);
const WARHEAD: f32 = 1.4;
/// white-out after the detonation: in, hold, out (the view comes back far away)
const WHITE_START: f32 = 2.0;
const WHITE_IN: f32 = 1.5;
const WHITE_HOLD: f32 = 1.8;
const WHITE_OUT: f32 = 2.0;
/// the match is over (objectives.rs Endgame Done) this long after the detonation
const WIN_AT: f32 = 7.0;
// ---- pickup geometry (metres; NOT from data: Fulton STARS line ~150 m, pickup ~65 m/s)
const BALLOON_H: f32 = 150.0;
const PLANE_SPEED: f32 = 65.0;
const PLANE_CLIMB: f32 = 7.0;
const LINE_SWING: f32 = 1.1;
/// bot mode: harness (line end) above the bot's feet and ahead of its chest (m), the player's view
/// stays on the pickup this long after the catch (s) (NOT from data)
const HARNESS_H: f32 = 1.35;
const HARNESS_FWD: f32 = 0.18;
const BOT_VIEW_AFTER: f32 = 2.5;
/// frames the drawn pose of the towed bot runs ahead (HB_TOWLAG; see update; NOT from data)
static TOW_LAG_ENV: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
/// far view after the white-out: distance from ground zero and height (m)
const FAR_DIST: f32 = 950.0;
const FAR_HEIGHT: f32 = 160.0;
/// the shock front (m): S = 60 + 330 * tb^0.8; speed of sound 343 m/s
const SOUND_SPEED: f32 = 343.0;

fn shock_radius(tb: f32) -> f32 {
    if tb <= 0.0 { 0.0 } else { 60.0 + 330.0 * tb.powf(0.8) }
}

static REQ: AtomicBool = AtomicBool::new(false);
static DRAW: AtomicBool = AtomicBool::new(false);
static PRESS: AtomicBool = AtomicBool::new(false);
/// the end-of-match nuke waits for the player's call
static OFFERED: AtomicBool = AtomicBool::new(false);

/// Pose of the bot on the Fulton line (bot mode, bots.rs render_bots): feet (UE units), rotation
/// of the body (UE axes; X forward, Z up), sequence and whether it loops (NOT from data).
#[derive(Clone, Copy, Debug)]
pub struct TowPose {
    pub feet_ue: Vec3,
    pub rot: Quat,
    pub anim: &'static str,
    pub looped: bool,
}

/// The bot being picked up (bot mode): entity, bot slot, its pose once it is on the line.
struct Tow {
    e: Entity,
    slot: usize,
    pose: Option<TowPose>,
}
static TOW: Mutex<Option<Tow>> = Mutex::new(None);

fn set_tow(t: Option<Tow>) {
    if let Ok(mut g) = TOW.lock() {
        *g = t;
    }
}

/// bots.rs / combat.rs / botextras.rs / botvehicles.rs: this bot is the one the Fulton pickup
/// takes (bot mode, from the call on): no AI, not a target, no damage.
/// The nuke's camera (white-out / far view) owns the view this frame: the kill cam
/// (bots.rs render_bots) leaves it alone.
static CAM_OWNED: AtomicBool = AtomicBool::new(false);
/// The player died in the blast of a bot's nuke: no HUD (hud.rs), the kill cam shows his
/// ragdoll (user 2026-10-23 00:14 "crosshair still seen ... just ragdoll to the ground").
static DEAD_IN_BLAST: AtomicBool = AtomicBool::new(false);

pub fn camera_owned() -> bool {
    CAM_OWNED.load(Relaxed)
}

pub fn hide_hud() -> bool {
    DEAD_IN_BLAST.load(Relaxed)
}

pub fn held(e: Entity) -> bool {
    TOW.lock().map(|t| t.as_ref().is_some_and(|t| t.e == e)).unwrap_or(false)
}

/// bots.rs render_bots: the body of this bot slot hangs on the line (pose from update).
pub fn towed_pose(slot: usize) -> Option<TowPose> {
    TOW.lock().ok().and_then(|t| t.as_ref().filter(|t| t.slot == slot).and_then(|t| t.pose))
}

/// Gain of a pickup sound heard from the distance in bot mode (the waves are not positional;
/// half volume at `r` metres; NOT from data).
fn dist_gain(d: f32, r: f32) -> f32 {
    r / (r + d.max(0.0))
}

/// Systems that have to run after the nuke's update (bots.rs render_bots: the towed body).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NukeSet;

/// combat.rs: the first special slot key while the nuke is offered (and the debug menu).
pub fn request() {
    REQ.store(true, Relaxed);
}

/// combat.rs: the end-of-match nuke is the player's to call (the slot key calls it instead of
/// the loadout's item).
pub fn offered_to_player() -> bool {
    OFFERED.load(Relaxed)
}

/// The local player's team (combat.rs player_team; the spawn selection can switch it).
pub fn player_team() -> u8 {
    crate::combat::player_team()
}

/// Name of the key of the first special slot (EKBC_SpecialWeapon1) for the HUD prompt.
fn slot_key_name() -> String {
    let k = crate::input::keys_of(crate::input::Set::Foot, "EKBC_SpecialWeapon1").into_iter().next().unwrap_or_else(|| "Four".into());
    const DIGITS: [&str; 10] = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine"];
    match DIGITS.iter().position(|d| d.eq_ignore_ascii_case(&k)) {
        Some(i) => i.to_string(),
        None => k.to_ascii_uppercase(),
    }
}

/// combat.rs: draw the detonator now (once).
pub fn take_draw() -> bool {
    DRAW.swap(false, Relaxed)
}

/// combat.rs: press the detonator (held until the press is seen).
pub fn press_det() -> bool {
    PRESS.load(Relaxed)
}

// ---------------------------------------------------------------- pools

/// Extra pooled instances of the game effects the blast uses (main.rs fx pools).
pub fn add_pools(pools: &mut Vec<(String, usize)>) {
    for (t, n) in RING_FX.iter().chain(SMOKE_FX.iter()).map(|(t, n, _)| (*t, *n)).chain([(TRAIL_FX, 2usize)]) {
        match pools.iter_mut().find(|p| p.0 == t) {
            Some(p) => p.1 = p.1.max(n),
            None => pools.push((t.to_string(), n)),
        }
    }
}

/// dust bursts along the shock front (off: the volumetric base surge carries the dust; the game
/// impacts looked small, user 2026-10-19 22:54)
const RING_DUST: bool = false;
const RING_FX: [(&str, usize, f32); 2] = [("KFX_Impacts.FX_Surfaces.Imp_Dirt_TankShell", 16, 5.0), ("KFX_Pyrotechnics.FX_Explosions.FX_ClusterBomb_Gen", 10, 4.0)];
/// lingering smoke and fires afterwards
const SMOKE_FX: [(&str, usize, f32); 2] = [("KFX_Atmosphere.Smoke.FX_Large_Building_smoke", 4, 8.0), ("KFX_Pyrotechnics.FX_Fire.FX_FireLarge", 6, 4.0)];
const TRAIL_FX: &str = "KFX_Weapons.Smoketrails.ST_Hellfire";
/// the downloaded transport plane (working directory = project)
const PLANE_MODEL: &str = "assets/nuke/c130.hbm";
/// the Fulton balloon (made in Blender, origin at the bridle ring) and the hook-up props, the arm
/// animations (vmanim.rs)
const BALLOON_MODEL: &str = "assets/nuke/balloon.hbm";
const HOOK_PROPS: &str = "assets/nuke/hook_props.hbm";
const HOOK_CLIP: &str = "assets/nuke/hook.hba";
const FINGER_CLIP: &str = "assets/nuke/finger.hba";
/// carabiner: the gate opens on the ring, snaps shut (game waves, data)
const W_GATE: &str = "A2_UI_S.Click";
const W_CLIP: &str = "A2_Weapons_S.M4.M4WClip3_mono";
/// visible propeller turn of the downloaded model (rad/s, NOT from data; a camera sees ~1000 rpm
/// blades strobe, so a slow turn plus the blur disc)
const PROP_SPIN: f32 = 11.0;

// ---- sounds: the game's own waves (data), played by sfx.rs (sound slider)
const W_BEEP: &str = "A2_UI_S.CountdownBeep";
const W_CALL: &str = "A2_UI_S.beeps";
const W_DEPLOY: &str = "A2_UI_S.Resupply";
const W_SWISH: &str = "A2_UI_S.Swish";
const W_YANK: &str = "a_vehicle_globaluse.Sounds.Collision.S_VehicleFX_Large_Impact1_LEFT";
/// air rush while hanging: the distant jet roar, slowed down (the levels' wind waves have no
/// audio data in the install: A2_AmbientAudio_S_2_oldFL1 Fencewind* are 261-byte stubs)
const W_WIND: &str = "A2_PreditorDrone_S.S_VHR_JET_fighter_LOD";
const W_WARHEAD: &str = "A2_ProjctileFlight_S.RocketFlyby01_mono";
// ---- downloaded sounds (assets/nuke, Pixabay content licence, credits in assets/nuke/CREDITS.txt;
// NOT game data): converted to Ogg, trimmed and levelled by the converter (MODLOG 2026-10-19)
/// air-raid siren while the strike is inbound
const S_SIREN: &str = "ext:assets/nuke/nuke_siren.ogg";
/// propeller transport pass, its loudest point 4 s in (= the catch)
const S_PLANE: &str = "ext:assets/nuke/nuke_plane.ogg";
/// engine drone while towed (seamless loop cut from the same recording)
const S_ENGINE: &str = "ext:assets/nuke/nuke_engine.ogg";
/// the detonation: main boom with the long roll, the blast layer, the far rumble
const S_BOOM: &str = "ext:assets/nuke/nuke_boom.ogg";
const S_BLAST: &str = "ext:assets/nuke/nuke_blast.ogg";
const S_RUMBLE: &str = "ext:assets/nuke/nuke_rumble.ogg";
const S_ALL: [&str; 6] = [S_SIREN, S_PLANE, S_ENGINE, S_BOOM, S_BLAST, S_RUMBLE];
/// the plane pass reaches its loudest point this long after it starts
const S_PLANE_PEAK: f32 = 4.0;
/// decoded once per match (the long waves would stall the frame they are first played in)
static PRELOADED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------- state

#[derive(Resource, Default)]
pub struct Nuke {
    running: bool,
    t: f32,
    /// ground zero (UE)
    gz: Vec3,
    /// pickup point (Bevy m, the player's eye at the deploy), plane heading (unit, horizontal,
    /// away from ground zero), view direction at the deploy (Bevy)
    p0: Vec3,
    head: Vec3,
    dir0: Vec3,
    /// camera for this frame (Bevy position, look direction, roll), None = player camera
    cam: Option<(Vec3, Vec3, f32)>,
    fov: f32,
    shake: f32,
    done: u64,
    won: bool,
    warhead_trail: Option<(usize, f64)>,
    ring_acc: f32,
    seed: u32,
    last_count: i32,
    god_before: Option<bool>,
    /// the view's last direction (smoothed look)
    look: Vec3,
    hold_start: Option<std::time::Instant>,
    /// horizontal offset of the balloon to open sky (Bevy m)
    sky_off: Vec3,
    pub active: bool,
    /// bot mode (end-of-match nuke called by a bot): the caller (None: nobody of the team is
    /// left, the blast is the world's), its team, its name
    bot: Option<(Option<Entity>, u8, String)>,
    /// bot mode: the caller is picked up (entity, bot slot, its UE yaw at the call)
    tow: Option<(Entity, usize, f32)>,
    /// far view direction from ground zero (player mode: the plane's heading; bot mode: the
    /// player's side)
    view_head: Vec3,
    /// the offer prompt was announced
    offer_shown: bool,
}

impl Nuke {
    /// Seconds since the call while the sequence runs (record.rs).
    pub fn time(&self) -> Option<f32> {
        self.running.then_some(self.t)
    }
    fn once(&mut self, bit: u32) -> bool {
        let m = 1u64 << bit;
        let first = self.done & m == 0;
        self.done |= m;
        first
    }
    fn rnd(&mut self) -> f32 {
        let mut x = if self.seed == 0 { 0x1234_5679 } else { self.seed };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / 16_777_216.0
    }
    /// plane position (Bevy m) at time t (before CATCH: coming in along the heading)
    fn plane_at(&self, t: f32) -> Vec3 {
        let s = t - CATCH;
        let climb = if s > 0.0 { PLANE_CLIMB * s + 0.15 * s * s } else { 0.0 };
        self.p0 + self.sky_off + Vec3::Y * (BALLOON_H + 4.0 + climb) + self.head * PLANE_SPEED * s
    }
    /// line angle from the vertical (back from the plane) after the catch
    fn swing(&self, t: f32) -> f32 {
        let s = (t - CATCH).max(0.0);
        // theta'(0) = 0.6 v / L: the yank throws the player forward at once, then the line
        // streams back; a slow pendulum on top
        let tau = LINE_SWING / (0.6 * PLANE_SPEED / BALLOON_H);
        LINE_SWING * (1.0 - (-s / tau).exp()) + 0.06 * (s * 1.3).sin() * (1.0 - (-s / 2.0).exp())
    }
    /// the player (bot mode: the bot's harness; Bevy m) on the line after the catch
    fn towed_at(&self, t: f32) -> Vec3 {
        let th = self.swing(t);
        let len = BALLOON_H - (t - CATCH).max(0.0) * 2.5;
        self.plane_at(t) - (Vec3::Y * th.cos() + self.head * th.sin()) * len
    }
}

/// Post pass parameters (extracted to the render world).
#[derive(Resource, Default, Clone, ExtractResource)]
pub struct NukeFx {
    pub on: bool,
    /// flash, -, white-out, -
    pub a: [f32; 4],
    /// time, radial blur, centre uv
    pub b: [f32; 4],
    /// chromatic aberration, -, grain, desaturation
    pub c: [f32; 4],
}

#[derive(Component)]
struct Warhead;
#[derive(Component)]
struct Line;
#[derive(Component)]
struct Balloon;
/// the downloaded / own balloon model is in use (no sphere scaling)
#[derive(Component)]
struct BalloonModel;
/// hook-up prop (hook_props.hbm part index = object index in hook.hba)
#[derive(Component)]
struct HookProp(usize);
/// parent of the hook-up props: the eye with the view rotation and the viewmodel's lateral scale
/// (right and up times K, as viewmodel.rs draws the arms), so the props sit on the hands exactly
/// as in Blender (HB_VMSpace)
#[derive(Component)]
struct HookSpace;

#[derive(Resource, Default)]
pub struct NukeClips {
    hook: Option<crate::vmanim::Clip>,
    finger: Option<crate::vmanim::Clip>,
}
#[derive(Component)]
struct Plane;
#[derive(Component)]
struct Prop;
/// propeller hub of the downloaded model (blades turn about its local X)
#[derive(Component)]
struct PropHub;

pub struct NukePlugin;

impl Plugin for NukePlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "nukefx.wgsl");
        app.init_resource::<Nuke>()
            .init_resource::<NukeFx>()
            .init_resource::<NukeClips>()
            .add_plugins(ExtractResourcePlugin::<NukeFx>::default())
            .add_systems(Startup, setup)
            .add_systems(Update, update.after(crate::combat::CombatSet).in_set(NukeSet))
            .add_systems(PostUpdate, camera.before(bevy::transform::TransformSystem::TransformPropagate));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_render_graph_node::<ViewNodeRunner<NukeFxNode>>(Core3d, NukeFxLabel)
            .add_render_graph_edges(Core3d, (Node3d::Tonemapping, NukeFxLabel, crate::hfpost::HfPostLabel));
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<NukeFxPipeline>();
    }
}

// ---------------------------------------------------------------- meshes

/// UE -> Bevy metres.
fn b(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y) * 0.01
}

/// Bevy metres -> UE.
fn ue(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y) * 100.0
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let paint = mats.add(StandardMaterial { base_color: Color::linear_rgb(0.085, 0.095, 0.085), perceptual_roughness: 0.78, metallic: 0.15, reflectance: 0.35, ..default() });
    let dark = mats.add(StandardMaterial { base_color: Color::linear_rgb(0.02, 0.02, 0.02), perceptual_roughness: 0.6, ..default() });
    let glass = mats.add(StandardMaterial { base_color: Color::linear_rgb(0.01, 0.012, 0.015), perceptual_roughness: 0.1, reflectance: 0.8, ..default() });
    let prop_m = mats.add(StandardMaterial { base_color: Color::linear_rgba(0.02, 0.02, 0.02, 0.32), alpha_mode: AlphaMode::Blend, unlit: true, double_sided: true, cull_mode: None, ..default() });
    let plane = commands.spawn((Transform::default(), Visibility::Hidden, Plane)).id();
    let rx = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    // ---- transport plane: the downloaded C-130 (assets/nuke/c130.hbm, CC BY 4.0 manilov.ap,
    // repainted near-black by the converter; extmodel.rs), else the procedural shape below
    if let Some(parts) = crate::extmodel::load(PLANE_MODEL, &mut images) {
        info!("nuke: plane model {PLANE_MODEL} ({} parts)", parts.len());
        let mut hubs: Vec<(i32, Entity)> = Vec::new();
        for p in parts {
            let parent = if p.group >= 0 {
                match hubs.iter().find(|h| h.0 == p.group) {
                    Some(h) => h.1,
                    None => {
                        let h = commands.spawn((Transform::from_translation(p.pivot), Visibility::Inherited, PropHub)).id();
                        commands.entity(plane).add_child(h);
                        // the blur disc of the spinning blades
                        let disc = commands.spawn((Mesh3d(meshes.add(Cylinder::new(2.0, 0.04).mesh().resolution(32).build())), MeshMaterial3d(prop_m.clone()), Transform::from_translation(p.pivot + Vec3::X * 0.25).with_rotation(rx), NotShadowReceiver, NotShadowCaster)).id();
                        commands.entity(plane).add_child(disc);
                        hubs.push((p.group, h));
                        h
                    }
                }
            } else {
                plane
            };
            let e = commands.spawn((Mesh3d(meshes.add(p.mesh)), MeshMaterial3d(mats.add(p.material)), Transform::default(), NotShadowReceiver)).id();
            commands.entity(parent).add_child(e);
        }
        // Fulton nose yoke (MC-130E): two arms spread forward from the nose
        for k in [-1.0f32, 1.0] {
            let e = commands.spawn((Mesh3d(meshes.add(Cuboid::new(0.12, 0.12, 8.5).mesh().build())), MeshMaterial3d(dark.clone()), Transform::from_xyz(14.6, 0.3, k * 3.2).with_rotation(Quat::from_rotation_y(k * -0.55)), NotShadowReceiver)).id();
            commands.entity(plane).add_child(e);
        }
    } else {
        warn!("nuke: {PLANE_MODEL} not found, procedural plane");
        procedural_plane(&mut commands, &mut meshes, plane, &paint, &dark, &glass, &prop_m);
    }
    // ---- balloon (helium, grey) and the line
    let bal_m = mats.add(StandardMaterial { base_color: Color::linear_rgb(0.32, 0.33, 0.33), perceptual_roughness: 0.55, ..default() });
    match crate::extmodel::load(BALLOON_MODEL, &mut images) {
        Some(parts) => {
            let bal = commands.spawn((Transform::default(), Visibility::Hidden, Balloon, BalloonModel)).id();
            for p in parts {
                let e = commands.spawn((Mesh3d(meshes.add(p.mesh)), MeshMaterial3d(mats.add(p.material)), Transform::default(), NotShadowReceiver)).id();
                commands.entity(bal).add_child(e);
            }
        }
        None => {
            warn!("nuke: {BALLOON_MODEL} not found, sphere balloon");
            commands.spawn((Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(24, 16))), MeshMaterial3d(bal_m), Transform::default(), Visibility::Hidden, NotShadowReceiver, Balloon));
        }
    }
    if let Some(parts) = crate::extmodel::load(HOOK_PROPS, &mut images) {
        let space = commands.spawn((Transform::default(), Visibility::Visible, HookSpace)).id();
        for (i, p) in parts.into_iter().enumerate() {
            let e = commands.spawn((Mesh3d(meshes.add(p.mesh)), MeshMaterial3d(mats.add(p.material)), Transform::default(), Visibility::Hidden, NotShadowReceiver, NotShadowCaster, bevy::render::view::NoFrustumCulling, HookProp(i))).id();
            commands.entity(space).add_child(e);
        }
    }
    let clips = NukeClips { hook: crate::vmanim::load(HOOK_CLIP), finger: crate::vmanim::load(FINGER_CLIP) };
    info!("nuke: hook-up clip {:?} s, finger clip {:?} s", clips.hook.as_ref().map(|c| c.length()), clips.finger.as_ref().map(|c| c.length()));
    commands.insert_resource(clips);
    // three-strand nylon rope: a procedural strand texture repeated along the line (uv_transform
    // set every frame from the line's length)
    let rope_mat = mats.add(StandardMaterial {
        base_color: Color::linear_rgb(0.30, 0.31, 0.21),
        base_color_texture: Some(images.add(rope_image())),
        perceptual_roughness: 0.92,
        reflectance: 0.25,
        ..default()
    });
    commands.spawn((Mesh3d(meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(14).build())), MeshMaterial3d(rope_mat), Transform::default(), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, bevy::render::view::NoFrustumCulling, Line));
    // ---- warhead: body, nose cone (re-entry vehicle shape)
    let wh_mat = mats.add(StandardMaterial { base_color: Color::linear_rgb(0.05, 0.05, 0.05), perceptual_roughness: 0.5, metallic: 0.6, ..default() });
    let wh = commands.spawn((Transform::default(), Visibility::Hidden, Warhead)).id();
    for (m, t) in [(Cone::new(0.55, 2.4).mesh().build(), Transform::from_xyz(0.0, -1.2, 0.0).with_rotation(Quat::from_rotation_x(std::f32::consts::PI))), (Cylinder::new(0.55, 0.8).mesh().build(), Transform::from_xyz(0.0, 0.4, 0.0))] {
        let c = commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(wh_mat.clone()), t, NotShadowCaster, NotShadowReceiver)).id();
        commands.entity(wh).add_child(c);
    }
}

/// Procedural C-130 shape (fallback without the downloaded model; NOT from data).
fn procedural_plane(commands: &mut Commands, meshes: &mut Assets<Mesh>, plane: Entity, paint: &Handle<StandardMaterial>, dark: &Handle<StandardMaterial>, glass: &Handle<StandardMaterial>, prop_m: &Handle<StandardMaterial>) {
    let rx = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    // ---- transport plane (C-130 proportions, +X forward; NOT from data, no such mesh in the
    // install): fuselage, high wing, four turboprops, T-less tail, Fulton nose yoke
    let mut part = |c: &mut Commands, m: Mesh, mat: &Handle<StandardMaterial>, t: Transform| {
        let e = c.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(mat.clone()), t, NotShadowReceiver)).id();
        c.entity(plane).add_child(e);
        e
    };
    part(commands, Cylinder::new(2.1, 23.0).mesh().resolution(24).build(), paint, Transform::from_rotation(rx));
    part(commands, Sphere::new(1.0).mesh().uv(24, 12), paint, Transform::from_xyz(11.4, -0.15, 0.0).with_scale(Vec3::new(3.4, 2.0, 2.1)));
    part(commands, Sphere::new(1.0).mesh().uv(24, 12), glass, Transform::from_xyz(12.6, 0.9, 0.0).with_scale(Vec3::new(1.6, 0.8, 1.5)));
    part(commands, Sphere::new(1.0).mesh().uv(24, 12), paint, Transform::from_xyz(-13.0, 0.9, 0.0).with_rotation(Quat::from_rotation_z(0.18)).with_scale(Vec3::new(5.5, 1.3, 1.9)));
    // wing, flaps edge, tail planes
    part(commands, Cuboid::new(4.6, 0.45, 40.4).mesh().build(), paint, Transform::from_xyz(1.2, 2.25, 0.0));
    part(commands, Cuboid::new(3.6, 0.3, 15.6).mesh().build(), paint, Transform::from_xyz(-14.6, 2.3, 0.0));
    part(commands, Cuboid::new(5.0, 7.6, 0.45).mesh().build(), paint, Transform::from_xyz(-14.4, 5.6, 0.0).with_rotation(Quat::from_rotation_z(0.22)));
    // engines and propellers
    let mut props = Vec::new();
    for z in [-11.6f32, -5.9, 5.9, 11.6] {
        part(commands, Cylinder::new(0.75, 5.6).mesh().resolution(16).build(), paint, Transform::from_xyz(3.0, 1.75, z).with_rotation(rx));
        part(commands, Cone::new(0.5, 1.1).mesh().build(), dark, Transform::from_xyz(6.3, 1.75, z).with_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2)));
        let pe = part(commands, Cylinder::new(2.05, 0.05).mesh().resolution(32).build(), prop_m, Transform::from_xyz(6.0, 1.75, z).with_rotation(rx));
        props.push(pe);
    }
    for pe in props {
        commands.entity(pe).insert(Prop);
    }
    // Fulton nose yoke: two arms spread forward from the nose
    for s in [-1.0f32, 1.0] {
        part(commands, Cuboid::new(0.12, 0.12, 8.5).mesh().build(), dark, Transform::from_xyz(14.4, 0.2, s * 3.2).with_rotation(Quat::from_rotation_y(s * -0.55)));
    }
}

/// Strand texture of a three-strand twisted rope (NOT from data): u around the rope, v along it,
/// one twist per texture height; each strand shaded round, dark grooves between them.
fn rope_image() -> Image {
    const W: usize = 64;
    const H: usize = 64;
    let mut data = vec![0u8; W * H * 4];
    for y in 0..H {
        for x in 0..W {
            let u = x as f32 / W as f32;
            let v = y as f32 / H as f32;
            // three strands spiralling: phase along the helix
            let ph = (u * 3.0 + v * 3.0).fract();
            let round = (ph * std::f32::consts::PI).sin().powf(0.6);
            // fibres inside a strand
            let fib = 0.92 + 0.08 * ((u * 41.0 + v * 87.0) * std::f32::consts::TAU).sin();
            let c = (0.18 + 0.82 * round) * fib;
            let i = (y * W + x) * 4;
            let g = (c.clamp(0.0, 1.0) * 255.0) as u8;
            data[i..i + 4].copy_from_slice(&[g, g, g, 255]);
        }
    }
    let mut img = Image::new(bevy::render::render_resource::Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 }, bevy::render::render_resource::TextureDimension::D2, data, bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb, bevy::asset::RenderAssetUsages::RENDER_WORLD);
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        mipmap_filter: bevy::image::ImageFilterMode::Linear,
        ..default()
    });
    img
}

// ---------------------------------------------------------------- logic

/// Ground zero: HB_NUKE_AT, else the middle of the active line's objectives (all objectives when
/// none), on the ground.
fn ground_zero(obj: &crate::objectives::Objectives, col: &crate::player::CollisionWorld, fallback: Vec3) -> Vec3 {
    if let Ok(v) = std::env::var("HB_NUKE_AT") {
        let f: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if f.len() >= 3 {
            return Vec3::new(f[0], f[1], f[2]);
        }
    }
    let line: Vec<&crate::objectives::Objective> = obj.list.iter().filter(|o| o.line == obj.active_line).collect();
    let list: Vec<&crate::objectives::Objective> = if line.is_empty() { obj.list.iter().collect() } else { line };
    let c = if list.is_empty() { fallback } else { list.iter().map(|o| Vec3::from(o.location)).sum::<Vec3>() / list.len() as f32 };
    let mut buf = Vec::new();
    let top = b(Vec3::new(c.x, c.y, c.z + 20000.0));
    match col.raycast(top, Vec3::NEG_Y, 400.0, &mut buf) {
        Some((t, _)) => Vec3::new(c.x, c.y, c.z + 20000.0 - t * 100.0),
        None => c,
    }
}

/// Cloud shape at `tb` seconds after the detonation (Bevy m): cap height, ring radius, tube
/// radius, stem radius, stem growth, heat.
fn cloud_shape(tb: f32) -> [f32; 6] {
    // (scaled up after the user's 22:54 feedback "the detonation is still underwhelming")
    let r = 120.0 * (1.0 - (-tb * 3.0).exp()) + 60.0 * (1.0 - (-tb / 6.0).exp());
    let h = r * 0.9 + 600.0 * (1.0 - (-tb / 10.0).exp());
    let ring = 160.0 * (1.0 - (-(tb - 1.0).max(0.0) / 5.0).exp());
    let rs = 26.0 + 34.0 * (1.0 - (-tb / 6.0).exp());
    let grow = smooth((tb - 0.6) / 3.5);
    let heat = (0.85 * (-tb / 3.5).exp() + 0.2 * (-tb / 16.0).exp()).clamp(0.0, 1.0);
    [h, ring, r, rs, grow, heat]
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    time: Res<Time>,
    mut nk: ResMut<Nuke>,
    mut fxp: ResMut<NukeFx>,
    mut cloud: ResMut<crate::nukecloud::NukeCloud>,
    mut crater: ResMut<crate::crater::Crater>,
    mut sfx: ResMut<crate::sfx::SfxQueue>,
    (mut flow, obj, col, mut cheats, mut end): (ResMut<crate::menu::MatchFlow>, Res<crate::objectives::Objectives>, Res<crate::player::CollisionWorld>, ResMut<crate::combat::Cheats>, ResMut<crate::objectives::Endgame>),
    (mut pc, mut queue, mut feed, mut vm): (ResMut<crate::combat::PlayerCombat>, ResMut<crate::combat::DamageQueue>, ResMut<crate::combat::KillFeed>, Option<ResMut<crate::viewmodel::ViewModel>>),
    mut pawns: Query<&mut crate::player::Pawn>,
    mut soldiers: Query<(Entity, &mut crate::combat::Soldier, Option<&crate::bots::Bot>)>,
    (mut fx, mut dl, el): (ResMut<crate::particles::Particles>, ResMut<crate::dynlights::DynLights>, Res<crate::dynlights::ExplosionLights>),
    suns: Query<(&DirectionalLight, &GlobalTransform)>,
    mut vis: ParamSet<(
        Query<(&mut Transform, &mut Visibility), With<Warhead>>,
        Query<(&mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>), With<Line>>,
        Query<(&mut Transform, &mut Visibility, Has<BalloonModel>), With<Balloon>>,
        Query<(&mut Transform, &mut Visibility), With<Plane>>,
        Query<(&mut Transform, Has<PropHub>), Or<(With<Prop>, With<PropHub>)>>,
        Query<(&mut Transform, &mut Visibility, Option<&HookProp>), Or<(With<HookProp>, With<HookSpace>)>>,
    )>,
    clips: Res<NukeClips>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) {
    use crate::menu::Phase;
    let dt = time.delta_secs().min(0.1);
    let in_match = matches!(flow.phase, Phase::Playing | Phase::Spawn);
    // set again below while the player lies dead in a running bot nuke
    DEAD_IN_BLAST.store(false, Relaxed);
    // back in the menu: everything off
    if !in_match {
        REQ.store(false, Relaxed);
        set_tow(None);
        OFFERED.store(false, Relaxed);
        PRELOADED.store(false, Relaxed);
        if nk.offer_shown && !nk.running {
            *nk = Nuke::default();
        }
        // (also when the sequence is over: its post pass, cloud and crater must not carry over
        // into the next match)
        if nk.running || fxp.on || cloud.on {
            if let Some(g) = nk.god_before.take() {
                cheats.god = g;
            }
            *nk = Nuke::default();
            *fxp = NukeFx::default();
            cloud.on = false;
            crater.reset();
            for (_, mut v) in vis.p0().iter_mut() {
                *v = Visibility::Hidden;
            }
            for (_, mut v, _) in vis.p1().iter_mut() {
                *v = Visibility::Hidden;
            }
            for (_, mut v, _) in vis.p2().iter_mut() {
                *v = Visibility::Hidden;
            }
            for (_, mut v, _) in vis.p5().iter_mut() {
                *v = Visibility::Hidden;
            }
            crate::vmanim::set(None);
            for (_, mut v) in vis.p3().iter_mut() {
                *v = Visibility::Hidden;
            }
        }
        return;
    }
    // HB_NUKETEST=<s>: call it s seconds after the spawn
    if let Ok(v) = std::env::var("HB_NUKETEST") {
        let s = v.split(',').next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(3.0);
        if !nk.running && pc.alive_for >= s && pc.alive_for - dt < s {
            REQ.store(true, Relaxed);
        }
    }
    if !PRELOADED.swap(true, Relaxed) {
        for w in S_ALL {
            sfx.preload(w);
        }
    }
    let Ok(mut p) = pawns.single_mut() else { return };
    if !nk.running {
        use crate::objectives::EndStage;
        let me_team = player_team();
        let offered_me = end.stage == EndStage::Offered && end.winner == me_team;
        OFFERED.store(offered_me, Relaxed);
        let req = REQ.swap(false, Relaxed);
        // the prompt: banner while it waits, a crawl line once
        if offered_me {
            let left = (crate::objectives::END_PLAYER_GRACE - end.since).max(0.0).ceil() as i32;
            flow.hud.banner = Some(format!("{NAME} BEREIT: TASTE {}  ({left})", slot_key_name()));
            if !nk.offer_shown {
                nk.offer_shown = true;
                feed.0.push((format!("DEIN TEAM GEWINNT: {NAME} MIT TASTE {} ANFORDERN", slot_key_name()), 2, String::new(), 2, None, crate::objectives::CRAWL_SECS * 2.0));
                sfx.wave(W_BEEP, 1.0, 0.8, 0.0);
            }
        }
        let can_player = flow.phase == Phase::Playing && !p.dead && !p.in_vehicle && !p.remote;
        // the player calls it when it is his (or nothing is awarded yet: debug menu / HB_NUKETEST,
        // his team then wins); a nuke awarded to the other team is not his to call
        let player_ok = end.stage == EndStage::Idle || offered_me;
        let by_player = req && player_ok && can_player;
        if req && !by_player {
            if !player_ok {
                info!("nuke: request ignored (awarded to team {})", end.winner);
            } else if p.in_vehicle {
                feed.0.push((format!("{NAME}: ZUERST AUSSTEIGEN"), 2, String::new(), 2, None, crate::objectives::CRAWL_SECS));
            }
        }
        // a bot of the winning team: after END_BOT_DELAY, or when the player let his chance go
        let due = !by_player && end.stage == EndStage::Offered && end.since >= if offered_me { crate::objectives::END_PLAYER_GRACE } else { crate::objectives::END_BOT_DELAY };
        if !by_player && !due {
            return;
        }
        if end.stage == EndStage::Idle {
            end.award(me_team, &flow, "player call");
        }
        end.stage = EndStage::Called;
        OFFERED.store(false, Relaxed);
        if offered_me || flow.hud.banner.as_deref().is_some_and(|b| b.starts_with(NAME)) {
            flow.hud.banner = None;
        }
        nk.running = true;
        nk.done = 0;
        nk.won = false;
        nk.tow = None;
        set_tow(None);
        nk.seed = 0x9e37_79b9 ^ (time.elapsed_secs() * 1000.0) as u32;
        let eye = p.eye_ue();
        nk.gz = ground_zero(&obj, &col, eye);
        nk.last_count = i32::MAX;
        nk.p0 = b(eye);
        nk.dir0 = b(p.dir_ue()).normalize_or(Vec3::X);
        nk.look = nk.dir0;
        nk.fov = p.fov;
        if by_player {
            // HB_NUKETEST=<s>,<t0>: start the sequence at t0 (tests of the later parts)
            nk.t = std::env::var("HB_NUKETEST").ok().and_then(|v| v.split(',').nth(1).and_then(|x| x.parse().ok())).unwrap_or(0.0);
            nk.god_before = Some(cheats.god);
            // the plane comes in over ground zero and flies on away from it
            let mut away = Vec3::new(eye.x - nk.gz.x, eye.y - nk.gz.y, 0.0);
            if away.length() < 3000.0 {
                away = -Vec3::new(p.dir_ue().x, p.dir_ue().y, 0.0);
            }
            nk.head = b(away).normalize_or(Vec3::X);
            nk.view_head = nk.head;
            info!("nuke: called by the player, ground zero {:?}, player {:?}", nk.gz, eye);
            DRAW.store(true, Relaxed);
            feed.0.push((format!("{NAME} ANGEFORDERT"), 2, String::new(), 2, None, crate::objectives::CRAWL_SECS));
        } else {
            // the caller: a living bot of the winning team on foot (botextras.rs drives none of
            // it: vehicle, launcher, drone), else a living one, else any of the team
            let team = end.winner;
            let caller = soldiers.iter().filter(|(_, s, _)| s.team == team).min_by_key(|(e, s, _)| (!s.alive, crate::botextras::controlled(*e))).map(|(e, s, _)| (e, s.name.clone()));
            let name = caller.as_ref().map(|c| c.1.clone()).unwrap_or_else(|| if team == crate::combat::TEAM_USA { "USA".into() } else { "KVA".into() });
            nk.t = 0.0;
            nk.god_before = None;
            // the far view looks from the player's side of ground zero
            nk.head = b(Vec3::new(eye.x - nk.gz.x, eye.y - nk.gz.y, 0.0)).normalize_or(Vec3::X);
            nk.view_head = nk.head;
            // the Fulton pickup of the caller (user 2026-10-22 22:08): a living bot on foot
            if let Some((ce, _)) = caller.as_ref().filter(|c| !crate::botextras::controlled(c.0)) {
                if let Ok((_, mut s, Some(bot))) = soldiers.get_mut(*ce) {
                    if s.alive {
                        s.crouched = false;
                        let fwd = Vec3::new(bot.yaw.cos(), 0.0, bot.yaw.sin());
                        nk.p0 = s.feet + Vec3::Y * HARNESS_H + fwd * HARNESS_FWD;
                        // the plane comes in over ground zero and flies on away from it (from
                        // the player when the bot stands at ground zero)
                        let gzb = b(nk.gz);
                        let mut away = Vec3::new(s.feet.x - gzb.x, 0.0, s.feet.z - gzb.z);
                        if away.length() < 30.0 {
                            let pl = b(eye);
                            away = Vec3::new(s.feet.x - pl.x, 0.0, s.feet.z - pl.z);
                        }
                        nk.head = away.normalize_or(Vec3::X);
                        nk.tow = Some((*ce, bot.slot, bot.yaw));
                        set_tow(Some(Tow { e: *ce, slot: bot.slot, pose: None }));
                        info!("nuke: bot {name:?} (slot {}) is picked up at {:?}", bot.slot, s.feet);
                    }
                }
            }
            info!("nuke: called by bot {name:?} (team {team}), ground zero {:?}, player {:?}", nk.gz, eye);
            feed.0.push((format!("{name}: {NAME} ANGEFORDERT"), 2, String::new(), 2, None, crate::objectives::CRAWL_SECS));
            nk.bot = Some((caller.map(|c| c.0), team, name));
        }
        if feed.0.len() > 5 {
            feed.0.remove(0);
        }
    }
    nk.active = true;
    if std::env::var_os("HB_NUKEDBG").is_some() && (nk.t + dt).floor() != nk.t.floor() {
        info!("nuke: t {:.1}", nk.t + dt);
    }
    // HB_NUKE_HOLD=<s>: the sequence waits that many real seconds after the call (tests: let
    // the software renderer compile the pipelines first)
    let hold = std::env::var("HB_NUKE_HOLD").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
    // (wall clock: the frame time is clamped, so slow frames would stretch the hold)
    let started = *nk.hold_start.get_or_insert_with(std::time::Instant::now);
    if started.elapsed().as_secs_f32() >= hold {
        nk.t += dt;
    }
    let t = nk.t;
    let gz = nk.gz;
    let gzb = b(gz);
    let tb = t - IMPACT;
    // bot mode: the player stays on the ground with his own view (see the module doc); the
    // calling bot is picked up when it was alive and on foot at the call
    let bot = nk.bot.is_some();
    let tow = nk.tow;
    let pickup = !bot || tow.is_some();
    // the player's ear / eye (bot mode: the pickup is seen and heard from here)
    let ear = b(p.eye_ue());
    if !bot {
        cheats.god = true;
    }
    // (bot mode: the player respawns as usual until the detonation, the blast is the end)
    if !bot || tb >= 0.0 {
        flow.respawn = false;
        if p.dead {
            pc.dead_for = 0.0;
            flow.hud.death = None;
        }
    }
    // (the caller - the player's own nuke - is picked up and never dies)
    DEAD_IN_BLAST.store(bot && tb >= 0.0 && p.dead, Relaxed);
    // ---- call: detonator press
    if !bot && t > 0.3 && t < HOOK_AT - 0.1 && nk.done & (1 << 40) == 0 {
        PRESS.store(true, Relaxed);
    }
    if bot || pc.det_fired.is_some() || t >= HOOK_AT - 0.1 {
        PRESS.store(false, Relaxed);
        nk.done |= 1 << 40;
    }
    if !bot && t > 0.6 && nk.once(0) {
        sfx.wave(W_CALL, 0.8, 1.0, 0.0);
    }
    // the siren over the town from the call until the boom, fainter as the player is towed away
    if t > 0.8 {
        let away = if bot { 0.0 } else { smooth((t - CATCH) / 5.0) };
        let cut = if tb > 0.0 { 1.0 - smooth((tb - 0.5) / 1.5) } else { 1.0 };
        if cut > 0.0 {
            sfx.wave_loop("nuke_siren", S_SIREN, 0.5 * smooth((t - 0.8) / 1.5) * (1.0 - 0.6 * away) * cut, 1.0);
        }
    }
    // ---- countdown (banner + the game's countdown beep)
    if t < IMPACT {
        let left = (IMPACT - t).ceil() as i32;
        if left != nk.last_count {
            nk.last_count = left;
            let enemy = nk.bot.as_ref().is_some_and(|b| b.1 != player_team());
            flow.hud.banner_msg = Some((left.to_string(), format!("{} {NAME} IM ANFLUG", if enemy { "FEINDLICHER" } else { "TAKTISCHER" }), flow.hud.clock, false));
            sfx.wave(W_BEEP, if left <= 3 { 1.0 } else { 0.7 }, 1.0, 0.0);
        }
    }
    // ---- own arm animations: the hook-up, the middle finger while towed (vmanim.rs)
    let hook_len = clips.hook.as_ref().map(|c| c.length()).unwrap_or(0.0);
    let hooking = !bot && clips.hook.is_some() && t >= HOOK_AT && t < HOOK_AT + hook_len;
    // the middle finger: up from 2 s before the detonation until the view is fully white; the hand
    // goes up (finger.hba 0..FINGER_HOLD.0) and then rocks back and forth in the clip's hold part
    // (ping-pong), it never goes down
    let fingering = !bot && clips.finger.is_some() && t >= FINGER_AT && tb < WHITE_START + WHITE_IN;
    let finger_t = {
        let s = t - FINGER_AT;
        let (a, b) = FINGER_HOLD;
        if s <= b {
            s
        } else {
            let span = b - a;
            let u = (s - b) % (2.0 * span);
            if u < span { b - u } else { a + (u - span) }
        }
    };
    let clip_pose = if hooking {
        clips.hook.as_ref().map(|c| c.sample(t - HOOK_AT))
    } else if fingering {
        clips.finger.as_ref().map(|c| c.sample(finger_t))
    } else {
        None
    };
    crate::vmanim::set(clip_pose);
    // gate pushed open by the ring (frame 21), snaps shut behind it (frame 27, hook.hba at 30 fps)
    if hooking && t - HOOK_AT >= 0.70 && nk.once(13) {
        sfx.wave(W_GATE, 0.8, 0.8, 0.0);
    }
    if hooking && t - HOOK_AT >= 0.90 && nk.once(14) {
        sfx.wave(W_CLIP, 1.1, 1.25, 0.0);
    }
    // ---- Fulton deploy: balloon up on its line, the view held on it
    let deployed = pickup && t >= DEPLOY;
    if deployed && nk.once(1) {
        if !bot {
            p.remote = true;
        }
        let g = if bot { dist_gain(ear.distance(nk.p0), 8.0) } else { 1.0 };
        sfx.wave(W_DEPLOY, 1.0 * g, 0.9, 0.0);
        sfx.wave(W_SWISH, 0.8 * g, 0.7, 0.15);
    }
    // the weapon goes away for the hook-up (the arm animations draw the arms themselves)
    if let Some(vm) = vm.as_mut().filter(|_| !bot && t >= HOOK_AT) {
        vm.lowered = true;
    }
    let up = smooth((t - DEPLOY) / BALLOON_RISE);
    // under a roof (the spawn under the overpass): the balloon goes up through the nearest open
    // sky within 40 m, the line slants to it (NOT from data)
    if deployed && nk.once(11) {
        let mut buf = Vec::new();
        let mut best = None;
        'search: for r in [0.0f32, 5.0, 10.0, 15.0, 20.0, 28.0, 36.0] {
            for k in 0..12 {
                let a = k as f32 / 12.0 * std::f32::consts::TAU;
                let o = Vec3::new(a.cos(), 0.0, a.sin()) * r;
                // down from the balloon height (one-sided triangles: the underside of a deck
                // does not stop a ray going up)
                let clear = col.raycast_mask(nk.p0 + o + Vec3::Y * BALLOON_H, Vec3::NEG_Y, BALLOON_H - 2.0, &mut buf, crate::player::COL_PLAYER | crate::player::COL_TRACE).is_none();
                if clear {
                    best = Some(o);
                    break 'search;
                }
                if r == 0.0 {
                    break;
                }
            }
        }
        nk.sky_off = best.unwrap_or(Vec3::ZERO);
        info!("nuke: balloon offset to open sky {:?} (pickup {:?})", best, nk.p0);
    }
    let lean = nk.sky_off * smooth((t - DEPLOY) / 0.8);
    let balloon = nk.p0 + lean + Vec3::Y * (2.0 + (BALLOON_H - 2.0) * up) + Vec3::new((t * 0.7).sin(), 0.0, (t * 0.5).cos()) * 2.0 * up;
    // ---- plane
    let plane = nk.plane_at(t);
    let plane_on = pickup && t >= PLANE_SEEN - 1.0 && t < IMPACT + WHITE_START + WHITE_IN + WHITE_HOLD;
    {
        let head = nk.head;
        let s = t - CATCH;
        let pitch = if s > 0.0 { 0.12 * smooth(s / 2.0) } else { 0.0 };
        for (mut tf, mut v) in vis.p3().iter_mut() {
            *v = if plane_on { Visibility::Visible } else { Visibility::Hidden };
            tf.translation = plane;
            tf.rotation = Quat::from_rotation_arc(Vec3::X, head) * Quat::from_rotation_z(pitch);
        }
        for (mut tf, hub) in vis.p4().iter_mut() {
            if hub {
                tf.rotate_local_x(dt * PROP_SPIN);
            } else {
                tf.rotate_local_y(dt * 70.0);
            }
        }
    }
    // engine: the recorded pass, timed so its loudest point is the catch, then the drone while
    // towed (fades out with the white-out)
    // (bot mode: from the distance to the catch point / the plane)
    if pickup && t >= CATCH - S_PLANE_PEAK && nk.once(12) {
        let g = if bot { dist_gain(ear.distance(nk.p0 + nk.sky_off + Vec3::Y * BALLOON_H), 120.0) } else { 1.0 };
        sfx.wave(S_PLANE, 1.2 * g, 1.0, 0.0);
    }
    if t > CATCH + 2.0 && plane_on {
        let fade = 1.0 - smooth((t - IMPACT - WHITE_START - WHITE_IN) / 1.0);
        let g = if bot { dist_gain(ear.distance(plane), 120.0) } else { 1.0 };
        sfx.wave_loop("nuke_engine", S_ENGINE, 0.55 * smooth((t - CATCH - 2.0) / 2.0) * fade * g, 0.92);
    }
    // ---- catch
    if pickup && t >= CATCH && nk.once(2) {
        let g = if bot { dist_gain(ear.distance(nk.p0), 25.0) } else { 1.0 };
        sfx.wave(W_YANK, 1.4 * g, 0.7, 0.0);
        sfx.wave(W_YANK, 1.0 * g, 0.5, 0.12);
    }
    if !bot && t >= CATCH && t < IMPACT + WHITE_START + WHITE_IN + WHITE_HOLD {
        let s = t - CATCH;
        sfx.wave_loop("nuke_wind", W_WIND, (0.35 + 0.5 * smooth(s / 2.0)) * (1.0 - smooth((t - IMPACT - WHITE_START - WHITE_IN) / 1.0)), 0.42 + 0.08 * smooth(s / 3.0));
    }
    // ---- the player's position and the view
    let towed = !bot && t >= CATCH;
    // the one on the line: the player, in bot mode the bot's harness
    // (from a roofed spot the yank first drags the player out to the line)
    let me = if pickup && t >= CATCH { nk.p0.lerp(nk.towed_at(t), smooth((t - CATCH) / 0.6)) } else { nk.p0 };
    let far = tb > WHITE_START + WHITE_IN + WHITE_HOLD;
    // bot mode: the player's view goes to the pickup (balloon, plane, snag) from the deploy until
    // shortly after the catch, then it is his again; only while he plays on foot
    let bot_view = bot && tow.is_some() && t >= DEPLOY && t < CATCH + BOT_VIEW_AFTER && flow.phase == Phase::Playing && !p.dead && !p.in_vehicle && !p.remote;
    if bot_view && nk.once(15) {
        nk.look = b(p.dir_ue()).normalize_or(nk.dir0);
    }
    // look target: balloon -> plane until the catch, the plane briefly, then ground zero (bot
    // mode: after the catch the bot on the line with the plane above it)
    let target = if t < PLANE_SEEN {
        balloon
    } else if t < CATCH {
        plane + nk.head * 6.0
    } else if bot {
        me.lerp(plane, 0.3)
    } else if t < CATCH + 0.8 {
        plane + nk.head * 6.0
    } else {
        gzb + Vec3::Y * (60.0 + cloud_shape(tb.max(0.0))[0] * 0.5 * smooth(tb / 3.0))
    };
    let want = (target - if bot { ear } else { me }).normalize_or(nk.dir0);
    // turn rate: quick onto the balloon / plane, slower round to ground zero (bot mode: a softer
    // start from wherever the player looked)
    let rate = if bot {
        1.5 + 2.5 * smooth((t - DEPLOY) / 1.0)
    } else if t < CATCH + 0.8 {
        4.0
    } else {
        1.3
    };
    let k = 1.0 - (-dt * rate).exp();
    // during the hook-up: the view ahead, pitched as in the animation
    let hook_dir = clips.hook.as_ref().filter(|_| hooking).map(|c| {
        let h = Vec3::new(nk.dir0.x, 0.0, nk.dir0.z).normalize_or(Vec3::X);
        let a = c.pitch_at(t - HOOK_AT);
        (h * a.cos() + Vec3::Y * a.sin()).normalize()
    });
    if let Some(d) = hook_dir {
        nk.look = d;
    } else if (deployed && !bot) || bot_view {
        nk.look = nk.look.lerp(want, k).normalize_or(want);
    }
    // bot mode: the pawn's own view turns (his camera, his weapon; UE yaw / pitch, 65536 = full
    // turn, the yaw kept near his own so it does not jump a turn)
    if bot_view {
        let ue_d = Vec3::new(nk.look.x, nk.look.z, nk.look.y);
        let mut yaw = ue_d.y.atan2(ue_d.x) / std::f32::consts::TAU * 65536.0;
        yaw += ((p.yaw - yaw) / 65536.0).round() * 65536.0;
        p.yaw = yaw;
        p.pitch = (ue_d.z.clamp(-1.0, 1.0).asin() / std::f32::consts::TAU * 65536.0).clamp(-16000.0, 16000.0);
    }
    // bot mode: the bot on the line (bots.rs render_bots draws its body with this pose): the yank
    // (jump_up) and then hanging (jump_dn), the body turning from upright to along the line, facing
    // the flight direction; frozen once the plane is gone (NOT from data)
    if let Some((te, _, yaw0)) = tow.filter(|_| t >= CATCH && plane_on) {
        // the body at time tau: feet (Bevy m) and rotation (UE axes; columns forward, left, up)
        let rot_ue = |f: Vec3, u: Vec3| {
            let (f, u) = (Vec3::new(f.x, f.z, f.y), Vec3::new(u.x, u.z, u.y));
            Quat::from_mat3(&Mat3::from_cols(f, u.cross(f), u)).normalize()
        };
        let f0 = Vec3::new(yaw0.cos(), 0.0, yaw0.sin());
        let body_at = |tau: f32| {
            let s = (tau - CATCH).max(0.0);
            let me_t = nk.p0.lerp(nk.towed_at(tau), smooth(s / 0.6));
            let rope = (nk.plane_at(tau) + nk.head * 14.0 - me_t).normalize_or(Vec3::Y);
            let up1 = Vec3::Y.lerp(rope, smooth(s / 0.4)).normalize_or(Vec3::Y);
            let f1 = (nk.head - up1 * nk.head.dot(up1)).normalize_or(nk.head);
            let rot = rot_ue(f0, Vec3::Y).slerp(rot_ue(f1, up1), smooth(s / 0.5));
            let up_b = { let u = rot * Vec3::Z; Vec3::new(u.x, u.z, u.y) };
            let fwd_b = { let f = rot * Vec3::X; Vec3::new(f.x, f.z, f.y) };
            (me_t - up_b * HARNESS_H - fwd_b * HARNESS_FWD, rot)
        };
        // HB_TOWLAG=<frames>: the drawn pose runs that many frames ahead (default 0), in case the
        // CPU-skinned body reaches the screen later than the line (container recordings hinted at
        // ~2 frames, not confirmed); the soldier itself is where the line is now
        let (feet_now, _) = body_at(t);
        let lag = *TOW_LAG_ENV.get_or_init(|| std::env::var("HB_TOWLAG").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0));
        let (feet, rot) = body_at(t + lag * dt);
        let s = t - CATCH;
        let (anim, looped) = if s < 0.25 { ("jump_up", false) } else { ("jump_dn", true) };
        if let Ok(mut g) = TOW.lock() {
            if let Some(tw) = g.as_mut() {
                tw.pose = Some(TowPose { feet_ue: ue(feet), rot, anim, looped });
            }
        }
        if let Ok((_, mut sd, _)) = soldiers.get_mut(te) {
            sd.feet = feet_now;
        }
    }
    let mut cam = None;
    if (deployed && !bot) || hook_dir.is_some() || (bot && far) {
        let mut pos = me;
        let mut dir = nk.look;
        let mut roll = 0.0;
        if towed {
            let s = t - CATCH;
            roll = (s * 0.9).sin() * 0.06 + nk.swing(t) * 0.05;
            // hand-held sway
            dir = (dir + Vec3::new((s * 0.7).sin(), (s * 0.9 + 2.0).sin(), (s * 1.1).sin()) * 0.01).normalize();
        }
        if far {
            // after the white-out: far away, looking at the cloud (out of the plane's ramp)
            let x = tb - WHITE_START - WHITE_IN - WHITE_HOLD;
            let side = Vec3::new(-nk.view_head.z, 0.0, nk.view_head.x);
            pos = gzb + nk.view_head * (FAR_DIST + 12.0 * x.min(25.0)) + side * 260.0 + Vec3::Y * (FAR_HEIGHT + 3.0 * x.min(25.0));
            let h = cloud_shape(tb)[0];
            dir = (gzb + Vec3::Y * (h * 0.55) - pos).normalize();
            roll = (x * 0.4).sin() * 0.02;
        }
        // camera shake (last frame's strength) on the view itself, so the arms drawn from the pawn's
        // view (the middle finger, the hook-up) shake with the camera instead of jumping on screen
        let sh = nk.shake;
        let tj = time.elapsed_secs();
        let jitter = Vec3::new((tj * 37.0).sin() + (tj * 61.0).cos(), (tj * 43.0).cos() + (tj * 71.0).sin(), (tj * 53.0).sin()) * 0.5;
        let dir = (dir.normalize_or(Vec3::NEG_Z) + jitter * sh * 0.01).normalize();
        let pos = pos + jitter * sh * 0.12;
        let roll = roll + jitter.z * sh * 0.008;
        cam = Some((pos, dir, roll));
        // the pawn rides along (bots aim at it, the listener follows the camera); its view too,
        // the arm animations are drawn from it (viewmodel.rs: UE yaw / pitch, 65536 = full turn)
        if !p.dead && !bot {
            p.feet = pos - Vec3::Y * p.eye;
            let ue_d = Vec3::new(dir.x, dir.z, dir.y);
            p.yaw = ue_d.y.atan2(ue_d.x) / std::f32::consts::TAU * 65536.0;
            p.pitch = ue_d.z.clamp(-1.0, 1.0).asin() / std::f32::consts::TAU * 65536.0;
        }
    }
    nk.cam = cam;
    let cam_pos = cam.map(|c| c.0).unwrap_or(b(p.eye_ue()));
    // ---- balloon and line
    for (mut tf, mut v, model) in vis.p2().iter_mut() {
        let on = deployed && t < CATCH;
        *v = if on { Visibility::Visible } else { Visibility::Hidden };
        if model {
            // the bridle ring on the line's end, the nose into the wind (towards the incoming
            // plane), inflating as it rises
            tf.translation = balloon;
            tf.rotation = Quat::from_rotation_arc(Vec3::X, -nk.head) * Quat::from_rotation_z(0.05 * (t * 0.8).sin());
            tf.scale = Vec3::splat(0.35 + 0.65 * smooth(up * 2.5));
        } else {
            tf.translation = balloon + Vec3::Y * 3.0;
            tf.scale = Vec3::new(1.5, 3.2, 1.5) * (0.4 + 0.6 * up);
        }
    }
    // hook-up props (carabiner, gate, line, harness ring, tether) in the view as animated
    let view_rot = cam.map(|(_, d, roll)| Transform::default().looking_to(d, Vec3::Y).rotation * Quat::from_rotation_z(roll));
    // lateral scale of the 1P mesh (viewmodel.rs; mesh FOV 50 of the M4 dump vmpose_m4idle.json)
    let lat_k = crate::viewmodel::lateral_scale(p.fov, 50.0);
    for (mut tf, mut v, hp) in vis.p5().iter_mut() {
        let Some(hp) = hp else {
            // HookSpace: the props' parent
            if let (Some((eye, _, _)), Some(vr)) = (cam, view_rot) {
                *tf = Transform { translation: eye, rotation: vr, scale: Vec3::new(lat_k, lat_k, 1.0) };
            }
            continue;
        };
        let pose = clips.hook.as_ref().filter(|_| hooking).and_then(|c| {
            let name = c.objects.get(hp.0)?.clone();
            c.object_at(&name, t - HOOK_AT)
        });
        match (pose, cam) {
            (Some((r, pp, sc)), Some(_)) => {
                *v = Visibility::Visible;
                tf.translation = pp;
                tf.rotation = r;
                tf.scale = sc;
            }
            _ => *v = Visibility::Hidden,
        }
    }
    for (mut tf, mut v, lm) in vis.p1().iter_mut() {
        // the line: from the chest to the balloon, after the catch from the chest to the nose
        let (a, z) = if !deployed || tb > WHITE_START + WHITE_IN {
            (Vec3::ZERO, Vec3::ZERO)
        } else if t < CATCH {
            // (bot mode: me is the harness on the bot's chest)
            (if bot { me } else { me - Vec3::Y * 0.5 + nk.dir0 * 0.3 }, balloon)
        } else {
            let mut a = if bot { me } else { me + Vec3::Y * 1.2 };
            let z = plane + nk.head * 14.0;
            // keep the line off the raised hand (middle finger, finger.hba: right of centre,
            // about 0.26 m ahead; NOT from data): the harness end moves to the left until the
            // line passes the hand by 25 cm
            if let (true, Some(vr)) = (fingering, view_rot) {
                let hand = me + vr * Vec3::new(0.08 * lat_k, 0.06 * lat_k, -0.26);
                let left = vr * Vec3::NEG_X;
                for _ in 0..10 {
                    let d = z - a;
                    let k = ((hand - a).dot(d) / d.length_squared().max(1e-6)).clamp(0.0, 1.0);
                    if (a + d * k - hand).length() > 0.25 {
                        break;
                    }
                    a += left * 0.15;
                }
            }
            (a, z)
        };
        let len = (z - a).length();
        if len < 0.5 {
            *v = Visibility::Hidden;
            continue;
        }
        *v = Visibility::Visible;
        tf.translation = (a + z) * 0.5;
        tf.rotation = Quat::from_rotation_arc(Vec3::Y, (z - a) / len);
        // 2.2 cm rope (thick enough to read from the far plane view), a twist every 9 cm
        // (bot mode: seen from the ground far away, at least ~1.5 px wide: radius 0.8 mm per metre
        // from the player's eye; NOT from data)
        let r = if bot { 0.022f32.max(0.0008 * ear.distance((a + z) * 0.5)) } else { 0.022 };
        tf.scale = Vec3::new(r, len, r);
        if let Some(m) = std_mats.get_mut(&lm.0) {
            let want = Vec2::new(1.0, len / 0.09);
            if m.uv_transform.matrix2.y_axis.y != want.y {
                m.uv_transform = bevy::math::Affine2::from_scale(want);
            }
        }
    }
    // ---- warhead
    let wt = t - (IMPACT - WARHEAD);
    if (0.0..WARHEAD).contains(&wt) {
        let u = wt / WARHEAD;
        let from = gzb + Vec3::Y * 2600.0 - nk.head * 400.0;
        let at = from.lerp(gzb + Vec3::Y * 180.0, u);
        let vel = (gzb + Vec3::Y * 180.0 - from).normalize();
        if nk.once(3) {
            sfx.wave(W_WARHEAD, 1.0, 0.6, 0.0);
            nk.warhead_trail = fx.fire_scaled(TRAIL_FX, ue(at), crate::combat::dir_quat(-Vec3::new(vel.x, vel.z, vel.y)), 3.0);
        }
        if let Some(id) = nk.warhead_trail {
            fx.track(id, ue(at), crate::combat::dir_quat(-Vec3::new(vel.x, vel.z, vel.y)));
        }
        for (mut tf, mut v) in vis.p0().iter_mut() {
            *v = Visibility::Visible;
            tf.translation = at;
            tf.rotation = Quat::from_rotation_arc(Vec3::NEG_Y, vel);
            tf.scale = Vec3::splat(2.0);
        }
    } else {
        for (_, mut v) in vis.p0().iter_mut() {
            *v = Visibility::Hidden;
        }
    }
    // ---- detonation
    if tb >= 0.0 && nk.once(5) {
        info!("nuke: detonation at {gz:?}");
        if let Some(id) = nk.warhead_trail.take() {
            fx.release(id);
        }
        dl.flash(&el, "GCExplosionLight_Nuke", gz + Vec3::Z * 1500.0);
        // (no game explosion templates at the fireball any more: next to a 100 m fireball they
        // read as small fireworks, user 2026-10-19 22:54)
    }
    if tb >= 3.0 && nk.once(7) {
        for k in 0..4 {
            let a = k as f32 * 1.57;
            fx.fire_scaled(SMOKE_FX[0].0, gz + Vec3::new(a.cos(), a.sin(), 0.0) * 6000.0, Quat::IDENTITY, SMOKE_FX[0].2);
        }
        for k in 0..6 {
            let a = k as f32 * 1.05 + 0.3;
            let r = 8000.0 + 4000.0 * (k % 3) as f32;
            fx.fire_scaled(SMOKE_FX[1].0, gz + Vec3::new(a.cos(), a.sin(), 0.0) * r, Quat::IDENTITY, SMOKE_FX[1].2);
        }
    }
    // fireball light (NOT from data, on top of GCExplosionLight_Nuke)
    if tb >= 0.0 {
        let sh = cloud_shape(tb);
        let b0 = 60.0 * (-tb / 0.8).exp() + 10.0 * sh[5];
        dl.steady.push(crate::dynlights::Light { at: gz + Vec3::Z * sh[0] * 100.0 * 0.8, color: [b0, b0 * 0.6, b0 * 0.3], radius: 150000.0 });
    }
    // ---- shock wave: kills, dust bursts on the ground along the front
    let sr = shock_radius(tb);
    let cam_dist = (cam_pos - gzb).length();
    if tb > 0.0 {
        // the caller: the player, or the bot (bot mode; the world when nobody of its team is left)
        let (attacker, attacker_team) = match &nk.bot {
            None => (crate::combat::Actor::Player, player_team()),
            Some((Some(e), team, _)) => (crate::combat::Actor::Bot(*e), *team),
            Some((None, team, _)) => (crate::combat::Actor::World, *team),
        };
        for (e, s, _) in &soldiers {
            // (bot mode: the bot on the line is far up and away, combat.rs ignores its hits too)
            if !s.alive || tow.is_some_and(|tw| tw.0 == e) {
                continue;
            }
            let c = s.center_ue();
            let d = Vec2::new(c.x - gz.x, c.y - gz.y).length() * 0.01;
            if d < sr {
                queue.0.push(crate::combat::Hit { target: crate::combat::Target::Bot(e), amount: 100000.0, attacker, attacker_team, headshot: false, explosive: true, from: gz, icon: None, dmg_type: None });
            }
        }
        // bot mode: the player was not picked up, the shock front kills him too
        if bot && !p.dead && Vec2::new(p.feet.x - gzb.x, p.feet.z - gzb.z).length() < sr {
            queue.0.push(crate::combat::Hit { target: crate::combat::Target::Player, amount: 100000.0, attacker, attacker_team, headshot: false, explosive: true, from: gz, icon: None, dmg_type: None });
        }
        nk.ring_acc += dt;
        if RING_DUST && sr < 900.0 && nk.ring_acc > 0.08 {
            nk.ring_acc = 0.0;
            let mut buf = Vec::new();
            for k in 0..3 {
                let a = nk.rnd() * std::f32::consts::TAU;
                let p2 = gz + Vec3::new(a.cos(), a.sin(), 0.0) * sr * 100.0;
                let top = b(p2 + Vec3::Z * 8000.0);
                if let Some((h, _)) = col.raycast(top, Vec3::NEG_Y, 300.0, &mut buf) {
                    let at = p2 + Vec3::Z * (8000.0 - h * 100.0);
                    let (tpl, _, sc) = RING_FX[k % 2];
                    fx.fire_scaled(tpl, at, Quat::IDENTITY, sc);
                }
            }
        }
    }
    // the boom with the speed of sound (from where the player was then), the shock at the player
    let sound_at = cam_dist / SOUND_SPEED;
    if tb >= sound_at && tb > 0.0 && nk.once(8) {
        sfx.wave(S_BOOM, 1.6, 1.0, 0.0);
        sfx.wave(S_BLAST, 1.0, 0.8, 0.0);
    }
    let shock_t = ((cam_dist - 60.0).max(0.0) / 330.0).powf(1.0 / 0.8);
    if tb > 0.0 && tb >= shock_t && !far && nk.once(9) {
        sfx.wave(S_BLAST, 1.0, 1.05, 0.0);
    }
    // the roll of the cloud heard from far away once the view is back
    if far && nk.once(10) {
        sfx.wave(S_RUMBLE, 1.2, 0.9, 0.0);
    }
    // ---- the crater, swapped in while the view is fully white (crater.rs)
    if tb >= WHITE_START + WHITE_IN + 0.2 {
        crater.trigger(gz);
    }
    // ---- match over: objectives.rs ends it with the caller's team (Endgame Done), the frozen
    // score stays; HB_NUKE_NOWIN: no match end (tests of the far view)
    if tb >= WIN_AT && !nk.won && std::env::var_os("HB_NUKE_NOWIN").is_none() {
        nk.won = true;
        info!("nuke: sequence over, match to team {}", end.winner);
        end.stage = crate::objectives::EndStage::Done;
    }
    if tb > 0.0 {
        flow.hud.rewards.clear();
    }
    // ---- shake
    let mut shake = 0.0f32;
    if !bot && (CATCH..CATCH + 0.8).contains(&t) {
        shake += 2.2 * (1.0 - (t - CATCH) / 0.8);
    }
    if tb > 0.0 && !far {
        shake += 0.5 * (-tb / 1.5).exp();
        if tb > shock_t - 0.3 {
            shake += 3.5 * (-(tb - shock_t).max(0.0) / 1.0).exp() * smooth((tb - shock_t + 0.3) / 0.3);
        }
    }
    if far {
        shake += 0.15 * (-(tb - WHITE_START - WHITE_IN - WHITE_HOLD) / 6.0).exp();
    }
    nk.shake = shake;
    // ---- post pass: flash, white-out, blur from the blast, grain
    let mut a = [0.0f32; 4];
    let mut bb = [time.elapsed_secs_wrapped(), 0.0, 0.5, 0.5];
    let mut c = [0.0f32; 4];
    if tb >= 0.0 {
        // 0-0.05 white; impact frame 0.05-0.33 (overloaded sensor: negative, positive,
        // negative), then the flash decays
        a[0] = if tb < 0.05 {
            1.0
        } else if tb < 0.33 {
            0.0
        } else {
            0.75 * (-(tb - 0.33) / 0.25).exp()
        };
        if (0.05..0.33).contains(&tb) && !far {
            a[1] = 1.0;
            c[1] = if (0.14..0.23).contains(&tb) { 0.0 } else { 1.0 };
        }
        // the fireball's glow on the lens (sun-bright first, then the glowing cloud)
        a[3] = if far { 0.0 } else { 1.6 * (-tb / 1.0).exp() + 0.35 * cloud_shape(tb)[5] };
        // the fireball is seen for a moment, then the light (and the shock) white the view out
        let w_in = smooth((tb - WHITE_START) / WHITE_IN).powf(1.6);
        let w_out = smooth((tb - WHITE_START - WHITE_IN - WHITE_HOLD) / WHITE_OUT);
        a[2] = w_in * (1.0 - w_out);
        bb[1] = if far { 0.0 } else { (0.22 * (-tb / 1.2).exp() + shake * 0.03).min(0.5) };
        c[0] = if far { 0.0 } else { shake * 0.15 + 0.35 * (-tb / 0.8).exp() };
        c[2] = 0.08 + 0.06 * (-tb / 4.0).exp();
        c[3] = 0.12 * smooth(tb / 6.0);
    }
    fxp.on = t > 0.0 && (a.iter().any(|x| *x > 0.0) || bb[1] > 0.0 || c.iter().any(|x| *x > 0.0));
    fxp.a = a;
    fxp.b = bb;
    fxp.c = c;
    // ---- cloud
    cloud.on = tb >= 0.0;
    if cloud.on {
        let [h, ring, r, rs, grow, heat] = cloud_shape(tb);
        let sun = suns.iter().next().map(|(l, g)| (-g.forward().as_vec3(), l.color.to_linear())).unwrap_or((Vec3::new(0.3, 0.8, 0.4).normalize(), LinearRgba::WHITE));
        let surge = smooth(tb / 0.5) * (1.0 - smooth((sr - 900.0) / 700.0));
        let shell = smooth((tb - 0.25) / 0.35) * (1.0 - smooth((tb - 1.5) / 1.5));
        let half = (sr + 80.0).max(ring + r + 60.0).min(1700.0);
        let top = h + r * 1.6 + 80.0;
        let sun_k = 2.2;
        cloud.p = crate::nukecloud::CloudParams {
            a: gzb.extend(tb),
            b: sun.0.normalize().extend(sr),
            c: Vec4::new(h, ring, r, rs),
            d: Vec4::new(heat, surge, shell, 1.0),
            e: Vec4::new(sun.1.red * sun_k, sun.1.green * sun_k, sun.1.blue * sun_k, 0.9),
            f: Vec4::new(half, top, if std::env::var_os("HB_CLOUDDBG").is_some() { -1.0 } else { 56.0 }, grow),
        };
    }
    let _ = &mut commands;
}

/// Escape camera (after the player's camera systems).
fn camera(
    nk: Res<Nuke>,
    _time: Res<Time>,
    mut cams: Query<(&Camera, &mut Transform, &mut Projection), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    mut fxp: ResMut<NukeFx>,
) {
    CAM_OWNED.store(nk.cam.is_some(), Relaxed);
    // (bot mode before the far view: the player's own camera, only the blur centre is set)
    if nk.cam.is_none() && !nk.running {
        return;
    }
    let Ok((camera, mut tf, mut proj)) = cams.single_mut() else { return };
    if let Some((pos, dir, roll)) = nk.cam {
        // (the shake is already in nk.cam, see update)
        tf.translation = pos;
        tf.look_to(dir.normalize_or(Vec3::NEG_Z), Vec3::Y);
        tf.rotate_local_z(roll);
        if let Projection::Perspective(pp) = &mut *proj {
            let f = nk.fov.max(30.0).to_radians();
            if (pp.fov - f).abs() > 1e-6 {
                pp.fov = f;
            }
        }
    }
    // blast centre on screen for the radial blur
    let gt = GlobalTransform::from(*tf);
    // (the fireball's centre: it rises with the cap)
    let h = cloud_shape((nk.t - IMPACT).max(0.0))[0];
    if let Some(ndc) = camera.world_to_ndc(&gt, b(nk.gz) + Vec3::Y * h.max(60.0)) {
        if ndc.z > 0.0 {
            fxp.b[2] = (ndc.x * 0.5 + 0.5).clamp(-0.5, 1.5);
            fxp.b[3] = (0.5 - ndc.y * 0.5).clamp(-0.5, 1.5);
        }
    }
}

// ---------------------------------------------------------------- post pass

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct NukeFxLabel;

#[derive(Resource)]
struct NukeFxPipeline {
    layout: BindGroupLayout,
    sampler: Sampler,
    pipeline: CachedRenderPipelineId,
}

impl FromWorld for NukeFxPipeline {
    fn from_world(world: &mut World) -> Self {
        let dev = world.resource::<RenderDevice>();
        let layout = dev.create_bind_group_layout(
            "nukefx_layout",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering), uniform_buffer_sized(false, std::num::NonZeroU64::new(48)))),
        );
        let sampler = dev.create_sampler(&SamplerDescriptor { address_mode_u: AddressMode::ClampToEdge, address_mode_v: AddressMode::ClampToEdge, mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
        let shader = world.load_asset(SHADER);
        let pipeline = world.resource_mut::<PipelineCache>().queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("nukefx".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader_vertex_state(),
            fragment: Some(FragmentState { shader, shader_defs: vec![], entry_point: "fragment".into(), targets: vec![Some(ColorTargetState { format: ViewTarget::TEXTURE_FORMAT_HDR, blend: None, write_mask: ColorWrites::ALL })] }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });
        NukeFxPipeline { layout, sampler, pipeline }
    }
}

#[derive(Default)]
struct NukeFxNode;

impl ViewNode for NukeFxNode {
    type ViewQuery = (&'static ViewTarget, Has<crate::minimap::MinimapCam>);

    fn run(&self, _graph: &mut RenderGraphContext, ctx: &mut RenderContext, (target, minimap): QueryItem<Self::ViewQuery>, world: &World) -> Result<(), NodeRunError> {
        if minimap {
            return Ok(());
        }
        let Some(fx) = world.get_resource::<NukeFx>() else { return Ok(()) };
        if !fx.on {
            return Ok(());
        }
        let pipe = world.resource::<NukeFxPipeline>();
        let Some(p) = world.resource::<PipelineCache>().get_render_pipeline(pipe.pipeline) else { return Ok(()) };
        let dev = ctx.render_device().clone();
        let mut params = [0.0f32; 12];
        params[0..4].copy_from_slice(&fx.a);
        params[4..8].copy_from_slice(&fx.b);
        params[8..12].copy_from_slice(&fx.c);
        let buf = dev.create_buffer_with_data(&BufferInitDescriptor { label: Some("nukefx_params"), contents: bytemuck::cast_slice(&params), usage: BufferUsages::UNIFORM });
        let post = target.post_process_write();
        let bg = dev.create_bind_group("nukefx_bg", &pipe.layout, &BindGroupEntries::sequential((post.source, &pipe.sampler, buf.as_entire_binding())));
        let mut rp = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("nukefx"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, resolve_target: None, ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_render_pipeline(p);
        rp.set_bind_group(0, &bg, &[]);
        rp.draw(0..3, 0..1);
        Ok(())
    }
}
