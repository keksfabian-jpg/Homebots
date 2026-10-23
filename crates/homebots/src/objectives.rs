//! Bodenkontrolle objectives: Objective_GameMode_VolumeHold actors of <map>_FrontlinesData with
//! their GameMode_FrontlineVolume brushes (upk brush_volumes).
//!
//! Data: fCapTime (Crossroads: 20 s), Default__Objective_GameMode_VolumeHold RegressionRate 2,
//! RegressionDelay 2; ObjectiveName EON_Alpha/Bravo/Charlie (default Alpha); reward
//! GCBattlePoints.ini PSR_CaptureObjective Points=250; HUD text gcgame.deu
//! Objective_GameMode LocalizedObjectiveName[0..2], LocalizedObjectiveStatus[1] "%s WIRD GESICHERT",
//! [5] "%s GESICHERT".
//! The 9 objectives form three lines of A/B/C (west, middle, east); the match starts on the
//! middle line.
use crate::menu::{MatchConfig, MatchFlow, Phase};
use crate::player::Pawn;
use bevy::prelude::*;

pub struct ObjectivesPlugin;

impl Plugin for ObjectivesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Objectives>().init_resource::<Endgame>().add_systems(Update, update_objectives);
    }
}

#[derive(Debug, Clone)]
pub struct Objective {
    /// 0 Alpha, 1 Bravo, 2 Charlie
    pub letter: usize,
    pub volume: upk::level::BrushVolume,
    pub location: [f32; 3],
    pub cap_time: f32,
    /// None neutral, Some(0) USA, Some(1) KVA
    pub owner: Option<u8>,
    pub progress: f32,
    pub since_left: f32,
    /// team currently capturing (progress belongs to it)
    pub capturing_team: Option<u8>,
    pub line: usize,
}

#[derive(Resource, Default)]
pub struct Objectives {
    pub list: Vec<Objective>,
    pub active_line: usize,
    pub loaded_for: Option<std::path::PathBuf>,
    /// HUD message: (LocalizedObjectiveStatus index, letter, seconds left)
    pub message: Option<(usize, usize, f32)>,
    /// objective the player is capturing (index), for the HUD
    pub capturing: Option<usize>,
    pub banner_left: f32,
    pub match_over: bool,
    /// FrontlineMovedUp, FrontlineMovedBack, YourTeamWon, YourTeamLost (gcgame.deu)
    pub texts: [String; 4],
    /// GCUIBannerMessageManager BPGain, GCCrawlMessage Friendly/EnemyCapturedObjectiveMessage,
    /// FrontlineMovedMessage
    pub msgs: [String; 4],
    /// GCPointSpendingSystem LocalizedRewards: Id -> Name
    pub reward_names: std::collections::HashMap<String, String>,
    /// LocalizedObjectiveName[0..3]
    pub names: [String; 4],
    /// Team Deathmatch (MatchConfig mode TDM / TDM,BC): no objectives, one round
    pub tdm: bool,
    /// living characters this frame (UE cylinder centre, team): TDM spawn influence
    pub people: Vec<([f32; 3], u8)>,
    /// TDM: the infantry spawn points (Bevy), where bots roam when no enemy is alive
    pub roam: Vec<Vec3>,
}


/// Objectives and the full map's MinimapVolume bounds, read by main.rs from the level packages
/// it has open (re-opening every package at match start stalled the first frames for seconds).
#[derive(Resource, Clone, Default)]
pub struct LevelData {
    pub maps: Vec<std::path::PathBuf>,
    pub objectives: Vec<Objective>,
    pub minimap_bounds: Option<[f32; 4]>,
    pub frontlines: Vec<Frontline>,
}

/// GameMode_FocusedFrontline (<map>_FrontlinesData): the objectives of one front line and, per
/// team, its SpawnPointCollection (SpawnPoints: Type GMST_Infantry / GMST_Jeep / ... with their
/// GameMode_PlayerStarts). The match spawns at the active line's points: Crossroads
/// GameMode_FocusedFrontline_2 lists Objective_GameMode_VolumeHold_7 / 13 / 14 and 16 infantry
/// starts per team.
#[derive(Debug, Clone, Default)]
pub struct Frontline {
    /// objective locations (UE units; matched to Objective::location)
    pub objectives: Vec<[f32; 3]>,
    /// per team (TEAM_USA 0, TEAM_KVA 1): (spawn type, starts as Bevy cylinder centre + yaw)
    pub spawns: [Vec<(String, Vec<(Vec3, f32)>)>; 2],
    /// GameMode_TDMFrontline (TDM_<map>): no objectives, both teams use the same points
    /// (Default__PlayerSpawnManagerTDM bAgnosticSpawnPoints true); `spawns` holds them for both
    pub agnostic: bool,
    /// spawn collections whose starts' Group (Infantry_High / *_Low) names the other team than
    /// the collection's Team (mapcheck.rs reports it; 0 on Crossroads)
    pub group_mismatch: usize,
}

fn obj_list(data: &[u8]) -> Vec<i32> {
    if data.len() < 4 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if data.len() != 4 + n * 4 {
        return Vec::new();
    }
    (0..n).map(|k| i32::from_le_bytes(data[4 + k * 4..8 + k * 4].try_into().unwrap())).collect()
}

