//! Infantry combat: the player's primary weapon (fire, aim, reload), frag grenades, health,
//! damage to and from the bots, kills, battle points and the kill streak.
//!
//! Data (weaponcfg.rs): FireModes[0] (FireInterval, AmmoClip / AmmoTotal, ReloadTime,
//! WeaponRange / FullDamageRange, InstantHitDamageType), Recoil Hip / Zoom, Spread, scope
//! FOVZoom; MP.Damage.DamageGridMP (damage per damage type and physical material, near / far
//! halves); GCThrowableWeaponInfo (GrenadeFuseTime 3, PreThrowAnimationTime 0.56,
//! ThrowAnimationTime 0.5, Ammo 2); GCProj_Grenade (Speed 1500, TossZ 100, DamageRadius 400,
//! m_fDampenFactor 0.2 / Parallel 0.45); Default__GCCharacter HealthRegeneration (5 s, 10 HP/s),
//! HeadOffset 32 / HeadRadius 9; Engine Pawn Health 100; GCGame.ini [KillStreak
//! Objective_Mission_GameStreak] Tiers (KillBP 20, KillGameScore, streak stars);
//! GCBattlePoints.ini Rewards PSR_Headshot 30; GCInput_deu.ini BindingsFoot (LMB fire,
//! RMB aim, R reload, G frag grenade).
use crate::menu::{MatchFlow, Phase};
use crate::player::{CollisionWorld, Pawn};
use crate::viewmodel::{ViewModel, WeaponAnim};
use crate::weaponcfg::{DamageGrid, GrenadeCfg, LoadoutCfg, WeaponCfg};
use bevy::prelude::*;
use crate::input::{down as key_down, hit, Set};

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerCombat>()
            .init_resource::<Cheats>()
            .init_resource::<Grenades>()
            .init_resource::<DamageQueue>()
            .init_resource::<VehicleHits>()
            .init_resource::<KillFeed>()
            .init_resource::<LockOn>()
            .add_systems(Update, (player_lock_on, player_weapon, update_grenades, apply_damage, render_projectiles).chain().in_set(CombatSet).after(crate::player::PlayerSet));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSet;

/// Debug menu (F1) switches for the player.
#[derive(Resource, Default)]
pub struct Cheats {
    /// no damage to the player (except the debug menu's own kill)
    pub god: bool,
    /// magazine, grenades and tactical grenades are not used up
    pub inf_ammo: bool,
    /// killstreak items to activate for free (debug menu "Killstreaks")
    pub grant: Vec<String>,
    /// vehicle class to place at the player for free (debug menu; vehicles.rs spawn_vehicle)
    pub vehicle: Option<String>,
}

pub const TEAM_USA: u8 = 0;
pub const TEAM_KVA: u8 = 1;

/// The local player's team (USA at match start; the spawn selection's "TEAM WECHSELN",
/// GCCommand_UISwitchTeam, changes it - menu.rs switch_team).
static PLAYER_TEAM: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(TEAM_USA);

pub fn player_team() -> u8 {
    PLAYER_TEAM.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_player_team(t: u8) {
    PLAYER_TEAM.store(t & 1, std::sync::atomic::Ordering::Relaxed);
}
/// Engine Default__Pawn HealthMax
pub const HEALTH_MAX: f32 = 100.0;
const REGEN_DELAY: f32 = 5.0;
const REGEN_RATE: f32 = 10.0;
/// Default__GCCharacter HeadOffset / HeadRadius (UE units above the cylinder centre)
pub const HEAD_OFFSET: f32 = 32.0;
pub const HEAD_RADIUS: f32 = 9.0;
pub const CYL_RADIUS: f32 = 15.0;
pub const CYL_HALF: f32 = 41.5;
/// DefaultGravityZ * CustomGravityScaling (UE units / s^2) for thrown grenades
const GRAVITY_UE: f32 = 750.0;

#[derive(Resource, Clone)]
pub struct CombatData {
    pub weapons: Vec<WeaponCfg>,
    pub loadouts: Vec<LoadoutCfg>,
    pub grid: DamageGrid,
    pub grenade: GrenadeCfg,
    /// GCBattlePoints SpecialAbilityCosts: class -> (Cost, PurchaseAmount, stinger cue)
    pub costs: std::collections::HashMap<String, (i32, i32, Option<String>)>,
    /// display names of the special abilities by cost key (weaponcfg::special_names)
    pub special_names: std::collections::HashMap<String, String>,
    /// GCWeap_EQP_FlakJacket MaxHealthScale
    pub flak_scale: f32,
    /// loadout item cfg (lowercase) -> UIIcon_Small (HUD special weapon slots)
    pub small_icons: std::collections::HashMap<String, String>,
    /// GCWeap_EQP_UAV MaxSweepDuration
    pub uav_secs: f32,
    /// thermal goggles: MaxThermalDuration, on / off cues
    pub goggles: (f32, String, String),
    /// PlayerSpawnManagerMP ServerSpawnDelayTime: kill cam / "ENTSTEHUNG IN:" countdown (s)
    pub spawn_delay: f32,
    pub hellfire: Option<crate::weaponcfg::HellfireCfg>,
    /// index of the Hellfire missile entry in `weapons`
    pub hellfire_weapon: Option<usize>,
    /// all airstrikes by cost key (airstrike.rs)
    pub strikes: Vec<Strike>,
    /// anti tank drone fire modes and their weapon entries (drone.rs)
    pub drone_rockets: std::collections::HashMap<String, Vec<(crate::weaponcfg::RocketMode, usize)>>,
    /// projectile fire modes of the vehicles' guns by weapon config (lower case,
    /// "mp.vehicle.cfg_m1a3_seat0"): the mode and its weapon entry
    pub vehicle_rockets: std::collections::HashMap<String, Vec<(crate::weaponcfg::RocketMode, usize)>>,
    /// tactical grenades by loadout item (lowercase cfg)
    pub tactical: std::collections::HashMap<String, crate::weaponcfg::TacticalCfg>,
}

/// Explosion effect of a tactical grenade (NOT from data: the explosion template of these
/// projectiles is not found in the content; chosen by name from KFX_Pyrotechnics /
/// KFX_Electrical).
/// Damage type whose surface particles (Imp_<surface>_TankShell) are added under rocket and C4
/// explosions (NOT from data)
const DEBRIS_DMG: &str = "GCDmgType_EXP_Hellfire";

pub fn tactical_explosion_fx(kind: u8) -> &'static str {
    match kind {
        1 => "KFX_Pyrotechnics.FX_Grenades.FlashBang_Conc",
        2 => "KFX_Electrical.EMP.EMP_Explosion_Impact",
        _ => "KFX_Pyrotechnics.FX_C4.FX_C4_EXP",
    }
}

impl CombatData {
    /// Weapon index for a loadout (DefaultLoadouts order).
    pub fn weapon_for_loadout(&self, l: usize) -> usize {
        self.loadouts.get(l).and_then(|lo| self.weapons.iter().position(|w| w.cfg.eq_ignore_ascii_case(&lo.primary))).unwrap_or(0)
    }
}

/// Who did it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Actor {
    Player,
    Bot(Entity),
    World,
}

/// A soldier other than the local player (bots.rs).
#[derive(Component, Debug, Clone)]
pub struct Soldier {
    pub team: u8,
    pub name: String,
    pub health: f32,
    /// feet position, Bevy metres
    pub feet: Vec3,
    pub crouched: bool,
    pub alive: bool,
    pub since_damage: f32,
    pub kills: i32,
    pub deaths: i32,
    pub streak: i32,
    pub last_attacker: Option<Actor>,
    /// flash bang: seconds left blinded (no shooting)
    pub blinded: f32,
    /// last hit taken: (from, UE units; explosive) for the ragdoll's push
    pub last_hit: Option<(Vec3, bool)>,
}

impl Soldier {
    /// Collision cylinder centre (UE units).
    pub fn center_ue(&self) -> Vec3 {
        let half = if self.crouched { 32.5 } else { CYL_HALF };
        Vec3::new(self.feet.x, self.feet.z, self.feet.y) * 100.0 + Vec3::Z * half
    }
}

/// GCUIObject_WeaponCrosshair HitEnemyFadeTime of PC_U_PlayerHudScreen's WeaponCrosshair.
pub const HIT_FADE: f32 = 0.15;

#[derive(Resource, Debug)]
pub struct PlayerCombat {
    pub weapon: usize,
    pub clip: i32,
    pub reserve: i32,
    pub grenades: i32,
    /// tactical grenades left (loadout item 2), the running throw is one, C4 placed
    pub tactical: i32,
    pub tac_throw: bool,
    /// flash bang on the player: seconds of white-out left
    pub blinded: f32,
    /// the killing hit was an explosion (ragdoll push)
    pub death_explosive: bool,
    pub fire_cooldown: f32,
    pub reload_left: f32,
    pub aiming: bool,
    pub ads_blend: f32,
    pub health: f32,
    pub since_damage: f32,
    pub dead_for: f32,
    pub spread: f32,
    /// view kick still to apply (UE rotation units) and recovery to undo
    pub kick: Vec2,
    pub kick_total: Vec2,
    pub kick_time: f32,
    /// grenade: seconds since G was pressed
    pub throwing: Option<f32>,
    pub thrown: bool,
    pub kills: i32,
    pub deaths: i32,
    pub streak: i32,
    pub spawn_serial: u32,
    pub last_attacker: Option<Actor>,
    pub flak_jacket: bool,
    /// thermal goggles bought this life: seconds left (MaxThermalDuration), switched on
    pub goggles_left: f32,
    pub goggles_on: bool,
    pub goggles_bought: bool,
    pub health_max: f32,
    /// loadout primary and its stored ammo while another weapon is out
    pub primary: usize,
    pub primary_ammo: (i32, i32),
    /// bought launcher (weapon index) and its ammo
    pub special: Option<usize>,
    pub special_ammo: (i32, i32),
    /// loadout sidearm (CFG_PST_*, key two) and its stored ammo
    pub sidearm: Option<usize>,
    pub sidearm_ammo: (i32, i32),
    /// hit marker / damage direction for the HUD
    pub hit_marker: f32,
    /// killer name, own fault (suicide / world), where the deadly hit came from (UE)
    pub killer: Option<(String, bool, Vec3)>,
    /// seconds since this life started (test hooks)
    pub alive_for: f32,
    /// C4 detonator out: the weapon to return to, seconds since the detonation (back after
    /// the Fire1 sequence), seconds R is held (quick detonation)
    pub pre_det: Option<usize>,
    pub det_fired: Option<f32>,
    pub reload_hold: f32,
}

impl Default for PlayerCombat {
    fn default() -> Self {
        PlayerCombat {
            weapon: 0,
            clip: 0,
            reserve: 0,
            grenades: 0,
            tactical: 0,
            tac_throw: false,
            blinded: 0.0,
            death_explosive: false,
            fire_cooldown: 0.0,
            reload_left: 0.0,
            aiming: false,
            ads_blend: 0.0,
            health: HEALTH_MAX,
            since_damage: 99.0,
            dead_for: 0.0,
            spread: 0.0,
            pre_det: None,
            det_fired: None,
            reload_hold: 0.0,
            kick: Vec2::ZERO,
            kick_total: Vec2::ZERO,
            kick_time: 0.0,
            throwing: None,
            thrown: false,
            kills: 0,
            deaths: 0,
            streak: 0,
            spawn_serial: 0,
            last_attacker: None,
            flak_jacket: false,
            goggles_left: 0.0,
            goggles_on: false,
            goggles_bought: false,
            health_max: HEALTH_MAX,
            primary: 0,
            primary_ammo: (0, 0),
            special: None,
            alive_for: 0.0,
            special_ammo: (0, 0),
            sidearm: None,
            sidearm_ammo: (0, 0),
            hit_marker: 0.0,
            killer: None,
        }
    }
}

pub struct Grenade {
    /// rocket: weapon index (straight flight, explodes on contact)
    pub rocket: Option<usize>,
    /// UE units
    pub pos: Vec3,
    pub vel: Vec3,
    pub fuse: f32,
    pub owner: Actor,
    pub team: u8,
    pub resting: bool,
    /// flight effect (ProjFlightTemplate) while in the air
    pub trail: Option<(usize, f64)>,
    /// tactical grenade (flash bang / EMP / C4) instead of the frag
    pub tac: Option<crate::weaponcfg::TacticalCfg>,
    /// airstrike bomb: index into CombatData::strikes and the target point (UE); bursts at
    /// PayloadReleaseDistance from it
    pub payload: Option<(usize, Vec3)>,
}

/// A bomblet still to be released (sensor fused: spread over StrikeDuration).
#[derive(Clone, Debug)]
pub struct SubShot {
    pub t: f32,
    pub from: Vec3,
    pub center: Vec3,
    pub strike: usize,
    pub owner: Actor,
    pub team: u8,
}

/// Projectile meshes (ProjectileMeshComponent: GCProj_ATW_ATRL_C SKW_PRJ_ATMissile01; frag
/// SKW_EXP_Frag_3P_static), a pool of hidden instances per kind (0 rocket, 1 grenade).
#[derive(Component)]
pub struct ProjVis {
    pub kind: u8,
    pub slot: usize,
}

#[derive(Resource, Default)]
pub struct Grenades {
    pub list: Vec<Grenade>,
    /// explosions this frame (UE position, GCExplosionLight_* class or "") for effects
    pub exploded: Vec<(Vec3, &'static str)>,
    /// EMP blasts this frame: (UE position, EMPDamageRadius, EMPBlastDuration) for vehicles.rs /
    /// drone.rs
    pub emp: Vec<(Vec3, f32, f32)>,
    /// bomblets waiting for their release
    pub pending: Vec<SubShot>,
    pub seed: u32,
    test_boom: f32,
    test_rocket: f32,
    /// homing rockets: per `list` entry (same index) the locked vehicle and the flight time
    pub seek: Vec<Option<Seek>>,
}

/// A rocket fired with a lock (GCProj_Rocket TargetActor): the vehicle it seeks.
#[derive(Clone, Copy, Debug)]
pub struct Seek {
    pub target: Entity,
    pub age: f32,
}

/// The held launcher's lock-on (GCLockOnTargeter): the vehicle in the box, seconds on it,
/// locked after LockTime, seconds out of the box while locked (LockGraceTime).
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct LockOn {
    pub target: Option<Entity>,
    pub t: f32,
    pub locked: bool,
    pub grace: f32,
}

/// A flying tracer's light (dynlights.rs): start (UE), direction, speed (UE/s), life, age, colour.
#[derive(Clone, Copy, Debug)]
pub struct TracerLight {
    pub from: Vec3,
    pub dir: Vec3,
    pub speed: f32,
    pub life: f32,
    pub age: f32,
    pub color: [f32; 3],
}

/// Tracers in flight (player, bots, vehicle guns) for their lights; dynlights.rs ages them.
pub static TRACER_LIGHTS: std::sync::Mutex<Vec<TracerLight>> = std::sync::Mutex::new(Vec::new());
static TRACER_SEED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x2545_f491);

