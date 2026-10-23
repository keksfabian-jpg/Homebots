//! Bots use the drone killstreaks of their loadouts (CFG_DRO_GAssault / GAntiTank / AAssault /
//! ARecon; user 2026-10-22 21:32 "sie sollen auch drohnen nutzen"). A bot buys its drone with its
//! battle points, sets it down in front of itself and steers it from where it stands (crouched,
//! vulnerable; botextras Mode::Drone) until the battery is empty, the drone is destroyed or the
//! bot dies.
//!
//! Data (the same as the player's drone, drone.rs / main.rs load_drone): meshes, skeleton,
//! AnimTree controls, treads, rotors, mirror attachments, collision cylinder, speed (GroundSpeed /
//! AirSpeed), battery (VehicleFuelInfo MaxFuel, moving / idle / per shot), health (drone.rs
//! DRONE_HEALTH, the player drone's), the gun (Seats[0].GunClass: FireInterval, FireSpread,
//! WeaponRange, FullDamageRange, overheat, damage type, fire cue), the rocket modes
//! (CombatData::drone_rockets: clip, burst, interval, reload, spread, missile, lock-on), the fire
//! sockets, the engine cue (VehicleAudio SCEngineIdle), the recon drone's marking distance
//! (DroneLockOnDistanceSQ) and DroneMarkDuration, the costs (SpecialAbilityCosts), the purchase
//! stinger, PSR_DestroyDrone 130 (GCBattlePoints Rewards), the destroyed effect of the player's
//! drone (FX_C4_EXP).
//!
//! NOT from data (the MP game had no bots; also in MODLOG): when a bot deploys (enemy within
//! 60 m or an open objective within 80 m, 50 % per check, 30 s cooldown), at most 2 bot drones per
//! team and 3 drawable copies per drone kind, the drone's driving / flying (walk-grid path, air
//! drones 6 m above the ground, climbing over obstacles), target choice and ranges (gun:
//! WeaponRange, rockets 60 m, rockets at soldiers beyond 8 m, vehicles first for rockets), reaction
//! / aim errors / bursts (as the bot vehicle gunners), turn rate, the operator crouching in place,
//! the drone exploding when its operator dies, the hit box (the mesh's bounds), the destroyed
//! sound (A2_Destruction_C SC_DroneSmallVehicleAnnihilation, chosen by its name), the recon drone's
//! marks shared with the team (the player's HUD marks / the team UAV for the bots), the kill crawl
//! lines, the engine loop volume.
use crate::botextras::{enabled, skill_factor, BotKit, Mode, TeamUav};
use crate::bots::{Bot, BotLog};
use crate::botvehicles::{self, BotVehicle, VehInfo};
use crate::combat::{player_team, Actor, CombatData, DamageQueue, Grenade, Grenades, Hit, Seek, ShotHit, Soldier, Target, TEAM_USA};
use crate::drone::{DroneAnim, DroneAssets, DroneKinds, ReconMarks};
use crate::menu::{MatchFlow, Phase};
use crate::nav::Nav;
use crate::player::{CollisionWorld, Pawn};
use crate::vehicles::{VehVol, Vehicle, VehicleAssets, VehicleDamage, VehicleVolumes};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub struct BotDronesPlugin;

impl Plugin for BotDronesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, bot_drones.after(crate::vehicles::update_volumes).after(crate::player::PlayerSet).before(crate::combat::CombatSet)).add_systems(
            Update,
            (drone_damage, render_bot_drones).chain().after(crate::combat::CombatSet).before(crate::vehicles::empty_vehicle_damage),
        );
    }
}

/// Drawable copies of each drone kind for the bots (main.rs load_drone). NOT from data.
pub const INSTANCES: usize = 3;
/// NOT from data: bot drones per team at once.
const TEAM_DRONES: usize = 2;
/// NOT from data: a bot deploys when an enemy is this close (UE) or an open objective of the
/// active line is within OBJ_RANGE.
const DEPLOY_RANGE: f32 = 6000.0;
const OBJ_RANGE: f32 = 8000.0;
/// NOT from data: killstreak cooldown of the bot after a deploy (s; botextras ks_cd).
const DEPLOY_CD: f32 = 30.0;
/// NOT from data: rocket drones engage up to 60 m, not at soldiers closer than 8 m.
const ROCKET_RANGE: f32 = 6000.0;
const ROCKET_MIN: f32 = 800.0;
/// NOT from data: air drones fly this high above the ground (UE), climb at most this much more
/// over obstacles.
const AIR_ALT: f32 = 600.0;
const AIR_CLIMB_MAX: f32 = 1500.0;
/// NOT from data: turn rate of body / turret (rad/s).
const TURN_RATE: f32 = 3.0;
/// GCBattlePoints Rewards (Id=PSR_DestroyDrone, Points=130).
pub const DESTROY_DRONE_BP: i32 = 130;
/// Bullets passing this close to the player's drone centre hit it (as bots.rs DRONE_HIT_RADIUS).
const PLAYER_DRONE_RADIUS: f32 = 35.0;
/// The player drone's destroyed effect (drone.rs; FX_C4_EXP) and the destroyed sound (NOT from
/// data: chosen by its name).
const DESTROY_FX: &str = "KFX_Pyrotechnics.FX_C4.FX_C4_EXP";
const DESTROY_SOUND: &str = "A2_Destruction_C.SC_DroneSmallVehicleAnnihilation";

#[derive(Clone, Copy, Debug, PartialEq)]
enum DTgt {
    Soldier(Entity),
    Player,
    Vehicle(Entity),
    Drone(Entity),
    PlayerDrone,
}

/// A bot's drone in the field.
#[derive(Component)]
pub struct BotDrone {
    pub owner: Entity,
    pub name: String,
    pub team: u8,
    /// DroneKinds index and the drawable copy (DroneAssets::bots)
    pub kind: usize,
    pub inst: usize,
    /// feet (Bevy metres), body yaw / turret pitch (radians, UE)
    pub feet: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    vel: Vec3,
    on_ground: bool,
    pub health: f32,
    fuel: f32,
    heat: f32,
    overheated: f32,
    fire_cd: f32,
    ammo: [i32; 2],
    reload: [f32; 2],
    burst: Option<(usize, i32, f32)>,
    shots: usize,
    path: Vec<Vec3>,
    repath: f32,
    goal: Option<Vec3>,
    best: f32,
    best_t: f32,
    side: f32,
    side_dir: f32,
    target: Option<DTgt>,
    think: f32,
    seen: f32,
    gun_t: f32,
    anim: DroneAnim,
    seed: u32,
    last_hit: Option<(Actor, u8, Option<String>)>,
    mark_t: f32,
    kills_at: i32,
    climb: f32,
    hold: bool,
    gas: f32,
    dbg_t: f32,
}

impl BotDrone {
    fn rnd(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 100_000) as f32 / 100_000.0
    }
}

/// A bot drone as a target / hit box for this frame (UE).
#[derive(Clone, Copy)]
pub struct DroneInfo {
    pub e: Entity,
    pub team: u8,
    pub centre: Vec3,
    pub vol: VehVol,
}

static DRONES: Mutex<Vec<DroneInfo>> = Mutex::new(Vec::new());
static HITS: Mutex<Vec<(Entity, Hit)>> = Mutex::new(Vec::new());

/// Bot drones (entity, aim point UE, team) for the infantry bots (bots.rs).
pub fn targets() -> Vec<(Entity, Vec3, u8)> {
    DRONES.lock().map(|g| g.iter().map(|d| (d.e, d.centre, d.team)).collect()).unwrap_or_default()
}