pub fn load_frontlines<'a>(pkgs: impl Iterator<Item = &'a upk::Package>) -> Vec<Frontline> {
    let mut out = Vec::new();
    for p in pkgs {
        for (i, e) in p.exports.iter().enumerate() {
            let tdm = p.export_class(e) == "GameMode_TDMFrontline";
            if p.export_class(e) != "GameMode_FocusedFrontline" && !tdm {
                continue;
            }
            let Some(pr) = p.export_props(i) else { continue };
            let mut f = Frontline::default();
            if let Some(upk::props::Value::Raw { data, .. }) = pr.get("Objectives") {
                for o in obj_list(data).into_iter().filter(|&o| o > 0) {
                    if let Some(l) = p.export_props(o as usize - 1).and_then(|op| op.vector("Location")) {
                        f.objectives.push(l);
                    }
                }
            }
            for sc in pr.all("SpawnCollection") {
                let upk::props::Value::Raw { data, .. } = &sc.value else { continue };
                let Some(col) = p.read_props(&mut upk::props::Rd::new(data)) else { continue };
                let mut sets = Vec::new();
                // the starts' Group (Infantry_High / VehicleSpawnsHigh = USA, *_Low = KVA; player.rs /
                // bots.rs), cross-checked against the collection's Team below
                let (mut high, mut low) = (0, 0);
                for sp in p.struct_array(&col, "SpawnPoints") {
                    let ty = sp.enum_name("Type").unwrap_or("").to_string();
                    let mut pts = Vec::new();
                    if let Some(upk::props::Value::Raw { data, .. }) = sp.get("PlayerStart") {
                        for o in obj_list(data).into_iter().filter(|&o| o > 0) {
                            let Some(st) = p.export_props(o as usize - 1) else { continue };
                            let Some(loc) = st.vector("Location") else { continue };
                            let yaw = st.rotator("Rotation").map(|r| r[1].rem_euclid(65536) as f32 / 65536.0 * std::f32::consts::TAU).unwrap_or(0.0);
                            let g = st.enum_name("Group").unwrap_or("");
                            if g.ends_with("High") {
                                high += 1;
                            } else if g.ends_with("Low") {
                                low += 1;
                            }
                            pts.push((Vec3::new(loc[0], loc[2], loc[1]) * 0.01, yaw));
                        }
                    }
                    sets.push((ty, pts));
                }
                if tdm {
                    // tdm_Overpass: SpawnCollection[0] empty, [1] all 29 infantry + vehicle starts
                    // (Group alt_tdm_infantry / none): the points are team-agnostic
                    for t in 0..2 {
                        for (ty, pts) in &sets {
                            match f.spawns[t].iter_mut().find(|s| &s.0 == ty) {
                                Some(s) => s.1.extend(pts.iter().copied()),
                                None => f.spawns[t].push((ty.clone(), pts.clone())),
                            }
                        }
                    }
                    continue;
                }
                // team of the collection: SpawnPointCollection.Team (struct default TEAM_RedStar,
                // Default__GameMode_Frontline SpawnCollection[1] Team TEAM_Western; Core.u ETeam
                // 0 TEAM_RedStar = KVA, 1 TEAM_Western = USA). Angel Island's starts have no Group
                // (all 198 ""), so the Group rule put both collections on USA -> no KVA spawns.
                let team_name = col.enum_name("Team").unwrap_or(if sc.index == 1 { "TEAM_Western" } else { "TEAM_RedStar" });
                let by_group = (high != low).then_some(if low > high { 1 } else { 0 });
                let team = match team_name {
                    "TEAM_Western" => 0,
                    "TEAM_RedStar" => 1,
                    _ => by_group.unwrap_or(0),
                };
                if by_group.is_some_and(|g| g != team) {
                    f.group_mismatch += 1;
                }
                f.spawns[team] = sets;
            }
            f.agnostic = tdm;
            out.push(f);
        }
    }
    out
}

/// Starts of a spawn type (GMST_Infantry, GMST_Jeep, ...) for a team on the active front line,
/// empty when the level has no frontline data.
pub fn line_spawns(level: Option<&LevelData>, obj: &Objectives, team: u8, ty: &str) -> Vec<(Vec3, f32)> {
    let Some(level) = level else { return Vec::new() };
    if obj.list.is_empty() {
        if let Some(f) = level.frontlines.iter().find(|f| f.agnostic) {
            let pts: Vec<(Vec3, f32)> = f.spawns[team as usize & 1].iter().filter(|s| s.0 == ty).flat_map(|s| s.1.iter().copied()).collect();
            return crate::tdm::best_spawns(&pts, team, &obj.people);
        }
    }
    let active: Vec<[f32; 3]> = obj.list.iter().filter(|o| o.line == obj.active_line).map(|o| o.location).collect();
    let near = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() < 10.0;
    let Some(f) = level.frontlines.iter().find(|f| !active.is_empty() && active.iter().all(|a| f.objectives.iter().any(|b| near(*a, *b)))) else { return Vec::new() };
    f.spawns[team as usize & 1].iter().filter(|s| s.0 == ty).flat_map(|s| s.1.iter().copied()).collect()
}

