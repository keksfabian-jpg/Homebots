//! Bots like online players: battle points (KP), killstreak items, anti-vehicle launchers
//! (vehicles: botvehicles.rs). bots.rs keeps the infantry (movement, rifle, objectives); this
//! module only takes a bot over while it holds a launcher or sits in a vehicle (`controlled`).
//!
//! Data:
//! - GCBattlePoints InitialBattlePoints 500, BattlePointsCap 7000, SpecialAbilityCosts (Cost,
//!   PurchaseAmount: PZF3 / PROX 250 for 3 rockets, UAV 400, flak jacket 250, Hellfire 1300,
//!   cluster / phosphorus 1600, sensor fused 1000), VehicleCosts (botvehicles.rs);
//! - rewards as the player gets them (combat.rs / objectives.rs): KillBP of the streak tier
//!   (TDM: tdm::kill_bp), PSR_CaptureObjective 250, PSR_DestroyVehicle 250 (GCBattlePoints
//!   FL.Infantry Rewards);
//! - loadouts: GCCustomLoadoutProfiles DefaultLoadouts (by the bot's primary weapon): items 4 / 5
//!   (FlakJacket + PZF3, FlakJacket + GAssault, Hellfire + PZF3, GAssault + Hellfire, FlakJacket +
//!   UAV, AAssault + PROX);
//! - launchers: the weapon's projectile (speed, damage), equip / reload time, LockOn (PROX: the
//!   JAV lock-on, main.rs) with LockTime / MaxLockDist, the rockets seek like the player's
//!   (combat.rs Seek);
//! - airstrikes: CombatData::strikes (missile / payload, clip, reload, fuel, orbit of
//!   GCV_AirstrikeUAV), fired from the orbiting UAV like airstrike.rs;
//! - UAV: GCWeap_EQP_UAV MaxSweepDuration.
//!
//! NOT from data (marked where used): when a bot buys and uses what (the MP game had no bots),
//! the strike variants (only Hellfire is in the default loadouts: bots with it pick Hellfire /
//! cluster / phosphorus / sensor fused by slot, as custom loadouts would), the target choice,
//! reaction / aim times and errors, ranges, cooldowns, the UAV sweep shared with the team.
//! Drones: botdrones.rs (Mode::Drone). Not for bots: thermal goggles, resupply.
use crate::bots::{Bot, BotLog};
use crate::botvehicles::{self, BotVehicle};
use crate::combat::{Actor, CombatData, DamageQueue, Grenade, Grenades, Hit, Seek, Soldier, Target, TEAM_KVA, TEAM_USA};
use crate::menu::{MatchFlow, Phase};
use crate::player::{CollisionWorld, Pawn};
use crate::vehicles::{Vehicle, VehicleAssets, VehicleDamage, VehicleVolumes};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub struct BotExtrasPlugin;

impl Plugin for BotExtrasPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BotStrikes>()
            .init_resource::<botvehicles::BotVehTrack>()
            .init_resource::<TeamUav>()
            .add_systems(
                Update,
                (attach_kits, economy, killstreaks, launchers, botvehicles::bot_vehicles, publish_control)
                    .chain()
                    .after(crate::bots::bot_ai)
                    .after(crate::player::PlayerSet)
                    .before(crate::combat::CombatSet)
                    .before(crate::vehicles::update_volumes),
            )
            .add_systems(Update, filter_damage.after(crate::combat::update_grenades).before(crate::combat::apply_damage));
    }
}

/// Skill factor of the launcher / vehicle bots: bots::team_skill (1 + 0.75 x level, bots/brain.rs)
/// relative to the default level 0.8 (= 1.6), so the base times / errors below hold at the default
/// difficulty; easy ~0.8, guerilla ~1.12 (NOT from data). Clamped so they never become laser-exact.
pub fn skill_factor(team: u8) -> f32 {
    (crate::bots::team_skill(team) / 1.6).clamp(0.7, 1.15)
}

/// HB_BOTX=0 switches the whole module off (bots as before).
pub fn enabled() -> bool {
    std::env::var("HB_BOTX").map(|v| v != "0").unwrap_or(true)
}

/// Bots this module controls (bots.rs bot_ai skips them) and bot slots whose body is hidden
/// (in a vehicle; bots.rs render_bots).
static CONTROL: Mutex<(Vec<Entity>, Vec<usize>)> = Mutex::new((Vec::new(), Vec::new()));

pub fn controlled(e: Entity) -> bool {
    CONTROL.lock().map(|c| c.0.contains(&e)).unwrap_or(false)
}

pub fn hidden_slot(slot: usize) -> bool {
    CONTROL.lock().map(|c| c.1.contains(&slot)).unwrap_or(false)
}

/// GCBattlePoints InitialBattlePoints / BattlePointsCap (Coalesced_deu GCBattlePoints.ini).
const INITIAL_KP: i32 = 500;
const KP_CAP: i32 = 7000;
/// GCBattlePoints Rewards PSR_CaptureObjective, FL.Infantry PSR_DestroyVehicle.
const CAPTURE_BP: i32 = 250;
pub const DESTROY_VEHICLE_BP: i32 = 250;