/// Aim point (UE) of a bot drone still in the field.
pub fn centre(e: Entity) -> Option<Vec3> {
    DRONES.lock().ok().and_then(|g| g.iter().find(|d| d.e == e).map(|d| d.centre))
}

pub fn team_of(e: Entity) -> Option<u8> {
    DRONES.lock().ok().and_then(|g| g.iter().find(|d| d.e == e).map(|d| d.team))
}

/// combat.rs apply_damage passes Target::BotDrone hits here (drone_damage applies them).
pub fn queue_hit(e: Entity, h: Hit) {
    if let Ok(mut g) = HITS.lock() {
        g.push((e, h));
    }
}

/// Ray (UE) against an oriented box: entry distance.
fn ray_box(v: &VehVol, o: Vec3, d: Vec3, max: f32) -> Option<f32> {
    let inv = v.rot.inverse();
    let lo = inv * (o - v.pos);
    let ld = inv * d;
    let (mut t0, mut t1) = (0.0f32, max);
    for a in 0..3 {
        let (oa, da, mn, mx) = (lo[a], ld[a], v.min[a], v.max[a]);
        if da.abs() < 1e-6 {
            if oa < mn || oa > mx {
                return None;
            }
            continue;
        }
        let (mut ta, mut tb) = ((mn - oa) / da, (mx - oa) / da);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

fn inside(v: &VehVol, p: Vec3, margin: f32) -> bool {
    let l = v.rot.inverse() * (p - v.pos);
    l.cmpge(v.min - Vec3::splat(margin)).all() && l.cmple(v.max + Vec3::splat(margin)).all()
}

/// An instant-hit shot from `o` along `d` that ended at `shot` (combat::trace_shot, which stops
/// at the drones' boxes like at vehicles): the enemy bot drone it hit, with the distance. The
/// shooter's own drone (origin inside its box) is skipped; a friendly drone in front stops it.
pub fn bullet_hit(o: Vec3, d: Vec3, shot: &ShotHit, max: f32, team: u8) -> Option<(Entity, f32)> {
    let end = match shot {
        ShotHit::World(t, _, _) | ShotHit::Soldier(_, t, _) | ShotHit::Player(t, _) => *t,
        ShotHit::Nothing => max,
    };
    let g = DRONES.lock().ok()?;
    let mut best: Option<(f32, &DroneInfo)> = None;
    for x in g.iter() {
        if inside(&x.vol, o, 20.0) {
            continue;
        }
        if let Some(t) = ray_box(&x.vol, o, d, end + 2.0) {
            if best.is_none_or(|b| t < b.0) {
                best = Some((t, x));
            }
        }
    }
    best.filter(|b| b.1.team != team).map(|b| (b.1.e, b.0))
}

fn to_bevy(ue: Vec3) -> Vec3 {
    Vec3::new(ue.x, ue.z, ue.y) * 0.01
}

fn to_ue(b: Vec3) -> Vec3 {
    Vec3::new(b.x, b.z, b.y) * 100.0
}

fn team_name(t: u8) -> &'static str {
    if t == TEAM_USA {
        "USA"
    } else {
        "KVA"
    }
}

fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(t) - std::f32::consts::PI
}

/// Mesh origin (UE): the drone stands on its treads (drone.rs).
fn mesh_origin(d: &BotDrone, a: &DroneAssets) -> Vec3 {
    to_ue(d.feet) + Vec3::Z * a.tread_low.unwrap_or(a.half_height + a.mesh_z)
}

fn info_of(e: Entity, d: &BotDrone, a: &DroneAssets) -> DroneInfo {
    let rot = Quat::from_rotation_z(d.yaw);
    let pos = mesh_origin(d, a);
    let (mn, mx) = a.hit_box;
    let vol = VehVol { e, pos, rot, min: mn, max: mx, player_inside: false };
    DroneInfo { e, team: d.team, centre: pos + rot * ((mn + mx) * 0.5), vol }
}

/// Where the gun fires from (UE): inside the drone's box, up front (NOT from data; the player's
/// drone fires from its camera).
fn muzzle(d: &BotDrone, a: &DroneAssets) -> Vec3 {
    let (mn, mx) = a.hit_box;
    let view = Quat::from_rotation_z(d.yaw);
    mesh_origin(d, a) + view * Vec3::new(mx.x * 0.5, 0.0, mn.z + (mx.z - mn.z) * 0.75)
}

fn sees(col: &CollisionWorld, from: Vec3, to: Vec3, buf: &mut Vec<u32>) -> bool {
    let (a, b) = (to_bevy(from), to_bevy(to));
    let d = b - a;
    let l = d.length();
    l < 0.5 || col.raycast(a, d / l, (l - 0.4).max(0.0), buf).is_none()
}

/// A shot origin pushed along `d` out of the drone's own box (rockets explode on any box).
fn out_of_box(v: &VehVol, o: Vec3, d: Vec3) -> Vec3 {
    if !inside(v, o, 5.0) {
        return o;
    }
    let inv = v.rot.inverse();
    let lo = inv * (o - v.pos);
    let ld = inv * d;
    let mut t = f32::MAX;
    for a in 0..3 {
        if ld[a].abs() > 1e-4 {
            let edge = if ld[a] > 0.0 { v.max[a] } else { v.min[a] };
            t = t.min((edge - lo[a]) / ld[a]);
        }
    }
    if t == f32::MAX {
        o
    } else {
        o + d * (t.max(0.0) + 20.0)
    }
}

/// Where a drone heads without a target (Bevy): the nearest open objective of the active line,
/// else the nearest enemy, else a roam point (as the bot vehicles).
fn drone_goal(obj: &crate::objectives::Objectives, team: u8, from: Vec3, enemies: &[Vec3]) -> Option<Vec3> {
    let pos = |o: &crate::objectives::Objective| Vec3::new(o.location[0], o.location[2], o.location[1]) * 0.01;
    let open = obj.list.iter().filter(|o| o.line == obj.active_line && o.owner != Some(team)).min_by(|a, b| pos(a).distance(from).total_cmp(&pos(b).distance(from)));
    if let Some(o) = open {
        return Some(pos(o));
    }
    if let Some(e) = enemies.iter().min_by(|a, b| a.distance(from).total_cmp(&b.distance(from))) {
        return Some(*e);
    }
    obj.roam.iter().max_by(|a, b| a.distance(from).total_cmp(&b.distance(from))).copied()
}

/// Display name for the kill crawl (CombatData::special_names, else an English default).
fn label(data: &CombatData, key: &str) -> String {
    data.special_names.get(key).map(|n| n.to_uppercase()).unwrap_or_else(|| {
        match key {
            "cfg_dro_gantitank" => "ANTI-TANK DRONE",
            "cfg_dro_aassault" => "ASSAULT DRONE",
            "cfg_dro_arecon" => "RECON DRONE",
            _ => "GROUND DRONE",
        }
        .to_string()
    })
}

static ENV_FORCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static ENV_KIND: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
static ENV_DBG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// HB_BOTDRONE=1: bots with a drone item deploy it at once (no cooldown / situation check, free
/// when they cannot pay). HB_BOTDRONEKIND=<0..3>: every bot with a drone item uses that kind
/// (main.rs DRONE_KINDS: 0 ground assault, 1 anti tank, 2 air assault, 3 air recon). Tests.
fn force() -> bool {
    *ENV_FORCE.get_or_init(|| std::env::var("HB_BOTDRONE").is_ok_and(|v| v == "1"))
}