/// Tracer colour per shot: the data's TracerColor (yellow), red or green at random.
/// NOT from data - user request 2026-10-21 ("Instead of only Yellow make them also Red and
/// Green and randomise while shooting"); red / green keep the data colour's strongest channel.
pub fn tracer_color(data: [f32; 3]) -> [f32; 3] {
    use std::sync::atomic::Ordering;
    let mut s = TRACER_SEED.load(Ordering::Relaxed);
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    TRACER_SEED.store(s, Ordering::Relaxed);
    let m = data.iter().cloned().fold(0.0f32, f32::max).max(1e-3);
    match (s >> 8) % 3 {
        0 => data,
        1 => [m, m * 0.06, m * 0.04],
        _ => [m * 0.1, m, m * 0.12],
    }
}

/// Fire a tracer effect (WeaponEffectTracer instance parameters TracerSize / TracerColor /
/// TracerSpeed / TracerOffset) for `life` seconds and give it a light while it flies.
/// The view camera (UE units), set every frame by dynlights.rs (tracer clipping).
static CAMERA_UE: std::sync::Mutex<Option<Vec3>> = std::sync::Mutex::new(None);

pub fn set_camera_ue(c: Option<Vec3>) {
    if let Ok(mut g) = CAMERA_UE.lock() {
        *g = c;
    }
}

/// Tracers keep out of TRACER_CAM_CLEAR around the camera (UE units; NOT from data): the
/// tracer's sprite right at the camera filled the whole screen with its colour for a frame -
/// bot tracers ending at the player's head, the player's own leaving the 1P muzzle ("glitching
/// coloured screen", user 2026-10-22 20:48 / 2026-10-23 00:30).
const TRACER_CAM_CLEAR: f32 = 200.0;

pub fn fire_tracer(fx: &mut crate::particles::Particles, tc: &crate::weaponcfg::TracerCfg, at: Vec3, rot: Quat, dir: Vec3, life: f32) {
    let (mut at, mut life) = (at, life);
    if let (Some(cam), true) = (CAMERA_UE.lock().ok().and_then(|g| *g), tc.speed > 1.0) {
        let len = tc.speed * life;
        let along = (cam - at).dot(dir);
        let closest = (at + dir * along.clamp(0.0, len)).distance(cam);
        if closest < TRACER_CAM_CLEAR {
            // the stretch of the path inside the clear sphere
            let half = (TRACER_CAM_CLEAR * TRACER_CAM_CLEAR - (at + dir * along - cam).length_squared()).max(0.0).sqrt();
            let (enter, exit) = (along - half, along + half);
            if enter > TRACER_CAM_CLEAR {
                // ends before it reaches the camera
                life = (enter - TRACER_CAM_CLEAR * 0.5).max(0.0) / tc.speed;
            } else if exit < len - TRACER_CAM_CLEAR {
                // starts at / near the camera: from where it leaves the sphere
                at += dir * exit.max(0.0);
                life = (len - exit.max(0.0)) / tc.speed;
            } else {
                return;
            }
            if life <= 0.01 {
                return;
            }
        }
    }
    let color = tracer_color(tc.color);
    // user request 2026-10-21 ("looks like star wars"): the tracer's glow 20 % weaker at night,
    // 40 % weaker by day (NOT from data); the light in dynlights.rs gets the same scale
    let glow = 0.8 * (0.8 + (0.6 - 0.8) * crate::weather::daylight().clamp(0.0, 1.0));
    let params = [("TracerSize", tc.size), ("TracerColor", color.map(|c| c * glow)), ("TracerSpeed", [tc.speed, 0.0, 0.0]), ("TracerOffset", tc.offset)];
    fx.fire_timed(&tc.template, at, rot, &params, life);
    if let Ok(mut l) = TRACER_LIGHTS.lock() {
        // bounded (a full-auto fight of many bots); the oldest go first
        if l.len() >= 256 {
            l.remove(0);
        }
        l.push(TracerLight { from: at, dir, speed: tc.speed, life, age: 0.0, color });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Player,
    Bot(Entity),
    /// the player's ground assault drone (drone.rs), deployed or parked
    Drone,
    /// a bot's drone (botdrones.rs)
    BotDrone(Entity),
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub target: Target,
    pub amount: f32,
    pub attacker: Actor,
    pub attacker_team: u8,
    pub headshot: bool,
    pub explosive: bool,
    /// UE position of the attacker (damage direction)
    pub from: Vec3,
    /// weapon UIIcon for the kill crawl
    pub icon: Option<String>,
    /// damage type (GCDmgType_*): a player in a vehicle passes the hit on to the vehicle,
    /// which takes the damage grid's value for its armour (vehicles.rs)
    pub dmg_type: Option<String>,
}

/// Hits on the player while in a vehicle, for vehicles.rs (apply_damage forwards them).
#[derive(Resource, Default)]
pub struct VehicleHits(pub Vec<Hit>);

#[derive(Resource, Default)]
pub struct DamageQueue(pub Vec<Hit>);

/// Recent kills for the HUD kill crawl: (killer, killer team, victim, victim team, weapon icon,
/// seconds left).
#[derive(Resource, Default)]
pub struct KillFeed(pub Vec<(String, u8, String, u8, Option<String>, f32)>);

/// Physical material of a hit on a soldier cylinder (DamageGridMP columns).
fn body_region(center: Vec3, hit: Vec3) -> (&'static str, bool) {
    let head = center + Vec3::Z * HEAD_OFFSET;
    if hit.distance(head) <= HEAD_RADIUS * 1.5 || hit.z >= head.z - HEAD_RADIUS {
        return ("PMA_Head", true);
    }
    if hit.z < center.z - 10.0 {
        return ("PMA_Limbs", false);
    }
    ("PMA_Soldier_Chest", false)
}

/// Ray vs vertical cylinder (UE units): distance along the unit direction.
fn ray_cylinder(o: Vec3, d: Vec3, c: Vec3, r: f32, half: f32) -> Option<f32> {
    let (ox, oy) = (o.x - c.x, o.y - c.y);
    let a = d.x * d.x + d.y * d.y;
    let mut best = None;
    if a > 1e-9 {
        let b = 2.0 * (ox * d.x + oy * d.y);
        let cc = ox * ox + oy * oy - r * r;
        let disc = b * b - 4.0 * a * cc;
        if disc >= 0.0 {
            let s = disc.sqrt();
            for t in [(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)] {
                if t >= 0.0 {
                    let z = o.z + d.z * t;
                    if (z - c.z).abs() <= half {
                        best = Some(best.map_or(t, |b: f32| b.min(t)));
                        break;
                    }
                }
            }
        }
    }
    // caps
    if d.z.abs() > 1e-6 {
        for zc in [c.z - half, c.z + half] {
            let t = (zc - o.z) / d.z;
            if t >= 0.0 {
                let p = o + d * t;
                if (p.x - c.x).powi(2) + (p.y - c.y).powi(2) <= r * r {
                    best = Some(best.map_or(t, |b: f32| b.min(t)));
                }
            }
        }
    }
    best
}

/// What a shot hits first.
pub enum ShotHit {
    Nothing,
    /// distance, normal (UE), physical material id of the triangle (impacts.rs)
    World(f32, Vec3, u16),
    Soldier(Entity, f32, Vec3),
    Player(f32, Vec3),
}

/// Trace a shot (UE units) against the level and the soldiers / the player.
#[allow(clippy::too_many_arguments)]
pub fn trace_shot(col: &CollisionWorld, soldiers: &[(Entity, Vec3, u8, bool)], player: Option<(Vec3, bool)>, o: Vec3, d: Vec3, range: f32, ignore: Option<Entity>, ignore_player: bool, buf: &mut Vec<u32>) -> ShotHit {
    let ob = Vec3::new(o.x, o.z, o.y) * 0.01;
    let db = Vec3::new(d.x, d.z, d.y);
    let world = col.raycast_hit(ob, db, range * 0.01, buf, crate::player::COL_TRACE).map(|(t, n, tri)| (t * 100.0, Vec3::new(n.x, n.z, n.y), col.phys_of(tri)));
    let mut best = ShotHit::Nothing;
    let mut best_t = world.map(|w| w.0).unwrap_or(range);
    if let Some((t, n, ph)) = world {
        best = ShotHit::World(t, n, ph);
    }
    // vehicles stop the bullet (no damage to the vehicle, user 2026-10-21): a world hit on the
    // hull box (vehicles.rs shot_block; physical material 0 = default impact)
    if let Some((t, n)) = crate::vehicles::shot_block(o, d, best_t) {
        if t < best_t {
            best_t = t;
            best = ShotHit::World(t, n, 0);
        }
    }
    for &(e, c, _, alive) in soldiers {
        if !alive || Some(e) == ignore {
            continue;
        }
        if let Some(t) = ray_cylinder(o, d, c, CYL_RADIUS, CYL_HALF) {
            if t < best_t {
                best_t = t;
                best = ShotHit::Soldier(e, t, o + d * t);
            }
        }
    }
    if let Some((c, alive)) = player {
        if alive && !ignore_player {
            if let Some(t) = ray_cylinder(o, d, c, CYL_RADIUS, CYL_HALF) {
                if t < best_t {
                    best = ShotHit::Player(t, o + d * t);
                }
            }
        }
    }
    best
}

/// Damage of an instant hit on a soldier cylinder.
pub fn bullet_damage(grid: &DamageGrid, w: &WeaponCfg, center: Vec3, hit: Vec3, dist: f32) -> (f32, bool) {
    let (mat, head) = body_region(center, hit);
    (grid.damage(&w.dmg_type, mat, dist, w.full_range, w.range), head)
}

fn rand01(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed as f32) / (u32::MAX as f32)
}

/// Unit view direction with a random cone deviation (degrees).
pub fn spread_dir(d: Vec3, deg: f32, seed: &mut u32) -> Vec3 {
    if deg <= 0.0 {
        return d;
    }
    let a = rand01(seed) * std::f32::consts::TAU;
    let r = rand01(seed).sqrt() * deg.to_radians();
    let up = if d.z.abs() < 0.99 { Vec3::Z } else { Vec3::X };
    let x = d.cross(up).normalize();
    let y = x.cross(d).normalize();
    (d + (x * a.cos() + y * a.sin()) * r.tan()).normalize()
}

#[allow(clippy::too_many_arguments)]
/// Lock-on of the held launcher (GCLockOnTargeter, LockOnConfigInfo of the weapon; the targeter
/// itself is native and not decoded, the rules read the config values): while zoomed
/// (bAllowUnZoomedLock false) a vehicle (SupportedPawnClasses GCVehicle) whose box centre is
/// inside BigRectangle, within MinLockDist..MaxLockDist and in sight (bSkipLineOfSightCheck
/// false) becomes the target; after LockTime on it the lock holds; a locked target outside the
/// box keeps the lock for LockGraceTime while within LockGraceMaxAngle of the view.
/// NOT from data: the box centre as the test point, the target closest to the screen centre,
/// a pending (not yet locked) target that leaves the box starts over, empty vehicles count
/// (no team test: the offline vehicles have no driver), zoomed = ADS blend >= 0.95.
#[allow(clippy::too_many_arguments)]
fn player_lock_on(
    time: Res<Time>,
    mut flow: ResMut<MatchFlow>,
    data: Option<Res<CombatData>>,
    pc: Res<PlayerCombat>,
    pawns: Query<&Pawn>,
    (vols, col): (Res<crate::vehicles::VehicleVolumes>, Res<CollisionWorld>),
    cams: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    mut lock: ResMut<LockOn>,
    mut buf: Local<Vec<u32>>,
) {
    flow.hud.weapon_lock = None;
    let lo = data.as_ref().and_then(|d| d.weapons.get(pc.weapon)).and_then(|w| w.lock_on.clone());
    let p = pawns.single().ok().filter(|p| !p.dead && !p.in_vehicle && !p.remote);
    let zoomed = pc.aiming && pc.ads_blend >= 0.95;
    let (Some(lo), Some(p), true) = (lo, p, flow.phase == Phase::Playing && !flow.paused) else {
        *lock = LockOn::default();
        return;
    };
    if !lo.allow_unzoomed && !zoomed {
        *lock = LockOn::default();
        return;
    }
    let Some((cam, gt)) = cams.iter().find(|(c, _)| c.is_active) else { return };
    let dt = time.delta_secs().min(0.1);
    let vp = cam.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
    let k = Vec2::new(1280.0 / vp.x.max(1.0), 720.0 / vp.y.max(1.0));
    // screen point (1280x720 units from the centre) of a UE position
    let screen = |ue: Vec3| cam.world_to_viewport(gt, Vec3::new(ue.x, ue.z, ue.y) * 0.01).ok().map(|s| s * k - Vec2::new(640.0, 360.0));
    let eye = p.eye_ue();
    let view = p.dir_ue();
    let centre = |v: &crate::vehicles::VehVol| v.pos + v.rot * ((v.min + v.max) * 0.5);
    let in_sight = |v: &crate::vehicles::VehVol, buf: &mut Vec<u32>| {
        if lo.skip_los {
            return true;
        }
        let c = centre(v);
        let d = (c - eye).normalize_or_zero();
        // the first vehicle box along the line must be this one, and the level must not be
        // nearer than its box
        let Some((e, tv)) = vols.ray(eye, d, eye.distance(c), true) else { return false };
        if e != v.e {
            return false;
        }
        let ob = Vec3::new(eye.x, eye.z, eye.y) * 0.01;
        let db = Vec3::new(d.x, d.z, d.y);
        col.raycast(ob, db, tv * 0.01, buf).is_none()
    };
    let mut best: Option<(Entity, f32)> = None;
    let mut cur_in_box = false;
    for v in vols.0.iter().filter(|v| !v.player_inside) {
        let c = centre(v);
        let dist = eye.distance(c);
        if dist < lo.min_dist || (lo.max_dist > 0.0 && dist > lo.max_dist) || (c - eye).dot(view) <= 0.0 {
            continue;
        }
        let Some(sp) = screen(c) else { continue };
        let (a, b) = lo.big;
        if sp.x < a[0] || sp.x > b[0] || sp.y < a[1] || sp.y > b[1] || !in_sight(v, &mut buf) {
            continue;
        }
        if Some(v.e) == lock.target {
            cur_in_box = true;
        }
        let r = sp.length();
        if best.is_none_or(|x| r < x.1) {
            best = Some((v.e, r));
        }
    }
    match lock.target {
        Some(_) if cur_in_box => {
            lock.grace = 0.0;
            lock.t += dt;
            if lock.t >= lo.lock_time {
                lock.locked = true;
            }
        }
        Some(t) if lock.locked => {
            // LockGraceTime / LockGraceMaxAngle
            lock.grace += dt;
            let keep = vols.0.iter().find(|v| v.e == t).is_some_and(|v| {
                let to = (centre(v) - eye).normalize_or_zero();
                to.dot(view) >= lo.grace_angle.to_radians().cos() && (lo.max_dist <= 0.0 || eye.distance(centre(v)) <= lo.max_dist)
            });
            if !keep || lock.grace > lo.grace_time {
                *lock = LockOn::default();
            }
        }
        _ => {
            *lock = LockOn { target: best.map(|b| b.0), ..Default::default() };
        }
    }
    let target = lock.target.and_then(|t| vols.0.iter().find(|v| v.e == t)).and_then(|v| screen(centre(v))).map(|sp| {
        let (a, b) = lo.small_mat;
        ([640.0 + sp.x + a[0], 360.0 + sp.y + a[1]], [640.0 + sp.x + b[0], 360.0 + sp.y + b[1]], (lock.t / lo.lock_time.max(1e-3)).min(1.0), lock.locked)
    });
    flow.hud.weapon_lock = Some(crate::hud::WeaponLockHud { big: lo.big, target });
    if std::env::var_os("HB_LOCKDBG").is_some() {
        info!("lock-on {:?} t {:.2} locked {} grace {:.2}", lock.target, lock.t, lock.locked, lock.grace);
    }
}