/// What the bot does besides the infantry logic of bots.rs.
#[derive(Clone, Debug)]
pub enum Mode {
    Foot,
    /// in a vehicle (botvehicles.rs), seat index
    Vehicle { veh: Entity, seat: usize },
    /// launcher out against a vehicle
    Launcher(LauncherState),
    /// steering its drone (botdrones.rs): crouched in place, no infantry logic
    Drone(Entity),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    /// drawing the launcher (EquipTime)
    Draw,
    /// on the target: lead / lock-on
    Aim,
    /// after a shot (ReloadTime)
    Reload,
    /// back to the rifle (EquipTime)
    Holster,
}

#[derive(Clone, Debug)]
pub struct LauncherState {
    pub target: Entity,
    pub stage: Stage,
    pub t: f32,
    pub lock: f32,
    pub lost: f32,
    pub aim_need: f32,
    pub shots: u32,
}

/// Per bot: battle points, loadout, killstreak and vehicle state.
#[derive(Component)]
pub struct BotKit {
    pub kp: i32,
    pub loadout: usize,
    /// loadout items 4 / 5 (lower case cfg)
    pub items: [String; 2],
    pub mode: Mode,
    /// bought launcher: weapon index, rounds left (clip + reserve)
    pub launcher: Option<(usize, i32)>,
    pub flak: bool,
    /// the airstrike this bot uses (index into CombatData::strikes)
    pub strike: Option<usize>,
    /// buys vehicles (one bot in three per team, NOT from data)
    pub driver: bool,
    /// vehicle role of a driver (NOT from data; user 2026-10-22 21:32 "use tanks and helicopters
    /// too"): 0 any affordable vehicle, 1 saves for the team's most expensive ground vehicle
    /// (tank), 2 saves for a helicopter
    pub role: u8,
    /// respawned this frame (botvehicles.rs: vehicle purchase)
    pub just_spawned: bool,
    pub board_cd: f32,
    kills_seen: i32,
    alive_seen: bool,
    think: f32,
    /// killstreak cooldown (s; botdrones.rs sets it after a drone deploy)
    pub ks_cd: f32,
    seen_vehicle_cd: f32,
    pub seed: u32,
}

impl BotKit {
    pub fn rnd(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 100_000) as f32 / 100_000.0
    }

    pub fn in_vehicle(&self) -> Option<(Entity, usize)> {
        match self.mode {
            Mode::Vehicle { veh, seat } => Some((veh, seat)),
            _ => None,
        }
    }

    pub fn add_kp(&mut self, n: i32) {
        self.kp = (self.kp + n).min(KP_CAP);
    }
}

/// Team UAV sweeps (seconds left per team): a bot's UAV shows the enemies to its team; for the
/// player's team it also lights the player's minimap (NOT from data: the sweep shared with the team).
#[derive(Resource, Default)]
pub struct TeamUav(pub [f32; 2]);

static UAV: Mutex<[f32; 2]> = Mutex::new([0.0; 2]);

/// A team's UAV sweep is running (bots.rs may use it for awareness).
pub fn team_uav(team: u8) -> bool {
    UAV.lock().map(|u| u[team as usize & 1] > 0.0).unwrap_or(false)
}

/// A bot's airstrike in flight: the UAV circles the target point (GCV_AirstrikeUAV FlyRadius /
/// FlyAltitude / FlySpeed of the strike's orbit) and fires its clip at ReloadTime intervals.
pub struct BotStrike {
    pub owner: Entity,
    pub name: String,
    pub team: u8,
    pub kind: usize,
    pub centre: Vec3,
    pub angle: f32,
    pub left: i32,
    pub cd: f32,
    pub fuel: f32,
    pub kills_at: i32,
}

#[derive(Resource, Default)]
pub struct BotStrikes(pub Vec<BotStrike>);

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

/// Strike keys a bot with the Hellfire item picks from (NOT from data: custom loadouts).
const STRIKE_VARIANTS: [&str; 4] = ["cfg_bmb_hellfire", "cfg_bmb_cluster", "cfg_bmb_phosphorus", "cfg_bmb_sensorfused"];

