//! Team Deathmatch spawn choice (objectives::line_spawns on a GameMode_TDMFrontline map).
//!
//! Data: TDM spawn points are team-agnostic (Default__PlayerSpawnManagerTDM bAgnosticSpawnPoints
//! true; tdm_Overpass GameMode_TDMFrontline_0 lists all of them in one SpawnCollection). Weights:
//! Default__PlayerSpawnManagerTDM FriendlySpawnScore 4, CharacterInfluenceRadius 6000; the enemy
//! weight EnemySpawnScore -10 comes from the parent Default__PlayerSpawnManagerMP (TDM does not
//! override it). GCGame.u.
//! NOT from data: the scoring formula (sum of the weights of the living characters within the
//! radius, best score wins, ties random by the caller), and the match-start split (nobody alive
//! yet: each team takes one half of the points along the two farthest-apart points; USA the
//! first half). The game's PlayerSpawnManager code (distance / facing / death terms, dormant
//! upweight) is not decoded.
use bevy::prelude::*;

/// TDM kill reward (Coalesced gcbattlepoints [TDM.Infantry GCBattlePoints] Rewards
/// PSR_KillAsInfantry Points=160) while a TDM match runs, else 0 (objectives.rs sets it).
static KILL_BP: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub fn set_kill_bp(v: Option<i32>) {
    KILL_BP.store(v.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
}

/// The TDM kill reward (KP), None outside TDM. combat.rs also adds it to the team score in TDM
/// (NOT from data: the game's TDM team score is not decoded; with 160 per kill the TDM score
/// limit m_TDMScoreLimit 9000 is ~56 kills, with KillGameScore 10 it would be 900).
pub fn kill_bp() -> Option<i32> {
    let v = KILL_BP.load(std::sync::atomic::Ordering::Relaxed);
    (v > 0).then_some(v)
}

/// "(Id=PSR_KillAsInfantry, Points=160)" entries of an ini key: the points of `id`.
pub fn reward_points(values: &[String], id: &str) -> Option<i32> {
    values.iter().find(|v| v.contains(&format!("Id={id},")) || v.contains(&format!("Id={id})")))?.split("Points=").nth(1)?.split([',', ')']).next()?.trim().parse().ok()
}

/// "( CurrentVal=9000, MinRange=1000, MaxRange=30000, IncrementVal=1000 )": (current, min, max,
/// increment).
pub fn range_value(v: &str) -> Option<(f32, f32, f32, f32)> {
    let f = |k: &str| -> Option<f32> { v.split(&format!("{k}=")).nth(1)?.split([',', ')']).next()?.trim().parse().ok() };
    Some((f("CurrentVal")?, f("MinRange")?, f("MaxRange")?, f("IncrementVal")?))
}

const FRIENDLY_SPAWN_SCORE: i32 = 4;
const ENEMY_SPAWN_SCORE: i32 = -10;
const CHARACTER_INFLUENCE_RADIUS: f32 = 6000.0;

/// Spawn points as stored in objectives::Frontline (Bevy cylinder centre, metres) to UE units.
fn ue(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.z, p.y) * 100.0
}

/// The best points of `pts` for `team` given the living characters (UE cylinder centre, team).
pub fn best_spawns(pts: &[(Vec3, f32)], team: u8, people: &[([f32; 3], u8)]) -> Vec<(Vec3, f32)> {
    if pts.is_empty() {
        return Vec::new();
    }
    if people.is_empty() {
        return start_half(pts, team);
    }
    let r2 = CHARACTER_INFLUENCE_RADIUS * CHARACTER_INFLUENCE_RADIUS;
    let scores: Vec<i32> = pts
        .iter()
        .map(|(p, _)| {
            let u = ue(*p);
            people.iter().filter(|(c, _)| Vec3::from_array(*c).distance_squared(u) <= r2).map(|(_, t)| if *t == team { FRIENDLY_SPAWN_SCORE } else { ENEMY_SPAWN_SCORE }).sum()
        })
        .collect();
    let best = scores.iter().copied().max().unwrap_or(0);
    pts.iter().zip(&scores).filter(|(_, s)| **s == best).map(|(p, _)| *p).collect()
}

/// Match start: one half of the points per team along the axis of the two farthest-apart
/// points (NOT from data, see the module comment).
fn start_half(pts: &[(Vec3, f32)], team: u8) -> Vec<(Vec3, f32)> {
    let flat = |p: Vec3| Vec2::new(p.x, p.z);
    let (mut a, mut b, mut d) = (flat(pts[0].0), flat(pts[0].0), -1.0f32);
    for (i, p) in pts.iter().enumerate() {
        for q in &pts[i + 1..] {
            let dd = flat(p.0).distance_squared(flat(q.0));
            if dd > d {
                (a, b, d) = (flat(p.0), flat(q.0), dd);
            }
        }
    }
    let axis = b - a;
    let mut v: Vec<(f32, (Vec3, f32))> = pts.iter().map(|p| ((flat(p.0) - a).dot(axis), *p)).collect();
    v.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let half = v.len().div_ceil(2);
    let out: Vec<(Vec3, f32)> = if team & 1 == 0 { v[..half].iter().map(|x| x.1).collect() } else { v[half..].iter().map(|x| x.1).collect() };
    if out.is_empty() { pts.to_vec() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn influence() {
        let pts = vec![(Vec3::new(0.0, 0.0, 0.0), 0.0), (Vec3::new(100.0, 0.0, 0.0), 0.0), (Vec3::new(200.0, 0.0, 0.0), 0.0)];
        // match start: halves
        assert_eq!(best_spawns(&pts, 0, &[]).len(), 2);
        assert_eq!(best_spawns(&pts, 1, &[]).len(), 1);
        // an enemy at the first point (UE 0,0,0): team 0 avoids it, a friend there attracts
        let enemy = [([0.0, 0.0, 0.0], 1u8)];
        let b = best_spawns(&pts, 0, &enemy);
        assert!(b.iter().all(|p| p.0.x > 50.0));
        assert_eq!(reward_points(&["(Id=PSR_KillAsInfantry, Points=160)".to_string()], "PSR_KillAsInfantry"), Some(160));
        assert_eq!(range_value("( CurrentVal=9000, MinRange=1000, MaxRange=30000, IncrementVal=1000 )"), Some((9000.0, 1000.0, 30000.0, 1000.0)));
        let friend = [([0.0, 0.0, 0.0], 0u8)];
        assert_eq!(best_spawns(&pts, 0, &friend), vec![pts[0]]);
    }
}