/// Full-map area (min x, min y, max x, max y, UE units): the ground GameMode_CombatVolume (the
/// lower of the two; Crossroads 48000 x 24000 uu), else the ground MinimapVolume (those span the
/// whole world, ~160000 uu: the level would be a small patch in the middle).
pub fn minimap_bounds<'a>(pkgs: impl Iterator<Item = &'a upk::Package> + Clone) -> Option<[f32; 4]> {
    let mut combat: Option<(f32, [f32; 4])> = None;
    for p in pkgs.clone() {
        for v in p.brush_volumes("GameMode_CombatVolume") {
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for q in v.polys.iter().flatten() {
                b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])];
            }
            if b[0] < b[2] && combat.is_none_or(|c| v.location[2] < c.0) {
                combat = Some((v.location[2], b));
            }
        }
    }
    if let Some((_, b)) = combat {
        return Some(b);
    }
    let mut bb: Option<[f32; 4]> = None;
    for p in pkgs {
        for v in p.brush_volumes("MinimapVolume") {
            // the air volume (bGroundVolumeType false) is for aircraft
            if p.export_props(v.export).is_some_and(|pr| matches!(pr.get("bGroundVolumeType"), Some(upk::props::Value::Bool(false)))) {
                continue;
            }
            for q in v.polys.iter().flatten() {
                let b = bb.get_or_insert([q[0], q[1], q[0], q[1]]);
                b[0] = b[0].min(q[0]);
                b[1] = b[1].min(q[1]);
                b[2] = b[2].max(q[0]);
                b[3] = b[3].max(q[1]);
            }
        }
    }
    bb
}

fn load(folder: &std::path::Path, mode: &str) -> Vec<Objective> {
    let pkgs: Vec<upk::Package> = crate::mapmode::match_maps(folder, mode).iter().filter_map(|m| upk::Package::open(m).ok()).collect();
    load_pkgs(pkgs.iter())
}

pub fn load_pkgs<'a>(pkgs: impl Iterator<Item = &'a upk::Package>) -> Vec<Objective> {
    let mut out = Vec::new();
    // per objective: its line from the front-line order (line_ranks), None = not found
    let mut ranked: Vec<Option<usize>> = Vec::new();
    for p in pkgs {
        let vols = p.brush_volumes("GameMode_FrontlineVolume");
        if vols.is_empty() {
            continue;
        }
        // only the objectives a GameMode_FocusedFrontline of this package lists (its Objectives
        // are exports of the same package). Lowlands' lowlands_church.map holds a
        // stray Objective_GameMode_VolumeHold + volume (copy of Lowlands_FrontlinesData's A of
        // the east line, same location) that no front line lists: it counted as a 10th
        // objective, and in Team Deathmatch (tdm_Lowlands_mid, no objectives) as a front line
        // of its own, so TDM had 0 spawns. A package without front lines contributes none.
        let mut listed: std::collections::HashSet<usize> = Default::default();
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "GameMode_FocusedFrontline" {
                continue;
            }
            let Some(pr) = p.export_props(i) else { continue };
            if let Some(upk::props::Value::Raw { data, .. }) = pr.get("Objectives") {
                listed.extend(obj_list(data).into_iter().filter(|&o| o > 0).map(|o| o as usize - 1));
            }
        }
        let ranks = line_ranks(p);
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "Objective_GameMode_VolumeHold" || !listed.contains(&i) {
                continue;
            }
            let Some(pr) = p.export_props(i) else { continue };
            let loc = match pr.get("Location") {
                Some(upk::props::Value::Vector(v)) => *v,
                _ => continue,
            };
            let cap_time = match pr.get("fCapTime") {
                Some(upk::props::Value::Float(f)) => *f,
                _ => 30.0,
            };
            let letter = match pr.get("ObjectiveName") {
                Some(upk::props::Value::Enum(s)) if s == "EON_Bravo" => 1,
                Some(upk::props::Value::Enum(s)) if s == "EON_Charlie" => 2,
                _ => 0,
            };
            let vol_idx = match pr.get("Volume") {
                Some(upk::props::Value::Object(o)) if *o > 0 => *o as usize - 1,
                _ => continue,
            };
            let Some(volume) = vols.iter().find(|v| v.export == vol_idx).cloned() else { continue };
            let line = ranks.iter().find(|(f, _)| f.iter().any(|q| (q[0] - loc[0]).abs() + (q[1] - loc[1]).abs() < 10.0)).map(|x| x.1);
            ranked.push(line);
            out.push(Objective { letter, volume, location: loc, cap_time, owner: None, progress: 0.0, since_left: 0.0, capturing_team: None, line: line.unwrap_or(0) });
        }
    }
    if ranked.iter().all(|r| r.is_some()) && !out.is_empty() {
        return out;
    }
    // fallback (an objective no front line lists): sort along X, three objectives per line
    let mut idx: Vec<usize> = (0..out.len()).collect();
    idx.sort_by(|&a, &b| out[a].location[0].partial_cmp(&out[b].location[0]).unwrap());
    for (k, &i) in idx.iter().enumerate() {
        out[i].line = k / 3;
    }
    out
}