fn attach_kits(mut commands: Commands, flow: Res<MatchFlow>, data: Option<Res<CombatData>>, assets: Option<Res<crate::bots::BotAssets>>, bots: Query<(Entity, &Bot, &Soldier), Without<BotKit>>) {
    if !enabled() || !matches!(flow.phase, Phase::Playing | Phase::Spawn) {
        return;
    }
    let (Some(data), Some(assets)) = (data, assets) else { return };
    let start_kp = std::env::var("HB_BOTKP").ok().and_then(|v| v.parse().ok()).unwrap_or(INITIAL_KP);
    let team_first = [TEAM_USA, TEAM_KVA].map(|t| assets.slots.iter().position(|sl| sl.team == t).unwrap_or(0));
    for (e, b, s) in &bots {
        let wcfg = assets.slots.get(b.slot).and_then(|sl| data.weapons.get(sl.weapon_cfg)).map(|w| w.cfg.clone()).unwrap_or_default();
        // the default loadout whose primary the bot carries (main.rs: slot k % primaries)
        let loadout = data.loadouts.iter().position(|l| l.primary.eq_ignore_ascii_case(&wcfg)).unwrap_or(b.slot % data.loadouts.len().max(1));
        let item = |i: usize| data.loadouts.get(loadout).and_then(|l| l.items.get(i)).map(|x| x.to_ascii_lowercase()).unwrap_or_default();
        let items = [item(3), item(4)];
        let local = b.slot.saturating_sub(team_first[s.team as usize & 1]);
        let strike = items.iter().any(|i| i == "cfg_bmb_hellfire").then(|| {
            let key = STRIKE_VARIANTS[local % STRIKE_VARIANTS.len()];
            data.strikes.iter().position(|st| st.key == key).or_else(|| data.strikes.iter().position(|st| st.key == "cfg_bmb_hellfire"))
        });
        let driver = local % 3 == 1;
        // the first driver of a team saves for the tank, the second for the helicopter
        let role = if driver { match local / 3 { 0 => 1, 1 => 2, _ => 0 } } else { 0 };
        commands.entity(e).insert(BotKit {
            kp: start_kp,
            loadout,
            items,
            mode: Mode::Foot,
            launcher: None,
            flak: false,
            strike: strike.flatten(),
            driver,
            role,
            just_spawned: false,
            board_cd: 2.0,
            kills_seen: s.kills,
            // the first frame counts as a spawn: drivers may buy at the match start
            alive_seen: false,
            think: 0.5 + (b.slot as f32) * 0.07,
            ks_cd: 20.0,
            seen_vehicle_cd: 0.0,
            seed: 0x51ed_270bu32.wrapping_mul(b.slot as u32 + 3),
        });
    }
}

/// Rewards and per-life purchases (flak jacket).
#[allow(clippy::too_many_arguments)]
fn economy(
    time: Res<Time>,
    flow: Res<MatchFlow>,
    data: Option<Res<CombatData>>,
    obj: Res<crate::objectives::Objectives>,
    mut bots: Query<(Entity, &Bot, &mut Soldier, &mut BotKit)>,
    mut log: ResMut<BotLog>,
    mut owners: Local<Vec<Option<u8>>>,
    mut uav: ResMut<TeamUav>,
) {
    if !enabled() || flow.phase != Phase::Playing {
        owners.clear();
        return;
    }
    let Some(data) = data else { return };
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    for u in uav.0.iter_mut() {
        *u = (*u - dt).max(0.0);
    }
    if let Ok(mut g) = UAV.lock() {
        *g = uav.0;
    }
    // captures: the team's bots standing in the volume when it changes hands
    let cur: Vec<Option<u8>> = obj.list.iter().map(|o| o.owner).collect();
    let mut captured: Vec<(usize, u8)> = Vec::new();
    if owners.len() == cur.len() {
        for (i, (a, b)) in owners.iter().zip(cur.iter()).enumerate() {
            if a != b {
                if let Some(t) = b {
                    captured.push((i, *t));
                }
            }
        }
    }
    *owners = cur;
    for (_, _b, mut s, mut kit) in &mut bots {
        // kills since the last frame: KillBP of the streak tier (combat.rs, as the player)
        if s.kills > kit.kills_seen {
            let n = s.kills - kit.kills_seen;
            let mut got = 0;
            for k in 0..n {
                let streak = (s.streak - (n - 1 - k)).max(0);
                got += crate::tdm::kill_bp().unwrap_or(crate::combat::streak_tier(streak - 1).1);
            }
            kit.add_kp(got);
            log.event(now, format!("{} +{got} KP (kill, streak {}) -> {} KP", s.name, s.streak, kit.kp));
        }
        kit.kills_seen = s.kills;
        for &(i, t) in &captured {
            let o = &obj.list[i];
            if s.alive && s.team == t && o.volume.contains(s.center_ue().to_array()) {
                kit.add_kp(CAPTURE_BP);
                log.event(now, format!("{} +{CAPTURE_BP} KP (PSR_CaptureObjective {}) -> {} KP", s.name, ["A", "B", "C", "D"][o.letter.min(3)], kit.kp));
            }
        }
        // a new life: the vehicle purchase (botvehicles.rs) and the flak jacket
        if s.alive && !kit.alive_seen {
            kit.just_spawned = true;
            kit.flak = false;
            kit.launcher = None;
            kit.mode = Mode::Foot;
            kit.ks_cd = kit.ks_cd.max(4.0);
        }
        if !s.alive {
            // dead: whatever it held is gone (botvehicles.rs empties the seat)
            if !matches!(kit.mode, Mode::Foot) {
                kit.mode = Mode::Foot;
            }
            kit.just_spawned = false;
        }
        kit.alive_seen = s.alive;
        // flak jacket (GCWeap_EQP_FlakJacket MaxHealthScale): once per life, when the KP for
        // the bot's bigger plans are left over (NOT from data: the reserve)
        // (drivers saving for a tank / helicopter do not spend on it)
        if s.alive && !kit.flak && kit.role == 0 && kit.items.iter().any(|i| i == "cfg_eqp_flakjacket") {
            let cost = data.costs.get("cfg_eqp_flakjacket").map(|c| c.0).unwrap_or(250);
            let reserve = if kit.driver { 300 } else { 0 };
            if kit.kp >= cost + reserve && kit.in_vehicle().is_none() {
                kit.kp -= cost;
                kit.flak = true;
                s.health = crate::combat::HEALTH_MAX * data.flak_scale;
                log.event(now, format!("{} buys flak jacket ({cost} KP): health {:.0} -> {} KP", s.name, s.health, kit.kp));
            }
        }
    }
}

