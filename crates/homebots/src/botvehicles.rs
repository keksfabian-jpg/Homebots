//! Bots in vehicles: buying (GCBattlePoints VehicleCosts BPCost at the respawn, like the player in
//! the spawn selection), spawning at the team's vehicle spawn points of the active front line
//! (SpawnPoints type by vehicle class, vehicles.rs spawn_type), driving along the walk grid
//! (nav.rs) towards the objectives, flying helicopters (kinematic: altitude hold, approach,
//! orbit), manning gunner seats (team mates board a waiting / slow vehicle), aiming the seats'
//! turrets (TurretControls rates and pitch limits) and firing their guns (the seat's
//! VehicleWeaponInfo fire modes: instant hit with overheat, projectiles with clip / reload),
//! bailing out of a burning ground vehicle (below 30 % health) and abandoning a stuck one.
//!
//! Vehicles of both teams: the USA classes (vehload::CLASSES) and their KVA counterparts
//! (vehload::KVA_CLASSES, same costs in VehicleCosts).
//!
//! NOT from data (the MP game had no bots; marked in MODLOG): who buys what and when, the
//! team limits, the driving / flight control (steering gains, 80 % speed, altitude 25 m,
//! orbit 35 m, acceleration), stuck handling (reverse after 2 s, abandon after 15 s without
//! progress), bail-out below 30 % health, reaction / aim errors and bursts of the gunners,
//! occupants not drawn and protected inside (they die with the vehicle).
use crate::botextras::{enabled, BotKit, Mode, DESTROY_VEHICLE_BP};
use crate::bots::{Bot, BotLog};
use crate::combat::{Actor, CombatData, DamageQueue, Grenade, Grenades, Hit, Seek, ShotHit, Soldier, Target, TEAM_KVA, TEAM_USA};
use crate::menu::{MatchFlow, Phase};
use crate::nav::Nav;
use crate::player::{CollisionWorld, Pawn};
use crate::vehicles::{body_rot, wrap, FireKind, Motion, Vehicle, VehicleAssets, VehicleKind};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// A vehicle the bots use: its team and the bots per seat.
#[derive(Component)]
pub struct BotVehicle {
    pub team: u8,
    pub seats: Vec<Option<Entity>>,
    pub name: String,
    path: Vec<Vec3>,
    repath: f32,
    goal: Option<Vec3>,
    goal_letter: Option<usize>,
    slow: f32,
    reverse: f32,
    reverse_steer: f32,
    best: f32,
    best_t: f32,
    unstick: u32,
    /// seconds left waiting at the spawn for gunners
    wait: f32,
    seat_ai: Vec<SeatAi>,
    seed: u32,
    low_air: f32,
    /// throttle for the engine sound, smoothed (0..1)
    gas: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tgt {
    Soldier(Entity),
    Player,
    Vehicle(Entity),
}

#[derive(Default, Clone)]
struct SeatAi {
    target: Option<Tgt>,
    think: f32,
    seen: f32,
    /// burst: seconds firing / pausing (instant guns)
    burst: f32,
}

impl BotVehicle {
    fn rnd(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 100_000) as f32 / 100_000.0
    }
}

/// Bot vehicles of this frame (vehicles.rs: no unpiloted fall for a bot-flown helicopter, the
/// player's drive leaves the movement to the bot driver, a bot vehicle is recycled last).
static BOT_VEH: Mutex<Vec<(Entity, bool)>> = Mutex::new(Vec::new());

pub fn is_bot_vehicle(e: Entity) -> bool {
    BOT_VEH.lock().map(|v| v.iter().any(|x| x.0 == e)).unwrap_or(false)
}

/// A bot sits in the vehicle's seat 0 (drives / flies it).
pub fn piloted(e: Entity) -> bool {
    BOT_VEH.lock().map(|v| v.iter().any(|x| x.0 == e && x.1)).unwrap_or(false)
}

struct Tracked {
    team: u8,
    name: String,
    occupants: Vec<Entity>,
    pos: Vec3,
    last_hit: Option<(Actor, u8, Option<String>, f32)>,
}

/// Bot vehicles seen last frame (a vanished one with a fresh hit was destroyed: its occupants
/// die, the attacker gets PSR_DestroyVehicle) and vehicles the bots left.
#[derive(Resource, Default)]
pub struct BotVehTrack {
    info: HashMap<Entity, Tracked>,
    abandoned: HashSet<Entity>,
}

impl BotVehTrack {
    pub fn hit(&mut self, e: Entity, h: &Hit, now: f32) {
        if let Some(t) = self.info.get_mut(&e) {
            t.last_hit = Some((h.attacker, h.attacker_team, h.icon.clone(), now));
        }
    }
}

/// A vehicle as a target: team (None: empty), hit box centre and velocity (UE).
#[derive(Clone, Debug)]
pub struct VehInfo {
    pub e: Entity,
    pub team: Option<u8>,
    pub centre: Vec3,
    pub vel: Vec3,
    pub heli: bool,
    pub name: String,
}

/// Team of a vehicle class: GCV_CH_* KVA, the rest USA.
pub fn kind_team(k: &VehicleKind) -> u8 {
    if k.class.to_ascii_uppercase().starts_with("GCV_CH_") {
        TEAM_KVA
    } else {
        TEAM_USA
    }
}

fn box_centre(k: &VehicleKind, v: &Vehicle) -> Vec3 {
    v.pos + body_rot(v) * Vec3::new(0.0, 0.0, (k.top.max(60.0) - k.ride_height) * 0.5)
}

fn vel_of(k: &VehicleKind, v: &Vehicle) -> Vec3 {
    if k.motion == Motion::Heli || v.vel != Vec3::ZERO {
        v.vel
    } else {
        body_rot(v) * Vec3::X * v.speed + Vec3::Z * v.vz
    }
}

pub fn vehicle_infos<'a>(a: &VehicleAssets, it: impl Iterator<Item = (Entity, &'a Vehicle, Option<&'a BotVehicle>)>) -> Vec<VehInfo> {
    it.filter_map(|(e, v, bv)| {
        let k = a.kinds.get(v.kind)?;
        let team = bv.filter(|b| b.seats.iter().any(|s| s.is_some())).map(|b| b.team).or(v.player_seat.map(|_| crate::combat::player_team()));
        Some(VehInfo { e, team, centre: box_centre(k, v), vel: vel_of(k, v), heli: k.motion == Motion::Heli, name: k.name.clone() })
    })
    .collect()
}

fn team_name(t: u8) -> &'static str {
    if t == TEAM_USA {
        "USA"
    } else {
        "KVA"
    }
}

fn to_bevy(ue: Vec3) -> Vec3 {
    Vec3::new(ue.x, ue.z, ue.y) * 0.01
}

fn to_ue(b: Vec3) -> Vec3 {
    Vec3::new(b.x, b.z, b.y) * 100.0
}

/// Ground point (Bevy) under a vehicle origin.
fn vehicle_feet(k: &VehicleKind, v: &Vehicle) -> Vec3 {
    to_bevy(v.pos - Vec3::Z * k.ride_height)
}

/// NOT from data: bot vehicles per team at once, helicopters among them, distance (m) to the
/// goal that counts as arrived, seconds without progress before a ground vehicle is abandoned,
/// health share below which the bots bail out of a ground vehicle, helicopter altitude above the
/// ground and orbit radius (UE).
const TEAM_VEHICLES: usize = 3;
const TEAM_HELIS: usize = 1;
const ARRIVE: f32 = 12.0;
const STUCK_ABANDON: f32 = 15.0;
const BAIL_HEALTH: f32 = 0.3;
const HELI_ALT: f32 = 2500.0;
const HELI_ORBIT: f32 = 3500.0;
const WAIT_GUNNER: f32 = 4.0;

