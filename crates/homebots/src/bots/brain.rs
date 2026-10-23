//! Bot combat skill: aim (reaction, tracking error, recoil, weapon spread, ADS), fire control
//! (bursts by range), target choice, hearing, look direction, cover peeking, grenades.
//! Submodule of bots.rs (uses the Bot fields directly); bot_ai calls these functions.
//!
//! Data used (GCGame.u, the SP bot AI; Homefront's MP had no bots, so these are adapted):
//! - GCAIConfig.BotMemoryConfig: SightProcessMin/Max 0.25 / 0.9 s (reaction at the lowest
//!   level), NoiseProcessesTimeMin/Max 0.1 / 0.4 s, NoiseProcessesDistanceMax 3000 uu (hearing)
//! - GCAIConfig.BotFireConfig: BotIsMovingModifier / TargetIsMovingModifier 0.75 (accuracy
//!   while moving), SimpleFireDistance 1800, PointBlankDistance 300; BotAnticipationConfig
//!   Distance_MinForAnticipation 200
//! - Default__GCBotConfig Weapon0: Stationary Wait 0.375-0.675 / Duration 0.675-1.375 s,
//!   Moving Wait 2.5-6.0 / Duration 0.5-1.0 s, OpenFullFireDistanceMax 500; Reload MaxBurstTime
//!   1.75
//! - Default__GCAIEngagementConfig: Targeting (BotTargetScoringConfig: VisibleBonus 2000,
//!   DistanceBonusMaxDist 3000, zones 550 / 900 / 1500 / 2000 with the _Human multipliers
//!   5 / 0.7 / 0.55 / 0.4 / 0.2, ShotRecentlyBonusPlayer 1000, CurTargetBonusPerSec 50 up to
//!   CurTargetBonusMaxTime 4), CombatInCover fDuckAfterShotByBotChance 0.5 / ByPlayer 0.8,
//!   Grenades (BotGrenadeConfig: MinCooldown 4, MaxCooldown 10, AreaCooldown 3.5, AreaRadius
//!   1000, AllyRadius 200, throw distance 120-2000 / height -200..200, ThrowChanceHidden 80-100,
//!   ThrowChanceVisible 5, MinTargetStationaryTime 1, ThrowReEvalInterval 1), MaxSWATRange 800
//! - weapon data as for the player: Spread (Stand / Crouch Min, Max, FireAdd, DecayRate),
//!   Recoil Hip / Zoom (ViewPitchMin/Max, ViewRecoveryRate), ADSMovementPercent, ADSBringUpTime
//! Everything else here is NOT from data (marked).
use super::*;
use crate::combat::Grenade;

/// Per-bot combat state.
#[derive(Default, Clone)]
pub struct Brain {
    /// aim error (degrees): x yaw, y pitch; settles over time (tracking)
    pub err: Vec2,
    /// recoil the bot did not pull down (degrees of pitch)
    pub recoil: f32,
    /// weapon deviation like PlayerCombat::spread (Spread.Stand/Crouch Min .. Max)
    pub spread: f32,
    /// aim-down-sights blend 0..1
    pub ads: f32,
    /// aim height above the target's cylinder centre (uu)
    pub aim_h: f32,
    /// target centre at the last frame and its velocity (UE units / s)
    pub tgt_last: Option<Vec3>,
    pub tgt_vel: Vec3,
    /// seconds the target has stood (nearly) still
    pub tgt_still: f32,
    /// seconds on the current target (target scoring hysteresis)
    pub tgt_time: f32,
    /// last known position of the lost target (UE centre) and when it was lost
    pub lkp: Option<(Vec3, f32)>,
    /// heard enemy fire: position (UE) and the time it is "processed" (heard + delay)
    pub heard: Option<(Vec3, f32)>,
    /// idle look direction (UE yaw) and seconds until the next one
    pub look: (f32, f32),
    /// crouch in this engagement while standing still at range
    pub crouch_fight: bool,
    /// in cover: peeking (standing) and seconds left in the phase
    pub peek: (bool, f32),
    /// zig-zag phase while moving under fire
    pub weave: f32,
    /// frag grenades left this life, cooldown (s), seconds to the next evaluation
    pub nades: i32,
    pub nade_cd: f32,
    pub nade_eval: f32,
    pub init: bool,
}

/// Noise events (gunfire) of the last second: (UE position, team, time).
static NOISE: std::sync::Mutex<Vec<(Vec3, u8, f32)>> = std::sync::Mutex::new(Vec::new());