/// Enemy soldiers / player / vehicles as (UE position, is vehicle) for target scoring.
fn enemy_points(team: u8, soldiers: &[(Entity, Vec3, u8, bool, bool)], player: Option<Vec3>, vehs: &[botvehicles::VehInfo]) -> Vec<(Vec3, bool)> {
    let mut v: Vec<(Vec3, bool)> = soldiers.iter().filter(|s| s.3 && s.2 != team && !s.4).map(|s| (s.1, false)).collect();
    if team != crate::combat::player_team() {
        if let Some(p) = player {
            v.push((p, false));
        }
    }
    for x in vehs.iter().filter(|x| x.team.is_some_and(|t| t != team)) {
        v.push((x.centre, true));
    }
    v
}

/// Killstreak items: UAV and airstrikes (bought and used at once, like the player's).
#[allow(clippy::too_many_arguments)]
fn killstreaks(
    time: Res<Time>,
    mut flow: ResMut<MatchFlow>,
    data: Option<Res<CombatData>>,
    col: Res<CollisionWorld>,
    pawns: Query<&Pawn>,
    mut bots: Query<(Entity, &Bot, &Soldier, &mut BotKit)>,
    vehicles: Query<(Entity, &Vehicle, Option<&BotVehicle>)>,
    vassets: Option<Res<VehicleAssets>>,
    (mut strikes, mut uav, mut gr, mut fx, mut sfx, mut log): (ResMut<BotStrikes>, ResMut<TeamUav>, ResMut<Grenades>, ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>, ResMut<BotLog>),
    (mut team_cd, mut buf, mut warned): (Local<[f32; 2]>, Local<Vec<u32>>, Local<HashSet<String>>),
    mut feed: ResMut<crate::combat::KillFeed>,
) {
    if !enabled() || flow.phase != Phase::Playing {
        strikes.0.clear();
        *team_cd = [0.0; 2];
        return;
    }
    let Some(data) = data else { return };
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    for c in team_cd.iter_mut() {
        *c = (*c - dt).max(0.0);
    }
    let player = pawns.single().ok().filter(|p| !p.dead).map(|p| {
        let e = p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
        Vec3::new(e.x, e.z, e.y) * 100.0
    });
    let vehs = vassets.as_ref().map(|a| botvehicles::vehicle_infos(a, vehicles.iter())).unwrap_or_default();
    let soldiers: Vec<(Entity, Vec3, u8, bool, bool)> = bots.iter().map(|(e, _, s, k)| (e, s.center_ue(), s.team, s.alive, k.in_vehicle().is_some())).collect();
    // ---- strikes in flight
    let mut done = Vec::new();
    for (i, st) in strikes.0.iter_mut().enumerate() {
        let Some(sk) = data.strikes.get(st.kind) else {
            done.push(i);
            continue;
        };
        let hf = &sk.cfg;
        st.fuel -= dt;
        st.angle += hf.orbit[2] * dt;
        let pos = st.centre + Vec3::new(st.angle.cos() * hf.orbit[1], st.angle.sin() * hf.orbit[1], hf.orbit[0]);
        if !sk.engine.is_empty() {
            sfx.looping(&format!("botstrike{}", st.owner.index()), &sk.engine, Some(pos));
        }
        st.cd -= dt;
        if st.left > 0 && st.cd <= 0.0 {
            // re-aimed at the enemy nearest the marked point (the operator follows the target,
            // NOT from data), else the point itself
            let r = strike_radius(hf);
            let aim = enemy_points(st.team, &soldiers, player, &vehs).into_iter().map(|x| x.0).filter(|p| Vec2::new(p.x - st.centre.x, p.y - st.centre.y).length() < r * 2.0).min_by(|a, b| a.distance(st.centre).total_cmp(&b.distance(st.centre))).unwrap_or(st.centre);
            let start = pos + (aim - pos).normalize_or_zero() * 150.0;
            let md = (aim - start).normalize_or_zero();
            let trail = hf.missile.flight_fx.as_ref().and_then(|t| fx.fire_tracked(t, start, crate::combat::dir_quat(md)));
            gr.list.push(Grenade { rocket: Some(sk.weapon), pos: start, vel: md * hf.missile.speed, fuse: 14.0, owner: Actor::Bot(st.owner), team: st.team, resting: false, trail, tac: None, payload: hf.payload.as_ref().map(|_| (st.kind, aim)) });
            let n = gr.list.len();
            gr.seek.resize(n, None);
            if !sk.fire_sound.is_empty() {
                sfx.play(&sk.fire_sound, Some(pos));
            }
            st.left -= 1;
            st.cd = hf.reload.max(0.3);
            log.event(now, format!("{} ({}) {}: shot at ({:.0} {:.0} {:.0}), {} left", st.name, team_name(st.team), sk.key, aim.x, aim.y, aim.z, st.left));
        }
        if (st.left <= 0 && st.cd < -3.0) || st.fuel <= 0.0 {
            done.push(i);
        }
    }
    for i in done.into_iter().rev() {
        let st = strikes.0.remove(i);
        let kills = bots.get(st.owner).map(|(_, _, s, _)| s.kills - st.kills_at).unwrap_or(0);
        log.event(now, format!("{} ({}) airstrike over: {kills} kills since it was called", st.name, team_name(st.team)));
    }
    // ---- decisions
    for (e, _b, s, mut kit) in &mut bots {
        kit.ks_cd = (kit.ks_cd - dt).max(0.0);
        kit.think -= dt;
        if kit.think > 0.0 || !s.alive {
            continue;
        }
        kit.think = 1.0;
        if kit.ks_cd > 0.0 {
            continue;
        }
        let items = kit.items.clone();
        // UAV (CFG_EQP_UAV): when the team has no sweep running (NOT from data: 25 % per second
        // once affordable, kept for a moment with no enemy in sight)
        if items.iter().any(|i| i == "cfg_eqp_uav") && uav.0[s.team as usize & 1] <= 0.0 {
            let cost = data.costs.get("cfg_eqp_uav").map(|c| c.0).unwrap_or(400);
            if kit.kp >= cost && kit.rnd() < 0.25 {
                kit.kp -= cost;
                uav.0[s.team as usize & 1] = data.uav_secs.max(1.0);
                if s.team == crate::combat::player_team() {
                    flow.hud.uav_left = flow.hud.uav_left.max(data.uav_secs);
                }
                if let Some(c) = data.costs.get("cfg_eqp_uav").and_then(|c| c.2.clone()).filter(|_| s.team == crate::combat::player_team()) {
                    sfx.play(&c, None);
                }
                kit.ks_cd = 10.0;
                // kill crawl line so the player sees it (NOT from data)
                feed.0.push((format!("{}: UAV", s.name), s.team, String::new(), 2, None, crate::objectives::CRAWL_SECS));
                log.event(now, format!("{} ({}) killstreak UAV ({cost} KP): enemies shown {:.0} s -> {} KP", s.name, team_name(s.team), data.uav_secs, kit.kp));
                continue;
            }
        }
        // airstrike: the best enemy group within reach (100 m of the bot, the whole map while the
        // team's UAV runs), at least 2 enemies or a vehicle in the strike's radius (NOT from data)
        if let Some(k) = kit.strike {
            let sk = &data.strikes[k];
            let cost = data.costs.get(&sk.key).map(|c| c.0).unwrap_or(1300);
            if kit.kp >= cost && team_cd[s.team as usize & 1] <= 0.0 {
                // grouping radius at least 5 m; reach 150 m (user 2026-10-22 21:09: no bot
                // killstreak seen - with "2 enemies within the Hellfire's 3-4 m within 100 m" a
                // strike almost never happened). NOT from data.
                let r = strike_radius(&sk.cfg).max(500.0);
                let me = s.center_ue();
                let reach = if uav.0[s.team as usize & 1] > 0.0 { f32::MAX } else { 15_000.0 };
                let pts = enemy_points(s.team, &soldiers, player, &vehs);
                let best = pts
                    .iter()
                    .filter(|p| p.0.distance(me) < reach)
                    .map(|p| {
                        let score: f32 = pts.iter().filter(|q| Vec2::new(q.0.x - p.0.x, q.0.y - p.0.y).length() <= r).map(|q| if q.1 { 2.0 } else { 1.0 }).sum();
                        (score, p.0)
                    })
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((score, at)) = best.filter(|b| b.0 >= 1.0) {
                    // on the ground below the point
                    let ab = to_bevy(at) + Vec3::Y * 1.0;
                    let ground = col.floor(ab, 30.0, &mut buf).map(|(y, _)| Vec3::new(at.x, at.y, y * 100.0)).unwrap_or(at);
                    kit.kp -= cost;
                    kit.ks_cd = 30.0;
                    team_cd[s.team as usize & 1] = 20.0;
                    let ang = (me.y - ground.y).atan2(me.x - ground.x);
                    strikes.0.push(BotStrike { owner: e, name: s.name.clone(), team: s.team, kind: k, centre: ground, angle: ang, left: sk.cfg.clip.max(1), cd: 3.0, fuel: sk.cfg.fuel.max(10.0), kills_at: s.kills });
                    if let Some(c) = data.costs.get(&sk.key).and_then(|c| c.2.clone()).filter(|_| s.team == crate::combat::player_team()) {
                        sfx.play(&c, None);
                    }
                    // the game's localized name (CombatData::special_names), else an English default
                    // (NOT from data)
                    let label = data.special_names.get(&sk.key).map(|n| n.to_uppercase()).unwrap_or_else(|| {
                        match sk.key.as_str() {
                            "cfg_bmb_cluster" => "CLUSTER BOMB",
                            "cfg_bmb_phosphorus" => "WHITE PHOSPHORUS",
                            "cfg_bmb_sensorfused" => "SENSOR FUSED",
                            _ => "HELLFIRE",
                        }
                        .to_string()
                    });
                    feed.0.push((format!("{}: {label}", s.name), s.team, String::new(), 2, None, crate::objectives::CRAWL_SECS));
                    log.event(now, format!("{} ({}) killstreak {} ({cost} KP) on ({:.0} {:.0} {:.0}): score {score:.0} (enemies, vehicles x2) within {r:.0} uu -> {} KP", s.name, team_name(s.team), sk.key, ground.x, ground.y, ground.z, kit.kp));
                    continue;
                }
            }
        }
        for i in &items {
            if (i == "cfg_eqp_thermal" || i == "cfg_eqp_resupply") && warned.insert(i.clone()) {
                log.event(now, format!("bots: item {i} not used by bots (not implemented for bots)"));
            }
        }
    }
}