/// Order of a package's GameMode_FocusedFrontlines as lines: (objective locations, line).
/// Line 0 is the USA end (objectives.rs: a USA round win moves the active line +1, a KVA win
/// -1). Axis = sum over the front lines of (KVA infantry start centroid - USA infantry start
/// centroid); the lines are sorted by their objectives' centroid along it. NOT from data: the
/// ordering rule (no list of the front lines found in the data; their Location / TerritoryPlane
/// are not read). It replaces "sort the objectives along X, three per line", which only fits
/// maps whose lines run along X: Harbor's run along Y, so the middle "line" mixed objectives of
/// three front lines and no front line matched it (0 starts at match start). Crossroads / Angel
/// Island / Farm give the same lines as the X rule.
fn line_ranks(p: &upk::Package) -> Vec<(Vec<[f32; 3]>, usize)> {
    let fl: Vec<Frontline> = load_frontlines(std::iter::once(p)).into_iter().filter(|f| !f.agnostic && !f.objectives.is_empty()).collect();
    let centroid = |v: &[(Vec3, f32)]| (!v.is_empty()).then(|| v.iter().map(|x| x.0).sum::<Vec3>() / v.len() as f32);
    let mut axis = Vec3::ZERO;
    for f in &fl {
        let inf = |t: usize| f.spawns[t].iter().filter(|s| s.0 == "GMST_Infantry").flat_map(|s| s.1.iter().copied()).collect::<Vec<_>>();
        if let (Some(u), Some(k)) = (centroid(&inf(0)), centroid(&inf(1))) {
            axis += k - u;
        }
    }
    // Bevy axes (x, height, y) -> UE x / y
    let (ax, ay) = (axis.x, axis.z);
    let key = |f: &Frontline| {
        let n = f.objectives.len() as f32;
        let (cx, cy) = (f.objectives.iter().map(|o| o[0]).sum::<f32>() / n, f.objectives.iter().map(|o| o[1]).sum::<f32>() / n);
        if ax == 0.0 && ay == 0.0 { cx } else { cx * ax + cy * ay }
    };
    let mut order: Vec<usize> = (0..fl.len()).collect();
    order.sort_by(|&a, &b| key(&fl[a]).partial_cmp(&key(&fl[b])).unwrap_or(std::cmp::Ordering::Equal));
    order.iter().enumerate().map(|(rank, &k)| (fl[k].objectives.clone(), rank)).collect()
}

// ---------------------------------------------------------------- end-of-match nuke
//
// User 2026-10-22 16:22: "hand over the killstreak to one bot or the player, depending on which
// team wins, before match end. Freeze the point count and let the nuke basically end the round."
// The game has no nuke: everything here is NOT from data.
//
// Rule (the match-deciding moment):
// - score: in the round that decides the match (the team's won rounds + 1 = rounds needed, TDM 1,
//   Bodenkontrolle 2) a team reaches END_SCORE_FRAC of the score limit -> that team;
// - time: the round time is down to END_TIME_LEFT and one team leads that wins the match with this
//   round -> the leader (user 2026-10-22 19:21: only shortly before a team wins the whole match);
// - earlier rounds of Bodenkontrolle end as before (front line moves).
// From then on the team scores, the round timer and the objectives are frozen (Endgame::frozen).
// The winning team gets the nuke: the player when he is on it (HUD prompt, key of the first
// special slot; after END_PLAYER_GRACE s without a call a bot of his team calls it), else a bot
// of the winning team calls it after END_BOT_DELAY s. When the sequence is over (nuke.rs WIN_AT)
// the stage is Done: the winning team gets the rounds it needs and the VICTORY / DEFEAT banner
// (GCUIScene_WinLossSplash) closes the match as before. Kills of the blast do not count.