/// Record a shot (bots in bot_ai, the player in player_noise).
pub fn noise(at: Vec3, team: u8, now: f32) {
    if let Ok(mut n) = NOISE.lock() {
        n.retain(|e| now - e.2 < 1.0 && now >= e.2);
        if n.len() < 256 {
            n.push((at, team, now));
        }
    }
}

/// Nearest enemy shot heard within NoiseProcessesDistanceMax 3000 uu in the last 0.5 s.
pub fn hear(me: Vec3, team: u8, now: f32) -> Option<(Vec3, f32)> {
    let n = NOISE.lock().ok()?;
    n.iter().filter(|e| e.1 != team && now - e.2 < 0.5).map(|e| (e.0, e.0.distance(me))).filter(|e| e.1 < 3000.0).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
}

/// Bot skill level 0..1 (settings "Schwierigkeit" = Bot-Stärke for both teams, settings.rs
/// bot_level; HB_BOTSKILL=<0..1> overrides). KVA +0.05 (user 2026-10-19: enemy team slightly
/// stronger). NOT from data.
pub fn level(team: u8) -> f32 {
    // the override is read once (level is asked several times per bot and frame)
    static OVERRIDE: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let base = OVERRIDE.get_or_init(|| std::env::var("HB_BOTSKILL").ok().and_then(|v| v.parse::<f32>().ok())).unwrap_or_else(crate::settings::bot_level);
    (base + if team == TEAM_KVA { 0.05 } else { 0.0 }).clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn gauss(seed: &mut u32) -> f32 {
    let u = rnd(seed).max(1e-4);
    let v = rnd(seed);
    (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
}

/// Reaction time before the first shot at a newly seen target. Data range SightProcessMin/Max
/// 0.25-0.9 s at level 0; 0.18-0.35 s at level 1 (user's wish "180-350 ms", NOT from data).
/// Expected (target appears where the bot looked: heard / last known position) x0.6,
/// peripheral (> 30 deg off the view) +0.08 s (NOT from data).
pub fn reaction(lv: f32, seed: &mut u32, expected: bool, peripheral: bool) -> f32 {
    let lo = lerp(0.25, 0.18, lv);
    let hi = lerp(0.9, 0.35, lv);
    let r = rnd(seed);
    let mut t = lo + (hi - lo) * r * r.sqrt();
    if expected {
        t *= 0.6;
    }
    if peripheral {
        t += 0.08;
    }
    t.max(0.12)
}

/// BotTargetScoringConfig (values from data; the native formula is not decoded, NOT from data:
/// sum of VisibleBonus, distance bonus (MaxDist - d) x zone multiplier, shot-me bonus,
/// current-target bonus per second).
pub fn target_score(d: f32, shot_me: bool, current_for: Option<f32>) -> f32 {
    let mult = if d < 550.0 {
        5.0
    } else if d < 900.0 {
        0.7
    } else if d < 1500.0 {
        0.55
    } else if d < 2000.0 {
        0.4
    } else {
        0.2
    };
    2000.0 + (3000.0 - d.min(3000.0)) * mult + if shot_me { 1000.0 } else { 0.0 } + current_for.map(|t| 50.0 * t.min(4.0)).unwrap_or(0.0)
}

/// New target acquired: initial aim error from the flick angle (NOT from data: 2.5-6 deg by
/// level plus 5 % of the turn), aim height chest .. neck by level (headshot share, NOT from
/// data), crouch decision for this engagement.
pub fn acquire(br: &mut Brain, lv: f32, flick_rad: f32, seed: &mut u32) {
    let e0 = lerp(6.0, 2.5, lv) + flick_rad.abs().to_degrees() * 0.05;
    let a = rnd(seed) * std::f32::consts::TAU;
    br.err = Vec2::new(a.cos(), a.sin() * 0.6) * e0;
    br.aim_h = 4.0 + rnd(seed) * lerp(8.0, 18.0, lv);
    br.crouch_fight = rnd(seed) < lerp(0.25, 0.6, lv);
    br.tgt_time = 0.0;
    br.tgt_still = 0.0;
}

/// Per frame while a target is seen: target velocity, aim settling (Ornstein-Uhlenbeck:
/// error decays with tau, residual jitter sigma; NOT from data), worse while the bot or the
/// target moves (BotIsMovingModifier / TargetIsMovingModifier 0.75 from data as 1/0.75),
/// spread decay, recoil recovery, ADS blend (ADSBringUpTime).
#[allow(clippy::too_many_arguments)]
pub fn track(br: &mut Brain, w: &crate::weaponcfg::WeaponCfg, lv: f32, dt: f32, tp: Vec3, bot_moving: bool, crouched: bool, want_ads: bool, seed: &mut u32) {
    if dt <= 0.0 {
        return;
    }
    if let Some(l) = br.tgt_last {
        let v = (tp - l) / dt;
        // jumps (target switch / respawn) ignored
        if v.length() < 1500.0 {
            br.tgt_vel = br.tgt_vel.lerp(v, (dt * 8.0).min(1.0));
        }
    }
    br.tgt_last = Some(tp);
    br.tgt_time += dt;
    let speed = Vec2::new(br.tgt_vel.x, br.tgt_vel.y).length();
    br.tgt_still = if speed < 60.0 { br.tgt_still + dt } else { 0.0 };
    let tau = lerp(0.9, 0.3, lv);
    let mut sigma = lerp(0.9, 0.22, lv);
    if bot_moving {
        sigma /= 0.75;
    }
    if speed > 150.0 {
        sigma /= 0.75;
    }
    let k = (dt / tau).min(1.0);
    br.err += -br.err * k + Vec2::new(gauss(seed), gauss(seed) * 0.6) * sigma * (2.0 * k).sqrt();
    let rc = if br.ads > 0.5 { &w.recoil_zoom } else { &w.recoil_hip };
    br.recoil = (br.recoil - rc.recovery.max(5.0) * dt).max(0.0);
    let dev = if crouched { &w.spread_crouch } else { &w.spread_stand };
    br.spread = (br.spread - dev.decay * dt).max(dev.min);
    let up = w.ads_up.max(0.05);
    br.ads = if want_ads && !w.disable_zoom { (br.ads + dt / up).min(1.0) } else { (br.ads - dt / w.ads_down.max(0.05)).max(0.0) };
}

/// Direction of one shot (UE): aim point on the target (aim height, tracking lag partly
/// anticipated), rotated by the aim error and the uncompensated recoil, then the weapon's cone
/// as for the player (spread / 2 at the hip, 95 % less in ADS). Lag 0.12 s and anticipation
/// 0.3-0.85 by level NOT from data (BotAnticipationConfig only from 200 uu on).
pub fn shot_dir(br: &Brain, lv: f32, eye: Vec3, tp: Vec3, seed: &mut u32) -> Vec3 {
    let d0 = tp.distance(eye);
    let antic = if d0 > 200.0 { lerp(0.3, 0.85, lv) } else { 1.0 };
    let aim = tp + Vec3::Z * br.aim_h - br.tgt_vel * 0.12 * (1.0 - antic);
    let d = (aim - eye).normalize_or_zero();
    let yaw = d.y.atan2(d.x) + br.err.x.to_radians();
    let pitch = d.z.clamp(-1.0, 1.0).asin() + (br.err.y + br.recoil).to_radians();
    let dd = Vec3::new(yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), pitch.sin());
    crate::combat::spread_dir(dd, br.spread * 0.5 * (1.0 - 0.95 * br.ads), seed)
}

/// After a shot: recoil kick (Recoil Hip / Zoom ViewPitchMin..Max) of which the bot pulls down
/// 50-85 % by level (NOT from data), spread + FireAdd up to Max.
pub fn fired(br: &mut Brain, w: &crate::weaponcfg::WeaponCfg, lv: f32, crouched: bool, seed: &mut u32) {
    let rc = if br.ads > 0.5 { &w.recoil_zoom } else { &w.recoil_hip };
    let kick = rc.pitch_min + (rc.pitch_max - rc.pitch_min).max(0.0) * rnd(seed);
    br.recoil = (br.recoil + kick * (1.0 - lerp(0.5, 0.85, lv))).min(6.0);
    let dev = if crouched { &w.spread_crouch } else { &w.spread_stand };
    br.spread = (br.spread + dev.fire_add).min(dev.max.max(dev.min));
}

/// Burst length (s) by range: full auto within OpenFullFireDistanceMax 500 (up to MaxBurstTime
/// 1.75), else Stationary / Moving Duration capped by the weapon's BotMaxFireRate; past
/// SimpleFireDistance 1800 shorter (taps, NOT from data).
pub fn burst_len(w: &crate::weaponcfg::WeaponCfg, dist: f32, moving: bool, seed: &mut u32) -> f32 {
    if dist < 500.0 {
        return 1.75;
    }
    let (a, b) = if moving { (0.5, 1.0) } else { (0.675, 1.375) };
    let mut t = (a + (b - a) * rnd(seed)).min(w.bot_fire_rate.max(0.3));
    if dist > 1800.0 {
        t *= (1800.0 / dist).max(0.3);
    }
    t.max(w.fire_interval * 1.01)
}

/// Pause between bursts: Stationary Wait 0.375-0.675 s; moving: between Moving Wait 2.5-6 s
/// (level 0) and the stationary pause (level 1) (blend NOT from data).
pub fn burst_pause(moving: bool, lv: f32, seed: &mut u32) -> f32 {
    let st = 0.375 + 0.3 * rnd(seed);
    if !moving {
        return st;
    }
    lerp(2.5 + 3.5 * rnd(seed), st * 1.3, lv)
}

/// Shot interval: semi-automatic weapons are tapped at a human rate (0.3-0.16 s by level, NOT
/// from data), never faster than FireInterval.
pub fn interval(w: &crate::weaponcfg::WeaponCfg, lv: f32) -> f32 {
    if w.full_auto {
        w.fire_interval
    } else {
        w.fire_interval.max(lerp(0.3, 0.16, lv))
    }
}

/// Look direction (UE yaw) without a target: heard fire (after NoiseProcessesTime), the last
/// known position (4 s), the walking direction, else when standing a scan over open sight lines
/// leaning to the enemy side (every 1.5-3.5 s; 16 rays at eye height up to 25 m, NOT from data):
/// bots stood facing walls.
#[allow(clippy::too_many_arguments)]
/// Largest angle (rad) between the running direction and a heard shot / last known position
/// that a running bot without a target turns to (60 deg: front / front-diagonal run animations).
/// NOT from data.
const LOOK_WHILE_MOVING: f32 = 1.05;

pub fn idle_yaw(b: &mut Bot, me: Vec3, eye: Vec3, dir: Vec3, now: f32, dt: f32, enemy_side: Option<Vec3>, col: &CollisionWorld, buf: &mut Vec<u32>) -> f32 {
    let at = |p: Vec3| (p.y - me.y).atan2(p.x - me.x);
    // while running, a heard shot / the last known position is only looked at when it lies
    // within LOOK_WHILE_MOVING of the running direction; otherwise the bot faces where it runs.
    // Before, bots turned to every heard shot for 3-4 s and ran backwards / sideways with the
    // run_back / run_left animations ("running animation wrongly turned", user 2026-10-22
    // 20:43). NOT from data.
    let move_a = (dir != Vec3::ZERO).then(|| dir.z.atan2(dir.x));
    let ok_while_moving = |a: f32| move_a.is_none_or(|m| {
        let d = (a - m + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        d.abs() <= LOOK_WHILE_MOVING
    });
    if let Some((p, t)) = b.brain.heard {
        if now >= t && now - t < 3.0 && ok_while_moving(at(p)) {
            return at(p);
        }
        if now - t >= 3.0 {
            b.brain.heard = None;
        }
    }
    if let Some((p, t)) = b.brain.lkp {
        if now - t < 4.0 {
            if ok_while_moving(at(p)) {
                return at(p);
            }
        } else {
            b.brain.lkp = None;
        }
    }
    if dir != Vec3::ZERO {
        b.brain.look.1 = 0.0;
        return dir.z.atan2(dir.x);
    }
    b.brain.look.1 -= dt;
    if b.brain.look.1 <= 0.0 {
        b.brain.look.1 = 1.5 + rnd(&mut b.seed) * 2.0;
        let o = Vec3::new(eye.x, eye.z, eye.y) * 0.01;
        let side = enemy_side.map(|p| at(p));
        let mut best = (f32::MIN, b.yaw);
        for k in 0..16 {
            let a = k as f32 / 16.0 * std::f32::consts::TAU;
            let d = Vec3::new(a.cos(), 0.0, a.sin());
            let open = col.raycast(o, d, 25.0, buf).map(|h| h.0).unwrap_or(25.0);
            if open < 4.0 {
                continue;
            }
            let lean = side.map(|s| 1.0 + (a - s).cos()).unwrap_or(1.0);
            let score = open * lean * (0.6 + 0.8 * rnd(&mut b.seed));
            if score > best.0 {
                best = (score, a);
            }
        }
        b.brain.look.0 = best.1;
    }
    b.brain.look.0
}

/// Turn rate (rad/s) by level: 6 .. 11 (NOT from data).
pub fn turn_rate(lv: f32) -> f32 {
    lerp(6.0, 11.0, lv)
}

/// Chance to break off to cover when hit: fDuckAfterShotByBotChance 0.5 / ByPlayer 0.8;
/// pushing to an objective x0.6, below 50 health at least 0.85 (NOT from data).
pub fn cover_chance(by_player: bool, pushing: bool, health: f32) -> f32 {
    let mut c: f32 = if by_player { 0.8 } else { 0.5 };
    if pushing {
        c *= 0.6;
    }
    if health < 50.0 {
        c = c.max(0.85);
    }
    c
}

/// Seconds in cover: 3-5, 6-9 when hurt (regeneration starts 5 s after the last hit at
/// BotHealthConfig RegenRate_HPPerSeconds 10) (NOT from data).
pub fn cover_time(health: f32, seed: &mut u32) -> f32 {
    if health < 60.0 {
        6.0 + rnd(seed) * 3.0
    } else {
        3.0 + rnd(seed) * 2.0
    }
}

/// In cover: alternate hiding (crouched, reloads) 1-2 s and peeking (standing, can see and
/// shoot) 1.5-3 s; returns whether the bot stands (NOT from data).
pub fn peek(b: &mut Bot, dt: f32) -> bool {
    b.brain.peek.1 -= dt;
    if b.brain.peek.1 <= 0.0 {
        b.brain.peek.0 = !b.brain.peek.0;
        b.brain.peek.1 = if b.brain.peek.0 { 1.5 + rnd(&mut b.seed) * 1.5 } else { 1.0 + rnd(&mut b.seed) };
    }
    b.brain.peek.0
}

/// Zig-zag while walking under fire or pushing through a fight: up to 0.55 sideways,
/// period ~1.6 s (NOT from data).
pub fn weave(b: &mut Bot, dir: Vec3, dt: f32) -> Vec3 {
    b.brain.weave += dt * 3.9;
    let side = Vec3::new(-dir.z, 0.0, dir.x);
    (dir + side * b.brain.weave.sin() * 0.55).normalize_or_zero()
}

/// Gunfire of the player (PlayerCombat fire_cooldown restarted) for the bots' hearing.
pub fn player_noise(time: Res<Time>, pc: Option<Res<crate::combat::PlayerCombat>>, pawns: Query<&Pawn>, mut last: Local<f32>) {
    let Some(pc) = pc else { return };
    let cd = pc.fire_cooldown;
    if cd > *last + 1e-4 {
        if let Ok(p) = pawns.single() {
            if !p.dead {
                noise(feet_ue(p.feet) + Vec3::Z * 70.0, TEAM_USA, time.elapsed_secs());
            }
        }
    }
    *last = cd;
}

/// Frag grenades (BotGrenadeConfig, see the module head): at a target that went behind cover
/// (last known position < 3 s, ThrowChanceHidden 80-100 %) or a visible one standing still
/// for MinTargetStationaryTime (ThrowChanceVisible 5 %), re-evaluated every 1 s; never with a
/// team mate within AllyRadius of the point or the bot within the blast; team-wide
/// AreaCooldown within AreaRadius. Thrown like the player's (speed + TossZ, GCProj fuse),
/// pitch searched so the arc lands at the point (NOT from data: the search, 2 per life).
#[allow(clippy::too_many_arguments)]
pub fn bot_grenades(
    time: Res<Time>,
    flow: Res<MatchFlow>,
    data: Option<Res<CombatData>>,
    dbg: Res<BotDebug>,
    grenades: Option<ResMut<crate::combat::Grenades>>,
    mut bots: Query<(Entity, &mut Bot, &Soldier)>,
    mut log: ResMut<BotLog>,
    mut area: Local<Vec<(Vec3, u8, f32)>>,
) {
    if flow.phase != Phase::Playing || dbg.passive || dbg.frozen {
        return;
    }
    let (Some(data), Some(mut grenades)) = (data, grenades) else { return };
    let g = &data.grenade;
    if g.speed <= 0.0 {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    // user 2026-10-22 21:19 "the bots really spam grenades": with the game's chances (80-100 %
    // at a target that went behind cover) 15 bots with 2 frags each threw constantly. NOT from
    // data: chance x NADE_SCALE, cooldown 10-20 s, the area cooldown kept 6 s and one frag per
    // team at most every TEAM_NADE_GAP s.
    const NADE_SCALE: f32 = 0.35;
    const TEAM_NADE_GAP: f32 = 3.0;
    area.retain(|a| now - a.2 < 6.0);
    let mates: Vec<(Vec3, u8, bool)> = bots.iter().map(|(_, _, s)| (s.center_ue(), s.team, s.alive)).collect();
    for (e, mut b, s) in &mut bots {
        let b = &mut *b;
        if !s.alive || b.brain.nades <= 0 {
            continue;
        }
        b.brain.nade_cd -= dt;
        b.brain.nade_eval -= dt;
        if b.brain.nade_cd > 0.0 || b.brain.nade_eval > 0.0 || b.reload > 0.0 || s.blinded > 0.0 {
            continue;
        }
        b.brain.nade_eval = 1.0;
        let lv = level(s.team);
        let (point, chance) = match (b.target.is_some(), b.brain.lkp, b.brain.tgt_last) {
            (false, Some((p, t)), _) if now - t < 3.0 => (p, (0.8 + 0.2 * rnd(&mut b.seed)) * NADE_SCALE),
            (true, _, Some(p)) if b.brain.tgt_still >= 1.0 => (p, 0.05 * NADE_SCALE),
            _ => continue,
        };
        // lower levels throw less often (NOT from data)
        if rnd(&mut b.seed) >= chance * lerp(0.5, 1.0, lv) {
            continue;
        }
        let eye = s.center_ue() + Vec3::Z * 35.5;
        let floor = point - Vec3::Z * crate::combat::CYL_HALF;
        let flat = Vec2::new(floor.x - eye.x, floor.y - eye.y);
        let dist = flat.length();
        let dz = floor.z - (eye.z - 35.5 - crate::combat::CYL_HALF);
        if !(120.0..=2000.0).contains(&dist) || !(-200.0..=200.0).contains(&dz) || dist < g.radius * 1.2 {
            continue;
        }
        if mates.iter().any(|m| m.2 && m.1 == s.team && m.0.distance(point) < 200.0f32.max(g.radius * 0.8)) {
            continue;
        }
        if area.iter().any(|a| a.1 == s.team && (a.0.distance(point) < 1000.0 || now - a.2 < TEAM_NADE_GAP)) {
            continue;
        }
        // pitch search: the arc (gravity 750, no bounce) that comes down at the point's height
        let yaw = flat.y.atan2(flat.x);
        let o = eye + Vec3::new(yaw.cos(), yaw.sin(), 0.0) * 30.0;
        let mut best: Option<(f32, Vec3)> = None;
        for k in -10..=60 {
            let pitch = (k as f32).to_radians();
            let d = Vec3::new(yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), pitch.sin());
            let v0 = d * g.speed + Vec3::Z * g.toss_z;
            let (mut p, mut v) = (o, v0);
            let mut landed = None;
            for _ in 0..240 {
                v.z -= 750.0 / 60.0;
                p += v / 60.0;
                if v.z < 0.0 && p.z <= floor.z + 10.0 {
                    landed = Some(p);
                    break;
                }
            }
            let Some(l) = landed else { continue };
            let miss = Vec2::new(l.x - floor.x, l.y - floor.y).length();
            if best.is_none_or(|bb| miss < bb.0) {
                best = Some((miss, v0));
            }
        }
        let Some((miss, v0)) = best.filter(|bb| bb.0 < 150.0) else { continue };
        grenades.list.push(Grenade { rocket: None, pos: o, vel: v0, fuse: g.fuse, owner: Actor::Bot(e), team: s.team, resting: false, trail: None, tac: None, payload: None });
        b.brain.nades -= 1;
        b.brain.nade_cd = 10.0 + rnd(&mut b.seed) * 10.0;
        // no shots while throwing (PrimeWeaponTime-like pause, the frag's throw animation)
        b.fire_cd = b.fire_cd.max(g.pre_throw + g.throw_anim);
        area.push((point, s.team, now));
        log.event(now, format!("{} throws a grenade {:.0} m (miss {:.1} m)", s.name, dist / 100.0, miss / 100.0));
    }
}