/// Area a strike covers (target scoring): the bomb's spread, else the missile's full radius /
/// radius.
fn strike_radius(hf: &crate::weaponcfg::HellfireCfg) -> f32 {
    match &hf.payload {
        Some(p) => p.spread.max(300.0),
        None => hf.missile.radius_full.max(hf.missile.radius).max(200.0),
    }
}

/// Launcher weapon of a loadout item list (CFG_ATW_*), as a CombatData weapon index.
fn launcher_of(data: &CombatData, items: &[String; 2]) -> Option<(usize, String)> {
    items.iter().filter(|i| i.starts_with("cfg_atw_")).find_map(|i| data.weapons.iter().position(|w| w.cfg.eq_ignore_ascii_case(i) && w.projectile.is_some()).map(|wi| (wi, i.clone())))
}

/// Range (UE units) a bot uses its launcher at: lock-on launchers MaxLockDist (at most 100 m),
/// the straight PZF3 60 m, 40 m against helicopters (NOT from data).
fn launcher_range(w: &crate::weaponcfg::WeaponCfg, heli: bool) -> f32 {
    match &w.lock_on {
        Some(l) if l.max_dist > 0.0 => l.max_dist.min(10_000.0),
        _ if heli => 4000.0,
        _ => 6000.0,
    }
}