fn kind_override() -> Option<usize> {
    *ENV_KIND.get_or_init(|| std::env::var("HB_BOTDRONEKIND").ok().and_then(|v| v.parse().ok()).filter(|&k: &usize| k < crate::DRONE_KINDS.len()))
}

/// Deploys, steers and fires the bot drones; publishes their hit boxes (VehicleVolumes: rockets,
/// explosions, lock-on; vehicles::SHOT_VOLUMES: bullets stop at them).
#[allow(clippy::too_many_arguments)]
fn bot_drones(
    time: Res<Time>,
    flow: Res<MatchFlow>,
    (data, kinds, col, nav, obj, vassets): (Option<Res<CombatData>>, Option<Res<DroneKinds>>, Res<CollisionWorld>, Res<Nav>, Res<crate::objectives::Objectives>, Option<Res<VehicleAssets>>),
    pawns: Query<&Pawn>,
    mut commands: Commands,
    mut drones: Query<(Entity, &mut BotDrone)>,
    mut bots: Query<(Entity, &mut Bot, &mut Soldier, &mut BotKit)>,
    vehicles: Query<(Entity, &Vehicle, Option<&BotVehicle>)>,
    (mut queue, mut gr, mut fx, mut sfx, mut wsfx, impacts): (ResMut<DamageQueue>, ResMut<Grenades>, ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>, ResMut<crate::sfx::WeaponSfx>, Option<Res<crate::impacts::Impacts>>),
    (mut vols, mut uav, mut marks, mut feed, mut log): (ResMut<VehicleVolumes>, ResMut<TeamUav>, ResMut<ReconMarks>, ResMut<crate::combat::KillFeed>, ResMut<BotLog>),
    (mut think, mut buf, mut warned): (Local<HashMap<Entity, f32>>, Local<(Vec<u32>, Vec<Vec3>)>, Local<HashSet<String>>),
) {
    let playing = enabled() && flow.phase == Phase::Playing;
    let (Some(data), Some(kinds)) = (data.filter(|_| playing), kinds.filter(|k| !k.0.is_empty())) else {
        // match over / no drones: the drones go, their operators are free again
        for (e, d) in &drones {
            if let Ok((_, _, _, mut kit)) = bots.get_mut(d.owner) {
                if matches!(kit.mode, Mode::Drone(x) if x == e) {
                    kit.mode = Mode::Foot;
                }
            }
            commands.entity(e).despawn();
        }
        think.clear();
        if let Ok(mut g) = DRONES.lock() {
            g.clear();
        }
        if let Ok(mut g) = HITS.lock() {
            g.clear();
        }
        return;
    };
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    // ---- snapshot of everyone (UE)
    let in_veh: HashSet<Entity> = bots.iter().filter(|x| x.3.in_vehicle().is_some()).map(|x| x.0).collect();
    let soldiers: Vec<(Entity, Vec3, u8, bool)> = bots.iter().filter(|x| !in_veh.contains(&x.0)).map(|(e, _, s, _)| (e, s.center_ue(), s.team, s.alive)).collect();
    let pawn = pawns.single().ok().filter(|p| !p.dead);
    let player_any = pawn.map(|p| to_ue(p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT));
    let player = pawn.filter(|p| !p.in_vehicle).map(|p| to_ue(p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT));
    let vehs: Vec<VehInfo> = vassets.as_ref().map(|a| botvehicles::vehicle_infos(a, vehicles.iter())).unwrap_or_default();
    let pdrone = crate::drone::target();
    let infos: Vec<DroneInfo> = drones.iter().filter_map(|(e, d)| kinds.0.get(d.kind).map(|a| info_of(e, &d, a))).collect();
    let enemies_bevy = |team: u8| -> Vec<Vec3> {
        let mut v: Vec<Vec3> = soldiers.iter().filter(|s| s.3 && s.2 != team).map(|s| to_bevy(s.1)).collect();
        if team != player_team() {
            v.extend(player_any.map(to_bevy));
        }
        v.extend(vehs.iter().filter(|x| x.team.is_some_and(|t| t != team)).map(|x| to_bevy(x.centre)));
        v
    };
    // ---- the drones in the field
    let mut gone: Vec<(Entity, Entity, &'static str)> = Vec::new();
    for (e, mut d) in &mut drones {
        let Some(a) = kinds.0.get(d.kind) else {
            gone.push((e, d.owner, "drone kind not loaded"));
            continue;
        };
        if d.health <= 0.0 {
            continue;
        }
        // the operator: alive and steering this drone (else the drone is lost)
        let owner_ok = bots.get(d.owner).is_ok_and(|(_, _, s, k)| s.alive && matches!(k.mode, Mode::Drone(x) if x == e));
        if !owner_ok {
            let dead = bots.get(d.owner).map(|x| !x.2.alive).unwrap_or(true);
            if dead {
                // NOT from data: the drone goes up with its operator (drone_damage: no reward)
                d.health = 0.0;
                d.last_hit = None;
                log.event(now, format!("{} ({}) died: its drone {} is lost", d.name, team_name(d.team), a.key));
            } else {
                gone.push((e, d.owner, "operator busy elsewhere"));
            }
            continue;
        }
        if d.fuel <= 0.0 {
            gone.push((e, d.owner, "battery empty"));
            continue;
        }
        // the operator crouches where it stands and does not board vehicles meanwhile
        if let Ok((_, mut b, mut s, mut kit)) = bots.get_mut(d.owner) {
            s.crouched = true;
            kit.board_cd = kit.board_cd.max(1.0);
            let to = to_ue(d.feet) - s.center_ue();
            if to.truncate().length() > 50.0 {
                b.yaw = to.y.atan2(to.x);
            }
        }
        let team = d.team;
        let skill = skill_factor(team).max(0.5);
        let rockets: &[(crate::weaponcfg::RocketMode, usize)] = data.drone_rockets.get(a.key).map(|v| v.as_slice()).unwrap_or(&[]);
        let g = &a.gun;
        let has_gun = rockets.is_empty() && !g.dmg_type.is_empty();
        let armed = has_gun || !rockets.is_empty();
        let me_info = info_of(e, &d, a);
        let o = muzzle(&d, a);
        let range = if has_gun {
            g.range
        } else if armed {
            ROCKET_RANGE
        } else {
            a.mark.map(|m| m.0).unwrap_or(3000.0)
        };
        // ---- target (every 0.3 s): nearest in sight and range; vehicles first for rockets
        d.think -= dt;
        if d.think <= 0.0 {
            d.think = 0.3;
            let mut best: Option<(f32, DTgt)> = None;
            let mut consider = |s: f32, t: DTgt| {
                if best.is_none_or(|b| s < b.0) {
                    best = Some((s, t));
                }
            };
            for s in soldiers.iter().filter(|s| s.3 && s.2 != team) {
                let dd = s.1.distance(o);
                let aim = s.1 + Vec3::Z * 20.0;
                if dd < range && (has_gun || !armed || dd > ROCKET_MIN) && sees(&col, o, aim, &mut buf.0) && crate::vehicles::shot_block(o, (aim - o).normalize_or_zero(), dd - 50.0).is_none() {
                    consider(dd, DTgt::Soldier(s.0));
                }
            }
            if team != player_team() {
                if let Some(p) = player {
                    let dd = p.distance(o);
                    if dd < range && (has_gun || !armed || dd > ROCKET_MIN) && sees(&col, o, p + Vec3::Z * 20.0, &mut buf.0) && crate::vehicles::shot_block(o, (p - o).normalize_or_zero(), dd - 50.0).is_none() {
                        consider(dd, DTgt::Player);
                    }
                }
                if let Some(c) = pdrone.filter(|_| armed) {
                    let dd = c.distance(o);
                    if dd < range && sees(&col, o, c, &mut buf.0) {
                        consider(dd * 0.8, DTgt::PlayerDrone);
                    }
                }
            }
            if !rockets.is_empty() {
                for x in vehs.iter().filter(|x| x.team.is_some_and(|t| t != team)) {
                    let dd = x.centre.distance(o);
                    if dd < ROCKET_RANGE * 1.3 && sees(&col, o, x.centre, &mut buf.0) {
                        consider(dd * 0.4, DTgt::Vehicle(x.e));
                    }
                }
            }
            if armed {
                for x in infos.iter().filter(|x| x.team != team) {
                    let dd = x.centre.distance(o);
                    if dd < range && sees(&col, o, x.centre, &mut buf.0) {
                        consider(dd * 0.8, DTgt::Drone(x.e));
                    }
                }
            }
            let new = best.map(|b| b.1);
            if new != d.target {
                d.seen = 0.0;
                if let Some(t) = new {
                    let what = match t {
                        DTgt::Soldier(s) => bots.get(s).map(|x| x.2.name.clone()).unwrap_or_default(),
                        DTgt::Player => "the player".into(),
                        DTgt::Vehicle(v) => vehs.iter().find(|x| x.e == v).map(|x| x.name.clone()).unwrap_or_default(),
                        DTgt::Drone(_) => "an enemy drone".into(),
                        DTgt::PlayerDrone => "the player's drone".into(),
                    };
                    log.event(now, format!("{}'s drone {} ({}) engages {what}", d.name, a.key, team_name(team)));
                }
            }
            d.target = new;
        }
        let tpos = d.target.and_then(|t| match t {
            DTgt::Soldier(s) => soldiers.iter().find(|x| x.0 == s && x.3).map(|x| x.1 + Vec3::Z * 10.0),
            DTgt::Player => player,
            DTgt::Vehicle(v) => vehs.iter().find(|x| x.e == v).map(|x| x.centre),
            DTgt::Drone(x) => infos.iter().find(|i| i.e == x).map(|i| i.centre),
            DTgt::PlayerDrone => pdrone,
        });
        if tpos.is_none() {
            d.target = None;
        }
        d.seen += dt;
        // ---- movement: hold while a target is in reach, else along the walk grid to the goal
        let hold_at = if armed { range * 0.7 } else { range * 0.5 };
        d.hold = tpos.is_some_and(|t| t.distance(o) < hold_at);
        d.repath -= dt;
        if d.goal.is_none() || d.repath <= 0.0 {
            let from = d.feet;
            let ng = match tpos {
                Some(t) => Some(to_bevy(t)),
                None => drone_goal(&obj, team, from, &enemies_bevy(team)),
            };
            if ng.is_some_and(|n| d.goal.is_none_or(|old| old.distance(n) > 4.0)) {
                d.path.clear();
                d.best = f32::MAX;
                d.best_t = 0.0;
            }
            d.goal = ng;
            d.repath = 4.0;
        }
        let flat = |p: Vec3, q: Vec3| Vec2::new(p.x - q.x, p.z - q.z).length();
        // the floor below (air drones fly above it)
        let ground = if a.flying { col.floor(d.feet + Vec3::Y * 0.3, 60.0, &mut buf.0).map(|(y, _)| y) } else { Some(d.feet.y) };
        let base = Vec3::new(d.feet.x, ground.unwrap_or(d.feet.y - AIR_ALT * 0.01), d.feet.z);
        let mut dir = Vec3::ZERO;
        if let (Some(goal), false) = (d.goal, d.hold) {
            if d.path.is_empty() {
                if let Some(grid) = nav.grid.as_ref() {
                    if let (Some(s0), Some(z)) = (grid.nearest(base), grid.nearest(goal)) {
                        let z = if grid.comp[z as usize] == grid.comp[s0 as usize] { Some(z) } else { grid.nearest_in_comp(goal, grid.comp[s0 as usize], 30.0).or_else(|| grid.closest_in_comp(goal, grid.comp[s0 as usize])) };
                        if let Some(p) = z.and_then(|z| grid.path(s0, z, 150_000, &nav.penalty)) {
                            d.path = p.iter().step_by(2).map(|&n| grid.nodes[n as usize]).collect();
                            if let Some(&l) = p.last() {
                                d.path.push(grid.nodes[l as usize]);
                            }
                        }
                    }
                }
                if d.path.is_empty() {
                    d.path.push(goal);
                }
            }
            while d.path.len() > 1 && flat(d.path[0], base) < 1.5 {
                d.path.remove(0);
            }
            let dist_goal = flat(goal, base);
            if dist_goal > 3.0 {
                // look ahead: the farthest of the next waypoints in sight
                let eye = base + Vec3::Y * 0.5;
                let mut wp = d.path[0];
                for &p in d.path.iter().take(5).skip(1) {
                    let q = p + Vec3::Y * 0.5;
                    let v = q - eye;
                    if col.raycast(eye, v.normalize_or_zero(), v.length(), &mut buf.0).is_none() {
                        wp = p;
                    } else {
                        break;
                    }
                }
                dir = Vec3::new(wp.x - base.x, 0.0, wp.z - base.z).normalize_or_zero();
                // no progress for 5 s: plan again and step aside for a second
                if dist_goal < d.best - 1.0 {
                    d.best = dist_goal;
                    d.best_t = 0.0;
                } else {
                    d.best_t += dt;
                }
                if d.best_t > 5.0 {
                    d.best_t = 0.0;
                    d.best = dist_goal;
                    d.path.clear();
                    d.side = 1.0;
                    d.side_dir = if d.rnd() < 0.5 { -1.0 } else { 1.0 };
                }
            }
        }
        if d.side > 0.0 {
            d.side -= dt;
            if dir != Vec3::ZERO {
                dir = Vec3::new(-dir.z, 0.0, dir.x) * d.side_dir;
            }
        }
        // ---- facing: the target, else the way it goes
        let want_yaw = match tpos {
            Some(t) => Some((t.y - o.y).atan2(t.x - o.x)),
            None if dir != Vec3::ZERO => Some(dir.z.atan2(dir.x)),
            None => None,
        };
        if let Some(w) = want_yaw {
            let dy = wrap(w - d.yaw);
            d.yaw = wrap(d.yaw + dy.clamp(-TURN_RATE * dt, TURN_RATE * dt));
        }
        let want_pitch = tpos.map(|t| (t.z - o.z).atan2(Vec2::new(t.x - o.x, t.y - o.y).length().max(1.0)).clamp(-0.9, 0.6)).unwrap_or(0.0);
        d.pitch += (want_pitch - d.pitch).clamp(-TURN_RATE * dt, TURN_RATE * dt);
        // ---- drive / fly
        let speed = a.speed * 0.01;
        if a.flying {
            // as the player's air drone: speed eased in / out, the body leans into the motion
            // (drone.rs FLY_ACCEL / FLY_TILT); height AIR_ALT above the floor, climbing when the
            // way ahead is blocked
            let want_y = base.y + (AIR_ALT + d.climb) * 0.01 - a.half_height * 0.01;
            let vy = ((want_y - d.feet.y) * 1.5).clamp(-speed, speed);
            let mut want = dir * speed;
            want.y = vy;
            if want.length() > speed {
                want = want.normalize() * speed;
            }
            let k = (crate::drone::FLY_ACCEL * dt).min(1.0);
            let v0 = d.vel;
            d.vel = v0 + (want - v0) * k;
            let f = Vec3::new(d.yaw.cos(), 0.0, d.yaw.sin());
            let r = Vec3::new(-d.yaw.sin(), 0.0, d.yaw.cos());
            let sp = speed.max(0.1);
            let tilt = Vec2::new(-d.vel.dot(f) / sp, -d.vel.dot(r) / sp) * crate::drone::FLY_TILT.to_radians();
            let kt = (crate::drone::FLY_ACCEL * 1.5 * dt).min(1.0);
            let t0 = d.anim.tilt;
            d.anim.tilt = t0 + (tilt - t0) * kt;
            let step = d.vel * dt;
            let len = step.length();
            let mut blocked = false;
            if len > 1e-5 {
                let c = d.feet + Vec3::Y * a.half_height * 0.01;
                let clear = a.radius.max(a.half_height) * 0.01;
                let free = col.raycast(c, step / len, len + clear, &mut buf.0).map(|(t, _)| (t - clear).max(0.0)).unwrap_or(len);
                d.feet += step / len * free.min(len);
                if free < len {
                    d.vel *= 0.3;
                    blocked = true;
                }
            }
            // a wall within 2 m ahead: climb over it
            if dir != Vec3::ZERO {
                let c = d.feet + Vec3::Y * a.half_height * 0.01;
                blocked |= col.raycast(c, dir, 2.0, &mut buf.0).is_some();
            }
            d.climb = if blocked { (d.climb + 400.0 * dt).min(AIR_CLIMB_MAX) } else { (d.climb - 150.0 * dt).max(0.0) };
            d.on_ground = false;
        } else {
            let (mut feet, mut vel, mut on_ground) = (d.feet, d.vel, d.on_ground);
            crate::player::walk(&col, &mut feet, &mut vel, &mut on_ground, true, dir, speed, false, dt, &mut buf);
            d.feet = feet;
            d.vel = vel;
            d.on_ground = on_ground;
        }
        let moving = d.vel.length() > 0.2;
        d.fuel -= dt * if moving { a.fuel_moving } else { a.fuel_idle };
        // HB_BOTDRONEDBG=1: state of every bot drone each second (tests)
        d.dbg_t -= dt;
        if *ENV_DBG.get_or_init(|| std::env::var_os("HB_BOTDRONEDBG").is_some()) && d.dbg_t <= 0.0 {
            d.dbg_t = 1.0;
            info!("botdrone {} {} team {} feet ({:.1} {:.1} {:.1}) vel {:.2} yaw {:.2} pitch {:.2} target {:?} hold {} path {} goal {:?} fuel {:.0} health {:.0} heat {:.1} ammo {:?}", d.name, a.key, d.team, d.feet.x, d.feet.y, d.feet.z, d.vel.length(), d.yaw, d.pitch, d.target, d.hold, d.path.len(), d.goal.map(|g| (g * 10.0).round() / 10.0), d.fuel, d.health, d.heat, d.ammo);
        }
        d.gas += ((moving as u8 as f32) - d.gas) * (dt * 3.0).min(1.0);
        // engine (VehicleAudio SCEngineIdle) at the drone; NOT from data: the loop volume over
        // the throttle
        if !g.engine_idle.is_empty() {
            sfx.looping_layers(&format!("botdrone{}", e.index()), &g.engine_idle, Some(me_info.centre), &[("Gas", (d.gas > 0.5) as usize)], 1.0, &[("", 0.6 + 0.4 * d.gas)]);
        }
        // ---- recon drone: marks the enemies near it in sight (GCLockOnTargeterRecon distance,
        // DroneMarkDuration); NOT from data: shared with the team (the player's HUD marks for the
        // player's team, the team UAV for the bots)
        if let Some((dist, secs)) = a.mark {
            d.mark_t -= dt;
            if d.mark_t <= 0.0 {
                d.mark_t = 0.5;
                for s in soldiers.iter().filter(|s| s.3 && s.2 != team) {
                    if s.1.distance(me_info.centre) <= dist && sees(&col, me_info.centre, s.1 + Vec3::Z * 30.0, &mut buf.0) {
                        if team == player_team() {
                            marks.marked.insert(s.0, secs);
                        }
                    }
                }
                let u = &mut uav.0[team as usize & 1];
                *u = u.max(1.0);
            }
        }
        // ---- weapons
        let reaction = 0.6 / skill;
        d.fire_cd = (d.fire_cd - dt).max(0.0);
        if d.overheated > 0.0 {
            d.overheated -= dt;
            if d.overheated <= 0.0 {
                d.heat = 0.0;
            }
        } else {
            d.heat = (d.heat - dt).max(0.0);
        }
        for m in 0..rockets.len().min(2) {
            if d.reload[m] > 0.0 {
                d.reload[m] -= dt;
                if d.reload[m] <= 0.0 {
                    d.ammo[m] = rockets[m].0.clip;
                }
            }
        }
        let Some(mut aim) = tpos else {
            d.burst = None;
            continue;
        };
        let is_veh = matches!(d.target, Some(DTgt::Vehicle(_)));
        let view_dir = |yaw: f32, pitch: f32| Vec3::new(yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), pitch.sin());
        let cur = view_dir(d.yaw, d.pitch);
        if has_gun {
            let want = (aim - o).normalize_or_zero();
            let err = cur.dot(want).clamp(-1.0, 1.0).acos().to_degrees();
            // NOT from data: bursts 1.6 s on / 0.7 s off, fire within 4 deg, extra deviation 1 deg
            // / skill (as the bot vehicle gunners)
            d.gun_t += dt;
            if d.gun_t > 2.3 {
                d.gun_t = 0.0;
            }
            if d.seen >= reaction && d.gun_t < 1.6 && err < 4.0 && d.overheated <= 0.0 && d.fire_cd <= 0.0 && aim.distance(o) <= g.range {
                d.fire_cd = g.fire_interval;
                d.heat += g.overheat_add + g.fire_interval;
                if d.heat >= g.overheat_max {
                    d.overheated = g.overheat_recover;
                }
                d.fuel -= a.fuel_shot;
                d.shots += 1;
                let mut seed = d.seed;
                let dd = crate::combat::spread_dir(want, g.spread * 0.5 + 1.0 / skill, &mut seed);
                d.seed = seed;
                if g.sound_looping {
                    wsfx.shot_cue(&format!("botdrone{}", e.index()), &g.sound, Some(o), g.fire_interval + 0.06);
                } else {
                    sfx.play(&g.sound, Some(o));
                }
                let pl = player.map(|c| (c, true));
                let shot = crate::combat::trace_shot(&col, &soldiers, pl, o, dd, g.range, Some(d.owner), team == player_team(), &mut buf.0);
                if let Some(im) = impacts.as_ref() {
                    let center = match &shot {
                        ShotHit::Soldier(se, ..) => soldiers.iter().find(|s| s.0 == *se).map(|s| s.1),
                        ShotHit::Player(..) => player,
                        _ => None,
                    };
                    im.shot(&shot, o, dd, &g.dmg_type, center, &mut fx, &mut sfx);
                }
                let attacker = Actor::Bot(d.owner);
                let region = |c: Vec3, hp: Vec3| if hp.z > c.z + crate::combat::HEAD_OFFSET - crate::combat::HEAD_RADIUS { ("PMA_Head", true) } else { ("PMA_Soldier_Chest", false) };
                let shot_t = match &shot {
                    ShotHit::World(t, _, _) | ShotHit::Soldier(_, t, _) | ShotHit::Player(t, _) => *t,
                    ShotHit::Nothing => g.range,
                };
                match &shot {
                    ShotHit::Soldier(se, t, hp) => {
                        if let Some(s) = soldiers.iter().find(|s| s.0 == *se).filter(|s| s.2 != team) {
                            let (mat, head) = region(s.1, *hp);
                            let dmg = data.grid.damage(&g.dmg_type, mat, *t, a.full_range, g.range);
                            queue.0.push(Hit { target: Target::Bot(*se), amount: dmg, attacker, attacker_team: team, headshot: head, explosive: false, from: o, icon: None, dmg_type: Some(g.dmg_type.clone()) });
                        }
                    }
                    ShotHit::Player(t, hp) if team != player_team() => {
                        let (mat, head) = region(player.unwrap_or(*hp), *hp);
                        let dmg = data.grid.damage(&g.dmg_type, mat, *t, a.full_range, g.range);
                        queue.0.push(Hit { target: Target::Player, amount: dmg, attacker, attacker_team: team, headshot: head, explosive: false, from: o, icon: None, dmg_type: Some(g.dmg_type.clone()) });
                    }
                    _ => {}
                }
                // the player's drone: a hit when the bullet passes within its radius (bots.rs)
                if let (Some(c), true) = (pdrone, team != player_team()) {
                    let along = (c - o).dot(dd);
                    if along > 0.0 && along < shot_t && (o + dd * along).distance(c) < PLAYER_DRONE_RADIUS {
                        let dmg = data.grid.damage(&g.dmg_type, "PMA_Soldier_Chest", along, a.full_range, g.range);
                        queue.0.push(Hit { target: Target::Drone, amount: dmg, attacker, attacker_team: team, headshot: false, explosive: false, from: o, icon: None, dmg_type: None });
                    }
                }
                // enemy bot drones (their boxes stop the shot)
                if let Some((de, t)) = bullet_hit(o, dd, &shot, g.range, team) {
                    let dmg = data.grid.damage(&g.dmg_type, "PMA_Soldier_Chest", t, a.full_range, g.range);
                    queue.0.push(Hit { target: Target::BotDrone(de), amount: dmg, attacker, attacker_team: team, headshot: false, explosive: false, from: o, icon: None, dmg_type: Some(g.dmg_type.clone()) });
                }
            }
        } else if !rockets.is_empty() {
            // lead a vehicle for the missile speed (straight flight)
            let m_pref = if is_veh && rockets.len() > 1 { 1 } else { 0 };
            if let Some(DTgt::Vehicle(ve)) = d.target {
                if let Some(x) = vehs.iter().find(|x| x.e == ve) {
                    let s = rockets[m_pref].0.missile.speed.max(500.0);
                    for _ in 0..2 {
                        aim = x.centre + x.vel * (aim.distance(o) / s);
                    }
                }
            }
            let want = (aim - o).normalize_or_zero();
            let err = cur.dot(want).clamp(-1.0, 1.0).acos().to_degrees();
            if d.burst.is_none() && d.seen >= reaction && err < 3.0 {
                let m = if d.ammo[m_pref] > 0 && d.reload[m_pref] <= 0.0 { Some(m_pref) } else { (0..rockets.len().min(2)).find(|&m| d.ammo[m] > 0 && d.reload[m] <= 0.0) };
                if let Some(m) = m {
                    d.burst = Some((m, rockets[m].0.burst.max(1).min(d.ammo[m]), 0.0));
                }
            }
            if let Some((m, left, cd)) = d.burst {
                let cd = cd - dt;
                if cd > 0.0 {
                    d.burst = Some((m, left, cd));
                } else {
                    let (rm, wi) = &rockets[m];
                    // Seat0FireMode<m>Socket<k> in the turret's frame (as the player's drone), out
                    // of the drone's own box
                    let view = Quat::from_rotation_z(d.yaw) * Quat::from_rotation_y(-d.pitch);
                    let socket = to_ue(d.feet) + Vec3::Z * (a.half_height + a.camera_z + a.lift());
                    let sock = a.fire_sockets.get(m).filter(|v| !v.is_empty()).map(|v| v[d.shots % v.len()]).unwrap_or([45.0, 0.0, 0.0]);
                    let start = socket + view * Vec3::from(sock);
                    let mut seed = d.seed;
                    let dir = crate::combat::spread_dir((aim - start).normalize_or_zero(), rm.spread * 0.5 + 0.5 / skill, &mut seed);
                    d.seed = seed;
                    let start = out_of_box(&me_info.vol, start, dir);
                    let trail = rm.missile.flight_fx.as_ref().and_then(|t| fx.fire_tracked(t, start, crate::combat::dir_quat(dir)));
                    // GCProj_Rocket LifeSpan 8 (as the player's drone)
                    gr.list.push(Grenade { rocket: Some(*wi), pos: start, vel: dir * rm.missile.speed, fuse: 8.0, owner: Actor::Bot(d.owner), team, resting: false, trail, tac: None, payload: None });
                    let seek = match d.target {
                        Some(DTgt::Vehicle(ve)) if rm.lock_on => Some(Seek { target: ve, age: 0.0 }),
                        Some(DTgt::Drone(de)) if rm.lock_on => Some(Seek { target: de, age: 0.0 }),
                        _ => None,
                    };
                    let n = gr.list.len();
                    gr.seek.resize(n - 1, None);
                    gr.seek.push(seek);
                    if let Some(snd) = a.mode_sounds.get(m).filter(|s| !s.is_empty()) {
                        sfx.play(snd, Some(start));
                    }
                    d.fuel -= a.fuel_shot;
                    d.shots += 1;
                    d.ammo[m] -= 1;
                    if d.ammo[m] <= 0 {
                        d.reload[m] = rm.reload;
                    }
                    d.burst = (left > 1 && d.ammo[m] > 0).then_some((m, left - 1, rm.interval));
                    if is_veh {
                        let tn = match d.target {
                            Some(DTgt::Vehicle(ve)) => vehs.iter().find(|x| x.e == ve).map(|x| x.name.clone()).unwrap_or_default(),
                            _ => String::new(),
                        };
                        log.event(now, format!("{}'s drone {} ({}) fires {} at {tn} {:.0} m{}", d.name, a.key, team_name(team), rm.missile.class, aim.distance(o) / 100.0, if seek.is_some() { " (seeking)" } else { "" }));
                    }
                }
            }
        }
    }
    for (e, owner, why) in gone {
        if let Ok((_, mut b, mut s, mut kit)) = bots.get_mut(owner) {
            if matches!(kit.mode, Mode::Drone(x) if x == e) {
                kit.mode = Mode::Foot;
                s.crouched = false;
                b.pitch = 0.0;
            }
            let kills = drones.get(e).map(|x| s.kills - x.1.kills_at).unwrap_or(0);
            log.event(now, format!("{} ({}) drone back: {why}; {kills} kills while it was out", s.name, team_name(s.team)));
        }
        commands.entity(e).despawn();
    }
    // ---- deploy decisions (every 1 s per bot)
    let mut used: HashSet<(usize, usize)> = drones.iter().map(|(_, d)| (d.kind, d.inst)).collect();
    let mut per_team = [0usize; 2];
    for (_, d) in &drones {
        per_team[d.team as usize & 1] += 1;
    }
    let forced = force();
    for (e, b, mut s, mut kit) in &mut bots {
        if !s.alive || !matches!(kit.mode, Mode::Foot) || in_veh.contains(&e) {
            continue;
        }
        let t = think.entry(e).or_insert(0.5 + (kit.seed % 100) as f32 * 0.01);
        *t -= dt;
        if *t > 0.0 {
            continue;
        }
        *t = 1.0;
        if kit.ks_cd > 0.0 && !forced {
            continue;
        }
        let Some(item) = kit.items.iter().find(|i| i.starts_with("cfg_dro_")).cloned() else { continue };
        let key = kind_override().map(|k| crate::DRONE_KINDS[k].to_string()).unwrap_or(item);
        let Some(k) = kinds.0.iter().position(|a| a.key == key) else {
            if warned.insert(key.clone()) {
                log.event(now, format!("bots: drone {key} not loaded, not used"));
            }
            continue;
        };
        let a = &kinds.0[k];
        let team = s.team as usize & 1;
        let Some(inst) = (0..a.bots.len()).find(|i| !used.contains(&(k, *i))) else { continue };
        if per_team[team] >= TEAM_DRONES {
            continue;
        }
        let cost = data.costs.get(&key).map(|c| c.0).unwrap_or(400);
        if kit.kp < cost && !forced {
            continue;
        }
        let me = s.center_ue();
        let near_enemy = soldiers.iter().any(|x| x.3 && x.2 != s.team && x.1.distance(me) < DEPLOY_RANGE)
            || (s.team != player_team() && player_any.is_some_and(|p| p.distance(me) < DEPLOY_RANGE))
            || vehs.iter().any(|x| x.team.is_some_and(|t| t != s.team) && x.centre.distance(me) < DEPLOY_RANGE);
        let near_obj = obj.list.iter().any(|o| o.line == obj.active_line && o.owner != Some(s.team) && Vec3::from(o.location).distance(me) < OBJ_RANGE);
        if !forced && (!(near_enemy || near_obj) || kit.rnd() >= 0.5) {
            continue;
        }
        // set down 1.2 m in front of the bot (on the floor; air drones at its eye height)
        let fwd = Vec3::new(b.yaw.cos(), 0.0, b.yaw.sin());
        let mut at = s.feet + fwd * 1.2;
        let chest = s.feet + Vec3::Y * 1.0;
        let clear = col.raycast(chest, fwd, 1.5, &mut buf.0).is_none();
        match col.floor(at + Vec3::Y * 1.0, 3.0, &mut buf.0) {
            Some((y, _)) if clear => at.y = y,
            _ => at = s.feet,
        }
        if a.flying {
            at.y = s.feet.y + 1.6 - a.half_height * 0.01;
        }
        let paid = cost.min(kit.kp.max(0));
        kit.kp -= paid;
        kit.ks_cd = DEPLOY_CD;
        let modes: &[(crate::weaponcfg::RocketMode, usize)] = data.drone_rockets.get(&key).map(|v| v.as_slice()).unwrap_or(&[]);
        let mut ammo = [0; 2];
        for (m, (rm, _)) in modes.iter().enumerate().take(2) {
            ammo[m] = rm.clip;
        }
        let de = commands
            .spawn(BotDrone {
                owner: e,
                name: s.name.clone(),
                team: s.team,
                kind: k,
                inst,
                feet: at,
                yaw: b.yaw,
                pitch: 0.0,
                vel: Vec3::ZERO,
                on_ground: false,
                health: crate::drone::DRONE_HEALTH,
                fuel: a.fuel_max,
                heat: 0.0,
                overheated: 0.0,
                fire_cd: 0.3,
                ammo,
                reload: [0.0; 2],
                burst: None,
                shots: 0,
                path: Vec::new(),
                repath: 0.0,
                goal: None,
                best: f32::MAX,
                best_t: 0.0,
                side: 0.0,
                side_dir: 1.0,
                target: None,
                think: 0.0,
                seen: 0.0,
                gun_t: 0.0,
                anim: DroneAnim::new(b.yaw),
                seed: kit.seed | 1,
                last_hit: None,
                mark_t: 0.0,
                kills_at: s.kills,
                climb: 0.0,
                hold: false,
                gas: 0.0,
                dbg_t: 0.0,
            })
            .id();
        kit.mode = Mode::Drone(de);
        s.crouched = true;
        used.insert((k, inst));
        per_team[team] += 1;
        if let Some(c) = data.costs.get(&key).and_then(|c| c.2.clone()).filter(|_| s.team == player_team()) {
            sfx.play(&c, None);
        }
        // kill crawl line so the player sees it (NOT from data), as the bots' airstrikes
        feed.0.push((format!("{}: {}", s.name, label(&data, &key)), s.team, String::new(), 2, None, crate::objectives::CRAWL_SECS));
        log.event(now, format!("{} ({}) killstreak {key} ({paid} KP) at ({:.1} {:.1} {:.1}), copy {inst}{}{} -> {} KP", s.name, team_name(s.team), at.x, at.y, at.z, if near_enemy { ", enemy near" } else { "" }, if forced { ", HB_BOTDRONE" } else { "" }, kit.kp));
    }
    // ---- hit boxes for this frame (vehicles.rs cleared and filled them before)
    let infos: Vec<DroneInfo> = drones.iter().filter(|(_, d)| d.health > 0.0).filter_map(|(e, d)| kinds.0.get(d.kind).map(|a| info_of(e, &d, a))).collect();
    vols.0.extend(infos.iter().map(|i| i.vol));
    if let Ok(mut g) = crate::vehicles::SHOT_VOLUMES.lock() {
        g.extend(infos.iter().map(|i| i.vol));
    }
    if let Ok(mut g) = DRONES.lock() {
        *g = infos;
    }
}