/// share of the score limit that hands out the nuke in the deciding round (HB_ENDNUKE_FRAC)
pub const END_SCORE_FRAC: f32 = 0.9;
/// round time left (s) that hands the nuke to the leading team
pub const END_TIME_LEFT: f32 = 30.0;
/// the player's time to call it before a bot of his team does (s)
pub const END_PLAYER_GRACE: f32 = 20.0;
/// a bot of the winning team calls it this long after the award (s)
pub const END_BOT_DELAY: f32 = 3.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EndStage {
    #[default]
    Idle,
    /// awarded, waiting for the call
    Offered,
    /// the nuke sequence runs (nuke.rs)
    Called,
    /// the sequence is over: the match ends with `winner`
    Done,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct Endgame {
    pub stage: EndStage,
    /// winning team (TEAM_USA 0 / TEAM_KVA 1)
    pub winner: u8,
    /// frozen team scores and round time
    pub score: [i32; 2],
    pub time_left: f32,
    /// seconds since the award
    pub since: f32,
    pub reason: &'static str,
}

static FROZEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The team scores are frozen (end-of-match nuke): combat.rs adds no kill score.
pub fn scores_frozen() -> bool {
    FROZEN.load(std::sync::atomic::Ordering::Relaxed)
}

impl Endgame {
    pub fn frozen(&self) -> bool {
        self.stage != EndStage::Idle
    }
    /// Hands the nuke to `winner` and freezes the score / timer (no-op when already awarded).
    pub fn award(&mut self, winner: u8, flow: &MatchFlow, reason: &'static str) {
        if self.frozen() {
            return;
        }
        *self = Endgame { stage: EndStage::Offered, winner, score: flow.hud.score, time_left: flow.hud.time_left, since: 0.0, reason };
        FROZEN.store(true, std::sync::atomic::Ordering::Relaxed);
        info!("endgame: nuke to team {winner} ({reason}), score {:?} frozen, {:.0} s left, rounds {:?}", flow.hud.score, flow.hud.time_left, flow.hud.rounds);
    }
    fn reset(&mut self) {
        *self = Endgame::default();
        FROZEN.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

const REGRESSION_RATE: f32 = 2.0;
const REGRESSION_DELAY: f32 = 2.0;

/// Team score per held objective and second while playing (not found in the data: the
/// game mode's scoring is script code; placeholder so the score bars move).
const HOLD_SCORE_PER_SEC: f32 = 1.0;
/// KP display count-up rate (user's video, see update_objectives).
const KP_COUNT_RATE: f32 = 167.0;
/// Crawl line lifetime: the capture message is on screen 77.2-81.7 s in the user's video.
pub const CRAWL_SECS: f32 = 4.5;

#[allow(clippy::too_many_arguments)]
fn update_objectives(
    time: Res<Time>,
    mut obj: ResMut<Objectives>,
    mut flow: ResMut<MatchFlow>,
    config: Option<Res<MatchConfig>>,
    pawns: Query<&Pawn>,
    soldiers: Query<&crate::combat::Soldier>,
    cams: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    mut hold_acc: Local<[f32; 2]>,
    mut feed: ResMut<crate::combat::KillFeed>,
    mut crawl_test: Local<bool>,
    level: Option<Res<LevelData>>,
    (mut end, mut play_t): (ResMut<Endgame>, Local<f32>),
) {
    let _perf = crate::perf::scope("objectives");
    if flow.phase != Phase::Playing && flow.phase != Phase::Spawn {
        if flow.phase == Phase::Front {
            *obj = Objectives::default();
            crate::tdm::set_kill_bp(None);
            if end.stage != EndStage::Idle || scores_frozen() {
                end.reset();
            }
            *play_t = 0.0;
        }
        return;
    }
    let Some(cfg) = config else { return };
    if obj.loaded_for.as_ref() != Some(&cfg.0.folder) {
        obj.list = match level.as_ref().filter(|l| l.maps.first().and_then(|m| m.parent()) == Some(cfg.0.folder.as_path())) {
            Some(l) => l.objectives.clone(),
            None => load(&cfg.0.folder, &cfg.0.mode),
        };
        // banner texts from the game's localisation (Coalesced_<lang>.bin next to the maps)
        let cooked = cfg.0.folder.ancestors().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("CookedPC"))).map(|p| p.to_path_buf());
        let lang = cooked.as_ref().and_then(|c| c.parent()?.parent().map(crate::menu::language)).unwrap_or_else(|| "int".into());
        if let Some(loc) = cooked.and_then(|c| upk::ui::Loc::load(&c.join(format!("Coalesced_{lang}.bin"))).or_else(|| upk::ui::Loc::load(&c.join("Coalesced_int.bin")))) {
            let g = |k: &str, d: &str| loc.get(k).unwrap_or(d).to_string();
            obj.texts = [
                g("gcgame.GCUIBannerMessageManager.FrontlineMovedUp", "ROUND %d WON"),
                g("gcgame.GCUIBannerMessageManager.FrontlineMovedBack", "ROUND %d LOST"),
                g("gcgame.GCUIScene_WinLossSplash.YourTeamWon", "VICTORY"),
                g("gcgame.GCUIScene_WinLossSplash.YourTeamLost", "DEFEAT"),
            ];
            obj.msgs = [
                g("gcgame.GCUIBannerMessageManager.BPGain", "+%dKP"),
                g("gcgame.GCCrawlMessage.FriendlyCapturedObjectiveMessage", "Your team captured:"),
                g("gcgame.GCCrawlMessage.EnemyCapturedObjectiveMessage", "The enemy captured:"),
                g("gcgame.GCCrawlMessage.FrontlineMovedMessage", "The front line moved!"),
            ];
            // TDM kill reward (gcbattlepoints [TDM.Infantry GCBattlePoints] Rewards)
            let tdm_bp = loc.ini.get("gcbattlepoints.tdm.infantry.rewards").and_then(|v| crate::tdm::reward_points(v, "PSR_KillAsInfantry"));
            crate::tdm::set_kill_bp(if crate::mapmode::is_tdm(&cfg.0.mode) { tdm_bp } else { None });
            obj.names = std::array::from_fn(|i| g(&format!("gcgame.Objective_GameMode.LocalizedObjectiveName[{i}]"), ["ALPHA", "BRAVO", "CHARLIE", "DELTA"][i]));
            // "(Id=PSR_CaptureObjective, Name=\"Einsatzziel erobert\")"
            obj.reward_names.clear();
            for i in 0..64 {
                let Some(v) = loc.get(&format!("gcgame.GCPointSpendingSystem.LocalizedRewards[{i}]")) else { continue };
                let id = v.split("Id=").nth(1).and_then(|r| r.split([',', ')']).next()).unwrap_or("").trim().to_string();
                let name = v.split("Name=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
                if !id.is_empty() {
                    obj.reward_names.insert(id, name);
                }
            }
        }
        obj.tdm = crate::mapmode::is_tdm(&cfg.0.mode);
        obj.roam = level.as_ref().and_then(|l| l.frontlines.iter().find(|f| f.agnostic)).map(|f| f.spawns[0].iter().filter(|s| s.0 == "GMST_Infantry").flat_map(|s| s.1.iter().map(|p| p.0)).collect()).unwrap_or_default();
        if obj.tdm && !obj.list.is_empty() {
            warn!("objectives: TDM match with {} objectives (the TDM sublevel should have none)", obj.list.len());
        }
        let lines = obj.list.iter().map(|o| o.line).max().map(|m| m + 1).unwrap_or(0);
        obj.active_line = lines / 2;
        obj.loaded_for = Some(cfg.0.folder.clone());
        info!("objectives: {} ({} lines), active line {}", obj.list.len(), lines, obj.active_line);
        for o in &obj.list {
            info!("objective {} line {} at {:?} cap {} s", ["A", "B", "C", "D"][o.letter.min(3)], o.line, o.location, o.cap_time);
        }
    }
    let dt = time.delta_secs();
    flow.hud.clock = time.elapsed_secs();
    // ---- end-of-match nuke (see Endgame): award, freeze
    let needed = if obj.tdm { 1 } else { 2 };
    if flow.phase == Phase::Playing {
        *play_t += dt;
        // team switch (menu.rs switch_team): banner once the player is in the match again (user
        // 2026-10-22 20:43). Text NOT from data (the game has only the button "TEAM WECHSELN");
        // GCUIBannerMessageManager slow animation, 2.5 s.
        if crate::menu::TEAM_BANNER.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let team = if crate::combat::player_team() == 0 { "USA" } else { "KVA" };
            flow.hud.banner_msg = Some(("TEAM GEWECHSELT".to_string(), format!("DU BIST JETZT {team}"), flow.hud.clock, true));
        }
    }
    // (score limit and round time are set at the first spawn, menu.rs Action::Spawn)
    if !end.frozen() && !obj.match_over && flow.hud.score_limit > 0 {
        let limit = flow.hud.score_limit.max(1);
        let frac = std::env::var("HB_ENDNUKE_FRAC").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(END_SCORE_FRAC);
        let at = ((limit as f32 * frac).ceil() as i32).clamp(1, limit);
        let s = flow.hud.score;
        let by_score = (0..2usize).filter(|&t| flow.hud.rounds[t] + 1 >= needed && s[t] >= at).max_by_key(|&t| s[t]);
        // only when the leader wins the whole match with this round (user 2026-10-22 19:21: the nuke
        // comes shortly before a team wins the whole match, not in earlier Bodenkontrolle rounds)
        let by_time = (flow.hud.time_left <= END_TIME_LEFT && s[0] != s[1]).then(|| if s[0] > s[1] { 0usize } else { 1 }).filter(|&t| flow.hud.rounds[t] + 1 >= needed);
        // HB_ENDNUKE=<s>[,<team>]: award after s seconds of play (tests; team default the player's)
        let by_test = std::env::var("HB_ENDNUKE").ok().and_then(|v| {
            let mut it = v.split(',');
            let secs = it.next()?.trim().parse::<f32>().ok()?;
            let team = it.next().and_then(|x| x.trim().parse::<usize>().ok()).unwrap_or(crate::nuke::player_team() as usize).min(1);
            (*play_t >= secs).then_some(team)
        });
        let pick = by_score.map(|t| (t, "score")).or(by_time.map(|t| (t, "time"))).or(by_test.map(|t| (t, "HB_ENDNUKE")));
        if let Some((w, reason)) = pick {
            end.award(w as u8, &flow, reason);
        }
    }
    let frozen = end.frozen();
    if frozen {
        end.since += dt;
        flow.hud.score = end.score;
        flow.hud.time_left = end.time_left;
    }
    // TotalBattlePoints counts to the new value (video 77-79 s: 500 -> 573 -> 724 -> 750 after a
    // capture; 80.8-83.8 s: 750 -> 250 in red after buying, about 167 KP/s both ways -
    // measured, NOT from data); the spawn screen shows it at once
    let target = flow.hud.kp as f32;
    if flow.phase == Phase::Spawn {
        flow.hud.kp_shown = target;
    } else if flow.hud.kp_shown < target {
        flow.hud.kp_shown = (flow.hud.kp_shown + KP_COUNT_RATE * dt).min(target);
    } else {
        flow.hud.kp_shown = (flow.hud.kp_shown - KP_COUNT_RATE * dt).max(target);
    }
    if let Some((_, _, t)) = obj.message.as_mut() {
        *t -= dt;
    }
    if obj.message.as_ref().is_some_and(|m| m.2 <= 0.0) {
        obj.message = None;
    }
    // who stands where: (UE cylinder centre, team, is the player)
    let mut people: Vec<([f32; 3], u8, bool)> = Vec::new();
    if let Ok(p) = pawns.single() {
        if !p.dead && flow.phase == Phase::Playing {
            let c = p.feet + Vec3::Y * 0.415;
            people.push(([c.x * 100.0, c.z * 100.0, c.y * 100.0], crate::combat::player_team(), true));
        }
    }
    for s in &soldiers {
        if s.alive {
            let c = s.center_ue();
            people.push((c.to_array(), s.team, false));
        }
    }
    obj.people = people.iter().map(|&(c, t, _)| (c, t)).collect();
    let active = obj.active_line;
    let mut player_in = None;
    let mut reward = 0;
    let mut crawl: Vec<String> = Vec::new();
    let (msgs, names) = (obj.msgs.clone(), obj.names.clone());
    let mut msg: Option<(usize, usize, f32)> = None;
    for (i, o) in obj.list.iter_mut().enumerate() {
        if o.line != active {
            continue;
        }
        let mut n = [0; 2];
        let mut has_player = false;
        for (c, team, me) in &people {
            if o.volume.contains(*c) {
                n[*team as usize] += 1;
                has_player |= *me;
            }
        }
        if has_player {
            player_in = Some(i);
        }
        let capturer = match (n[0] > 0, n[1] > 0) {
            (true, false) => Some(0u8),
            (false, true) => Some(1u8),
            _ => None,
        };
        let contested = n[0] > 0 && n[1] > 0;
        match capturer {
            _ if frozen => {}
            Some(t) if o.owner != Some(t) => {
                if o.capturing_team != Some(t) {
                    o.capturing_team = Some(t);
                    o.progress = 0.0;
                }
                o.since_left = 0.0;
                o.progress += dt;
                if o.progress >= o.cap_time {
                    o.progress = 0.0;
                    if o.owner.is_some() {
                        // enemy-held: neutralised first (LocalizedObjectiveStatus[0])
                        o.owner = None;
                        if has_player || t == 0 {
                            msg = Some((0, o.letter, 3.0));
                        }
                    } else {
                        o.owner = Some(t);
                        o.capturing_team = None;
                        if has_player {
                            reward += 250;
                        }
                        crawl.push(format!("{} {}", if t == 0 { &msgs[1] } else { &msgs[2] }, names[o.letter.min(3)]));
                        msg = Some((if t == 0 { 5 } else { 6 }, o.letter, 3.0));
                    }
                }
            }
            _ => {
                if !contested && o.progress > 0.0 {
                    o.since_left += dt;
                    if o.since_left >= REGRESSION_DELAY {
                        o.progress = (o.progress - REGRESSION_RATE * dt).max(0.0);
                    }
                }
            }
        }
        if has_player {
            let k = if contested {
                7
            } else if capturer == Some(0) && o.owner != Some(0) {
                if o.owner.is_some() { 0 } else { 1 }
            } else if o.owner == Some(0) {
                // own objective: "%s GESICHERT" stays while standing in it (video 76-92 s)
                5
            } else {
                usize::MAX
            };
            if k != usize::MAX && msg.is_none() {
                msg = Some((k, o.letter, 0.1));
            }
        }
            // held objectives score
        if let (Some(t), false) = (o.owner, frozen) {
            hold_acc[t as usize] += HOLD_SCORE_PER_SEC * dt;
        }
    }
    for t in 0..2 {
        let whole = hold_acc[t].floor();
        if whole >= 1.0 {
            flow.hud.score[t] += whole as i32;
            hold_acc[t] -= whole;
        }
    }
    obj.capturing = player_in;
    flow.hud.active_obj = player_in.map(|i| {
        let o = &obj.list[i];
        (o.letter, o.owner, if o.capturing_team.is_some() { (o.progress / o.cap_time.max(0.01)).clamp(0.0, 1.0) } else { 0.0 })
    });
    // rounds (Subtitles GC3/GC4: the first team to fill its score bar wins a round, two rounds win
    // the battle; GCUIBannerMessageManager FrontlineMovedUp / FrontlineMovedBack, the front moves
    // one line towards the loser; GCUIScene_WinLossSplash YourTeamWon / YourTeamLost)
    if obj.banner_left > 0.0 {
        obj.banner_left -= dt;
        if obj.banner_left <= 0.0 {
            flow.hud.banner = None;
            if obj.match_over {
                flow.leave = true;
            }
        }
    }
    let limit = flow.hud.score_limit.max(1);
    // the nuke is over: the match ends with the team that called it (end-of-round banner as before)
    if end.stage == EndStage::Done && !obj.match_over {
        let w = (end.winner as usize).min(1);
        flow.hud.rounds[w] = flow.hud.rounds[w].max(needed);
        obj.match_over = true;
        flow.hud.banner = Some(if w == crate::nuke::player_team() as usize { obj.texts[2].clone() } else { obj.texts[3].clone() });
        obj.banner_left = 8.0;
        info!("endgame: match over, team {w} wins (rounds {:?}, score {:?})", flow.hud.rounds, flow.hud.score);
    }
    let timeout = !frozen && flow.hud.time_left <= 0.0 && !obj.match_over && flow.hud.score[0] != flow.hud.score[1];
    let winner = if frozen {
        None
    } else if flow.hud.score[0] >= limit || (timeout && flow.hud.score[0] > flow.hud.score[1]) {
        Some(0usize)
    } else if flow.hud.score[1] >= limit || timeout {
        Some(1usize)
    } else {
        None
    };
    if let (Some(w), false) = (winner, obj.match_over) {
        flow.hud.rounds[w] += 1;
        let round = flow.hud.rounds[0] + flow.hud.rounds[1];
        flow.hud.score = [0, 0];
        // TDM: one round (gcgame GCUIScene_MissionBriefing TDMWinningPoints "DAS ERSTE TEAM, DAS
        // %d PUNKTE ERREICHT, GEWINNT!"); Bodenkontrolle: two
        if flow.hud.rounds[w] >= if obj.tdm { 1 } else { 2 } {
            obj.match_over = true;
            flow.hud.banner = Some(if w == 0 { obj.texts[2].clone() } else { obj.texts[3].clone() });
            obj.banner_left = 8.0;
        } else {
            // GCUIBannerMessageManager FrontlineMovedUp / Back in the banner widget (slow
            // animation), GCCrawlMessage FrontlineMovedMessage in the crawl
            let text = if w == 0 { obj.texts[0].replace("%d", &round.to_string()) } else { obj.texts[1].replace("%d", &round.to_string()) };
            flow.hud.banner_msg = Some((text, String::new(), flow.hud.clock, true));
            crawl.push(msgs[3].clone());
            let lines = obj.list.iter().map(|o| o.line).max().map(|m| m + 1).unwrap_or(1);
            let next = if w == 0 { (obj.active_line + 1).min(lines - 1) } else { obj.active_line.saturating_sub(1) };
            obj.active_line = next;
            for o in obj.list.iter_mut() {
                o.owner = None;
                o.progress = 0.0;
                o.capturing_team = None;
            }
            if w == 0 {
                flow.hud.kp += 100;
                flow.hud.rewards.push((100, "PSR_AdvanceFrontline"));
            }
        }
    }
    if let Some(m) = msg {
        if m.2 > 1.0 || obj.message.as_ref().is_none_or(|x| x.2 <= 0.2) {
            obj.message = Some(m);
        }
    }
    if reward > 0 {
        flow.hud.kp += reward;
        flow.hud.rewards.push((reward, "PSR_CaptureObjective"));
    }
    // HB_CRAWLTEST: one capture message + reward a few seconds into the match (layout checks)
    if std::env::var_os("HB_CRAWLTEST").is_some() && flow.phase == Phase::Playing && !*crawl_test {
        *crawl_test = true;
        crawl.push(format!("{} {}", msgs[1], names[1]));
        flow.hud.kp += 250;
        flow.hud.rewards.push((250, "PSR_CaptureObjective"));
    }
    // crawl lines (white, GCCrawlMessage); shown as long as kill lines
    for c in crawl {
        feed.0.push((c, 2, String::new(), 2, None, CRAWL_SECS));
        if feed.0.len() > 5 {
            feed.0.remove(0);
        }
    }
    // reward banner: "+%dKP" + LocalizedRewards name, MainBannerAnimation_Fast (1.3 s), one
    // after another (user's video 77.2-78.3 s: "+250KP / EINSATZZIEL EROBERT")
    let busy = flow.hud.banner_msg.as_ref().is_some_and(|b| flow.hud.clock - b.2 < if b.3 { 2.5 } else { 1.3 });
    if !busy {
        flow.hud.banner_msg = None;
        if !flow.hud.rewards.is_empty() {
            let (amount, id) = flow.hud.rewards.remove(0);
            let name = obj.reward_names.get(id).cloned().unwrap_or_default();
            flow.hud.banner_msg = Some((msgs[0].replace("%d", &amount.to_string()), name, flow.hud.clock, false));
        }
    }
    flow.hud.objective_msg = obj.message.map(|(k, l, _)| (k, l));
    // markers of the active line (GameObjectivesMP Max/MinOnScreenIconSize 52 / 32,
    // OffscreenIconSize 32; distance -> size mapping over 100 m not from the data); 50 uu = 1 m
    flow.hud.markers.clear();
    if let Ok((cam, gt)) = cams.single() {
        let eye = gt.translation();
        let inside = obj.capturing;
        for (i, o) in obj.list.iter().enumerate().filter(|(_, o)| o.line == obj.active_line) {
            // the objective the player stands in is shown as the active icon instead
            if inside == Some(i) {
                continue;
            }
            let w = Vec3::new(o.location[0], o.location[2] + 150.0, o.location[1]) * 0.01;
            let dist_m = w.distance(eye) * 100.0 / 50.0;
            let Some(n) = cam.world_to_ndc(gt, w) else { continue };
            let behind = n.z <= 0.0 || n.z >= 1.0;
            let (mut x, mut y) = (n.x, n.y);
            if behind {
                x = -x;
                y = -y;
            }
            let off = behind || x.abs() > 1.0 || y.abs() > 1.0;
            let size = if off { 32.0 } else { 52.0 - (52.0 - 32.0) * (dist_m / 100.0).clamp(0.0, 1.0) };
            if off {
                let m = x.abs().max(y.abs()).max(1e-3);
                x = (x / m) * 0.92;
                y = (y / m) * 0.88;
            }
            let sx = (x * 0.5 + 0.5) * 1280.0;
            let sy = (1.0 - (y * 0.5 + 0.5)) * 720.0;
            flow.hud.markers.push(([sx, sy], o.letter, o.owner, dist_m, size));
        }
    }
}