/// The objective (or enemy) a vehicle heads for (Bevy), as goal_for in bots.rs: the nearest
/// open objective of the active line, else the nearest held one, else the nearest enemy.
fn vehicle_goal(obj: &crate::objectives::Objectives, team: u8, from: Vec3, enemies: &[Vec3]) -> Option<(Vec3, Option<usize>)> {
    let pos = |o: &crate::objectives::Objective| Vec3::new(o.location[0], o.location[2], o.location[1]) * 0.01;
    let line: Vec<&crate::objectives::Objective> = obj.list.iter().filter(|o| o.line == obj.active_line).collect();
    fn near<'a>(v: Vec<&'a crate::objectives::Objective>, from: Vec3) -> Option<&'a crate::objectives::Objective> {
        let pos = |o: &crate::objectives::Objective| Vec3::new(o.location[0], o.location[2], o.location[1]) * 0.01;
        v.into_iter().min_by(|a, b| pos(a).distance(from).total_cmp(&pos(b).distance(from)))
    }
    if let Some(o) = near(line.iter().copied().filter(|o| o.owner != Some(team)).collect(), from) {
        return Some((pos(o), Some(o.letter)));
    }
    if let Some(e) = enemies.iter().min_by(|a, b| a.distance(from).total_cmp(&b.distance(from))) {
        return Some((*e, None));
    }
    if let Some(o) = near(line, from) {
        return Some((pos(o), Some(o.letter)));
    }
    obj.roam.iter().max_by(|a, b| a.distance(from).total_cmp(&b.distance(from))).map(|p| (*p, None))
}

/// Shifts a shot's origin along `d` out of the vehicle's own hit box (bots' rockets do not skip
/// their own vehicle in combat.rs).
fn out_of_box(k: &VehicleKind, v: &Vehicle, o: Vec3, d: Vec3) -> Vec3 {
    let r = body_rot(v);
    let inv = r.inverse();
    let lo = inv * (o - v.pos);
    let ld = inv * d;
    let (mn, mx) = (Vec3::new(-k.half_len, -k.half_width, -k.ride_height), Vec3::new(k.half_len, k.half_width, k.top.max(60.0)));
    if !(lo.cmpge(mn - Vec3::splat(5.0)).all() && lo.cmple(mx + Vec3::splat(5.0)).all()) {
        return o;
    }
    let mut t = f32::MAX;
    for a in 0..3 {
        if ld[a].abs() > 1e-4 {
            let edge = if ld[a] > 0.0 { mx[a] } else { mn[a] };
            t = t.min((edge - lo[a]) / ld[a]);
        }
    }
    if t == f32::MAX {
        o
    } else {
        o + d * (t.max(0.0) + 40.0)
    }
}

fn sees(col: &CollisionWorld, from: Vec3, to: Vec3, buf: &mut Vec<u32>) -> bool {
    let (a, b) = (to_bevy(from), to_bevy(to));
    let d = b - a;
    let l = d.length();
    l < 0.5 || col.raycast(a, d / l, (l - 0.5).max(0.0), buf).is_none()
}

/// Vehicle spawn point (Bevy floor point, UE yaw) for a team and class: the active line's
/// SpawnPoints of the class's type (GMST_Jeep as fallback), else the team's base vehicle starts
/// (VehicleSpawnsHigh / Low, "#air" for helicopters); at least 9 m from other vehicles.
#[allow(clippy::too_many_arguments)]
fn spawn_point(col: &CollisionWorld, level: Option<&crate::objectives::LevelData>, obj: &crate::objectives::Objectives, team: u8, k: &VehicleKind, taken: &[Vec3], seed: u32, buf: &mut Vec<u32>) -> Option<(Vec3, f32)> {
    let ty = crate::vehicles::spawn_type(&k.classes);
    let mut pts = crate::objectives::line_spawns(level, obj, team, ty);
    if pts.is_empty() && ty != "GMST_Jeep" {
        pts = crate::objectives::line_spawns(level, obj, team, "GMST_Jeep");
    }
    if pts.is_empty() {
        let group = if team == TEAM_USA { "VehicleSpawnsHigh" } else { "VehicleSpawnsLow" };
        let air = format!("{group}#air");
        if k.motion == Motion::Heli {
            pts = col.starts.iter().filter(|s| s.0 == air).map(|s| (s.1, s.2)).collect();
        }
        if pts.is_empty() {
            pts = col.starts.iter().filter(|s| s.0 == group).map(|s| (s.1, s.2)).collect();
        }
    }
    let free: Vec<(Vec3, f32)> = pts.into_iter().filter(|p| taken.iter().all(|t| Vec2::new(t.x - p.0.x, t.z - p.0.z).length() > 9.0)).collect();
    if free.is_empty() {
        return None;
    }
    let (p, yaw) = free[seed as usize % free.len()];
    let y = col.floor(p + Vec3::Y * 1.5, 60.0, buf).map(|(y, _)| y).unwrap_or(p.y - crate::player::PAWN_HALF_HEIGHT);
    Some((Vec3::new(p.x, y, p.z), yaw))
}

/// Where a bot leaves a seat (Seat<n>StartPosition, else beside the driver door), Bevy floor.
fn exit_point(k: &VehicleKind, v: &Vehicle, seat: usize, col: &CollisionWorld, buf: &mut Vec<u32>) -> Vec3 {
    let pose = k.pose(v);
    let exit = k
        .seats
        .get(seat)
        .and_then(|s| k.socket_world(v, &pose, &s.exit))
        .or_else(|| k.socket_world(v, &pose, "Seat0StartPosition"))
        .unwrap_or(v.pos + Quat::from_rotation_z(v.yaw) * Vec3::new(0.0, -(k.half_width + 80.0), 0.0));
    let mut feet = to_bevy(exit);
    if let Some((y, _)) = col.floor(feet + Vec3::Y * 2.0, if k.motion == Motion::Heli { 60.0 } else { 4.0 }, buf) {
        feet.y = y;
    }
    feet
}

/// Seats a bot may take: the driver's, and the ones with a gun.
fn usable_seats(k: &VehicleKind) -> Vec<usize> {
    (0..k.seats.len()).filter(|&s| s == 0 || !k.seats[s].modes.is_empty()).collect()
}