fn player_weapon(
    time: Res<Time>,
    _mouse: Res<ButtonInput<MouseButton>>,
    (_keys, _wheel): (Res<ButtonInput<KeyCode>>, Res<bevy::input::mouse::AccumulatedMouseScroll>),
    mut flow: ResMut<MatchFlow>,
    data: Option<Res<CombatData>>,
    vm: Option<ResMut<ViewModel>>,
    mut pc: ResMut<PlayerCombat>,
    mut pawns: Query<&mut Pawn>,
    soldiers: Query<(Entity, &Soldier)>,
    col: Res<CollisionWorld>,
    mut queue: ResMut<DamageQueue>,
    (mut grenades, mut wsfx, mut fx, mut sfx_q, mut drone, impacts): (ResMut<Grenades>, ResMut<crate::sfx::WeaponSfx>, ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>, ResMut<crate::drone::Drone>, Option<Res<crate::impacts::Impacts>>),
    (mut strike, mut cheats, mut lock): (ResMut<crate::airstrike::Airstrike>, ResMut<Cheats>, ResMut<LockOn>),
    mut locals: Local<(Vec<u32>, u32, u32, bool, u32)>,
    mut buy_test: Local<u32>,
) {
    let _perf = crate::perf::scope("player_weapon");
    let (buf, seed, serial, was_aiming, shots) = &mut *locals;
    if *seed == 0 {
        *seed = 0x9e37_79b9;
    }
    let Some(data) = data else { return };
    let Some(mut vm) = vm else { return };
    if flow.phase != Phase::Playing {
        if flow.phase == Phase::Front {
            *pc = PlayerCombat::default();
            grenades.list.clear();
            grenades.seek.clear();
            // MatchFlow (and its spawn_serial) starts at 0 again in the next match: the first
            // spawn there had the same serial as the last one here and got no loadout (empty
            // guns after a match ended without a death, user 2026-10-19)
            *serial = u32::MAX;
        }
        return;
    }
    let Ok(mut p) = pawns.single_mut() else { return };
    let dt = time.delta_secs().min(0.1);
    // new spawn: loadout weapon and ammo
    if flow.spawn_serial != *serial {
        *serial = flow.spawn_serial;
        let w = data.weapon_for_loadout(flow.loadout);
        let cfg = &data.weapons[w.min(data.weapons.len().saturating_sub(1))];
        let keep_score = (pc.kills, pc.deaths, pc.streak);
        let tac_ammo = data.loadouts.get(flow.loadout).and_then(|l| l.items.get(2)).and_then(|i| data.tactical.get(&i.to_ascii_lowercase())).map(|t| t.ammo).unwrap_or(0);
        *pc = PlayerCombat { weapon: w, primary: w, clip: cfg.clip, reserve: cfg.total, grenades: data.grenade.ammo, tactical: tac_ammo, ..Default::default() };
        (pc.kills, pc.deaths, pc.streak) = keep_score;
        pc.sidearm = data.loadouts.get(flow.loadout).and_then(|l| l.items.first()).filter(|i| i.to_ascii_lowercase().starts_with("cfg_pst_")).and_then(|i| data.weapons.iter().position(|w| w.cfg.eq_ignore_ascii_case(i)));
        if let Some(s) = pc.sidearm {
            pc.sidearm_ammo = (data.weapons[s].clip, data.weapons[s].total);
        }
        vm.active = w;
        vm.lowered = false;
        vm.throw_t = None;
        vm.play(WeaponAnim::Draw);
        p.dead = false;
    }
    let input = !flow.paused && !p.dead && !p.remote && !p.in_vehicle;
    flow.hud.c4_hint = false;
    flow.hud.uav_left = (flow.hud.uav_left - dt).max(0.0);
    let mut fx_sound: Vec<String> = Vec::new();
    // death: wait, then back to the spawn selection
    if p.dead {
        GOGGLES.store(false, std::sync::atomic::Ordering::Relaxed);
        pc.dead_for += dt;
        vm.lowered = true;
        flow.hud.death = Some((pc.killer.clone().map(|k| (k.0, k.1)), (data.spawn_delay - pc.dead_for).ceil().max(0.0) as i32));
        if pc.dead_for >= data.spawn_delay && !flow.respawn {
            flow.respawn = true;
        }
        update_hud(&mut flow, &pc, &data);
        return;
    }
    flow.hud.death = None;
    // HB_SUICIDE: die 3 s after spawning (kill cam tests)
    // pause menu "Selbstmord" (menu.rs ClassSuicide)
    if std::mem::take(&mut flow.suicide) || std::env::var_os("HB_SUICIDE").is_some() && pc.alive_for > 3.0 && pc.alive_for - dt <= 3.0 {
        let at = p.eye_ue();
        queue.0.push(Hit { target: Target::Player, amount: 10000.0, attacker: Actor::World, attacker_team: 2, headshot: false, explosive: true, from: at, icon: None, dmg_type: None });
    }
    // thermal goggles: the time runs from the purchase (FL2PlayerControllerMP
    // Thermal_Goggles_EndTime), switched on or not
    if pc.goggles_left > 0.0 {
        pc.goggles_left -= dt;
        if pc.goggles_left <= 0.0 {
            pc.goggles_left = 0.0;
            if pc.goggles_on && !data.goggles.2.is_empty() {
                sfx_q.play(&data.goggles.2, None);
            }
            pc.goggles_on = false;
        }
    }
    GOGGLES.store(pc.goggles_on && !p.dead && !p.remote, std::sync::atomic::Ordering::Relaxed);
    // health regeneration
    pc.since_damage += dt;
    if pc.since_damage >= REGEN_DELAY && pc.health < pc.health_max {
        pc.health = (pc.health + REGEN_RATE * dt).min(pc.health_max);
    }
    // keys 1 / 4 / 5 (GCInput BindingsFoot: one EKBC_Weapon1, four EKBC_SpecialWeapon1,
    // five EKBC_SpecialWeapon2 = the loadout's 4th / 5th inventory group): buy with battle
    // points (GCBattlePoints SpecialAbilityCosts) or switch
    let mut switch_to: Option<usize> = None;
    // HB_BUYTEST: press 4 and then 5 once, shortly after spawning (HUD checks)
    let mut test_key = None;
    if std::env::var_os("HB_BUYTEST").is_some() {
        *buy_test += 1;
        test_key = match *buy_test {
            5 => Some("EKBC_SpecialWeapon1"),
            12 => Some("EKBC_SpecialWeapon2"),
            _ => None,
        };
        if test_key.is_some() {
            info!("buy test: {:?} (kp {}, input {input}, paused {}, dead {}, throwing {})", test_key, flow.hud.kp, flow.paused, p.dead, pc.throwing.is_some());
        }
    }
    if input && pc.throwing.is_none() {
        if hit(Set::Foot, "EKBC_Weapon1") && pc.weapon != pc.primary {
            switch_to = Some(pc.primary);
        }
        if hit(Set::Foot, "EKBC_Weapon2") && pc.sidearm.is_some() && Some(pc.weapon) != pc.sidearm {
            switch_to = pc.sidearm;
        }
        // mouse wheel (EKBC_NextWeapon / EKBC_PrevWeapon): primary -> sidearm -> bought launcher
        let (next, prev) = (hit(Set::Foot, "EKBC_NextWeapon"), hit(Set::Foot, "EKBC_PrevWeapon"));
        if next != prev {
            let mut ring = vec![pc.primary];
            ring.extend(pc.sidearm);
            if let Some(s) = pc.special.filter(|&s| s == pc.weapon || pc.special_ammo.0 + pc.special_ammo.1 > 0) {
                ring.push(s);
            }
            if ring.len() > 1 {
                let cur = ring.iter().position(|&w| w == pc.weapon).unwrap_or(0);
                let n = ring.len();
                let k = if prev { (cur + n - 1) % n } else { (cur + 1) % n };
                switch_to = Some(ring[k]);
            }
        }
        // the slot keys 4 / 5 (loadout items 3 / 4) and the debug menu's free killstreaks
        let mut wanted: Vec<(String, bool)> = Vec::new();
        for (key, slot) in [("EKBC_SpecialWeapon1", 3usize), ("EKBC_SpecialWeapon2", 4usize)] {
            if !hit(Set::Foot, key) && test_key != Some(key) {
                continue;
            }
            // the end-of-match nuke waits for the player's call (nuke.rs / objectives.rs Endgame)
            if slot == 3 && crate::nuke::offered_to_player() {
                crate::nuke::request();
                continue;
            }
            if let Some(item) = data.loadouts.get(flow.loadout).and_then(|l| l.items.get(slot)).cloned() {
                wanted.push((item, false));
            }
        }
        wanted.extend(std::mem::take(&mut cheats.grant).into_iter().map(|i| (i, true)));
        // HB_GRANT=<cfg>[,<cfg>]: those killstreaks for free 2 s after the first spawn (tests)
        if let Ok(v) = std::env::var("HB_GRANT") {
            static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if pc.alive_for > 2.0 && !DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                wanted.extend(v.split(',').map(|x| (x.trim().to_ascii_lowercase(), true)));
            }
        }
        for (item, free) in wanted {
            let lower = item.to_ascii_lowercase();
            if lower == crate::nuke::ITEM.to_ascii_lowercase() { crate::nuke::request(); continue; }
            let (cost, amount, stinger) = data.costs.get(&lower).cloned().unwrap_or((0, 1, None));
            let ok = free || flow.hud.kp >= cost;
            let pay = if free { 0 } else { cost };
            if lower == "cfg_eqp_flakjacket" {
                if !pc.flak_jacket && ok {
                    flow.hud.kp -= pay;
                    pc.flak_jacket = true;
                    pc.health_max = HEALTH_MAX * data.flak_scale;
                    pc.health += pc.health_max - HEALTH_MAX;
                    if let Some(c) = stinger {
                        fx_sound.push(c);
                    }
                }
            } else if lower == "cfg_eqp_resupply" {
                // GCWeap_EQP_Resupply BeginSpecial -> FL2PlayerControllerMP.ResupplyAmmo (native, not
                // decoded); UI_Armory Resupply_Text: "Notaufstockung von Munition, Granaten und
                // Spezialsprengstoffen" -> primary, sidearm, frag and tactical grenades full
                // (GCBattlePoints ResupplyReuseCosts (0,0,0,0): no extra cost on reuse)
                if ok {
                    flow.hud.kp -= pay;
                    let full = |wi: usize| data.weapons.get(wi).map(|w| (w.clip, w.total)).unwrap_or((0, 0));
                    pc.primary_ammo = full(pc.primary);
                    if let Some(s) = pc.sidearm {
                        pc.sidearm_ammo = full(s);
                    }
                    if Some(pc.weapon) != pc.special {
                        let (c, t) = full(pc.weapon);
                        pc.clip = c;
                        pc.reserve = t;
                    }
                    pc.grenades = data.grenade.ammo;
                    if let Some(t) = data.loadouts.get(flow.loadout).and_then(|l| l.items.get(2)).and_then(|i| data.tactical.get(&i.to_ascii_lowercase())) {
                        pc.tactical = t.ammo;
                    }
                    if let Some(c) = stinger {
                        fx_sound.push(c);
                    }
                }
            } else if lower == "cfg_eqp_thermal" {
                // GCWeap_EQP_Thermal: once per life (UI_Armory Thermal_Text), MaxThermalDuration
                // seconds; the slot key switches it on / off (ThermalOnSC / ThermalOffSC)
                if pc.goggles_left > 0.0 {
                    pc.goggles_on = !pc.goggles_on;
                    let c = if pc.goggles_on { &data.goggles.1 } else { &data.goggles.2 };
                    if !c.is_empty() {
                        fx_sound.push(c.clone());
                    }
                } else if !pc.goggles_bought && ok {
                    flow.hud.kp -= pay;
                    pc.goggles_bought = true;
                    pc.goggles_left = data.goggles.0;
                    pc.goggles_on = true;
                    if let Some(c) = stinger {
                        fx_sound.push(c);
                    }
                    if !data.goggles.1.is_empty() {
                        fx_sound.push(data.goggles.1.clone());
                    }
                }
            } else if lower == "cfg_eqp_uav" {
                // UAV sweep: the enemies show on the minimap for MaxSweepDuration
                if flow.hud.uav_left <= 0.0 && ok {
                    flow.hud.kp -= pay;
                    flow.hud.uav_left = data.uav_secs;
                    if let Some(c) = stinger {
                        fx_sound.push(c);
                    }
                }
            } else if let Some(dk) = crate::DRONE_KINDS.iter().position(|k| *k == lower) {
                // ground drones (drone.rs: assault, anti tank): bought here, placed with the left
                // button
                if drone.phase == crate::drone::DronePhase::Parked && drone.key == crate::DRONE_KINDS[dk] {
                    // a parked drone: the slot key takes it over again (no cost)
                    drone.reenter = true;
                } else if matches!(drone.phase, crate::drone::DronePhase::Idle | crate::drone::DronePhase::Parked) && drone.request.is_none() && ok {
                    flow.hud.kp -= pay;
                    drone.request = Some(dk);
                    if let Some(c) = stinger {
                        fx_sound.push(c);
                    }
                }
            } else if let Some(k) = data.strikes.iter().position(|s| s.key == lower) {
                // airstrikes: Hellfire, cluster, white phosphorus, sensor fused (airstrike.rs)
                if let Some(h) = Some(&data.strikes[k].cfg).filter(|_| !strike.active && ok) {
                    flow.hud.kp -= pay;
                    strike.start(k, p.eye_ue(), p.yaw / 65536.0 * std::f32::consts::TAU, h.fuel, h.clip, h.orbit);
                    if let Some(c) = stinger {
                        fx_sound.push(c);
                    }
                }
            } else if let Some(wi) = data.weapons.iter().position(|w| w.cfg.eq_ignore_ascii_case(&item)) {
                if pc.special != Some(wi) {
                    if ok {
                        flow.hud.kp -= pay;
                        let clip = data.weapons[wi].clip.min(amount.max(1));
                        pc.special = Some(wi);
                        pc.special_ammo = (clip, (amount - clip).max(0));
                        if let Some(c) = stinger {
                            fx_sound.push(c);
                        }
                        switch_to = Some(wi);
                    }
                } else if pc.weapon != wi && pc.special_ammo.0 + pc.special_ammo.1 > 0 {
                    switch_to = Some(wi);
                }
            } else {
                info!("special item {item} not available here");
            }
        }
    }
    // tactical grenade of the loadout; C4: its detonator weapon (CFG_EXP_C4Detonator)
    let tac_cfg = data.loadouts.get(flow.loadout).and_then(|l| l.items.get(2)).and_then(|i| data.tactical.get(&i.to_ascii_lowercase())).cloned();
    let det_wi = tac_cfg.as_ref().and_then(|t| t.detonator.as_ref()).and_then(|d| data.weapons.iter().position(|w| w.cfg.eq_ignore_ascii_case(d)));
    if crate::nuke::take_draw() && switch_to.is_none() { if let Some(d) = det_wi.filter(|&d| pc.weapon != d) { pc.pre_det = Some(pc.weapon); switch_weapon(&mut pc, &mut vm, d); } }
    if let Some(wi) = switch_to {
        pc.pre_det = None;
        pc.det_fired = None;
        switch_weapon(&mut pc, &mut vm, wi);
    }
    let Some(cfg) = data.weapons.get(pc.weapon).cloned() else { return };
    pc.hit_marker = (pc.hit_marker - dt).max(0.0);
    pc.fire_cooldown = (pc.fire_cooldown - dt).max(0.0);
    // reload
    if pc.reload_left > 0.0 {
        pc.reload_left -= dt;
        if pc.reload_left <= 0.0 {
            let take = (cfg.clip - pc.clip).min(pc.reserve).max(0);
            pc.clip += take;
            pc.reserve -= take;
            pc.reload_left = 0.0;
        }
    }
    let throwing = pc.throwing.is_some();
    let can_act = input && pc.reload_left <= 0.0 && !throwing && vm.anim != WeaponAnim::Draw;
    // HB_TEST_RELOAD=<s>[,dry]: reload once that many seconds after spawning (tests)
    let test_reload = std::env::var("HB_TEST_RELOAD").ok().is_some_and(|v| {
        let d = v.split(',').next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0);
        if pc.alive_for >= d && pc.alive_for - time.delta_secs() < d {
            pc.clip = if v.contains("dry") { 0 } else { cfg.clip / 2 };
            info!("test reload (clip {}, input {input})", pc.clip);
            true
        } else {
            false
        }
    });
    let on_det = det_wi == Some(pc.weapon);
    let c4_out = grenades.list.iter().any(|n| n.owner == Actor::Player && n.tac.as_ref().is_some_and(|t| t.kind == 3));
    let mut detonate = false;
    // loadingtips MPTips Message31: holding reload sets off all placed charges (the hold time
    // is NOT from data)
    pc.reload_hold = if input && key_down(Set::Foot, "EKBC_Reload") { pc.reload_hold + dt } else { 0.0 };
    if c4_out && pc.reload_hold >= C4_HOLD && pc.reload_hold - dt < C4_HOLD {
        detonate = true;
    }
    if input && (hit(Set::Foot, "EKBC_Reload") || test_reload) && !on_det && pc.reload_left <= 0.0 && !throwing && pc.clip < cfg.clip && pc.reserve > 0 {
        pc.reload_left = cfg.reload_time;
        vm.play_reload(pc.clip == 0, (pc.alive_for * 1000.0) as u32);
    }
    // aiming (hold RMB), not while sprinting / reloading
    // HOMEBOTS_TEST_ADS / _FIRE / _GRENADE: automated tests
    let test_ads = std::env::var_os("HOMEBOTS_TEST_ADS").is_some();
    pc.aiming = input && (key_down(Set::Foot, "EKBC_AltFire") || test_ads) && !cfg.disable_zoom && pc.reload_left <= 0.0 && !throwing && !p.sprinting;
    if pc.aiming != *was_aiming {
        *was_aiming = pc.aiming;
        if let Some(si) = &cfg.sound_info {
            wsfx.0.push((si.clone(), if pc.aiming { 2 } else { 3 }, None));
        }
    }
    let (up, down) = (cfg.ads_up.max(1e-3), cfg.ads_down.max(1e-3));
    pc.ads_blend = if pc.aiming { (pc.ads_blend + dt / up).min(1.0) } else { (pc.ads_blend - dt / down).max(0.0) };
    vm.ads = pc.ads_blend;
    let zoom = cfg.scope.as_ref().map(|s| s.fov_zoom).unwrap_or(cfg.fov_zoom);
    let mesh_zoom = cfg.scope.as_ref().map(|s| s.fov_mesh_zoom).unwrap_or(cfg.fov_mesh_zoom);
    // world FOV and 1P mesh FOV change in their own windows of the zoom (WorldFOVStartTime /
    // EndTime, the scope's or weapon's ADSBlendStartTime / EndTime; M110: world 0.3-0.4 s, mesh
    // 0.01-0.4 s). Changed together the sight drifted off the screen centre while zooming in.
    // The angle is interpolated linearly in its window (NOT from data).
    let t_zoom = pc.ads_blend * up;
    let window = |(a, b): (f32, f32)| if b > a + 1e-4 { ((t_zoom - a) / (b - a)).clamp(0.0, 1.0) } else if t_zoom >= a { 1.0 } else { 0.0 };
    // a scope entry's ADSBlendStartTime / EndTime of 0 / 0 (CFG_LMG_SCARMG) leaves the weapon's
    let mesh_win = cfg.scope.as_ref().map(|s| s.ads_blend).filter(|b| b.1 > 0.0).unwrap_or(cfg.mesh_fov_time);
    let (wf, mf) = if pc.ads_blend >= 1.0 { (1.0, 1.0) } else if pc.ads_blend <= 0.0 { (0.0, 0.0) } else { (window(cfg.world_fov_time), window(mesh_win)) };
    let base = crate::settings::vfov();
    p.fov = base + (crate::viewmodel::vfov_for(zoom) - base) * wf;
    vm.mesh_fov = cfg.fov_mesh + (mesh_zoom - cfg.fov_mesh) * mf;
    p.speed_scale = cfg.move_speed * (1.0 + (cfg.ads_move - 1.0) * pc.ads_blend);
    p.no_sprint = pc.aiming || pc.reload_left > 0.0;
    // spread decays to its minimum
    let dev = if p.crouched { &cfg.spread_crouch } else { &cfg.spread_stand };
    pc.spread = (pc.spread - dev.decay * dt).max(dev.min);
    // recoil: kick applied over Duration at Speed, recovered at ViewRecoveryRate
    let rc = if pc.ads_blend > 0.5 { &cfg.recoil_zoom } else { &cfg.recoil_hip };
    if pc.kick_time > 0.0 {
        let k = (dt / pc.kick_time.max(dt)).min(1.0);
        let step = pc.kick * k;
        p.pitch = (p.pitch + step.y).clamp(-16000.0, 16000.0);
        p.yaw += step.x;
        pc.kick -= step;
        pc.kick_time -= dt;
    } else if pc.kick_total.y > 0.0 {
        // degrees per second -> rotation units
        let back = (rc.recovery * 182.044 * dt).min(pc.kick_total.y);
        p.pitch -= back;
        pc.kick_total.y -= back;
    }
    // fire
    let trigger = std::env::var_os("HOMEBOTS_TEST_FIRE").is_some() || if cfg.full_auto { key_down(Set::Foot, "EKBC_Fire") } else { hit(Set::Foot, "EKBC_Fire") };
    // C4 detonator (GCWeap_EXP_C4Detonator): the left button sets off the charges, Fire1 plays;
    // HB_TEST_DETONATE=<s>: press it once that many seconds after spawning (tests)
    let test_det = std::env::var("HB_TEST_DETONATE").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|d| pc.alive_for >= d && pc.alive_for - dt < d);
    let test_det = test_det || crate::nuke::press_det();
    if on_det {
        if c4_out {
            flow.hud.c4_hint = true;
        }
        if can_act && (hit(Set::Foot, "EKBC_Fire") || test_det) && pc.det_fired.is_none() {
            detonate = c4_out;
            pc.det_fired = Some(0.0);
            vm.play(WeaponAnim::Fire);
        }
        // back to the weapon held before once the detonation played (or no charge is out);
        // NOT from data: the original's switch back was not traced
        let back = match pc.det_fired.as_mut() {
            Some(t) => {
                *t += dt;
                *t >= vm.duration(WeaponAnim::Fire)
            }
            None => !c4_out && !throwing && vm.anim != WeaponAnim::Draw,
        };
        if back {
            let to = pc.pre_det.take().unwrap_or(pc.primary);
            pc.det_fired = None;
            switch_weapon(&mut pc, &mut vm, to);
        }
    } else if can_act && trigger && pc.fire_cooldown <= 0.0 && !p.sprinting {
        if pc.clip > 0 {
            if !cheats.inf_ammo {
                pc.clip -= 1;
            }
            pc.fire_cooldown = cfg.fire_interval;
            vm.play(WeaponAnim::Fire);
            if let Some(si) = &cfg.sound_info {
                // automatic fire loops until no shot follows within the fire interval; a
                // semi-automatic shot is a trigger press of its own (shot + release tail): the M9
                // fired fast kept one "loop" alive and its shots went silent
                wsfx.shot("player", si, 0, None, if cfg.full_auto { cfg.fire_interval + 0.06 } else { 0.0 });
            }
            // muzzle flash (View1P / ViewADS Day template) at the 1P muzzle bone
            let mf = if pc.ads_blend > 0.5 { &cfg.muzzle[2] } else { &cfg.muzzle[0] };
            if let (Some(mf), Some(at)) = (mf, crate::viewmodel::muzzle_world(&vm, &p)) {
                let yaw = p.yaw / 65536.0 * std::f32::consts::TAU;
                let pitch = p.pitch / 65536.0 * std::f32::consts::TAU;
                fx.fire(mf, at, Quat::from_rotation_z(yaw) * Quat::from_rotation_y(-pitch));
            }
            let o = p.eye_ue();
            let base = p.dir_ue();
            if let Some(pj) = &cfg.projectile {
                // launcher: the projectile flies from just ahead of the eye
                let pos = o + base * 60.0;
                let trail = pj.flight_fx.as_ref().and_then(|t| fx.fire_tracked(t, pos, dir_quat(base)));
                grenades.list.push(Grenade { rocket: Some(pc.weapon), pos, vel: base * pj.speed, fuse: 14.0, owner: Actor::Player, team: player_team(), resting: false, trail, tac: None, payload: None });
                // fired with a lock: the rocket seeks the locked vehicle (GCProj_Rocket
                // SetTargetActor); the lock is gone with the shot (ClearLockOnAfterEachShot)
                let seek = lock.target.filter(|_| lock.locked && cfg.lock_on.is_some()).map(|target| Seek { target, age: 0.0 });
                let n = grenades.list.len();
                grenades.seek.resize(n - 1, None);
                grenades.seek.push(seek);
                if cfg.lock_on.is_some() {
                    *lock = LockOn::default();
                }
                if pc.clip == 0 && pc.reserve == 0 {
                    // empty launcher: back to the primary
                    pc.special_ammo = (0, 0);
                }
            } else {
            // deviation: half-angle spread / 2 degrees at the hip (the crosshair shows this cone,
            // the bots aim with Spread.Stand.Min / 2), 95 % less while aiming (NOT from data)
            let dev_deg = pc.spread * 0.5 * (1.0 - 0.95 * pc.ads_blend);
            let d = spread_dir(base, dev_deg, seed);
            let list: Vec<(Entity, Vec3, u8, bool)> = soldiers.iter().map(|(e, s)| (e, s.center_ue(), s.team, s.alive)).collect();
            // the trace reaches far beyond WeaponRange (the damage falls off to its far value at
            // WeaponRange and stays there, weaponcfg::DamageGrid): with the range as trace length
            // nothing past 70 m (35 m M9) was hit
            let shot = trace_shot(&col, &list, None, o, d, SHOT_TRACE, None, true, buf);
            // tracer (View1P Day WeaponEffectTracer, every Frequency-th shot) from the muzzle to
            // the impact point; TracerSpeed along its X axis, TracerSize / TracerColor as set
            *shots += 1;
            if let (Some(tc), Some(at)) = (cfg.tracer[0].as_ref(), crate::viewmodel::muzzle_world(&vm, &p)) {
                if *shots % tc.frequency == 0 {
                    let dist = match &shot {
                        ShotHit::Soldier(_, t, _) | ShotHit::World(t, _, _) => *t,
                        _ => SHOT_TRACE,
                    };
                    let end = o + d * dist;
                    let dir = (end - at).normalize_or_zero();
                    fire_tracer(&mut fx, tc, at, dir_quat(dir), dir, end.distance(at) / tc.speed.max(1.0));
                }
            }
            // the bots' drones (botdrones.rs; their boxes stop the bullet like a vehicle's)
            if let Some((de, t)) = crate::botdrones::bullet_hit(o, d, &shot, SHOT_TRACE, player_team()) {
                let c = crate::botdrones::centre(de).unwrap_or(o + d * t);
                let (dmg, _) = bullet_damage(&data.grid, &cfg, c + Vec3::Z * 100.0, o + d * t, t);
                queue.0.push(Hit { target: Target::BotDrone(de), amount: dmg, attacker: Actor::Player, attacker_team: player_team(), headshot: false, explosive: false, from: o, icon: cfg.ui_icon.clone(), dmg_type: Some(cfg.dmg_type.clone()) });
            }
            match shot {
                ShotHit::Soldier(e, t, hit) => {
                    if let Ok((_, s)) = soldiers.get(e) {
                        if s.team != player_team() {
                            let (dmg, head) = bullet_damage(&data.grid, &cfg, s.center_ue(), hit, t);
                            queue.0.push(Hit { target: Target::Bot(e), amount: dmg, attacker: Actor::Player, attacker_team: player_team(), headshot: head, explosive: false, from: o, icon: cfg.ui_icon.clone(), dmg_type: None });
                            
                        }
                    }
                }
                _ => {}
            }
            // impact effect of the hit surface (impacts.rs)
            if let Some(im) = impacts.as_ref() {
                let center = if let ShotHit::Soldier(e, ..) = &shot { soldiers.get(*e).ok().map(|(_, s)| s.center_ue()) } else { None };
                im.shot(&shot, o, d, &cfg.dmg_type, center, &mut fx, &mut sfx_q);
            }
            }
            pc.spread = (pc.spread + dev.fire_add).min(dev.max);
            // view kick (degrees -> rotation units)
            let pitch = rc.pitch_min + (rc.pitch_max - rc.pitch_min) * rand01(seed);
            let yaw = rc.yaw_min + (rc.yaw_max - rc.yaw_min) * rand01(seed);
            let kick = Vec2::new(yaw, pitch) * 182.044 * 0.1;
            pc.kick += kick;
            pc.kick_total += Vec2::new(0.0, kick.y);
            pc.kick_time = rc.duration.max(0.05);
        } else if pc.reserve > 0 && pc.reload_left <= 0.0 {
            pc.reload_left = cfg.reload_time;
            vm.play_reload(pc.clip == 0, (pc.alive_for * 1000.0) as u32);
        }
    }
    // frag grenade: G; thrown after PreThrowAnimationTime, weapon back after ThrowAnimationTime
    let g = &data.grenade;
    // HOMEBOTS_TEST_GRENADE=<s>: throw once that many seconds after spawning (tests)
    pc.alive_for += dt;
    if std::env::var_os("HOMEBOTS_TEST_GRENADE").is_some() && (pc.alive_for / 5.0).floor() != ((pc.alive_for - dt) / 5.0).floor() {
        info!("grenade test: alive {:.1} input {input} paused {} dead {} remote {} grenades {} reload {:.2} throwing {}", pc.alive_for, flow.paused, p.dead, p.remote, pc.grenades, pc.reload_left, throwing);
    }
    let test_grenade = std::env::var("HOMEBOTS_TEST_GRENADE").ok().map(|v| v.parse::<f32>().unwrap_or(0.0)).is_some_and(|d| pc.alive_for >= d) && pc.grenades == g.ammo;
    // keys: G = tactical grenade (loadout item 2; a placed C4 is set off by the next G), F = frag.
    // GCInput binds G -> Grenade1 ("GamepadThrow 0") and F -> Grenade2 ("GamepadThrow 1"); which
    // throwable index 0 / 1 is was not traced - the original throws the tactical one on G (user
    // test 2026-10-18; before HomeBots had it the other way round). The detonator key is NOT
    // from data.
    let test_tac = std::env::var("HOMEBOTS_TEST_TACTICAL").ok().map(|v| v.parse::<f32>().unwrap_or(0.0)).is_some_and(|d| pc.alive_for >= d) && tac_cfg.as_ref().is_some_and(|t| pc.tactical == t.ammo);
    if detonate {
        for n in grenades.list.iter_mut().filter(|n| n.owner == Actor::Player && n.tac.as_ref().is_some_and(|t| t.kind == 3)) {
            n.fuse = 0.0;
        }
        info!("C4 detonated");
    }
    // several charges can be out at once (UI_Armory C4_Text); G throws the next one
    if input && (hit(Set::Foot, TACTICAL_KEY) || test_tac) && pc.tactical > 0 && !throwing && pc.reload_left <= 0.0 && pc.det_fired.is_none() {
        if let Some(t) = &tac_cfg {
            pc.throwing = Some(0.0);
            pc.thrown = false;
            pc.tac_throw = true;
            vm.throw_t = Some(0.0);
            vm.throw_c4 = t.kind == 3;
            vm.throw_release = t.pre_throw;
            info!("tactical throw {} (kind {})", t.cfg, t.kind);
        }
    }
    if input && (hit(Set::Foot, FRAG_KEY) || test_grenade) && pc.grenades > 0 && !throwing && pc.throwing.is_none() && pc.reload_left <= 0.0 {
        pc.throwing = Some(0.0);
        pc.thrown = false;
        pc.tac_throw = false;
        // quick throw on the 1P weapon skeleton (viewmodel.rs): grenade_cook + grenade_throw
        vm.throw_t = Some(0.0);
        vm.throw_c4 = false;
        vm.throw_release = g.pre_throw;
        info!("grenade throw (cook + throw {:.2} s)", vm.throw_len());
    }
    let (pre_throw, throw_anim) = match (&tac_cfg, pc.tac_throw) {
        (Some(t), true) => (t.pre_throw, t.throw_anim),
        _ => (g.pre_throw, g.throw_anim),
    };
    if let Some(t) = pc.throwing.as_mut() {
        *t += dt;
        let t = *t;
        if vm.throw_t.is_some() {
            if t < vm.throw_len().max(pre_throw) {
                vm.throw_t = Some(t);
            } else {
                // throw sequence over: the weapon comes back (ThrowAnimationTime); after a C4
                // the detonator is drawn (user's screenshots of the original)
                vm.throw_t = None;
                let c4 = pc.tac_throw && tac_cfg.as_ref().is_some_and(|t| t.kind == 3);
                match det_wi.filter(|_| c4) {
                    Some(d) if pc.weapon != d => {
                        pc.pre_det = Some(pc.weapon);
                        switch_weapon(&mut pc, &mut vm, d);
                    }
                    _ => vm.play(WeaponAnim::Draw),
                }
            }
        }
        if !pc.thrown && t >= pre_throw {
            pc.thrown = true;
            let d = p.dir_ue();
            let o = p.eye_ue() + d * 30.0;
            let inherit = Vec3::new(p.vel.x, p.vel.z, p.vel.y) * 100.0;
            match (&tac_cfg, pc.tac_throw) {
                (Some(tc), true) => {
                    if !cheats.inf_ammo {
                        pc.tactical -= 1;
                    }
                    info!("tactical released ({})", tc.cfg);
                    let vel = d * tc.speed + Vec3::Z * tc.toss_z + inherit;
                    // C4: no fuse (GrenadeFuseTime 0), set off by the detonator
                    let fuse = if tc.kind == 3 { f32::MAX } else { tc.fuse.max(0.05) };
                    let trail = tc.trail.as_ref().and_then(|t| fx.fire_tracked(t, o, dir_quat(d)));
                    grenades.list.push(Grenade { rocket: None, pos: o, vel, fuse, owner: Actor::Player, team: player_team(), resting: false, trail, tac: Some(tc.clone()), payload: None });
                }
                _ => {
                    if !cheats.inf_ammo {
                        pc.grenades -= 1;
                    }
                    info!("grenade released");
                    let vel = d * g.speed + Vec3::Z * g.toss_z + inherit;
                    grenades.list.push(Grenade { rocket: None, pos: o, vel, fuse: g.fuse, owner: Actor::Player, team: player_team(), resting: false, trail: None, tac: None, payload: None });
                }
            }
        }
        if t >= pre_throw + throw_anim && vm.throw_t.is_none() {
            pc.throwing = None;
        }
    }
    for c in fx_sound {
        sfx_q.play(&c, None);
    }
    // hip crosshair (GCUIObject_WeaponCrosshair, CrosshairInfoUI UnZoomed bCrosses): the dashes'
    // inner ends on the spread cone, half-angle = spread / 2 degrees projected with the view's
    // vertical FOV (calibrated on the user's screenshots: M4 Stand.Min 4.75 -> 30 px, M9 2.25 ->
    // 14 px of 720; the native formula is NOT decoded). Hidden while aiming, sprinting, in a
    // vehicle or throwing.
    let show = cfg.crosses && pc.ads_blend < 0.3 && !p.sprinting && !p.in_vehicle && !p.remote && pc.throwing.is_none() && cfg.projectile.is_none();
    flow.hud.crosshair = show.then(|| {
        let half = (pc.spread * 0.5).to_radians().tan();
        360.0 * half / (p.fov.to_radians() * 0.5).tan()
    });
    update_hud(&mut flow, &pc, &data);
}