/// Hits on the bot drones (combat.rs: bullets as Target::BotDrone, rockets / explosions on their
/// boxes as vehicle hits): friendly fire off, destroyed at 0 health (the player drone's effect,
/// PSR_DestroyDrone for the enemy who did it, a kill crawl line), the operator back on foot.
#[allow(clippy::too_many_arguments)]
fn drone_damage(
    time: Res<Time>,
    mut flow: ResMut<MatchFlow>,
    (data, kinds): (Option<Res<CombatData>>, Option<Res<DroneKinds>>),
    mut vdmg: ResMut<VehicleDamage>,
    mut drones: Query<(Entity, &mut BotDrone)>,
    mut bots: Query<(&mut Soldier, &mut BotKit)>,
    mut commands: Commands,
    (mut fx, mut sfx, mut feed, mut log): (ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>, ResMut<crate::combat::KillFeed>, ResMut<BotLog>),
) {
    let mut hits: Vec<(Entity, Hit)> = HITS.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    vdmg.0.retain(|(e, h)| {
        if drones.contains(*e) {
            hits.push((*e, h.clone()));
            false
        } else {
            true
        }
    });
    for (e, h) in hits {
        let Ok((_, mut d)) = drones.get_mut(e) else { continue };
        if d.health <= 0.0 || (h.attacker_team == d.team && h.attacker != Actor::World) {
            continue;
        }
        d.health -= h.amount;
        d.last_hit = Some((h.attacker, h.attacker_team, h.icon.clone()));
        if std::env::var_os("HB_DMGDBG").is_some() {
            info!("bot drone of {} hit {:?} {:.0} -> health {:.0}", d.name, h.dmg_type, h.amount, d.health);
        }
    }
    let now = time.elapsed_secs();
    for (e, d) in &drones {
        if d.health > 0.0 {
            continue;
        }
        let key = kinds.as_ref().and_then(|k| k.0.get(d.kind)).map(|a| a.key).unwrap_or("cfg_dro_gassault");
        let at = kinds.as_ref().and_then(|k| k.0.get(d.kind)).map(|a| info_of(e, d, a).centre).unwrap_or(to_ue(d.feet));
        fx.fire(DESTROY_FX, at, Quat::IDENTITY);
        sfx.play(DESTROY_SOUND, Some(at));
        let mut by = String::from("-");
        if let Some((attacker, ateam, icon)) = d.last_hit.clone() {
            let enemy = ateam != d.team;
            let name = match attacker {
                Actor::Player => {
                    if enemy {
                        // GCBattlePoints Rewards PSR_DestroyDrone, as for the vehicles
                        flow.hud.kp += DESTROY_DRONE_BP;
                        flow.hud.rewards.push((DESTROY_DRONE_BP, "PSR_DestroyDrone"));
                    }
                    "Du".to_string()
                }
                Actor::Bot(b) => match bots.get_mut(b) {
                    Ok((s, mut kit)) => {
                        if enemy {
                            kit.add_kp(DESTROY_DRONE_BP);
                        }
                        s.name.clone()
                    }
                    Err(_) => String::new(),
                },
                Actor::World => String::new(),
            };
            if attacker != Actor::World {
                // kill crawl: who destroyed whose drone (NOT from data)
                let dname = data.as_deref().map(|dt| label(dt, key)).unwrap_or_else(|| "DROHNE".into());
                feed.0.push((name.clone(), ateam, format!("{dname} ({})", d.name), d.team, icon, crate::objectives::CRAWL_SECS));
                if feed.0.len() > 5 {
                    feed.0.remove(0);
                }
            }
            by = if enemy && attacker != Actor::World { format!("{name} (+{DESTROY_DRONE_BP} KP)") } else { name };
        }
        if let Ok((mut s, mut kit)) = bots.get_mut(d.owner) {
            if matches!(kit.mode, Mode::Drone(x) if x == e) {
                kit.mode = Mode::Foot;
                s.crouched = false;
            }
            let kills = s.kills - d.kills_at;
            log.event(now, format!("{}'s drone {key} ({}) destroyed by {by}; {kills} kills while it was out", d.name, team_name(d.team)));
        }
        commands.entity(e).despawn();
    }
}