/// Shared state of one frame for the gunners.
struct Ctx<'a> {
    col: &'a CollisionWorld,
    data: &'a CombatData,
    soldiers: &'a [(Entity, Vec3, u8, bool)],
    player: Option<Vec3>,
    vehs: &'a [VehInfo],
    now: f32,
    dt: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn bot_vehicles(
    time: Res<Time>,
    mut flow: ResMut<MatchFlow>,
    (data, vassets, col, nav, obj, level): (Option<Res<CombatData>>, Option<Res<VehicleAssets>>, Res<CollisionWorld>, Res<Nav>, Res<crate::objectives::Objectives>, Option<Res<crate::objectives::LevelData>>),
    pawns: Query<&Pawn>,
    mut commands: Commands,
    mut vehicles: Query<(Entity, &mut Vehicle, Option<&mut BotVehicle>)>,
    mut bots: Query<(Entity, &mut Bot, &mut Soldier, &mut BotKit)>,
    (mut queue, mut gr, mut fx, mut sfx, mut wsfx, impacts): (ResMut<DamageQueue>, ResMut<Grenades>, ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>, ResMut<crate::sfx::WeaponSfx>, Option<Res<crate::impacts::Impacts>>),
    (mut track, mut log): (ResMut<BotVehTrack>, ResMut<BotLog>),
    mut buf: Local<(Vec<u32>, Vec<Vec3>)>,
) {
    if !enabled() || flow.phase != Phase::Playing {
        if flow.phase == Phase::Front {
            track.info.clear();
            track.abandoned.clear();
        }
        if let Ok(mut g) = BOT_VEH.lock() {
            g.clear();
        }
        return;
    }
    let (Some(data), Some(assets)) = (data, vassets) else { return };
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    // ---- vehicles that vanished: destroyed (a hit within 1.5 s) or recycled
    let gone: Vec<Entity> = track.info.keys().copied().filter(|e| vehicles.get(*e).is_err()).collect();
    for ve in gone {
        let t = track.info.remove(&ve).unwrap();
        let destroyed = t.last_hit.as_ref().is_some_and(|h| now - h.3 < 1.5);
        let mut dead = Vec::new();
        for o in &t.occupants {
            let Ok((_, mut b, mut s, mut kit)) = bots.get_mut(*o) else { continue };
            if kit.in_vehicle().is_some_and(|x| x.0 == ve) {
                kit.mode = Mode::Foot;
                if destroyed && s.alive {
                    let (attacker, team, icon) = t.last_hit.clone().map(|h| (h.0, h.1, h.2)).unwrap_or((Actor::World, 2, None));
                    queue.0.push(Hit { target: Target::Bot(*o), amount: 100_000.0, attacker, attacker_team: team, headshot: false, explosive: true, from: t.pos, icon, dmg_type: None });
                    dead.push(s.name.clone());
                } else {
                    let f = to_bevy(t.pos);
                    let floor = col.floor(f + Vec3::Y * 2.0, 60.0, &mut buf.0).map(|(y, _)| y).unwrap_or(f.y);
                    s.feet = Vec3::new(f.x, floor, f.z);
                    b.pitch = 0.0;
                }
            }
        }
        if destroyed {
            let (attacker, ateam) = t.last_hit.as_ref().map(|h| (h.0, h.1)).unwrap_or((Actor::World, 2));
            let by = match attacker {
                Actor::Bot(a) if ateam != t.team => {
                    let mut n = String::new();
                    if let Ok((_, _, s, mut kit)) = bots.get_mut(a) {
                        kit.add_kp(DESTROY_VEHICLE_BP);
                        n = format!("{} (+{DESTROY_VEHICLE_BP} KP -> {})", s.name, kit.kp);
                    }
                    n
                }
                Actor::Player if ateam != t.team => {
                    // GCBattlePoints FL.Infantry PSR_DestroyVehicle for the player too
                    flow.hud.kp += DESTROY_VEHICLE_BP;
                    flow.hud.rewards.push((DESTROY_VEHICLE_BP, "PSR_DestroyVehicle"));
                    format!("Du (+{DESTROY_VEHICLE_BP} KP)")
                }
                Actor::Bot(a) => bots.get(a).map(|x| x.2.name.clone()).unwrap_or_default(),
                Actor::Player => "Du".into(),
                Actor::World => "crash".into(),
            };
            log.event(now, format!("vehicle {} ({}) destroyed by {by}; killed inside: {:?}", t.name, team_name(t.team), dead));
        } else {
            log.event(now, format!("vehicle {} ({}) removed (recycled); occupants on foot", t.name, team_name(t.team)));
        }
    }
    track.abandoned.retain(|e| vehicles.get(*e).is_ok_and(|(_, v, bv)| bv.is_none() && v.player_seat.is_none()));
    // ---- seats: the occupants must still be alive and sitting there
    let mut exits: Vec<(Entity, Vec3, f32, &'static str)> = Vec::new();
    for (ve, mut v, bv) in &mut vehicles {
        let Some(mut bv) = bv else { continue };
        let Some(k) = assets.kinds.get(v.kind) else { continue };
        for s in 0..bv.seats.len() {
            if let Some(o) = bv.seats[s] {
                let ok = bots.get(o).is_ok_and(|(_, _, sd, kit)| sd.alive && kit.in_vehicle() == Some((ve, s)));
                if !ok {
                    bv.seats[s] = None;
                }
            }
        }
        // the player took a bot's seat (E enters seat 0, F1 / F2 switch): the bot moves on to a
        // free seat with a gun, else gets out
        if let Some(ps) = v.player_seat {
            if let Some(o) = bv.seats.get(ps).copied().flatten() {
                bv.seats[ps] = None;
                let free = usable_seats(k).into_iter().find(|&s| s != 0 && s != ps && bv.seats[s].is_none());
                match free {
                    Some(ns) => {
                        bv.seats[ns] = Some(o);
                        if let Ok((_, _, sd, mut kit)) = bots.get_mut(o) {
                            kit.mode = Mode::Vehicle { veh: ve, seat: ns };
                            log.event(now, format!("{} moves to seat {ns} of {} (the player took seat {ps})", sd.name, bv.name));
                        }
                    }
                    None => {
                        let p = exit_point(k, &v, ps, &col, &mut buf.0);
                        exits.push((o, p, v.yaw, "the player took the seat"));
                    }
                }
            }
        }
        // driver gone (the player left seat 0 or it was empty): a bot gunner takes the wheel
        if bv.seats[0].is_none() && v.player_seat != Some(0) {
            if let Some(gs) = (1..bv.seats.len()).find(|&s| bv.seats[s].is_some() && v.player_seat != Some(s)) {
                let o = bv.seats[gs].take().unwrap();
                bv.seats[0] = Some(o);
                if let Ok((_, _, sd, mut kit)) = bots.get_mut(o) {
                    kit.mode = Mode::Vehicle { veh: ve, seat: 0 };
                    log.event(now, format!("{} takes the driver seat of {}", sd.name, bv.name));
                }
            }
        }
        if bv.seats.iter().all(|s| s.is_none()) {
            log.event(now, format!("{} ({}) left by the bots{}", bv.name, team_name(bv.team), if v.player_seat.is_some() { " (the player drives it)" } else { ", parked" }));
            commands.entity(ve).remove::<BotVehicle>();
            track.info.remove(&ve);
            if v.player_seat.is_none() {
                track.abandoned.insert(ve);
            }
            continue;
        }
        let _ = &mut v;
    }
    // ---- snapshot for decisions / targets
    let vehs = vehicle_infos(&assets, vehicles.iter());
    let soldiers: Vec<(Entity, Vec3, u8, bool)> = bots.iter().filter(|(_, _, _, k)| k.in_vehicle().is_none()).map(|(e, _, s, _)| (e, s.center_ue(), s.team, s.alive)).collect();
    let player = pawns.single().ok().filter(|p| !p.dead && !p.in_vehicle).map(|p| {
        let e = p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
        to_ue(e)
    });
    let team_vehicles = |team: u8, heli: Option<bool>| vehicles.iter().filter(|(_, v, b)| b.as_ref().is_some_and(|b| b.team == team) && heli.is_none_or(|h| assets.kinds.get(v.kind).is_some_and(|k| (k.motion == Motion::Heli) == h))).count();
    let counts = [TEAM_USA, TEAM_KVA].map(|t| (team_vehicles(t, None), team_vehicles(t, Some(true))));
    // vehicles waiting for / taking gunners: (entity, team, Bevy feet, free gun seats, waiting)
    let boardable: Vec<(Entity, u8, Vec3, Vec<usize>, bool)> = vehicles
        .iter()
        .filter_map(|(e, v, b)| {
            let b = b?;
            let k = assets.kinds.get(v.kind)?;
            let free: Vec<usize> = usable_seats(k).into_iter().filter(|&s| s != 0 && b.seats[s].is_none() && v.player_seat != Some(s)).collect();
            (!free.is_empty() && (b.wait > 0.0 || v.speed.abs() < 150.0) && k.motion != Motion::Heli || !free.is_empty() && b.wait > 0.0).then(|| (e, b.team, vehicle_feet(k, v), free, b.wait > 0.0))
        })
        .collect();
    let mut taken: Vec<Vec3> = vehicles.iter().filter_map(|(_, v, _)| assets.kinds.get(v.kind).map(|k| vehicle_feet(k, v))).collect();
    // ---- purchases at the respawn, boarding
    let mut spawns: Vec<(Entity, usize, Vec3, f32, i32, Option<Entity>)> = Vec::new();
    let mut boards: Vec<(Entity, Entity, usize)> = Vec::new();
    let mut reserved: HashSet<(Entity, usize)> = HashSet::new();
    let mut new_counts = counts;
    let mut used_slots: Vec<Vec<usize>> = assets.kinds.iter().enumerate().map(|(ki, _)| vehicles.iter().filter(|(_, v, _)| v.kind == ki).map(|(_, v, _)| v.slot).collect()).collect();
    for (e, b, s, mut kit) in &mut bots {
        // (nuke.rs held: the bot the Fulton pickup takes boards nothing)
        if !s.alive || kit.in_vehicle().is_some() || crate::nuke::held(e) {
            kit.just_spawned = false;
            continue;
        }
        let fresh = std::mem::take(&mut kit.just_spawned);
        let team = s.team as usize & 1;
        if fresh && kit.driver && new_counts[team].0 < TEAM_VEHICLES {
            // the vehicles of the team this bot can pay, with a free instance (a class of the player's
            // team keeps one instance for the player); helicopters while the team has none in the air
            let mut opts: Vec<(usize, i32)> = assets
                .kinds
                .iter()
                .enumerate()
                .filter(|(_, k)| kind_team(k) == s.team)
                .filter(|(_, k)| k.motion != Motion::Heli || new_counts[team].1 < TEAM_HELIS)
                .filter_map(|(ki, k)| data.costs.get(&k.class.to_ascii_lowercase()).map(|c| (ki, c.0)))
                .filter(|&(_, c)| c > 0 && c <= kit.kp)
                .collect();
            opts.sort_by_key(|o| -o.1);
            // a driver with a role (botextras BotKit::role) only buys its class and saves until
            // it can pay it; without such a class on the map / for the team it buys like the rest
            // (NOT from data)
            if kit.role != 0 {
                let team_kinds: Vec<(usize, i32)> = assets
                    .kinds
                    .iter()
                    .enumerate()
                    .filter(|(_, k)| kind_team(k) == s.team)
                    .filter_map(|(ki, k)| data.costs.get(&k.class.to_ascii_lowercase()).map(|c| (ki, c.0)))
                    .filter(|&(_, c)| c > 0)
                    .collect();
                let heli = |ki: usize| assets.kinds[ki].motion == Motion::Heli;
                let wanted: Vec<usize> = if kit.role == 2 {
                    team_kinds.iter().filter(|x| heli(x.0)).map(|x| x.0).collect()
                } else {
                    let top = team_kinds.iter().filter(|x| !heli(x.0)).map(|x| x.1).max();
                    team_kinds.iter().filter(|x| !heli(x.0) && Some(x.1) == top).map(|x| x.0).collect()
                };
                if !wanted.is_empty() {
                    // empty = saving (helicopter: also while the team's one is in the air)
                    opts.retain(|o| wanted.contains(&o.0));
                    opts.truncate(1);
                }
            }
            // NOT from data: the most expensive one at 65 %, else the next; a HUMVEE alone only
            // every other time (the bot saves for more)
            let pick = match opts.len() {
                0 => None,
                1 if kit.role == 0 && kit.rnd() < 0.5 => None,
                1 => Some(opts[0]),
                n => Some(if kit.rnd() < 0.65 { opts[0] } else { opts[1 + (kit.rnd() * (n - 1) as f32) as usize % (n - 1)] }),
            };
            if let Some((ki, cost)) = pick {
                let k = &assets.kinds[ki];
                let need = if kind_team(k) == crate::combat::player_team() { 2 } else { 1 };
                let free_n = k.slots.len().saturating_sub(used_slots[ki].len());
                // an abandoned bot vehicle of the kind makes room
                let recycle = if free_n < need { track.abandoned.iter().copied().find(|a| vehicles.get(*a).is_ok_and(|(_, v, _)| v.kind == ki)) } else { None };
                let slot = if free_n >= need {
                    (0..k.slots.len()).find(|sl| !used_slots[ki].contains(sl))
                } else {
                    recycle.and_then(|a| vehicles.get(a).ok().map(|(_, v, _)| v.slot))
                };
                if let Some(slot) = slot {
                    if let Some((p, yaw)) = spawn_point(&col, level.as_deref(), &obj, s.team, k, &taken, kit.seed, &mut buf.0) {
                        kit.kp -= cost;
                        if !used_slots[ki].contains(&slot) {
                            used_slots[ki].push(slot);
                        }
                        new_counts[team].0 += 1;
                        if k.motion == Motion::Heli {
                            new_counts[team].1 += 1;
                        }
                        taken.push(p);
                        spawns.push((e, ki, p, yaw, cost, recycle));
                        let _ = b.slot;
                        continue;
                    }
                }
            }
        }
        // gunner: a team vehicle that waits at the spawn (30 m after a respawn) or stands /
        // crawls within 10 m; not every time (NOT from data)
        kit.board_cd -= dt;
        if kit.board_cd > 0.0 && !fresh {
            continue;
        }
        kit.board_cd = 2.0;
        if b.goal_kind == "guard" && !fresh {
            continue;
        }
        let reach = if fresh { 30.0 } else { 10.0 };
        let cand = boardable.iter().filter(|x| x.1 == s.team && x.2.distance(s.feet) < reach && (fresh || x.4 || true)).find_map(|x| x.3.iter().find(|&&seat| !reserved.contains(&(x.0, seat))).map(|&seat| (x.0, seat)));
        if let Some((ve, seat)) = cand {
            if kit.rnd() < if fresh { 0.8 } else { 0.5 } {
                reserved.insert((ve, seat));
                boards.push((e, ve, seat));
            }
        }
    }
    for (e, ki, p, yaw, cost, recycle) in spawns {
        let k = &assets.kinds[ki];
        if let Some(a) = recycle {
            if let Ok((_, v, _)) = vehicles.get(a) {
                let slot = v.slot;
                commands.entity(a).despawn();
                track.abandoned.remove(&a);
                log.event(now, format!("abandoned {} recycled (slot {slot})", k.name));
            }
        }
        let slot = recycle.and_then(|a| vehicles.get(a).ok().map(|(_, v, _)| v.slot)).or_else(|| (0..k.slots.len()).find(|sl| !vehicles.iter().any(|(_, v, _)| v.kind == ki && v.slot == *sl)));
        let Some(slot) = slot else { continue };
        let Ok((_, mut b, mut s, mut kit)) = bots.get_mut(e) else { continue };
        let pos = to_ue(p) + Vec3::Z * k.ride_height;
        let mut v = crate::vehicles::new_vehicle(k, ki, slot, pos, yaw);
        v.player_seat = None;
        v.seed ^= kit.seed;
        let mut seats = vec![None; k.seats.len().max(1)];
        let ve = commands.spawn_empty().id();
        seats[0] = Some(e);
        let bvc = BotVehicle {
            team: s.team,
            seats: seats.clone(),
            name: k.name.clone(),
            path: Vec::new(),
            repath: 0.0,
            goal: None,
            goal_letter: None,
            slow: 0.0,
            reverse: 0.0,
            reverse_steer: 1.0,
            best: f32::MAX,
            best_t: 0.0,
            unstick: 0,
            wait: if usable_seats(k).len() > 1 { WAIT_GUNNER } else { 0.5 },
            seat_ai: vec![SeatAi::default(); k.seats.len().max(1)],
            seed: kit.seed | 1,
            low_air: 0.0,
            gas: 0.0,
        };
        commands.entity(ve).insert((v, bvc));
        kit.mode = Mode::Vehicle { veh: ve, seat: 0 };
        s.feet = p;
        b.yaw = yaw;
        track.info.insert(ve, Tracked { team: s.team, name: k.name.clone(), occupants: vec![e], pos, last_hit: None });
        log.event(now, format!("{} ({}) buys {} ({} KP) at ({:.1} {:.1} {:.1}) -> {} KP", s.name, team_name(s.team), k.name, cost, p.x, p.y, p.z, kit.kp));
    }
    for (e, ve, seat) in boards {
        let Ok((_, mut v, Some(mut bv))) = vehicles.get_mut(ve) else { continue };
        if bv.seats.get(seat).is_none_or(|x| x.is_some()) || v.player_seat == Some(seat) {
            continue;
        }
        let Ok((_, mut b, mut s, mut kit)) = bots.get_mut(e) else { continue };
        bv.seats[seat] = Some(e);
        kit.mode = Mode::Vehicle { veh: ve, seat };
        b.yaw = v.yaw;
        let k = &assets.kinds[v.kind];
        s.feet = vehicle_feet(k, &v);
        // a gunner is in: the driver need not wait longer
        bv.wait = bv.wait.min(1.0);
        log.event(now, format!("{} ({}) boards {} as gunner (seat {seat})", s.name, team_name(s.team), bv.name));
        let _ = &mut v;
    }
    // ---- drive, fly, shoot
    let enemy_bevy: [Vec<Vec3>; 2] = [TEAM_USA, TEAM_KVA].map(|t| {
        let mut v: Vec<Vec3> = soldiers.iter().filter(|s| s.3 && s.2 != t).map(|s| to_bevy(s.1)).collect();
        if t != crate::combat::player_team() {
            if let Some(p) = player {
                v.push(to_bevy(p));
            }
        }
        v.extend(vehs.iter().filter(|x| x.team.is_some_and(|vt| vt != t)).map(|x| to_bevy(x.centre)));
        v
    });
    let ctx = Ctx { col: &col, data: &data, soldiers: &soldiers, player, vehs: &vehs, now, dt };
    let mut registry: Vec<(Entity, bool)> = Vec::new();
    for (ve, mut v, bv) in &mut vehicles {
        let Some(mut bv) = bv else { continue };
        let Some(k) = assets.kinds.get(v.kind) else { continue };
        let heli = k.motion == Motion::Heli;
        let driver = bv.seats[0].filter(|_| v.player_seat != Some(0));
        registry.push((ve, driver.is_some()));
        // engine (GCVehicleAudio SCEngineIdle) at the vehicle while a bot drives
        // Same switches / layer volumes as the player's engine (vehicles.rs drive). Before, the
        // cue played with no parameters and every loop layer at full volume: the Humvee's
        // tyre, acceleration and gas loops (audible to 20-90 m) ran flat out even when the bot
        // vehicle stood still ("looping humvee sound", user 2026-10-22 20:17). NOT from data:
        // the layer volumes over speed / throttle, as for the player.
        if driver.is_some() && v.emp <= 0.0 && !k.audio.engine_idle.is_empty() && v.player_seat.is_none() {
            let rev = ((v.speed.abs() - 10.0) / 990.0).clamp(0.0, 1.0);
            let sp = (v.speed.abs() / crate::vehicles::top_speed(k).max(1.0)).clamp(0.0, 1.0);
            let gas = bv.gas.clamp(0.0, 1.0);
            sfx.looping_layers(
                &format!("botveh_engine{}", ve.index()),
                &k.audio.engine_idle,
                Some(v.pos),
                &[("Gas", (gas > 0.5) as usize), ("BypassStartup", 0), ("Sprinting", 0)],
                1.0 + 0.2 * rev,
                &[("Tires", sp), ("JeepAccelerat", gas * (0.3 + 0.7 * sp)), ("HumV_Gas", gas)],
            );
        }
        // bail out of a burning ground vehicle
        if !heli && v.health < k.health * BAIL_HEALTH && v.player_seat.is_none() {
            for s in 0..bv.seats.len() {
                if let Some(o) = bv.seats[s].take() {
                    let p = exit_point(k, &v, s, &col, &mut buf.0);
                    exits.push((o, p, v.yaw, "bails out (vehicle health below 30 %)"));
                }
            }
            continue;
        }
        let mut abandon = false;
        if let Some(d) = driver {
            let team = bv.team;
            let from = vehicle_feet(k, &v);
            bv.repath -= dt;
            if bv.goal.is_none() || bv.repath <= 0.0 {
                if let Some((g, letter)) = vehicle_goal(&obj, team, from, &enemy_bevy[team as usize & 1]) {
                    if bv.goal_letter != letter || bv.goal.is_none_or(|old| old.distance(g) > 10.0) {
                        bv.best = f32::MAX;
                        bv.best_t = 0.0;
                        let l = letter.map(|l| ["A", "B", "C", "D"][l.min(3)]).unwrap_or("-");
                        let dname = bots.get(d).map(|x| x.2.name.clone()).unwrap_or_default();
                        log.event(now, format!("{dname} drives {} ({}) to {l} ({:.0} m)", bv.name, team_name(team), Vec2::new(g.x - from.x, g.z - from.z).length()));
                    }
                    bv.goal = Some(g);
                    bv.goal_letter = letter;
                    bv.path.clear();
                }
                bv.repath = 6.0;
            }
            // the pilot's fixed guns face the target (helicopters)
            let face = bv.seat_ai.first().and_then(|a| a.target).and_then(|t| target_point(&ctx, t));
            abandon = if heli { fly(k, &mut v, &mut bv, &col, &face, dt, &mut buf) } else { drive_ground(k, &mut v, &mut bv, &col, &nav, dt, &mut buf) };
        } else if v.player_seat != Some(0) {
            // nobody drives: the vehicle stays put (helicopters: unpiloted_fall)
            if !heli {
                let mut pos = v.pos;
                let top = crate::vehicles::top_speed(k);
                crate::vehicles::ground_step(k, &mut v, &mut pos, &col, &mut buf, 0.0, 0.0, top, top, dt);
                v.pos = pos;
            }
        }
        if abandon {
            for s in 0..bv.seats.len() {
                if let Some(o) = bv.seats[s].take() {
                    let p = exit_point(k, &v, s, &col, &mut buf.0);
                    exits.push((o, p, v.yaw, if heli { "abandons the helicopter (stuck on the ground)" } else { "abandons the vehicle (stuck, no progress for 15 s)" }));
                }
            }
            continue;
        }
        // gunners (and the driver's own gun: tank cannon, helicopter weapons)
        for seat in 0..bv.seats.len() {
            let Some(g) = bv.seats[seat] else { continue };
            if v.player_seat == Some(seat) || k.seats.get(seat).is_none_or(|s| s.modes.is_empty()) {
                continue;
            }
            let (gname, skill) = bots.get(g).map(|x| (x.2.name.clone(), crate::botextras::skill_factor(x.2.team))).unwrap_or((String::new(), 1.0));
            gunner(&ctx, k, ve, &mut v, &mut bv, seat, g, &gname, skill.max(0.5), &mut queue, &mut gr, &mut fx, &mut sfx, &mut wsfx, impacts.as_deref(), &mut log, &mut buf.0);
        }
        // the occupants ride along (objectives, bots' targeting)
        let feet = vehicle_feet(k, &v);
        let occ: Vec<Entity> = bv.seats.iter().flatten().copied().collect();
        for &o in &occ {
            if let Ok((_, mut b, mut s, _)) = bots.get_mut(o) {
                s.feet = feet;
                s.crouched = false;
                b.yaw = v.yaw;
            }
        }
        let pos = v.pos;
        let t = track.info.entry(ve).or_insert_with(|| Tracked { team: bv.team, name: bv.name.clone(), occupants: Vec::new(), pos, last_hit: None });
        t.occupants = occ;
        t.pos = pos;
        t.team = bv.team;
    }
    // ---- out of the vehicles
    for (o, p, yaw, why) in exits {
        let Ok((_, mut b, mut s, mut kit)) = bots.get_mut(o) else { continue };
        if let Some((ve, _)) = kit.in_vehicle() {
            let vname = track.info.get(&ve).map(|t| t.name.clone()).unwrap_or_default();
            log.event(now, format!("{} ({}) leaves {vname}: {why}", s.name, team_name(s.team)));
        }
        kit.mode = Mode::Foot;
        kit.board_cd = 8.0;
        s.feet = p;
        b.yaw = yaw;
        b.pitch = 0.0;
    }
    if let Ok(mut g) = BOT_VEH.lock() {
        *g = registry;
    }
}

/// Ground vehicle: follow the walk-grid path to the goal (seat 0). Returns true when it should be
/// abandoned (stuck).
fn drive_ground(k: &VehicleKind, v: &mut Vehicle, bv: &mut BotVehicle, col: &CollisionWorld, nav: &Nav, dt: f32, buf: &mut (Vec<u32>, Vec<Vec3>)) -> bool {
    let feet = vehicle_feet(k, v);
    let top = crate::vehicles::top_speed(k);
    // NOT from data: bots drive at 80 % of the top speed
    let cap = top * 0.8;
    let mut pos = v.pos;
    if bv.wait > 0.0 || bv.goal.is_none() {
        bv.gas += (0.0 - bv.gas) * (dt * 4.0).min(1.0);
    }
    if bv.wait > 0.0 {
        bv.wait -= dt;
        crate::vehicles::ground_step(k, v, &mut pos, col, buf, 0.0, 0.0, cap, top, dt);
        v.pos = pos;
        return false;
    }
    let Some(goal) = bv.goal else {
        crate::vehicles::ground_step(k, v, &mut pos, col, buf, 0.0, 0.0, cap, top, dt);
        v.pos = pos;
        return false;
    };
    // the walk grid path (nav.rs), every 4th node
    if bv.path.is_empty() {
        if let Some(grid) = nav.grid.as_ref() {
            if let (Some(a), Some(z)) = (grid.nearest(feet), grid.nearest(goal)) {
                let z = if grid.comp[z as usize] == grid.comp[a as usize] { Some(z) } else { grid.nearest_in_comp(goal, grid.comp[a as usize], 30.0).or_else(|| grid.closest_in_comp(goal, grid.comp[a as usize])) };
                if let Some(p) = z.and_then(|z| grid.path(a, z, 150_000, &nav.penalty)) {
                    bv.path = p.iter().step_by(4).map(|&n| grid.nodes[n as usize]).collect();
                    if let Some(&l) = p.last() {
                        bv.path.push(grid.nodes[l as usize]);
                    }
                }
            }
        }
        if bv.path.is_empty() {
            bv.path.push(goal);
        }
    }
    let flat = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();
    while bv.path.len() > 1 && flat(bv.path[0], feet) < 6.0 {
        bv.path.remove(0);
    }
    let dist_goal = flat(goal, feet);
    let arrived = dist_goal < ARRIVE;
    // look ahead: the farthest of the next waypoints in sight at bumper height
    let eye = feet + Vec3::Y * 1.2;
    let mut wp = bv.path[0];
    for &p in bv.path.iter().take(6).skip(1) {
        let q = p + Vec3::Y * 1.2;
        let d = q - eye;
        if col.raycast(eye, d.normalize_or_zero(), d.length(), &mut buf.0).is_none() {
            wp = p;
        } else {
            break;
        }
    }
    let to = wp - feet;
    let err = wrap(to.z.atan2(to.x) - v.yaw);
    let tracked = k.motion == Motion::Tracked;
    let (mut throttle, mut steer) = (0.0f32, 0.0f32);
    if bv.reverse > 0.0 {
        bv.reverse -= dt;
        throttle = -1.0;
        steer = bv.reverse_steer;
    } else if !arrived {
        steer = (err * 2.5).clamp(-1.0, 1.0);
        throttle = if err.abs() > 1.2 {
            if tracked {
                0.0
            } else {
                0.5
            }
        } else {
            1.0
        };
        if dist_goal < 25.0 {
            throttle *= 0.6;
        }
    } else if v.speed.abs() > 30.0 {
        // brake on the objective
        throttle = -v.speed.signum() * 0.5;
    }
    crate::vehicles::ground_step(k, v, &mut pos, col, buf, throttle, steer, cap, top, dt);
    v.pos = pos;
    // engine sound throttle, smoothed so short changes do not replay the rev one-shots
    bv.gas += ((throttle.abs() > 0.05) as u8 as f32 - bv.gas) * (dt * 4.0).min(1.0);
    // stuck: slow while giving throttle -> reverse with the wheel turned the other way
    if throttle > 0.0 && v.speed.abs() < 40.0 {
        bv.slow += dt;
    } else {
        bv.slow = 0.0;
    }
    if bv.slow > 2.0 {
        bv.slow = 0.0;
        bv.reverse = 1.5;
        bv.reverse_steer = if steer.abs() > 0.1 { -steer.signum() } else if bv.rnd() < 0.5 { -1.0 } else { 1.0 };
        bv.unstick += 1;
        bv.path.clear();
    }
    // progress towards the goal
    if arrived {
        bv.best_t = 0.0;
        bv.best = dist_goal;
        bv.unstick = 0;
    } else if dist_goal < bv.best - 3.0 {
        bv.best = dist_goal;
        bv.best_t = 0.0;
    } else {
        bv.best_t += dt;
    }
    bv.best_t > STUCK_ABANDON && bv.unstick >= 2
}

/// Helicopter (kinematic, NOT from data: the sim of vehicles.rs heli_step needs a pilot's stick
/// input): climbs to HELI_ALT above the ground, flies to the goal at 60 % of AirSpeed, circles it
/// at HELI_ORBIT, faces the pilot's target (fixed guns), nose down with the speed. Returns true
/// when it should be abandoned (cannot climb for 15 s).
fn fly(k: &VehicleKind, v: &mut Vehicle, bv: &mut BotVehicle, col: &CollisionWorld, face: &Option<Vec3>, dt: f32, buf: &mut (Vec<u32>, Vec<Vec3>)) -> bool {
    let ground_at = |p: Vec3, from_z: f32, buf: &mut Vec<u32>| col.floor(Vec3::new(p.x * 0.01, from_z * 0.01 + 1.0, p.y * 0.01), 400.0, buf).map(|(y, _)| y * 100.0);
    let pos0 = v.pos;
    let g0 = ground_at(pos0, pos0.z, &mut buf.0).unwrap_or(pos0.z - k.ride_height - HELI_ALT);
    if bv.wait > 0.0 {
        bv.wait -= dt;
        bv.gas = 0.0;
        return false;
    }
    // rotors under power while flying (engine sound)
    bv.gas += (1.0 - bv.gas) * (dt * 4.0).min(1.0);
    let hvel = Vec2::new(v.vel.x, v.vel.y);
    let ahead = pos0 + Vec3::new(hvel.x, hvel.y, 0.0) * 1.5;
    let ga = ground_at(ahead, pos0.z, &mut buf.0).unwrap_or(g0);
    let floor = g0.max(ga);
    let height = pos0.z - k.ride_height - g0;
    let cruise = (k.max_speed * 0.6).max(500.0);
    let mut want = Vec2::ZERO;
    if let Some(goal) = bv.goal.map(to_ue) {
        if height > HELI_ALT * 0.5 {
            let to = Vec2::new(goal.x - pos0.x, goal.y - pos0.y);
            let d = to.length().max(1.0);
            let radial = to / d;
            want = if d > HELI_ORBIT + 1500.0 {
                radial * cruise
            } else {
                let tangent = Vec2::new(-radial.y, radial.x);
                (tangent + radial * ((d - HELI_ORBIT) / 1500.0).clamp(-1.0, 1.0)).normalize_or_zero() * cruise * 0.5
            };
        }
    }
    // NOT from data: horizontal acceleration 500 uu/s^2, climb at HELI_CLIMB-like 450 uu/s
    let acc = 500.0 * dt;
    let dv = want - hvel;
    let dv = if dv.length() > acc { dv.normalize() * acc } else { dv };
    let nh = hvel + dv;
    let want_z = floor + k.ride_height + HELI_ALT;
    let vz = ((want_z - pos0.z) * 0.8).clamp(-450.0, 450.0);
    let mut vel = Vec3::new(nh.x, nh.y, v.vel.z + (vz - v.vel.z).clamp(-900.0 * dt, 900.0 * dt));
    let mut p = pos0 + vel * dt;
    // the fuselage as three spheres (as vehicles.rs drive), pushed out of the level
    let fwd = Vec3::new(v.yaw.cos(), v.yaw.sin(), 0.0);
    let r = (k.half_width * 0.8).clamp(60.0, 120.0);
    for fx in [-1.0f32, 0.0, 1.0] {
        let c = p + fwd * (fx * (k.half_len * 0.5)) + Vec3::Z * (r - k.ride_height).max(0.0);
        let (b0, b1) = &mut *buf;
        b1.clear();
        let corr = col.push_sphere(to_bevy(c), r * 0.01, b0, b1);
        if corr.length() > 0.001 {
            p += Vec3::new(corr.x, corr.z, corr.y) * 100.0;
            vel.x *= 0.5;
            vel.y *= 0.5;
            vel.z = vel.z.max(200.0);
        }
    }
    let gp = ground_at(p, p.z, &mut buf.0).unwrap_or(g0);
    if p.z < gp + k.ride_height {
        p.z = gp + k.ride_height;
        vel.z = vel.z.max(0.0);
    }
    // heading: the pilot's target, else the flight direction
    let look = match face {
        Some(t) => Some(Vec2::new(t.x - p.x, t.y - p.y)),
        None if nh.length() > 150.0 => Some(nh),
        None => None,
    };
    let old_yaw = v.yaw;
    if let Some(l) = look.filter(|l| l.length() > 1.0) {
        let rate = k.yaw_rate.max(0.8) * dt;
        v.yaw += wrap(l.y.atan2(l.x) - v.yaw).clamp(-rate, rate);
    }
    let yaw_rate = wrap(v.yaw - old_yaw) / dt.max(1e-3);
    let fwd = Vec3::new(v.yaw.cos(), v.yaw.sin(), 0.0);
    let fs = Vec3::new(vel.x, vel.y, 0.0).dot(fwd);
    let want_pitch = match face {
        Some(t) => {
            let d = *t - p;
            (d.z.atan2(Vec2::new(d.x, d.y).length().max(1.0))).clamp(-0.6, 0.15)
        }
        None => -0.25 * (fs / k.max_speed.max(1.0)).clamp(-1.0, 1.0),
    };
    let kk = (dt * 3.0).min(1.0);
    v.pitch += (want_pitch - v.pitch) * kk;
    v.roll += ((yaw_rate * 0.3).clamp(-0.4, 0.4) - v.roll) * kk;
    v.vel = vel;
    v.ang = Vec3::ZERO;
    v.speed = fs;
    v.vz = vel.z;
    v.pos = p;
    // cannot get up (under a roof, wedged): abandoned after 15 s
    if height < 300.0 && vz > 100.0 {
        bv.low_air += dt;
    } else {
        bv.low_air = 0.0;
    }
    bv.low_air > STUCK_ABANDON
}

/// Aim point (UE) of a gunner's target.
fn target_point(ctx: &Ctx, t: Tgt) -> Option<Vec3> {
    match t {
        Tgt::Soldier(e) => ctx.soldiers.iter().find(|s| s.0 == e && s.3).map(|s| s.1),
        Tgt::Player => ctx.player,
        Tgt::Vehicle(e) => ctx.vehs.iter().find(|x| x.e == e).map(|x| x.centre),
    }
}

/// One bot seat: pick a target in sight and range, turn the turret (rate, pitch limits), fire
/// the seat's modes (projectiles at vehicles, guns at soldiers).
#[allow(clippy::too_many_arguments)]
fn gunner(
    ctx: &Ctx,
    k: &VehicleKind,
    ve: Entity,
    v: &mut Vehicle,
    bv: &mut BotVehicle,
    seat: usize,
    g: Entity,
    gname: &str,
    skill: f32,
    queue: &mut DamageQueue,
    gr: &mut Grenades,
    fx: &mut crate::particles::Particles,
    sfx: &mut crate::sfx::SfxQueue,
    wsfx: &mut crate::sfx::WeaponSfx,
    impacts: Option<&crate::impacts::Impacts>,
    log: &mut BotLog,
    buf: &mut Vec<u32>,
) {
    let sc = &k.seats[seat];
    let team = bv.team;
    let vname = bv.name.clone();
    let dt = ctx.dt;
    // gun state (as vehicles.rs drive for the player's seat)
    {
        let st = &mut v.guns[seat];
        st.fire_cd = (st.fire_cd - dt).max(0.0);
        if st.overheated > 0.0 {
            st.overheated -= dt;
            if st.overheated <= 0.0 {
                st.heat = 0.0;
            }
        } else {
            st.heat = (st.heat - dt).max(0.0);
        }
        for m in 0..sc.modes.len() {
            if st.reload.get(m).is_some_and(|r| *r > 0.0) {
                st.reload[m] -= dt;
                if st.reload[m] <= 0.0 {
                    if let FireKind::Projectile(rm, _) = &sc.modes[m].kind {
                        st.ammo[m] = rm.clip.max(1);
                    }
                }
            }
        }
    }
    let pose = k.pose(v);
    let muzzle = |v: &Vehicle, m: usize, i: u32| sc.modes.get(m).and_then(|fm| (!fm.sockets.is_empty()).then(|| fm.sockets[i as usize % fm.sockets.len()].clone())).and_then(|s| k.socket_world(v, &pose, &s)).unwrap_or(v.pos + Vec3::Z * 120.0);
    let o0 = muzzle(&*v, 0, 0);
    let inst_range = sc.modes.iter().filter_map(|m| if let FireKind::Instant(gc) = &m.kind { Some(gc.range) } else { None }).fold(0.0f32, f32::max);
    let has_proj = sc.modes.iter().any(|m| matches!(m.kind, FireKind::Projectile(..)));
    // NOT from data: projectile modes engage up to 120 m
    let proj_range = if has_proj { 12_000.0 } else { 0.0 };
    let ai = &mut bv.seat_ai[seat];
    ai.think -= dt;
    if ai.think <= 0.0 {
        ai.think = 0.3;
        let mut best: Option<(f32, Tgt)> = None;
        let consider = |d: f32, t: Tgt, best: &mut Option<(f32, Tgt)>| {
            if best.is_none_or(|b| d < b.0) {
                *best = Some((d, t));
            }
        };
        // enemy vehicles for the projectile modes (threats first: x0.4)
        if has_proj {
            for x in ctx.vehs.iter().filter(|x| x.e != ve && x.team.is_some_and(|t| t != team)) {
                let d = x.centre.distance(o0);
                if d < proj_range && sees(ctx.col, o0, x.centre, buf) {
                    consider(d * 0.4, Tgt::Vehicle(x.e), &mut best);
                }
            }
        }
        let range = inst_range.max(proj_range);
        for s in ctx.soldiers.iter().filter(|s| s.3 && s.2 != team) {
            let d = s.1.distance(o0);
            if d < range && d > 300.0 && sees(ctx.col, o0, s.1 + Vec3::Z * 20.0, buf) && crate::vehicles::shot_block(o0, (s.1 - o0).normalize_or_zero(), d - 50.0).is_none() {
                consider(d, Tgt::Soldier(s.0), &mut best);
            }
        }
        if team != crate::combat::player_team() {
            if let Some(p) = ctx.player {
                let d = p.distance(o0);
                if d < range && d > 300.0 && sees(ctx.col, o0, p + Vec3::Z * 20.0, buf) && crate::vehicles::shot_block(o0, (p - o0).normalize_or_zero(), d - 50.0).is_none() {
                    consider(d, Tgt::Player, &mut best);
                }
            }
        }
        let new = best.map(|b| b.1);
        if new != ai.target {
            ai.seen = 0.0;
            match new {
                Some(Tgt::Vehicle(e)) => {
                    if let Some(x) = ctx.vehs.iter().find(|x| x.e == e) {
                        log.event(ctx.now, format!("{gname} ({}) in {vname} seat {seat} engages {} ({})", team_name(team), x.name, x.team.map(team_name).unwrap_or("-")));
                    }
                }
                Some(Tgt::Player) => log.event(ctx.now, format!("{gname} ({}) in {vname} seat {seat} engages the player", team_name(team))),
                _ => {}
            }
        }
        ai.target = new;
    }
    let Some(t) = ai.target else { return };
    let Some(mut aim) = target_point(ctx, t) else {
        ai.target = None;
        return;
    };
    ai.seen += dt;
    let is_veh = matches!(t, Tgt::Vehicle(_));
    if let (Tgt::Vehicle(e), Some(m)) = (t, sc.modes.iter().find_map(|m| if let FireKind::Projectile(rm, _) = &m.kind { Some(rm) } else { None })) {
        // lead the vehicle for the shell / rocket speed (straight flight)
        if let Some(x) = ctx.vehs.iter().find(|x| x.e == e) {
            let s = m.missile.speed.max(500.0);
            for _ in 0..2 {
                aim = x.centre + x.vel * (aim.distance(o0) / s);
            }
        }
    } else {
        aim += Vec3::Z * 10.0;
    }
    let want = (aim - o0).normalize_or_zero();
    // turret towards the aim (fTurretYaw/PitchMaxSpeed, TurretControllerMin/MaxLimit)
    if sc.turret.is_some() {
        let (ty, tp) = v.turrets[seat];
        let (ry, rp) = (sc.turret_rate.0.max(10.0).to_radians() * dt, sc.turret_rate.1.max(10.0).to_radians() * dt);
        let wy = wrap(want.y.atan2(want.x) - v.yaw);
        let wp = want.z.clamp(-1.0, 1.0).asin() - v.pitch;
        let ny = wrap(ty + wrap(wy - ty).clamp(-ry, ry));
        let yd = ny.to_degrees();
        let (lo, hi) = (crate::vehicles::curve(&sc.turret_pitch.0, yd), crate::vehicles::curve(&sc.turret_pitch.1, yd));
        let (lo, hi) = if sc.turret_pitch.0.is_empty() && sc.turret_pitch.1.is_empty() { (-30.0, 60.0) } else { (lo, hi.max(lo)) };
        let np = (tp + (wp - tp).clamp(-rp, rp)).clamp(lo.to_radians(), hi.to_radians());
        v.turrets[seat] = (ny, np);
    }
    let cur = crate::vehicles::seat_dir(v, k, seat);
    let err = cur.dot(want).clamp(-1.0, 1.0).acos().to_degrees();
    // NOT from data: reaction 0.6 s / skill factor (botextras::skill_factor, 1.0 at the default difficulty), bursts 1.6 s on / 0.7 s off, extra deviation
    // 1 deg / skill, fire within 3 deg (guns) / 2.5 deg (projectiles; helicopter fixed weapons 8)
    if ai.seen < 0.6 / skill {
        return;
    }
    ai.burst += dt;
    if ai.burst > 2.3 {
        ai.burst = 0.0;
    }
    let firing_window = ai.burst < 1.6;
    let fixed = sc.turret.is_none();
    let dist = aim.distance(o0);
    for (m, fm) in sc.modes.iter().enumerate().take(2) {
        match &fm.kind {
            FireKind::Instant(gc) => {
                if is_veh || dist > gc.range || !firing_window || err > if fixed { 8.0 } else { 3.0 } {
                    continue;
                }
                let st = &v.guns[seat];
                if st.overheated > 0.0 || st.fire_cd > 0.0 {
                    continue;
                }
                let st = &mut v.guns[seat];
                st.fire_cd = gc.fire_interval;
                st.heat += gc.overheat_add + gc.fire_interval;
                if st.heat >= gc.overheat_max {
                    st.overheated = gc.overheat_recover;
                }
                st.shots += 1;
                let shots = st.shots;
                let o = muzzle(&*v, m, shots);
                let mut seed = v.seed;
                let base = (aim - o).normalize_or_zero();
                let d = crate::combat::spread_dir(base, gc.spread * 0.5 + 1.0 / skill, &mut seed);
                v.seed = seed;
                let snd = if fm.sound.is_empty() { &gc.sound } else { &fm.sound };
                if gc.sound_looping {
                    wsfx.shot_cue(&format!("botveh{}_{seat}_{m}", ve.index()), snd, Some(o), gc.fire_interval + 0.06);
                } else {
                    sfx.play(snd, Some(o));
                }
                crate::vehicles::flash_fx(k, v, &pose, &fm.flash_tags, d, fx);
                let pl = ctx.player.map(|c| (c, true));
                let shot = crate::combat::trace_shot(ctx.col, ctx.soldiers, pl, o, d, gc.range, None, team == crate::combat::player_team(), buf);
                if let Some(tc) = gc.tracer.as_ref().filter(|tc| shots % tc.frequency.max(1) == 0) {
                    let dd = match &shot {
                        ShotHit::Soldier(_, t, _) | ShotHit::World(t, _, _) | ShotHit::Player(t, _) => *t,
                        _ => gc.range,
                    };
                    crate::combat::fire_tracer(fx, tc, o, crate::combat::dir_quat(d), d, dd / tc.speed.max(1.0));
                }
                if let Some(im) = impacts {
                    let center = match &shot {
                        ShotHit::Soldier(e, ..) => ctx.soldiers.iter().find(|s| s.0 == *e).map(|s| s.1),
                        ShotHit::Player(..) => ctx.player,
                        _ => None,
                    };
                    im.shot(&shot, o, d, &gc.dmg_type, center, fx, sfx);
                }
                let region = |c: Vec3, hp: Vec3| if hp.z > c.z + crate::combat::HEAD_OFFSET - crate::combat::HEAD_RADIUS { ("PMA_Head", true) } else { ("PMA_Soldier_Chest", false) };
                let (impact, direct) = match &shot {
                    ShotHit::Soldier(e, t, hp) => {
                        if let Some(s) = ctx.soldiers.iter().find(|s| s.0 == *e).filter(|s| s.2 != team) {
                            let (mat, head) = region(s.1, *hp);
                            let dmg = ctx.data.grid.damage(&gc.dmg_type, mat, *t, gc.full_range, gc.range);
                            queue.0.push(Hit { target: Target::Bot(*e), amount: dmg, attacker: Actor::Bot(g), attacker_team: team, headshot: head, explosive: false, from: o, icon: None, dmg_type: Some(gc.dmg_type.clone()) });
                        }
                        (Some(*hp), Some(Target::Bot(*e)))
                    }
                    ShotHit::Player(t, hp) => {
                        if team != crate::combat::player_team() {
                            let (mat, head) = region(ctx.player.unwrap_or(*hp), *hp);
                            let dmg = ctx.data.grid.damage(&gc.dmg_type, mat, *t, gc.full_range, gc.range);
                            queue.0.push(Hit { target: Target::Player, amount: dmg, attacker: Actor::Bot(g), attacker_team: team, headshot: head, explosive: false, from: o, icon: None, dmg_type: Some(gc.dmg_type.clone()) });
                        }
                        (Some(*hp), Some(Target::Player))
                    }
                    ShotHit::World(t, _, _) => (Some(o + d * *t), None),
                    _ => (None, None),
                };
                // splash (SplashDamageClass / Radius / RadiusFull, as vehicles.rs drive)
                if let (Some((scls, radius, full)), Some(at)) = (gc.splash.as_ref(), impact) {
                    let mut hurt: Vec<(Target, Vec3)> = ctx.soldiers.iter().filter(|s| s.3 && s.2 != team).map(|s| (Target::Bot(s.0), s.1)).collect();
                    if team != crate::combat::player_team() {
                        if let Some(p) = ctx.player {
                            hurt.push((Target::Player, p));
                        }
                    }
                    for (tg, c) in hurt {
                        if Some(tg) == direct {
                            continue;
                        }
                        let dd = c.distance(at);
                        if dd >= *radius {
                            continue;
                        }
                        let f = if dd <= *full { 1.0 } else { 1.0 - (dd - full) / (radius - full).max(1.0) };
                        let dmg = ctx.data.grid.damage(scls, "PMA_Soldier", 0.0, 1.0, 1.0) * f;
                        if dmg > 0.0 {
                            queue.0.push(Hit { target: tg, amount: dmg, attacker: Actor::Bot(g), attacker_team: team, headshot: false, explosive: true, from: at, icon: None, dmg_type: Some(scls.clone()) });
                        }
                    }
                }
            }
            FireKind::Projectile(rm, wi) => {
                // shells / rockets: at vehicles, or at soldiers beyond 8 m when the seat has no gun
                let has_gun = sc.modes.iter().any(|m| matches!(m.kind, FireKind::Instant(_)));
                if (!is_veh && (has_gun || dist < 800.0)) || err > if fixed { 8.0 } else { 2.5 } {
                    continue;
                }
                let st = &v.guns[seat];
                if st.ammo.get(m).is_none_or(|a| *a <= 0) || st.reload.get(m).is_some_and(|r| *r > 0.0) || st.fire_cd > 0.0 {
                    continue;
                }
                let st = &mut v.guns[seat];
                st.shots += 1;
                let shots = st.shots;
                let o = muzzle(&*v, m, shots);
                let mut seed = v.seed;
                let d = crate::combat::spread_dir((aim - o).normalize_or_zero(), rm.spread * 0.5 + 0.5 / skill, &mut seed);
                v.seed = seed;
                let o = out_of_box(k, v, o, d);
                crate::vehicles::flash_fx(k, v, &pose, &fm.flash_tags, d, fx);
                let trail = rm.missile.flight_fx.as_ref().and_then(|f| fx.fire_tracked(f, o, crate::combat::dir_quat(d)));
                gr.list.push(Grenade { rocket: Some(*wi), pos: o, vel: d * rm.missile.speed, fuse: 8.0, owner: Actor::Bot(g), team, resting: false, trail, tac: None, payload: None });
                let seek = match t {
                    Tgt::Vehicle(e) if rm.lock_on => Some(Seek { target: e, age: 0.0 }),
                    _ => None,
                };
                let n = gr.list.len();
                gr.seek.resize(n - 1, None);
                gr.seek.push(seek);
                if !fm.sound.is_empty() {
                    sfx.play(&fm.sound, Some(o));
                }
                let st = &mut v.guns[seat];
                st.ammo[m] -= 1;
                st.fire_cd = rm.interval.max(0.15);
                if st.ammo[m] <= 0 {
                    st.reload[m] = rm.reload;
                }
                if let Tgt::Vehicle(e) = t {
                    let tn = ctx.vehs.iter().find(|x| x.e == e).map(|x| x.name.clone()).unwrap_or_default();
                    log.event(ctx.now, format!("{gname} ({}) in {vname} fires {} at {tn} {:.0} m{}", team_name(team), rm.missile.class, dist / 100.0, if seek.is_some() { " (seeking)" } else { "" }));
                }
            }
        }
    }
}