/// Put weapon `wi` in hand: the current weapon's ammo is stored, the new one's taken (the C4
/// detonator has none), Draw plays.
fn switch_weapon(pc: &mut PlayerCombat, vm: &mut ViewModel, wi: usize) {
    if pc.weapon == pc.primary {
        pc.primary_ammo = (pc.clip, pc.reserve);
    } else if Some(pc.weapon) == pc.sidearm {
        pc.sidearm_ammo = (pc.clip, pc.reserve);
    } else if Some(pc.weapon) == pc.special {
        pc.special_ammo = (pc.clip, pc.reserve);
    }
    let (c, r) = if wi == pc.primary {
        pc.primary_ammo
    } else if Some(wi) == pc.sidearm {
        pc.sidearm_ammo
    } else if Some(wi) == pc.special {
        pc.special_ammo
    } else {
        (0, 0)
    };
    pc.weapon = wi;
    pc.clip = c;
    pc.reserve = r;
    pc.reload_left = 0.0;
    pc.ads_blend = 0.0;
    vm.active = wi;
    vm.play(WeaponAnim::Draw);
}

fn update_hud(flow: &mut MatchFlow, pc: &PlayerCombat, data: &CombatData) {
    flow.hud.ammo = pc.reserve;
    flow.hud.clip = pc.clip;
    flow.hud.clip_max = data.weapons.get(pc.weapon).map(|w| w.clip).unwrap_or(30);
    flow.hud.grenades[0] = pc.grenades;
    flow.hud.grenades[1] = pc.tactical;
    flow.hud.blinded = pc.blinded;
    flow.hud.health = pc.health;
    flow.hud.hit_marker = pc.hit_marker;
    flow.hud.bullet_icon = data.weapons.get(pc.weapon).and_then(|w| w.bullet_icon.clone().map(|i| (i, w.bullet_icon_size)));
    // special weapon slots (loadout items 4 / 5)
    for (k, slot) in [3usize, 4].into_iter().enumerate() {
        let item = data.loadouts.get(flow.loadout).and_then(|l| l.items.get(slot)).cloned().unwrap_or_default();
        let lower = item.to_ascii_lowercase();
        let icon = data.small_icons.get(&lower).cloned();
        let cost = data.costs.get(&lower).map(|c| c.0).unwrap_or(0);
        let (uses, red, selected) = if lower == "cfg_eqp_flakjacket" {
            (pc.flak_jacket.then_some(1), pc.flak_jacket, false)
        } else if lower == "cfg_eqp_thermal" {
            ((pc.goggles_left > 0.0).then_some(pc.goggles_left.ceil() as i32), false, pc.goggles_on)
        } else {
            let wi = data.weapons.iter().position(|w| w.cfg.eq_ignore_ascii_case(&item));
            let owned = wi.is_some() && pc.special == wi;
            let rounds = if owned && Some(pc.weapon) == wi { pc.clip + pc.reserve } else { pc.special_ammo.0 + pc.special_ammo.1 };
            (owned.then_some(rounds), false, owned && Some(pc.weapon) == wi)
        };
        let afford = uses.is_some() || flow.hud.kp >= cost;
        flow.hud.specials[k] = (icon, uses, red, selected, afford);
    }
}