/// Sight line from `eye` to a vehicle box centre: no level geometry and no other vehicle box in
/// between.
fn sees_vehicle(col: &CollisionWorld, vols: &VehicleVolumes, eye: Vec3, target: Entity, c: Vec3, buf: &mut Vec<u32>) -> bool {
    let d = (c - eye).normalize_or_zero();
    let dist = eye.distance(c);
    let tv = match vols.ray(eye, d, dist, false) {
        Some((e, t)) if e == target => t,
        Some(_) => return false,
        None => dist,
    };
    let ob = to_bevy(eye);
    let db = Vec3::new(d.x, d.z, d.y);
    col.raycast(ob, db, (tv - 30.0).max(0.0) * 0.01, buf).is_none()
}

/// Anti-vehicle: bots with a launcher in the loadout buy it (SpecialAbilityCosts) when an enemy
/// vehicle shows up, draw it, lead the target / lock on (PROX: the LockOn rules), fire, reload,
/// then go back to the rifle.
#[allow(clippy::too_many_arguments)]
fn launchers(
    time: Res<Time>,
    flow: Res<MatchFlow>,
    data: Option<Res<CombatData>>,
    col: Res<CollisionWorld>,
    vols: Res<VehicleVolumes>,
    vassets: Option<Res<VehicleAssets>>,
    vehicles: Query<(Entity, &Vehicle, Option<&BotVehicle>)>,
    mut bots: Query<(Entity, &mut Bot, &mut Soldier, &mut BotKit)>,
    (mut gr, mut fx, mut wsfx, mut log): (ResMut<Grenades>, ResMut<crate::particles::Particles>, ResMut<crate::sfx::WeaponSfx>, ResMut<BotLog>),
    mut buf: Local<Vec<u32>>,
) {
    if !enabled() || flow.phase != Phase::Playing {
        return;
    }
    let (Some(data), Some(vassets)) = (data, vassets) else { return };
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    let vehs = botvehicles::vehicle_infos(&vassets, vehicles.iter());
    for (e, mut b, mut s, mut kit) in &mut bots {
        // (nuke.rs held: the bot the Fulton pickup takes does nothing)
        if !s.alive || crate::nuke::held(e) {
            continue;
        }
        let Some((wi, item)) = launcher_of(&data, &kit.items) else { continue };
        let w = &data.weapons[wi];
        let eye = s.center_ue() + Vec3::Z * 35.5;
        let skill = skill_factor(s.team);
        match kit.mode.clone() {
            Mode::Foot => {
                kit.seen_vehicle_cd -= dt;
                if kit.seen_vehicle_cd > 0.0 || s.blinded > 0.0 {
                    continue;
                }
                kit.seen_vehicle_cd = 0.5;
                // the most threatening visible enemy vehicle in range: nearest, helicopters and
                // vehicles closer than 30 m first (NOT from data)
                let pick = vehs
                    .iter()
                    .filter(|x| x.team.is_some_and(|t| t != s.team))
                    .filter(|x| x.centre.distance(eye) < launcher_range(w, x.heli))
                    .filter(|x| sees_vehicle(&col, &vols, eye, x.e, x.centre, &mut buf))
                    .min_by(|a, c| {
                        let score = |x: &botvehicles::VehInfo| x.centre.distance(eye) * if x.heli || x.centre.distance(eye) < 3000.0 { 0.5 } else { 1.0 };
                        score(a).total_cmp(&score(c))
                    })
                    .cloned();
                let Some(t) = pick else { continue };
                if kit.launcher.is_none_or(|l| l.1 <= 0) {
                    // buy: Cost / PurchaseAmount (SpecialAbilityCosts)
                    let (cost, amount, _) = data.costs.get(&item).cloned().unwrap_or((250, 3, None));
                    if kit.kp < cost {
                        continue;
                    }
                    kit.kp -= cost;
                    kit.launcher = Some((wi, amount.max(1)));
                    log.event(now, format!("{} ({}) buys {} ({cost} KP, {} rockets) against {} -> {} KP", s.name, team_name(s.team), w.cfg, amount.max(1), t.name, kit.kp));
                }
                let need = (0.7 + kit.rnd() * 0.7) / skill;
                kit.mode = Mode::Launcher(LauncherState { target: t.e, stage: Stage::Draw, t: w.equip_time.max(0.4), lock: 0.0, lost: 0.0, aim_need: need, shots: 0 });
                log.event(now, format!("{} ({}) draws {} at {} ({}) {:.0} m{}", s.name, team_name(s.team), w.cfg, t.name, t.team.map(team_name).unwrap_or("-"), t.centre.distance(eye) / 100.0, if t.heli { " (helicopter)" } else { "" }));
            }
            Mode::Launcher(mut st) => {
                s.crouched = false;
                let tgt = vehs.iter().find(|x| x.e == st.target).cloned();
                let visible = tgt.as_ref().is_some_and(|t| t.team.is_some_and(|tt| tt != s.team) && t.centre.distance(eye) < launcher_range(w, t.heli) * 1.2 && sees_vehicle(&col, &vols, eye, t.e, t.centre, &mut buf));
                st.lost = if visible { 0.0 } else { st.lost + dt };
                // face the target (turn rate as bots.rs, 6 rad/s)
                if let Some(t) = &tgt {
                    let want = (t.centre.y - eye.y).atan2(t.centre.x - eye.x);
                    let mut dy = (want - b.yaw).rem_euclid(std::f32::consts::TAU);
                    if dy > std::f32::consts::PI {
                        dy -= std::f32::consts::TAU;
                    }
                    b.yaw += dy.clamp(-6.0 * dt, 6.0 * dt);
                    let flat = Vec2::new(t.centre.x - eye.x, t.centre.y - eye.y).length();
                    b.pitch = (t.centre.z - eye.z).atan2(flat.max(1.0));
                }
                st.t -= dt;
                match st.stage {
                    Stage::Draw | Stage::Reload => {
                        if st.lost > 1.5 {
                            st.stage = Stage::Holster;
                            st.t = w.equip_time.max(0.4);
                        } else if st.t <= 0.0 {
                            st.stage = Stage::Aim;
                            st.t = 0.0;
                            st.lock = 0.0;
                        }
                    }
                    Stage::Aim => {
                        if st.lost > 1.5 || tgt.is_none() {
                            log.event(now, format!("{} ({}) loses the launcher target, back to the rifle", s.name, team_name(s.team)));
                            st.stage = Stage::Holster;
                            st.t = w.equip_time.max(0.4);
                        } else if visible {
                            let t = tgt.as_ref().unwrap();
                            let lock_cfg = w.lock_on.as_ref();
                            let facing = {
                                let want = (t.centre.y - eye.y).atan2(t.centre.x - eye.x);
                                let mut dy = (want - b.yaw).rem_euclid(std::f32::consts::TAU);
                                if dy > std::f32::consts::PI {
                                    dy -= std::f32::consts::TAU;
                                }
                                dy.abs() < 0.08
                            };
                            if facing {
                                st.lock += dt;
                            }
                            // lock-on launchers fire once locked (LockTime), the PZF3 after the
                            // bot's aim time
                            let ready = match lock_cfg {
                                Some(l) => st.lock >= l.lock_time,
                                None => st.lock >= st.aim_need,
                            };
                            if ready {
                                let pj = w.projectile.as_ref().unwrap();
                                let speed = pj.speed.max(500.0);
                                // lead: where the vehicle is when the rocket gets there (straight
                                // flight, GCProj_Rocket m_fGravityFactor 0), two iterations
                                let mut aim = t.centre;
                                for _ in 0..2 {
                                    aim = t.centre + t.vel * (aim.distance(eye) / speed);
                                }
                                // NOT from data: aim error 1-2.5 deg / skill factor (default difficulty 1.0)
                                let err = (1.0 + kit.rnd() * 1.5) / skill;
                                let mut seed = kit.seed;
                                let d = crate::combat::spread_dir((aim - eye).normalize_or_zero(), err, &mut seed);
                                kit.seed = seed;
                                let pos = eye + d * 60.0;
                                let trail = pj.flight_fx.as_ref().and_then(|f| fx.fire_tracked(f, pos, crate::combat::dir_quat(d)));
                                gr.list.push(Grenade { rocket: Some(wi), pos, vel: d * speed, fuse: 14.0, owner: Actor::Bot(e), team: s.team, resting: false, trail, tac: None, payload: None });
                                let seek = lock_cfg.map(|_| Seek { target: t.e, age: 0.0 });
                                let n = gr.list.len();
                                gr.seek.resize(n - 1, None);
                                gr.seek.push(seek);
                                if let Some(si) = &w.sound_info {
                                    wsfx.shot(&format!("botl{}", e.index()), si, 1, Some(eye), 0.0);
                                }
                                if let Some(mf) = &w.muzzle[1] {
                                    fx.fire(mf, pos, crate::combat::dir_quat(d));
                                }
                                st.shots += 1;
                                if let Some(l) = kit.launcher.as_mut() {
                                    l.1 -= 1;
                                }
                                let left = kit.launcher.map(|l| l.1).unwrap_or(0);
                                log.event(now, format!("{} ({}) fires {} at {} ({}) {:.0} m{}, lead {:.1} m, {left} left", s.name, team_name(s.team), w.cfg, t.name, t.team.map(team_name).unwrap_or("-"), t.centre.distance(eye) / 100.0, if lock_cfg.is_some() { " locked" } else { "" }, (aim - t.centre).length() / 100.0));
                                if left > 0 {
                                    st.stage = Stage::Reload;
                                    st.t = w.reload_time.max(0.5);
                                } else {
                                    st.stage = Stage::Holster;
                                    st.t = w.equip_time.max(0.4);
                                }
                            }
                        }
                    }
                    Stage::Holster => {
                        if st.t <= 0.0 {
                            kit.mode = Mode::Foot;
                            kit.seen_vehicle_cd = 2.0;
                            continue;
                        }
                    }
                }
                kit.mode = Mode::Launcher(st);
            }
            Mode::Vehicle { .. } | Mode::Drone(_) => {}
        }
    }
}