/// Last skinned state of a drawable copy (skip re-skinning while unchanged) and whether its mesh
/// entities are shown.
#[derive(Default)]
struct RenderCache {
    shown: HashMap<(usize, usize), bool>,
    skin: HashMap<(usize, usize), (Vec<(Quat, Vec3)>, Quat, Vec3)>,
    tread: HashMap<(usize, usize), (f32, f32)>,
}

/// The bot drones' meshes: the drawable copies (DroneAssets::bots) skinned with the drone rig
/// (drone.rs drone_pose: treads, wheels, turret, rotors, antenna), unused copies hidden.
#[allow(clippy::too_many_arguments)]
fn render_bot_drones(
    time: Res<Time>,
    kinds: Option<Res<DroneKinds>>,
    mut drones: Query<(Entity, &mut BotDrone)>,
    mut meshes: ResMut<Assets<Mesh>>,
    me: Res<crate::MeshEntities>,
    mut ents: Query<(&mut Visibility, &mut Transform), With<Mesh3d>>,
    mut commands: Commands,
    mut cache: Local<RenderCache>,
) {
    let Some(kinds) = kinds else { return };
    // a new level: new mesh entities (spawned visible at the origin)
    if kinds.is_changed() {
        *cache = RenderCache::default();
    }
    let dt = time.delta_secs().min(0.1) * crate::rigdebug::anim_scale();
    let mut by_inst: HashMap<(usize, usize), Entity> = HashMap::new();
    for (e, d) in &drones {
        if d.health > 0.0 {
            by_inst.insert((d.kind, d.inst), e);
        }
    }
    for (k, a) in kinds.0.iter().enumerate() {
        for (i, inst) in a.bots.iter().enumerate() {
            let handles: Vec<&Handle<Mesh>> = inst.part.meshes.iter().chain(inst.attach.iter().flat_map(|p| p.meshes.iter())).collect();
            let user = by_inst.get(&(k, i)).copied();
            let Some(de) = user else {
                if cache.shown.get(&(k, i)) != Some(&false) {
                    let mut complete = true;
                    for h in &handles {
                        let Some(es) = me.0.get(&h.id()) else {
                            complete = false;
                            continue;
                        };
                        for &e in es {
                            if let Ok((mut v, _)) = ents.get_mut(e) {
                                *v = Visibility::Hidden;
                            }
                        }
                    }
                    if complete {
                        cache.shown.insert((k, i), false);
                        cache.skin.remove(&(k, i));
                        cache.tread.remove(&(k, i));
                    }
                }
                continue;
            };
            let Ok((_, mut d)) = drones.get_mut(de) else { continue };
            let origin = mesh_origin(&d, a);
            let tr = Vec3::new(origin.x, origin.z, origin.y) * 0.01;
            let (yaw, pitch, vel) = (d.yaw, d.pitch, d.vel);
            let pose = crate::drone::drone_pose(a, &mut d.anim, yaw, pitch, vel, origin, dt);
            let scales = crate::drone::rotor_scales(a);
            let tilt = d.anim.tilt;
            let body_rot = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(-tilt.x) * Quat::from_rotation_x(tilt.y);
            let unchanged = cache.skin.get(&(k, i)).is_some_and(|(p, r, o)| *r == body_rot && *o == origin && *p == pose);
            if !unchanged {
                crate::bots::skin_part_scaled(&inst.part, &pose, body_rot, origin, &mut meshes, Some(&scales));
                for at in &inst.attach {
                    crate::bots::skin_part(at, &pose, body_rot, origin, &mut meshes);
                }
                cache.skin.insert((k, i), (pose, body_rot, origin));
            }
            if let Some((tex_pan, _)) = &a.tread {
                // the powered wheels' rotation: tread distance / WheelRadius (as drone.rs)
                let rf = a.rig.roll_factor.max(0.01);
                let roll = d.anim.roll();
                if cache.tread.get(&(k, i)) != Some(&roll) {
                    crate::viewmodel::scroll_tread(&mut meshes, &inst.tread[0], (roll.0 / rf).to_degrees(), *tex_pan);
                    crate::viewmodel::scroll_tread(&mut meshes, &inst.tread[1], (roll.1 / rf).to_degrees(), *tex_pan);
                    cache.tread.insert((k, i), roll);
                }
            }
            for h in &handles {
                for &e in me.0.get(&h.id()).into_iter().flatten() {
                    if let Ok((mut v, mut tf)) = ents.get_mut(e) {
                        if tf.translation != tr {
                            tf.translation = tr;
                        }
                        if *v != Visibility::Visible {
                            *v = Visibility::Visible;
                            commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                        }
                    }
                }
            }
            cache.shown.insert((k, i), true);
        }
    }
}