/// Grenade flight (gravity, bounces with GCProj_Grenade dampening) and explosions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_grenades(time: Res<Time>, flow: Res<MatchFlow>, data: Option<Res<CombatData>>, mut gr: ResMut<Grenades>, col: Res<CollisionWorld>, pawns: Query<&Pawn>, mut soldiers: Query<(Entity, &mut Soldier)>, mut queue: ResMut<DamageQueue>, mut pc: ResMut<PlayerCombat>, (mut sfx, mut fx, impacts): (ResMut<crate::sfx::SfxQueue>, ResMut<crate::particles::Particles>, Option<Res<crate::impacts::Impacts>>), mut buf: Local<Vec<u32>>, (vols, mut vdmg): (Res<crate::vehicles::VehicleVolumes>, ResMut<crate::vehicles::VehicleDamage>)) {
    gr.exploded.clear();
    gr.emp.clear();
    pc.blinded = (pc.blinded - time.delta_secs()).max(0.0);
    if flow.phase != Phase::Playing {
        return;
    }
    let Some(data) = data else { return };
    let g = &data.grenade;
    let dt = time.delta_secs().min(0.1);
    let mut boom = Vec::new();
    let soldier_list: Vec<(Entity, Vec3, u8, bool)> = soldiers.iter().map(|(e, s)| (e, s.center_ue(), s.team, s.alive)).collect();
    // airstrike bomblets due now: toward a point of the strike area (cluster, white phosphorus) or
    // an enemy in it (sensor fused)
    if gr.seed == 0 {
        gr.seed = 0x1357_9bdf;
    }
    let mut due = Vec::new();
    for sub in gr.pending.iter_mut() {
        sub.t -= dt;
        if sub.t <= 0.0 {
            due.push(sub.clone());
        }
    }
    gr.pending.retain(|s| s.t > 0.0);
    for sub in due {
        let Some(st) = data.strikes.get(sub.strike) else { continue };
        let (Some(p), Some(wi)) = (st.cfg.payload.as_ref(), st.sub) else { continue };
        let mut rnd = || {
            gr.seed ^= gr.seed << 13;
            gr.seed ^= gr.seed >> 17;
            gr.seed ^= gr.seed << 5;
            (gr.seed as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        let enemy = if p.kind == 3 {
            let near: Vec<Vec3> = soldier_list.iter().filter(|s| s.3 && s.2 != sub.team && Vec2::new(s.1.x - sub.center.x, s.1.y - sub.center.y).length() <= p.spread).map(|s| s.1).collect();
            (!near.is_empty()).then(|| near[((rnd() * 0.5 + 0.5) * near.len() as f32) as usize % near.len()])
        } else {
            None
        };
        let to = enemy.unwrap_or_else(|| {
            // a point of the disc (uniform), on the ground below
            let (a, r) = (rnd() * std::f32::consts::PI, (rnd() * 0.5 + 0.5).sqrt() * p.spread);
            let xy = Vec3::new(sub.center.x + a.cos() * r, sub.center.y + a.sin() * r, sub.from.z);
            let ob = Vec3::new(xy.x, xy.z, xy.y) * 0.01;
            match col.raycast(ob, Vec3::NEG_Y, 500.0, &mut buf) {
                Some((t, _)) => xy - Vec3::Z * t * 100.0,
                None => Vec3::new(xy.x, xy.y, sub.center.z),
            }
        });
        let d = (to - sub.from).normalize_or_zero();
        let trail = p.bomblet.flight_fx.as_ref().and_then(|t| fx.fire_tracked(t, sub.from, dir_quat(d)));
        gr.list.push(Grenade { rocket: Some(wi), pos: sub.from, vel: d * p.bomblet_speed.max(500.0), fuse: 10.0, owner: sub.owner, team: sub.team, resting: false, trail, tac: None, payload: None });
    }
    let mut bursts: Vec<(Vec3, usize, Vec3, Actor, u8)> = Vec::new();
    // homing state per rocket (entries pushed by other systems have none)
    let mut seeks = std::mem::take(&mut gr.seek);
    seeks.resize(gr.list.len(), None);
    for (gi, n) in gr.list.iter_mut().enumerate() {
        n.fuse -= dt;
        // airstrike bomb: bursts at PayloadReleaseDistance from the target (no blast of its own)
        if let Some((si, target)) = n.payload {
            let rel = data.strikes.get(si).and_then(|s| s.cfg.payload.as_ref()).map(|p| p.release_dist).unwrap_or(0.0);
            if n.pos.distance(target) <= rel.max(100.0) {
                bursts.push((n.pos, si, target, n.owner, n.team));
                n.payload = None;
                n.fuse = -1000.0;
                if let Some(t) = n.trail.take() {
                    fx.release(t);
                }
                continue;
            }
        }
        if n.rocket.is_some() {
            let pj = n.rocket.and_then(|w| data.weapons.get(w)).and_then(|w| w.projectile.as_ref());
            // locked rocket: after Seek_DelayTime the flight direction turns towards the target's
            // box centre, VInterpTo(dir, to target, dt, Seek_VInterpToValue); the target is lost
            // below Seek_LoseTargetMaxDot (PROX 0.45) or when the vehicle is gone. The seeking
            // code itself is native (GCProj_Rocket Fly), not decoded: this reads the values'
            // names. NOT from data: the box centre as the aim point
            if let (Some(sk), Some(pj)) = (seeks[gi].as_mut(), pj) {
                sk.age += dt;
                let speed = n.vel.length();
                match vols.0.iter().find(|v| v.e == sk.target) {
                    Some(v) if speed > 1.0 => {
                        let aim = v.pos + v.rot * ((v.min + v.max) * 0.5);
                        let dir = n.vel / speed;
                        let to = (aim - n.pos).normalize_or_zero();
                        if dir.dot(to) < pj.seek_lose_dot {
                            seeks[gi] = None;
                        } else if sk.age >= pj.seek_delay {
                            let f = (dt * pj.seek_interp).clamp(0.0, 1.0);
                            n.vel = (dir + (to - dir) * f).normalize_or_zero() * speed;
                        }
                    }
                    _ => seeks[gi] = None,
                }
            }
            // straight flight (GCProj_Rocket m_fGravityFactor 0); contact with level or a soldier
            let mv = n.vel * dt;
            let len = mv.length().max(1e-4);
            let dir = mv / len;
            let hit = trace_shot(&col, &soldier_list, None, n.pos, dir, len + 10.0, None, true, &mut buf);
            // vehicles (vehicles.rs hit boxes; the player's own vehicle not for his rockets)
            let veh = vols.ray(n.pos, dir, len + 10.0, matches!(n.owner, Actor::Player));
            let t_hit = match hit {
                ShotHit::World(t, _, _) | ShotHit::Soldier(_, t, _) => Some(t),
                _ => None,
            };
            match (t_hit, veh) {
                (Some(t), Some((_, tv))) if tv < t => {
                    n.pos += dir * (tv - 5.0).max(0.0);
                    n.fuse = 0.0;
                }
                (None, Some((_, tv))) => {
                    n.pos += dir * (tv - 5.0).max(0.0);
                    n.fuse = 0.0;
                }
                (Some(t), _) => {
                    n.pos += dir * (t - 5.0).max(0.0);
                    n.fuse = 0.0;
                }
                _ => n.pos += mv,
            }
            // proximity fuse (GCProjectile.ProximityVsAnyVehicleRange, PROX 50): any vehicle box
            // that close (not the player's own for his rockets)
            if let Some(r) = pj.map(|p| p.prox_vehicle_range).filter(|&r| r > 0.0 && n.fuse > 0.0) {
                if vols.near(n.pos, r).iter().any(|x| !(x.2 && matches!(n.owner, Actor::Player))) {
                    n.fuse = 0.0;
                }
            }
        } else if !n.resting {
            let steps = 4;
            let h = dt / steps as f32;
            for _ in 0..steps {
                n.vel.z -= GRAVITY_UE * h;
                let mv = n.vel * h;
                let len = mv.length();
                if len < 1e-4 {
                    continue;
                }
                let dir = mv / len;
                let ob = Vec3::new(n.pos.x, n.pos.z, n.pos.y) * 0.01;
                let db = Vec3::new(dir.x, dir.z, dir.y);
                match col.raycast(ob, db, len * 0.01 + 0.05, &mut buf) {
                    Some((t, nb)) if t * 100.0 <= len + 5.0 => {
                        let nrm = Vec3::new(nb.x, nb.z, nb.y);
                        n.pos += dir * (t * 100.0 - 3.0).max(0.0);
                        // EMP: bDetonateOnImpact; C4 (GCProj_C4 without bBounce): sticks
                        if let Some(tc) = &n.tac {
                            if tc.detonate_on_impact {
                                n.fuse = 0.0;
                                n.vel = Vec3::ZERO;
                                break;
                            }
                            if !tc.bounce {
                                n.resting = true;
                                n.vel = Vec3::ZERO;
                                break;
                            }
                        }
                        let vn = nrm * n.vel.dot(nrm);
                        let vp = n.vel - vn;
                        if n.vel.length() > 150.0 {
                            if let Some(c) = &g.bounce_sound {
                                sfx.play(c, Some(n.pos));
                            }
                        }
                        n.vel = vp * g.dampen_parallel - vn * g.dampen;
                        if n.vel.length() < 40.0 && nrm.z > 0.7 {
                            n.resting = true;
                            n.vel = Vec3::ZERO;
                            break;
                        }
                    }
                    _ => n.pos += mv,
                }
            }
        }
        if let Some(t) = n.trail {
            if n.fuse <= 0.0 {
                // the trail reaches the point of impact
                fx.track(t, n.pos, dir_quat(n.vel.normalize_or_zero()));
                fx.release(t);
            } else {
                fx.track(t, n.pos, dir_quat(n.vel.normalize_or_zero()));
            }
        }
        // ProjectileClass AmbientSound (rocket flight loop) at the projectile
        if let (true, Some(t)) = (n.fuse > 0.0, n.trail) {
            if let Some(c) = n.rocket.and_then(|w| data.weapons.get(w)).and_then(|w| w.projectile.as_ref()).and_then(|p| p.flight_sound.as_ref()) {
                sfx.looping(&format!("proj{}", t.1.to_bits()), c, Some(n.pos));
            }
        }
        if n.fuse <= 0.0 && n.fuse > -999.0 {
            boom.push((n.pos, n.owner, n.team, n.rocket, n.tac.clone()));
        }
    }
    // payload release: the bomb's ProjExplosionTemplate and ExplosionSound, then the bomblets
    for (at, si, target, owner, team) in bursts {
        let Some(st) = data.strikes.get(si) else { continue };
        let Some(p) = st.cfg.payload.as_ref() else { continue };
        if let Some(f) = &p.release_fx {
            fx.fire(f, at, Quat::IDENTITY);
        }
        // NOT from data: a light for the burst (GCProj_Airstrike.SpawnProjectileExplosionParticles
        // passes LightClass None)
        gr.exploded.push((at, "GCExplosionLight_ClusterBomb"));
        if let Some(c) = &st.cfg.missile.explosion_sound {
            sfx.play(c, Some(at));
        }
        // sensor fused: the bomblets leave from AirburstHeight above the target (+ up to
        // AirburstRandRadius), one after another over StrikeDuration; cluster / phosphorus from the
        // burst point. NOT from data: the cluster bomblets within 0.4 s
        for k in 0..p.count.max(0) {
            gr.seed ^= gr.seed << 13;
            gr.seed ^= gr.seed >> 17;
            gr.seed ^= gr.seed << 5;
            let r1 = gr.seed as f32 / u32::MAX as f32;
            let (from, t) = match p.kind {
                3 => {
                    let a = r1 * std::f32::consts::TAU;
                    (target + Vec3::new(a.cos() * p.airburst_rand * r1, a.sin() * p.airburst_rand * r1, p.airburst_height), p.duration * k as f32 / p.count.max(1) as f32)
                }
                1 => (at, r1 * 0.4),
                _ => (at, 0.0),
            };
            gr.pending.push(SubShot { t, from, center: target, strike: si, owner, team });
        }
    }
    let keep: Vec<bool> = gr.list.iter().map(|n| n.fuse > 0.0).collect();
    gr.list.retain(|n| n.fuse > 0.0);
    let mut ki = keep.into_iter();
    seeks.retain(|_| ki.next().unwrap_or(false));
    gr.seek = seeks;
    // HB_TEST_ROCKET=<weapon cfg>[,<interval s>[,<turn deg>]]: that projectile from 1 m right of the eye
    // along the view every interval (tests: smoke trails, splash)
    if let Ok(v) = std::env::var("HB_TEST_ROCKET") {
        let mut it = v.split(',');
        let what = it.next().unwrap_or("");
        let every = it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(2.0);
        gr.test_rocket += dt;
        if gr.test_rocket >= every {
            gr.test_rocket = 0.0;
            let w = data.weapons.iter().position(|w| w.cfg.eq_ignore_ascii_case(what));
            if let (Some(p), Some(wi)) = (pawns.iter().find(|p| !p.dead), w) {
                if let Some(pj) = data.weapons[wi].projectile.as_ref() {
                    // slightly upward: the trail stays in view (tests); a third value turns the
                    // flight that many degrees to the right, starting 10 m to the left (side view)
                    let turn = v.split(',').nth(2).and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0).to_radians();
                    let view = p.dir_ue();
                    let d = (Quat::from_rotation_z(turn) * view + Vec3::Z * 0.1).normalize();
                    let right = d.cross(Vec3::Z).normalize_or_zero();
                    let vr = view.cross(Vec3::Z).normalize_or_zero();
                    let pos = if turn != 0.0 { p.eye_ue() + view * 3000.0 + vr * 1200.0 * turn.signum() } else { p.eye_ue() + d * 60.0 - right * 100.0 - Vec3::Z * 30.0 };
                    let trail = pj.flight_fx.as_ref().and_then(|t| fx.fire_tracked(t, pos, dir_quat(d)));
                    gr.list.push(Grenade { rocket: Some(wi), pos, vel: d * pj.speed, fuse: 14.0, owner: Actor::Player, team: player_team(), resting: false, trail, tac: None, payload: None });
                }
            }
        }
    }
    // HB_TEST_BOOM=<weapon cfg | tactical cfg | frag>[,<interval s>[,<m>]]: that explosion 8 m (or m) in front
    // of the player every interval (tests)
    if let Ok(v) = std::env::var("HB_TEST_BOOM") {
        let mut it = v.split(',');
        let what = it.next().unwrap_or("").to_ascii_lowercase();
        let every = it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(3.0);
        let dist = it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(8.0);
        gr.test_boom += dt;
        if gr.test_boom >= every {
            gr.test_boom = 0.0;
            if let Some(p) = pawns.iter().find(|p| !p.dead) {
                let yaw = crate::player::ue_rot(p.yaw);
                let f = p.feet + Vec3::new(yaw.cos(), 0.0, yaw.sin()) * dist + Vec3::Y * 0.3;
                let at = Vec3::new(f.x, f.z, f.y) * 100.0;
                let tac = data.tactical.get(&what).cloned();
                let rocket = data.weapons.iter().position(|w| w.cfg.eq_ignore_ascii_case(&what));
                boom.push((at, Actor::Player, player_team(), rocket, tac));
            }
        }
    }
    // HB_TEST_CARHITS=<s>,<s>,...: a GCDmgType_ATW_PROX rocket explosion at the level car nearest
    // to the player at those test-clock seconds (levelcars.rs test_carhit; tests)
    if let Some(p) = pawns.iter().find(|p| !p.dead) {
        let e = p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
        if let Some(at) = crate::levelcars::test_carhit(dt, Vec3::new(e.x, e.z, e.y) * 100.0) {
            let rocket = data.weapons.iter().position(|w| w.projectile.as_ref().is_some_and(|p| p.dmg_type.eq_ignore_ascii_case("GCDmgType_ATW_PROX")));
            boom.push((at, Actor::Player, player_team(), rocket, None));
        }
    }
    for (at, owner, team, rocket, tac) in boom {
        // explosion light class: NOT from data (native projectile code), see dynlights.rs
        let light = match (&tac, rocket) {
            (Some(tc), _) => match tc.kind {
                3 => "GCExplosionLight_C4",
                1 => "GCExplosionLight_Grenade",
                _ => "",
            },
            (None, Some(w)) => {
                let cfg = data.weapons.get(w).map(|w| w.cfg.to_ascii_lowercase()).unwrap_or_default();
                if cfg.contains("cluster") && cfg.ends_with("_bomblet") {
                    "GCExplosionLight_ClusterBomb"
                } else if cfg.ends_with("_bomblet") {
                    "GCExplosionLight_Grenade"
                } else if cfg.contains("hellfire") {
                    "GCExplosionLight_TankDES"
                } else {
                    "GCExplosionLight_Rocket"
                }
            }
            (None, None) => "GCExplosionLight_Grenade",
        };
        gr.exploded.push((at, light));
        if let Some(tc) = tac {
            // NOT from data: C4 also throws up the Hellfire's surface debris (TankShell impacts)
            let debris = (tc.kind == 3).then_some(DEBRIS_DMG);
            if !explosion_fx_debris(impacts.as_deref(), &col, at, &tc.dmg_type, debris, &mut fx, &mut sfx, &mut buf) {
                fx.fire(tactical_explosion_fx(tc.kind), at, Quat::IDENTITY);
            }
            tactical_blast(&tc, at, owner, team, &data, &col, &pawns, &mut soldiers, &mut queue, &mut pc, &mut gr.emp, &mut sfx, &mut buf);
            continue;
        }
        let pj = rocket.and_then(|w| data.weapons.get(w)).and_then(|w| w.projectile.clone());
        let (radius, full, dtype) = match &pj {
            Some(p) => (p.radius, p.radius_full, p.dmg_type.clone()),
            None => (g.radius, 0.0, g.dmg_type.clone()),
        };
        let snd = pj.as_ref().and_then(|p| p.explosion_sound.clone()).or_else(|| g.explosion_sound.clone());
        if let Some(c) = &snd {
            sfx.play(c, Some(at));
        }
        // cluster bomblets: BombletExplosionTemplate
        // (ProjExplosionTemplate: PROX rocket, phosphorus grenades); the impact table when its
        // pool is missing
        let own_fx = pj.as_ref().and_then(|p| p.explosion_fx.clone()).filter(|f| fx.fire(f, at, Quat::IDENTITY));
        // the explosion's effect for the surface below (PhysicalMaterialPropertyEx: frag on dirt
        // Frag_Conc, in the air PM_Default_Exp Frag_Air, ...); Frag_Metal when no table is loaded
        // NOT from data: rockets and missiles also throw up the Hellfire's surface debris
        let debris = (rocket.is_some() && !light.is_empty() && light != "GCExplosionLight_Grenade" && light != "GCExplosionLight_ClusterBomb" && light != "GCExplosionLight_TankDES").then_some(DEBRIS_DMG);
        if own_fx.is_none() && !explosion_fx_debris(impacts.as_deref(), &col, at, &dtype, debris, &mut fx, &mut sfx, &mut buf) {
            fx.fire("KFX_Pyrotechnics.FX_Grenades.Frag_Metal", at, Quat::IDENTITY);
        }
        let player = pawns.single().ok().filter(|p| !p.dead).map(|p| {
            let e = p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
            Vec3::new(e.x, e.z, e.y) * 100.0
        });
        let mut targets: Vec<(Target, Vec3, u8)> = soldiers.iter().filter(|(_, s)| s.alive).map(|(e, s)| (Target::Bot(e), s.center_ue(), s.team)).collect();
        if let Some(c) = player {
            targets.push((Target::Player, c, player_team()));
        }
        if let Some(c) = crate::drone::target() {
            targets.push((Target::Drone, c, player_team()));
        }
        // GCProj_AAssault_Missile_C (DamageRadius 50, DamageRadiusFull 150) and
        // GCProj_PhosphorusBomblet_C (300 / 500) have a full radius beyond the damage radius:
        // the larger one bounds the blast (otherwise they hurt nobody beyond 0.5 / 3 m)
        let radius = radius.max(full);
        for (t, c, tteam) in targets {
            // distance to the victim's collision cylinder, not its centre (UE3 HurtRadius takes
            // the actors whose collision touches the radius)
            let d = cylinder_distance(at, c);
            if d > radius {
                continue;
            }
            // friendly fire off (FriendlyFire setting default), the thrower is still hurt
            let own = match (t, owner) {
                (Target::Player, Actor::Player) => true,
                (Target::Bot(e), Actor::Bot(b)) => e == b,
                _ => false,
            };
            if tteam == team && !own {
                continue;
            }
            // line of sight from the explosion to the victim (UE3 VisibleCollidingActors traces to
            // the actor's location). NOT from data: the head and the knees count too, so a curb or
            // a terrain lip in front of the centre does not swallow the blast
            let ob = Vec3::new(at.x, at.z + 10.0, at.y) * 0.01;
            let seen = [0.0f32, 30.0, -28.0].iter().any(|dz| {
                let cb = Vec3::new(c.x, c.z + dz, c.y) * 0.01;
                let dir = (cb - ob).normalize_or_zero();
                col.raycast(ob, dir, ob.distance(cb), &mut buf).is_none()
            });
            if !seen {
                if std::env::var_os("HB_DMGDBG").is_some() {
                    info!("blast {dtype}: {t:?} at {d:.0} uu hidden");
                }
                continue;
            }
            // HurtRadius: full damage inside DamageRadiusFull, linear falloff to DamageRadius
            let base = data.grid.damage(&dtype, "PMA_Soldier", 0.0, 1.0, 1.0);
            let dmg = if d <= full { base } else { base * (1.0 - (d - full) / (radius - full).max(1.0)) };
            let icon = rocket.and_then(|w| data.weapons.get(w)).and_then(|w| w.ui_icon.clone());
            if std::env::var_os("HB_DMGDBG").is_some() {
                info!("blast {dtype}: {t:?} at {d:.0} uu (full {full}, radius {radius}) -> {dmg:.0}");
            }
            queue.0.push(Hit { target: t, amount: dmg, attacker: owner, attacker_team: team, headshot: false, explosive: true, from: at, icon, dmg_type: Some(dtype.clone()) });
        }
        // the level's interactive cars (levelcars.rs): BaseDamage to every bone in DamageRadius
        crate::levelcars::blast(at, radius, data.grid.damage(&dtype, crate::levelcars::CAR_PHYS_MAT, 0.0, 1.0, 1.0));
        // vehicles in the blast (distance to the hit box; the player's own vehicle takes the
        // player's hit above): the same falloff, vehicles.rs scales it to the armour
        let base = data.grid.damage(&dtype, "PMA_Soldier", 0.0, 1.0, 1.0);
        for (e, d, inside) in vols.near(at, radius) {
            if inside {
                continue;
            }
            let dmg = if d <= full { base } else { base * (1.0 - (d - full) / (radius - full).max(1.0)) };
            if std::env::var_os("HB_DMGDBG").is_some() {
                info!("blast {dtype}: vehicle {e:?} at {d:.0} uu -> {dmg:.0}");
            }
            let icon = rocket.and_then(|w| data.weapons.get(w)).and_then(|w| w.ui_icon.clone());
            vdmg.0.push((e, Hit { target: Target::Player, amount: dmg, attacker: owner, attacker_team: team, headshot: false, explosive: true, from: at, icon, dmg_type: Some(dtype.clone()) }));
        }
    }
    // exploding level cars (levelcars.rs; user decision, the SP behaviour on the MP cars):
    // GMS_ExplodingCars SeqAct_CauseDamageRadial (DamageAmount 1000, DamageRadius 1024,
    // bDamageFalloff, GCDmgType_Veh_HeliMissile) from the car, instigated by the car itself
    // (Actor::World: hurts every team). The damage grid's soldier value of the type replaces the
    // amount when it has one (as destruction.rs InitialVehicleExplosion); linear falloff to the
    // radius (UE3 HurtRadius with falloff); occluded victims are spared.
    for at in crate::levelcars::take_explosions() {
        let (radius, dtype) = (crate::levelcars::EXPLOSION_RADIUS, crate::levelcars::EXPLOSION_DMG_TYPE);
        let grid = data.grid.damage(dtype, "PMA_Soldier", 0.0, 1.0, 1.0);
        let base = if grid > 0.0 { grid } else { crate::levelcars::EXPLOSION_DAMAGE };
        // NOT from data: the explosion light class (dynlights.rs)
        gr.exploded.push((at, "GCExplosionLight_Rocket"));
        let mut targets: Vec<(Target, Vec3)> = soldiers.iter().filter(|(_, s)| s.alive).map(|(e, s)| (Target::Bot(e), s.center_ue())).collect();
        if let Some(p) = pawns.iter().find(|p| !p.dead) {
            let e = p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
            targets.push((Target::Player, Vec3::new(e.x, e.z, e.y) * 100.0));
        }
        if let Some(c) = crate::drone::target() {
            targets.push((Target::Drone, c));
        }
        for (t, c) in targets {
            let d = cylinder_distance(at, c);
            if d > radius {
                continue;
            }
            let ob = Vec3::new(at.x, at.z + 60.0, at.y) * 0.01;
            let seen = [0.0f32, 30.0, -28.0].iter().any(|dz| {
                let cb = Vec3::new(c.x, c.z + dz, c.y) * 0.01;
                col.raycast(ob, (cb - ob).normalize_or_zero(), ob.distance(cb), &mut buf).is_none()
            });
            if !seen {
                continue;
            }
            let dmg = base * (1.0 - d / radius);
            if std::env::var_os("HB_DMGDBG").is_some() || std::env::var_os("HB_CARDBG").is_some() {
                info!("car explosion {dtype}: {t:?} at {d:.0} uu -> {dmg:.0}");
            }
            queue.0.push(Hit { target: t, amount: dmg, attacker: Actor::World, attacker_team: 2, headshot: false, explosive: true, from: at, icon: None, dmg_type: Some(dtype.to_string()) });
        }
        // other cars (chain reaction as in SP) and vehicles
        let car_grid = data.grid.damage(dtype, crate::levelcars::CAR_PHYS_MAT, 0.0, 1.0, 1.0);
        crate::levelcars::blast_falloff(at, radius, if car_grid > 0.0 { car_grid } else { crate::levelcars::EXPLOSION_DAMAGE });
        // (the player's own vehicle takes the player's hit above)
        for (e, d, inside) in vols.near(at, radius) {
            if inside {
                continue;
            }
            let dmg = base * (1.0 - d / radius);
            vdmg.0.push((e, Hit { target: Target::Player, amount: dmg, attacker: Actor::World, attacker_team: 2, headshot: false, explosive: true, from: at, icon: None, dmg_type: Some(dtype.to_string()) }));
        }
    }
}