/// Publishes the controlled bots / hidden bodies for bots.rs.
fn publish_control(bots: Query<(Entity, &Bot, &Soldier, &BotKit)>) {
    let mut c = (Vec::new(), Vec::new());
    for (e, b, s, k) in &bots {
        if !s.alive {
            continue;
        }
        match k.mode {
            Mode::Foot => {}
            Mode::Launcher(_) | Mode::Drone(_) => c.0.push(e),
            Mode::Vehicle { .. } => {
                c.0.push(e);
                c.1.push(b.slot);
            }
        }
    }
    if let Ok(mut g) = CONTROL.lock() {
        *g = c;
    }
}

/// Between the projectiles and the damage: bots inside vehicles take no hits of their own (the
/// vehicle takes them; they die with it), vehicles of a team take no hits from that team
/// (friendly fire off), every hit on a bot vehicle is remembered (kill credit) and the
/// launcher hits are logged.
#[allow(clippy::too_many_arguments)]
fn filter_damage(
    time: Res<Time>,
    data: Option<Res<CombatData>>,
    mut queue: ResMut<DamageQueue>,
    mut vdmg: ResMut<VehicleDamage>,
    bots: Query<(&Soldier, &BotKit)>,
    vehicles: Query<(&Vehicle, Option<&BotVehicle>)>,
    pawns: Query<&Pawn>,
    vassets: Option<Res<VehicleAssets>>,
    (mut track, mut log): (ResMut<botvehicles::BotVehTrack>, ResMut<BotLog>),
) {
    if !enabled() {
        return;
    }
    let now = time.elapsed_secs();
    let launcher_types: HashSet<String> = data.as_ref().map(|d| d.weapons.iter().filter(|w| w.cfg.to_ascii_lowercase().starts_with("cfg_atw_")).filter_map(|w| w.projectile.as_ref().map(|p| p.dmg_type.to_ascii_lowercase())).collect()).unwrap_or_default();
    let is_launcher = |h: &Hit| h.dmg_type.as_ref().is_some_and(|t| launcher_types.contains(&t.to_ascii_lowercase()));
    let name_of = |a: Actor| match a {
        Actor::Bot(b) => bots.get(b).map(|(s, _)| s.name.clone()).unwrap_or_default(),
        Actor::Player => "Du".into(),
        Actor::World => "Welt".into(),
    };
    let player_in_vehicle = pawns.single().is_ok_and(|p| p.in_vehicle && !p.dead);
    let mut logged: HashMap<(Entity, u32), ()> = HashMap::new();
    queue.0.retain(|h| {
        if let Target::Bot(t) = h.target {
            if bots.get(t).is_ok_and(|(_, k)| k.in_vehicle().is_some()) {
                return false;
            }
        }
        if h.target == Target::Player && player_in_vehicle && matches!(h.attacker, Actor::Bot(_)) && is_launcher(h) {
            log.event(now, format!("{} launcher hit on the player's vehicle ({:.0} before armour)", name_of(h.attacker), h.amount));
        }
        true
    });
    vdmg.0.retain(|(ve, h)| {
        let Ok((v, bv)) = vehicles.get(*ve) else { return true };
        let team = bv.map(|b| b.team).or(v.player_seat.map(|_| crate::combat::player_team()));
        if let Some(team) = team {
            if h.attacker_team == team && h.attacker != Actor::World {
                return false;
            }
        }
        if bv.is_some() {
            track.hit(*ve, h, now);
        }
        if let (Actor::Bot(b), true) = (h.attacker, is_launcher(h)) {
            if logged.insert((*ve, b.index()), ()).is_none() {
                let vname = vassets.as_ref().and_then(|a| a.kinds.get(v.kind)).map(|k| k.name.clone()).unwrap_or_default();
                log.event(now, format!("{} launcher hit on {vname} ({}): {:.0} before armour, health {:.0}", name_of(h.attacker), team.map(team_name).unwrap_or("empty"), h.amount, v.health));
            }
        }
        true
    });
}