/// Distance from a point to a soldier's collision cylinder around its centre `c` (UE units).
fn cylinder_distance(at: Vec3, c: Vec3) -> f32 {
    let dz = ((at.z - c.z).abs() - CYL_HALF).max(0.0);
    let dxy = (Vec2::new(at.x - c.x, at.y - c.y).length() - CYL_RADIUS).max(0.0);
    (dxy * dxy + dz * dz).sqrt()
}

/// Explosion effect from the physical material 1.5 m below (else PM_Default_Exp, the table for
/// explosions in the air) for the damage type; false when there is no impact table entry.
fn explosion_fx(impacts: Option<&crate::impacts::Impacts>, col: &CollisionWorld, at: Vec3, dmg: &str, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue, buf: &mut Vec<u32>) -> bool {
    explosion_fx_debris(impacts, col, at, dmg, None, fx, sfx, buf)
}

/// `explosion_fx`, plus the surface particles of `debris` (another damage type) when the
/// explosion is near the ground.
#[allow(clippy::too_many_arguments)]
pub(crate) fn explosion_fx_debris(impacts: Option<&crate::impacts::Impacts>, col: &CollisionWorld, at: Vec3, dmg: &str, debris: Option<&str>, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue, buf: &mut Vec<u32>) -> bool {
    let Some(im) = impacts else { return false };
    let ob = Vec3::new(at.x, at.z + 20.0, at.y) * 0.01;
    let surface = col.raycast_hit(ob, Vec3::NEG_Y, 1.7, buf, crate::player::COL_TRACE).map(|(t, n, tri)| (col.phys_of(tri), at - Vec3::Z * (t * 100.0 - 20.0), Vec3::new(n.x, n.z, n.y)));
    if let (Some(s), Some(d)) = (surface, debris) {
        im.surface_debris(s, d, fx);
    }
    im.explosion(surface, dmg, at, fx, sfx)
}

/// Flash bang / EMP / C4 explosion (tactical grenades).
/// - flash bang: within DamageRadius with line of sight the player is whited out and bots stop
///   shooting; the time (1.5..5 s by distance) is NOT from data (the native flash code is not
///   decoded); damage only as far as the damage grid gives GCDmgType_FlashBang any
/// - EMP: EMPDamageRadius / EMPBlastDuration to vehicles and drones (Grenades::emp)
/// - C4: HurtRadius like the frag with DamageRadius / DamageRadiusFull and GCDmgType_EXP_C4
#[allow(clippy::too_many_arguments)]
fn tactical_blast(
    tc: &crate::weaponcfg::TacticalCfg,
    at: Vec3,
    owner: Actor,
    team: u8,
    data: &CombatData,
    col: &CollisionWorld,
    pawns: &Query<&Pawn>,
    soldiers: &mut Query<(Entity, &mut Soldier)>,
    queue: &mut DamageQueue,
    pc: &mut PlayerCombat,
    emp: &mut Vec<(Vec3, f32, f32)>,
    sfx: &mut crate::sfx::SfxQueue,
    buf: &mut Vec<u32>,
) {
    if let Some(c) = &tc.explosion_sound {
        sfx.play(c, Some(at));
    }
    if tc.kind == 2 {
        emp.push((at, tc.emp_radius, tc.emp_duration));
    }
    let player = pawns.single().ok().filter(|p| !p.dead).map(|p| {
        let e = p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
        Vec3::new(e.x, e.z, e.y) * 100.0
    });
    let mut targets: Vec<(Target, Vec3, u8)> = soldiers.iter().filter(|(_, s)| s.alive).map(|(e, s)| (Target::Bot(e), s.center_ue(), s.team)).collect();
    if let Some(c) = player {
        targets.push((Target::Player, c, player_team()));
    }
    let base = if tc.dmg_type.is_empty() { 0.0 } else { data.grid.damage(&tc.dmg_type, "PMA_Soldier", 0.0, 1.0, 1.0) };
    // the level's interactive cars (levelcars.rs)
    if !tc.dmg_type.is_empty() {
        crate::levelcars::blast(at, tc.radius, data.grid.damage(&tc.dmg_type, crate::levelcars::CAR_PHYS_MAT, 0.0, 1.0, 1.0));
    }
    for (t, c, tteam) in targets {
        let d = cylinder_distance(at, c);
        if d > tc.radius {
            continue;
        }
        let ob = Vec3::new(at.x, at.z + 10.0, at.y) * 0.01;
        let cb = Vec3::new(c.x, c.z, c.y) * 0.01;
        let dir = (cb - ob).normalize_or_zero();
        if col.raycast(ob, dir, ob.distance(cb), buf).is_some() {
            continue;
        }
        let own = matches!((t, owner), (Target::Player, Actor::Player)) || matches!((t, owner), (Target::Bot(e), Actor::Bot(b)) if e == b);
        if tc.kind == 1 {
            // the flash blinds everyone in sight, the thrower included
            let secs = 1.5 + 3.5 * (1.0 - d / tc.radius.max(1.0));
            match t {
                Target::Player => pc.blinded = pc.blinded.max(secs),
                Target::Bot(e) => {
                    if let Ok((_, mut s)) = soldiers.get_mut(e) {
                        s.blinded = s.blinded.max(secs);
                    }
                }
                Target::Drone | Target::BotDrone(_) => {}
            }
        }
        if base <= 0.0 || (tteam == team && !own) {
            continue;
        }
        let full = tc.radius_full;
        let dmg = if d <= full { base } else { base * (1.0 - (d - full) / (tc.radius - full).max(1.0)) };
        queue.0.push(Hit { target: t, amount: dmg, attacker: owner, attacker_team: team, headshot: false, explosive: true, from: at, icon: None, dmg_type: Some(tc.dmg_type.clone()) });
    }
}

/// Apply queued damage: deaths, kills, battle points (kill streak tier KillBP, headshot bonus),
/// team score (KillGameScore).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_damage(mut queue: ResMut<DamageQueue>, mut flow: ResMut<MatchFlow>, mut pc: ResMut<PlayerCombat>, mut pawns: Query<&mut Pawn>, mut soldiers: Query<&mut Soldier>, mut feed: ResMut<KillFeed>, time: Res<Time>, (mut sfx, cheats, mut drone, mut vhits, mut taken): (ResMut<crate::sfx::SfxQueue>, Res<Cheats>, ResMut<crate::drone::Drone>, ResMut<VehicleHits>, ResMut<crate::dmgfeedback::DamageTaken>)) {
    let dt = time.delta_secs();
    for k in feed.0.iter_mut() {
        k.5 -= dt;
    }
    feed.0.retain(|k| k.5 > 0.0);
    flow.hud.kills = feed.0.iter().map(|k| (k.0.clone(), k.1, k.2.clone(), k.3, k.4.clone())).collect();
    let hits = std::mem::take(&mut queue.0);
    for h in hits {
        let attacker_name = match h.attacker {
            Actor::Player => "Du".to_string(),
            Actor::Bot(e) => soldiers.get(e).map(|s| s.name.clone()).unwrap_or_default(),
            Actor::World => String::new(),
        };
        let killed: Option<(String, u8)> = match h.target {
            Target::Player => {
                let Ok(mut p) = pawns.single_mut() else { continue };
                if p.dead || (cheats.god && !matches!(h.attacker, Actor::World)) {
                    continue;
                }
                // damage feedback (dmgfeedback.rs: hit direction, hit sound), in a vehicle too
                taken.0.push((h.amount, h.from, h.explosive, h.attacker == Actor::World));
                // in a vehicle: the vehicle takes it (vehicles.rs, its armour's damage)
                if p.in_vehicle && !matches!(h.attacker, Actor::World) {
                    vhits.0.push(h.clone());
                    continue;
                }
                pc.health -= h.amount;
                pc.since_damage = 0.0;
                pc.last_attacker = Some(h.attacker);
                if pc.health <= 0.0 {
                    pc.health = 0.0;
                    p.dead = true;
                    // death message / kill cam: who, and the place of death
                    pc.killer = Some((attacker_name.clone(), matches!(h.attacker, Actor::Player | Actor::World), h.from));
                    pc.death_explosive = h.explosive;
                    info!("player died ({attacker_name:?})");
                    pc.dead_for = 0.0;
                    pc.deaths += 1;
                    pc.streak = 0;
                    Some(("Du".into(), player_team()))
                } else {
                    None
                }
            }
            Target::Drone => {
                // the drone takes the hit (drone.rs destroys it at 0)
                if drone.deployed() && !(cheats.god && !matches!(h.attacker, Actor::World)) {
                    drone.health -= h.amount;
                    drone.last_hit = Some(h.from);
                }
                None
            }
            Target::BotDrone(e) => {
                // botdrones.rs drone_damage applies it (friendly fire off there)
                if h.attacker == Actor::Player && crate::botdrones::team_of(e).is_some_and(|t| t != player_team()) {
                    if pc.hit_marker <= 0.0 {
                        sfx.play("A2_UI_C.SC_DamageProcured", None);
                    }
                    pc.hit_marker = HIT_FADE;
                }
                crate::botdrones::queue_hit(e, h.clone());
                None
            }
            Target::Bot(e) => {
                let Ok(mut s) = soldiers.get_mut(e) else { continue };
                // (nuke.rs held: the bot on the Fulton line takes no damage)
                if !s.alive || crate::nuke::held(e) {
                    continue;
                }
                s.health -= h.amount;
                s.since_damage = 0.0;
                s.last_attacker = Some(h.attacker);
                s.last_hit = Some((h.from, h.explosive));
                // hit notify (GCUIObject_WeaponCrosshair HitNotifyMaterial M_Crosshair_HitNotify,
                // HitEnemyFadeTime 0.15 of the MP HUD's widget, HitEnemySound SC_DamageProcured)
                if h.attacker == Actor::Player && s.team != player_team() {
                    if pc.hit_marker <= 0.0 {
                        sfx.play("A2_UI_C.SC_DamageProcured", None);
                    }
                    pc.hit_marker = HIT_FADE;
                }
                if s.health <= 0.0 {
                    s.health = 0.0;
                    s.alive = false;
                    s.deaths += 1;
                    s.streak = 0;
                    Some((s.name.clone(), s.team))
                } else {
                    None
                }
            }
        };
        let Some((victim, vteam)) = killed else { continue };
        let _ = &victim;
        feed.0.push((attacker_name, h.attacker_team, victim, vteam, h.icon.clone(), crate::objectives::CRAWL_SECS));
        if feed.0.len() > 5 {
            feed.0.remove(0);
        }
        // (not while the end-of-match nuke has frozen the scores, objectives.rs Endgame)
        if h.attacker_team != vteam && (h.attacker_team as usize) < 2 && !crate::objectives::scores_frozen() {
            // team score: KillGameScore of the killer's streak tier (first tier 10)
            let streak = match h.attacker {
                Actor::Player => pc.streak,
                Actor::Bot(e) => soldiers.get(e).map(|s| s.streak).unwrap_or(0),
                Actor::World => 0,
            };
            // TDM: the kill reward (tdm.rs kill_bp, NOT from data that it is the team score)
            flow.hud.score[h.attacker_team as usize] += crate::tdm::kill_bp().unwrap_or(streak_tier(streak).2);
        }
        match h.attacker {
            Actor::Player if vteam != player_team() => {
                // TDM: gcbattlepoints TDM.Infantry PSR_KillAsInfantry (tdm.rs)
                let kill_bp = crate::tdm::kill_bp().unwrap_or(streak_tier(pc.streak).1);
                pc.kills += 1;
                pc.streak += 1;
                flow.hud.kp += kill_bp + if h.headshot { 30 } else { 0 };
                flow.hud.rewards.push((kill_bp, "PSR_KillAsInfantry"));
                if h.headshot {
                    flow.hud.rewards.push((30, "PSR_Headshot"));
                }
            }
            Actor::Bot(e) => {
                if let Ok(mut s) = soldiers.get_mut(e) {
                    if s.team != vteam {
                        s.kills += 1;
                        s.streak += 1;
                    }
                }
            }
            _ => {}
        }
    }
}

/// GCGame.ini [KillStreak Objective_Mission_GameStreak] Tiers: (stars, KillBP, KillGameScore)
/// for a streak count.
pub fn streak_tier(streak: i32) -> (i32, i32, i32) {
    match streak {
        s if s >= 15 => (5, 20, 50),
        s if s >= 11 => (4, 20, 40),
        s if s >= 8 => (3, 20, 30),
        s if s >= 5 => (2, 20, 20),
        _ => (1, 20, 10),
    }
}

/// UE rotation whose X axis points along `d` (UE space).
pub fn dir_quat(d: Vec3) -> Quat {
    if d.length_squared() < 1e-8 {
        return Quat::IDENTITY;
    }
    let d = d.normalize();
    Quat::from_rotation_z(d.y.atan2(d.x)) * Quat::from_rotation_y(-d.z.clamp(-1.0, 1.0).asin())
}

/// Projectile meshes follow the flying rockets / grenades (pooled instances, hidden when unused).
pub fn render_projectiles(gr: Res<Grenades>, mut q: Query<(&ProjVis, &mut Transform, &mut Visibility)>) {
    // pools by mesh: 0 rocket, 1 frag, 2 EMP / flash bang, 3 C4
    let mut by_kind: [Vec<&Grenade>; 4] = Default::default();
    for n in &gr.list {
        let k = match (&n.rocket, &n.tac) {
            (Some(_), _) => 0,
            (None, None) => 1,
            (None, Some(t)) if t.kind == 3 => 3,
            (None, Some(_)) => 2,
        };
        by_kind[k].push(n);
    }
    if std::env::var_os("HB_PROJDBG").is_some() && !gr.list.is_empty() {
        let ents: Vec<(u8, usize)> = q.iter().map(|(pv, _, _)| (pv.kind, pv.slot)).collect();
        info!("PROJDBG {} projectiles, kinds {:?}, pos {:?}, pool entities {}", gr.list.len(), by_kind.iter().map(|v| v.len()).collect::<Vec<_>>(), gr.list.first().map(|n| n.pos), ents.len());
    }
    for (pv, mut tf, mut vis) in &mut q {
        let n = by_kind.get(pv.kind as usize).and_then(|v| v.get(pv.slot));
        match n {
            Some(n) => {
                tf.translation = Vec3::new(n.pos.x, n.pos.z, n.pos.y) * 0.01;
                // mesh vertices are UE units (build_game_mesh): metres like the level objects;
                // without this the rocket was a 100x black tube and the grenade enclosed the view
                tf.scale = Vec3::splat(0.01);
                // UE X along the flight direction, expressed in bevy axes (y <-> z)
                let d = n.vel.normalize_or_zero();
                if pv.kind == 0 && d != Vec3::ZERO {
                    tf.rotation = Quat::from_rotation_arc(Vec3::X, Vec3::new(d.x, d.z, d.y));
                }
                if *vis != Visibility::Visible {
                    *vis = Visibility::Visible;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

/// Length of an instant-hit trace (UE units, 164 m): UE3's usual WORLD_MAX-free trace length
/// (ZoomSnapToEnemyMaxDist 16384 on every weapon config); NOT from data as the shot length.
pub const SHOT_TRACE: f32 = 16384.0;

/// Seconds R is held to set off all placed C4 charges (NOT from data).
const C4_HOLD: f32 = 0.5;

/// An airstrike killstreak: the UAV's gun config, the weapon entry of its missile / bomb and
/// of the bomblets.
#[derive(Clone, Debug)]
pub struct Strike {
    pub key: String,
    pub cfg: crate::weaponcfg::HellfireCfg,
    pub weapon: usize,
    pub sub: Option<usize>,
    /// the UAV's VehicleAudio setup (GCV_AirstrikeUAV(_<kind>)_C -> A2_PreditorDrone_C
    /// AudioSetup_PredatorDrone(_Phosphorus / _Carpet)): SCEngineIdle, seat 0 SCFireSound
    pub engine: String,
    pub fire_sound: String,
}

/// Thermal goggles switched on (videofx.rs, bots.rs thermal_bodies).
pub static GOGGLES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Grenade keys (see the comment at the throw input).
const TACTICAL_KEY: &str = "EKBC_Grenade1";
const FRAG_KEY: &str = "EKBC_Grenade2";
