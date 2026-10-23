//! Bots: third-person soldiers that fill the match (MaxPlayersSlider - 1, USA / KVA).
//!
//! Content: Characters_FL2 US_AST.SkeletalMeshes.SKC_MP_US_AST / CH_AST SKC_MP_CH_AST (mesh space
//! like the 1P meshes: Z forward, -X right, -Y up; origin at the feet, Default__GCCharacter
//! m_f3PMeshStandTranslation -44 below the cylinder centre), CHC_MPAnimations AnimSets
//! (AS_CHC_PL_3P full-body locomotion "*_ads_us", mp_sprint_F; AS_CHC_PL_RifleStand_3P stand_idle;
//! AS_CHC_PL_RifleCrouch_3P crouch_idle; AS_CHC_PL_Death_Running_3P Death_1..10), the loadout
//! weapon's SKMesh3P rigid on the RightWeapon bone (same bone layout as the 1P Weapon bone).
//! Bone convention as in viewmodel.rs (reference pose as stored, animation rotations conjugated;
//! AS_CHC_PL_RifleStand_3P stand_idle frame 0 vs SKC_MP_US_AST: LeftToeBase ref (0,0,-.26,-.96),
//! anim (0,0,-.27,.96)).
//!
//! Behaviour (no AI data in the MP maps): move to the nearest objective of the active line the
//! team does not hold (nav.rs grid paths), engage the nearest visible enemy with the weapon's
//! fire mode in bursts (GCInfantryWeaponInfo BotMaxFireRate / BotMaxRefireRate), respawn at the
//! team's Infantry_High (USA) / Infantry_Low (KVA) PlayerStarts.
use crate::combat::{Actor, CombatData, DamageQueue, Hit, ShotHit, Soldier, Target, TEAM_KVA, TEAM_USA};
use crate::menu::{MatchConfig, MatchFlow, Phase};
use crate::nav::Nav;
use crate::objectives::Objectives;
use crate::player::{CollisionWorld, Pawn};
use crate::viewmodel::SkinPart;
use bevy::prelude::*;
use std::collections::HashMap;
use upk::anim::AnimSetData;

/// combat skill: aim, fire control, perception helpers, tactics, grenades (bots/brain.rs)
mod brain;

pub struct BotsPlugin;

impl Plugin for BotsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Nav>()
            .init_resource::<BotLog>()
            .init_resource::<BotDebug>()
            .add_systems(Update, (manage_bots, bot_ai).chain().after(crate::player::PlayerSet).before(crate::combat::CombatSet))
            .add_systems(Update, render_bots.after(crate::combat::CombatSet).after(crate::nuke::NukeSet))
            // gunfire the bots hear (player), bot frag grenades (bots/brain.rs)
            .add_systems(Update, (brain::player_noise.before(bot_ai), brain::bot_grenades.after(bot_ai).before(crate::combat::CombatSet)))
            .add_systems(Update, nav_selftest)
            .add_systems(Update, thermal_bodies);
    }
}

/// Animation sequences of several sets, by name.
pub struct AnimLib {
    pub sets: Vec<AnimSetData>,
    index: HashMap<String, (usize, usize)>,
}

impl AnimLib {
    pub fn new(sets: Vec<AnimSetData>) -> AnimLib {
        let mut index = HashMap::new();
        for (si, s) in sets.iter().enumerate() {
            for (qi, q) in s.seqs.iter().enumerate() {
                index.entry(q.name.to_ascii_lowercase()).or_insert((si, qi));
            }
        }
        AnimLib { sets, index }
    }

    pub fn find(&self, name: &str) -> Option<(usize, usize)> {
        self.index.get(&name.to_ascii_lowercase()).copied()
    }

    /// Root motion of a looped sequence: distance (uu) the "Root" track covers per cycle and
    /// the cycle length (s).
    pub fn root_cycle(&self, name: &str) -> Option<(f32, f32)> {
        let (si, qi) = self.find(name)?;
        let set = &self.sets[si];
        let tr = set.bones.iter().position(|b| b.eq_ignore_ascii_case("Root"))?;
        let s = &set.seqs[qi];
        let a = Vec3::from(s.sample(tr, 0.0)?.0);
        let b = Vec3::from(s.sample(tr, s.length * 0.999)?.0);
        let d = (b - a).length() / 0.999;
        (d > 1.0).then_some((d, s.length))
    }

    pub fn length(&self, name: &str) -> f32 {
        self.find(name).map(|(s, q)| self.sets[s].seqs[q].length).unwrap_or(1.0)
    }
}

/// A body skeleton with the track of each bone per animation set.
pub struct BodyRig {
    pub skeleton: Vec<upk::skeletal::Bone>,
    tracks: Vec<Vec<Option<usize>>>,
}

fn q(r: [f32; 4]) -> Quat {
    Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize()
}

impl BodyRig {
    pub fn new(skeleton: Vec<upk::skeletal::Bone>, lib: &AnimLib) -> BodyRig {
        let tracks = lib.sets.iter().map(|s| skeleton.iter().map(|b| s.bones.iter().position(|t| t.eq_ignore_ascii_case(&b.name))).collect()).collect();
        BodyRig { skeleton, tracks }
    }

    /// Local pose of a sequence at time t (looped or clamped).
    fn local(&self, lib: &AnimLib, name: &str, t: f32, looped: bool) -> Vec<(Quat, Vec3)> {
        let seq = lib.find(name);
        self.skeleton
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let reference = (q(b.rot), Vec3::from(b.pos));
                let Some((si, qi)) = seq else { return reference };
                let s = &lib.sets[si].seqs[qi];
                let tt = if looped { t.rem_euclid(s.length.max(1e-3)) } else { t.min(s.length) };
                match self.tracks[si][i].and_then(|tr| s.sample(tr, tt)) {
                    // the "Root" bone carries the locomotion's root motion (run_front_ads_us
                    // 142.7 uu per cycle, mp_sprint_F 444): the pawn moves itself, so looped
                    // sequences keep the root in place (UE3 RBA_Discard); without this the
                    // mesh ran ahead and snapped back every cycle
                    Some((_, r)) if looped && b.name.eq_ignore_ascii_case("Root") => (q(r).conjugate(), reference.1),
                    Some((p, r)) => (q(r).conjugate(), Vec3::from(p)),
                    None => reference,
                }
            })
            .collect()
    }

    fn compose(&self, local: &[(Quat, Vec3)]) -> Vec<(Quat, Vec3)> {
        let mut out: Vec<(Quat, Vec3)> = Vec::with_capacity(local.len());
        for (i, b) in self.skeleton.iter().enumerate() {
            let (lr, lp) = local[i];
            if i == 0 || b.parent < 0 || b.parent as usize >= i {
                out.push((lr, lp));
            } else {
                let (pr, pp) = out[b.parent as usize];
                out.push((pr * lr, pp + pr * lp));
            }
        }
        out
    }
}

pub struct BotSlot {
    pub team: u8,
    pub rig: usize,
    pub body: SkinPart,
    pub weapon: Option<SkinPart>,
    /// CombatData weapon index
    pub weapon_cfg: usize,
}

#[derive(Resource)]
pub struct BotAssets {
    pub lib: AnimLib,
    pub rigs: Vec<BodyRig>,
    pub slots: Vec<BotSlot>,
    /// the player's own 3P body (USA), shown by the kill cam while dead
    pub player: Option<BotSlot>,
    /// CH_3P.M_Character_Thermal_White (GCObjectPool_Content CharacterThermalMaterial)
    pub thermal: Option<Handle<crate::gamemat::GameMat>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Alive,
    Dead(f32),
}

#[derive(Component)]
pub struct Bot {
    pub slot: usize,
    /// UE yaw / pitch in radians
    pub yaw: f32,
    pub pitch: f32,
    vel: Vec3,
    on_ground: bool,
    state: State,
    anim: (&'static str, f32, bool),
    blend_from: Option<(Vec<(Quat, Vec3)>, f32)>,
    last_local: Vec<(Quat, Vec3)>,
    target: Option<Target>,
    seen_for: f32,
    burst: f32,
    pause: f32,
    fire_cd: f32,
    clip: i32,
    reload: f32,
    path: Vec<Vec3>,
    goal: Option<Vec3>,
    repath: f32,
    stuck: f32,
    /// feet at the last skinning (render_bots budget)
    skinned_at: Vec3,
    /// dead: the body as a ragdoll (ragdoll.rs)
    ragdoll: Option<Box<crate::ragdoll::Ragdoll>>,
    last_feet: Vec3,
    think: f32,
    seed: u32,
    death: usize,
    sprint: bool,
    shown: bool,
    /// cover point (Bevy floor position) and seconds to stay there
    cover: Option<(Vec3, f32)>,
    cover_cd: f32,
    /// seconds on the way to the current cover spot
    cover_travel: f32,
    /// reaction time before the first shot at a new target (seconds)
    react: f32,
    /// side step while engaging: direction sign and seconds left
    strafe: (f32, f32),
    /// seconds since an enemy was last seen
    calm: f32,
    /// seconds standing still while free to move (debug menu, idle watchdog)
    pub idle: f32,
    /// consecutive "stuck" events at one spot (trapped: moved out after 3)
    stuck_spot: Vec3,
    stuck_count: u32,
    /// what the bot is heading for (debug menu / bot log)
    pub goal_kind: &'static str,
    pub goal_letter: Option<usize>,
    /// the last plan found a nav path (false: straight line to the goal)
    pub nav_ok: bool,
    /// stuck / idle resets so far
    pub resets: u32,
    /// added to the slot when choosing among objectives after an idle reset
    bias: usize,
    /// stall watchdog: the spot the bot has stayed near (`idle` seconds), the objective it heads
    /// for with the best distance so far and the seconds since it improved, stalls in a row
    spot: Vec3,
    anchor: Option<Vec3>,
    anchor_best: f32,
    anchor_t: f32,
    stalls: u32,
    /// combat skill state (aim error, recoil, ADS, last known position, hearing, grenades)
    brain: brain::Brain,
}

/// Debug switches for the bots (debug menu, F1).
#[derive(Resource, Default)]
pub struct BotDebug {
    /// no thinking, no movement
    pub frozen: bool,
    /// no shooting
    pub passive: bool,
}

/// Bot behaviour log (debug menu): events in memory (the last 400) and, while `file` is set, in
/// a text file with a snapshot of every bot each 2 s.
#[derive(Resource)]
pub struct BotLog {
    pub enabled: bool,
    pub events: std::collections::VecDeque<String>,
    pub file: Option<std::path::PathBuf>,
    writer: Option<std::io::BufWriter<std::fs::File>>,
    snap_t: f32,
}

impl Default for BotLog {
    // events are kept in memory from the match start, so a file started later still holds the
    // history (user's bot log 2026-10-18 had only the 2 s before closing the game)
    fn default() -> Self {
        Self { enabled: true, events: Default::default(), file: None, writer: None, snap_t: 0.0 }
    }
}

impl BotLog {
    pub fn event(&mut self, t: f32, msg: String) {
        if !self.enabled {
            return;
        }
        let line = format!("[{t:8.2}] {msg}");
        self.write(&line);
        self.events.push_back(line);
        while self.events.len() > 400 {
            self.events.pop_front();
        }
    }

    fn write(&mut self, line: &str) {
        use std::io::Write;
        if self.writer.is_none() {
            if let Some(path) = &self.file {
                if let Some(d) = path.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                self.writer = std::fs::File::create(path).ok().map(std::io::BufWriter::new);
            }
        }
        if let Some(w) = self.writer.as_mut() {
            // flushed per line: the file must survive closing the window or a crash
            let _ = writeln!(w, "{line}");
            let _ = w.flush();
        }
    }

    /// Start writing to a new file (logs/bots_<unix time>.log next to the working directory).
    pub fn start_file(&mut self) -> std::path::PathBuf {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let path = std::path::PathBuf::from("logs").join(format!("bots_{secs}.log"));
        self.writer = None;
        self.file = Some(path.clone());
        self.enabled = true;
        let old: Vec<String> = self.events.iter().cloned().collect();
        self.write(&format!("--- {} earlier events ---", old.len()));
        for l in &old {
            self.write(l);
        }
        self.write("--- live ---");
        path
    }

    pub fn stop_file(&mut self) {
        use std::io::Write;
        if let Some(w) = self.writer.as_mut() {
            let _ = w.flush();
        }
        self.writer = None;
        self.file = None;
    }
}

/// One bot's state for the debug menu.
pub struct BotRow {
    pub entity: Entity,
    pub name: String,
    pub team: u8,
    pub alive: bool,
    pub health: f32,
    pub goal: String,
    pub goal_dist: f32,
    pub engaging: bool,
    pub idle: f32,
    pub resets: u32,
    pub nav_ok: bool,
    pub feet: Vec3,
}

impl Bot {
    pub fn row(&self, e: Entity, s: &Soldier) -> BotRow {
        let letter = self.goal_letter.map(|l| ["A", "B", "C", "D"][l.min(3)]).unwrap_or("");
        BotRow {
            entity: e,
            name: s.name.clone(),
            team: s.team,
            alive: s.alive,
            health: s.health,
            goal: if letter.is_empty() { self.goal_kind.to_string() } else { format!("{} {letter}", self.goal_kind) },
            goal_dist: self.goal.map(|g| Vec3::new(g.x - s.feet.x, 0.0, g.z - s.feet.z).length()).unwrap_or(0.0),
            engaging: self.target.is_some(),
            idle: self.idle,
            resets: self.resets,
            nav_ok: self.nav_ok,
            feet: s.feet,
        }
    }
}

/// Bot respawn delay (seconds); the player's own respawn goes through the spawn selection.
const BOT_RESPAWN: f32 = 5.0;
/// Longest walk to a cover spot (seconds; find_cover looks within 12 m) before the bot gives it
/// up (NOT from data).
const COVER_TRAVEL_MAX: f32 = 6.0;
/// Engagement distance (UE units): beyond WeaponRange the damage is already at its far value.
const SIGHT: f32 = 6000.0;

/// Bullets passing this close to the drone's centre hit it (UE units; the collision cylinder
/// radius 25 of GCV_SH_GAssaultWalker_Content plus its body above the cylinder, NOT exact)
const DRONE_HIT_RADIUS: f32 = 35.0;

/// Bot skill per team (USA, KVA): reaction time and aim spread are divided by it. KVA 1.25 by
/// default (user 2026-10-19: "increase the skill of the enemy team slightly"; NOT from data),
/// set from the settings (settings.rs).
pub static TEAM_SKILL: std::sync::Mutex<[f32; 2]> = std::sync::Mutex::new([1.0, 1.25]);

pub fn team_skill(team: u8) -> f32 {
    let _ = &TEAM_SKILL;
    // both teams now: bot strength level 0..1 (bots/brain.rs level) as the old multiplier
    1.0 + brain::level(team) * 0.75
}
/// m_f3PMeshStandTranslation
const MESH_Z: f32 = -44.0;

fn rnd(s: &mut u32) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s % 100_000) as f32 / 100_000.0
}

fn feet_ue(f: Vec3) -> Vec3 {
    Vec3::new(f.x, f.z, f.y) * 100.0
}

/// Spawn point for a team: the active front line's infantry starts (objectives::Frontline),
/// else one of the starts nearest its vehicle spawns (base side).
fn pick_start(col: &CollisionWorld, line: &[(Vec3, f32)], team: u8, seed: &mut u32) -> Option<(Vec3, f32)> {
    if !line.is_empty() {
        let k = (rnd(seed) * line.len() as f32) as usize % line.len();
        return Some(line[k]);
    }
    let (inf, veh) = if team == TEAM_USA { ("Infantry_High", "VehicleSpawnsHigh") } else { ("Infantry_Low", "VehicleSpawnsLow") };
    let base: Vec<Vec3> = col.starts.iter().filter(|s| s.0 == veh).map(|s| s.1).collect();
    let centre = if base.is_empty() { Vec3::ZERO } else { base.iter().copied().sum::<Vec3>() / base.len() as f32 };
    let mut c: Vec<&(String, Vec3, f32)> = col.starts.iter().filter(|s| s.0 == inf).collect();
    c.sort_by(|a, b| a.1.distance(centre).partial_cmp(&b.1.distance(centre)).unwrap());
    c.truncate(12);
    if c.is_empty() {
        return None;
    }
    let k = (rnd(seed) * c.len() as f32) as usize % c.len();
    Some((c[k].1, c[k].2))
}

#[allow(clippy::too_many_arguments)]
fn manage_bots(
    mut commands: Commands,
    flow: Res<MatchFlow>,
    config: Option<Res<MatchConfig>>,
    assets: Option<Res<BotAssets>>,
    data: Option<Res<CombatData>>,
    col: Res<CollisionWorld>,
    mut nav: ResMut<Nav>,
    levels: Res<crate::Levels>,
    bots: Query<(Entity, &Bot)>,
    mut seed: Local<u32>,
    (level, obj): (Option<Res<crate::objectives::LevelData>>, Res<Objectives>),
) {
    if *seed == 0 {
        *seed = 0x2545_f491;
    }
    nav.poll();
    // the walk grid builds in the background as soon as the match level's collision exists
    // (Creating / Loading / Movie): it started only with Spawn, and the 6.8 s build on Angel
    // Island left the bots 6.5 s on straight lines ("NO nav path" x104 in the user's bot log
    // 2026-10-21). A new level load (levels.loads) builds again.
    if !matches!(flow.phase, Phase::Front | Phase::Leaving) {
        nav.start(&col, levels.loads);
    }
    if matches!(flow.phase, Phase::Front | Phase::Leaving | Phase::Loading) {
        for (e, _) in &bots {
            commands.entity(e).despawn();
        }
        return;
    }
    if flow.phase != Phase::Playing && flow.phase != Phase::Spawn {
        return;
    }
    let (Some(assets), Some(data)) = (assets, data) else { return };
    let max = config.as_ref().and_then(|c| c.0.values.get("MaxPlayersSlider").copied()).unwrap_or(16.0) as usize;
    let want = max.saturating_sub(1);
    // the player's team gets one bot less (the player), the other team the rest. After a team
    // switch (menu.rs: spawn selection "TEAM WECHSELN") one bot changes sides: a bot of the
    // player's new team leaves (a dead one first), one joins the old team in a free slot
    // (NOT from data: the MP game had no bots)
    let mine = want / 2;
    let (usa, kva) = if crate::combat::player_team() == TEAM_USA { (mine, want - mine) } else { (want - mine, mine) };
    let team_of = |b: &Bot| assets.slots.get(b.slot).map(|s| s.team).unwrap_or(TEAM_USA);
    let (mut nu, mut nk) = (bots.iter().filter(|(_, b)| team_of(b) == TEAM_USA).count(), bots.iter().filter(|(_, b)| team_of(b) == TEAM_KVA).count());
    if !bots.is_empty() {
        for (t, n, lim) in [(TEAM_USA, &mut nu, usa), (TEAM_KVA, &mut nk, kva)] {
            if *n > lim {
                let mut list: Vec<(Entity, &Bot)> = bots.iter().filter(|(_, b)| team_of(b) == t).collect();
                list.sort_by_key(|(_, b)| (matches!(b.state, State::Alive), std::cmp::Reverse(b.slot)));
                for (e, b) in list.into_iter().take(*n - lim) {
                    info!("bots: team switch, slot {} leaves team {t}", b.slot);
                    commands.entity(e).despawn();
                }
                *n = lim;
            }
        }
        if nu >= usa && nk >= kva {
            return;
        }
    }
    let used: Vec<usize> = bots.iter().map(|(_, b)| b.slot).collect();
    let mut names = 2 + bots.iter().count();
    for (i, s) in assets.slots.iter().enumerate() {
        if used.contains(&i) {
            continue;
        }
        let take = if s.team == TEAM_USA { nu < usa } else { nk < kva };
        if !take {
            continue;
        }
        if s.team == TEAM_USA {
            nu += 1;
        } else {
            nk += 1;
        }
        let line = crate::objectives::line_spawns(level.as_deref(), &obj, s.team, "GMST_Infantry");
        let Some((p, yaw)) = pick_start(&col, &line, s.team, &mut seed) else { continue };
        let clip = data.weapons.get(s.weapon_cfg).map(|w| w.clip).unwrap_or(30);
        let feet = p - Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
        let name = format!("Spieler{names}");
        names += 1;
        let sd = 0x9e37_79b9u32.wrapping_mul(i as u32 + 7);
        commands.spawn((
            Soldier { team: s.team, name, health: crate::combat::HEALTH_MAX, feet, crouched: false, alive: true, since_damage: 99.0, kills: 0, deaths: 0, streak: 0, last_attacker: None, blinded: 0.0, last_hit: None },
            Bot {
                slot: i,
                yaw,
                pitch: 0.0,
                vel: Vec3::ZERO,
                on_ground: false,
                state: State::Alive,
                anim: ("stand_idle", 0.0, true),
                blend_from: None,
                last_local: Vec::new(),
                target: None,
                seen_for: 0.0,
                burst: 0.0,
                pause: 0.0,
                fire_cd: 0.0,
                clip,
                reload: 0.0,
                path: Vec::new(),
                goal: None,
                repath: 0.0,
                stuck: 0.0,
                skinned_at: Vec3::ZERO,
                ragdoll: None,
                last_feet: feet,
                think: rnd(&mut seed) * 0.3,
                seed: sd,
                death: 1,
                sprint: false,
                shown: false,
                cover: None,
                cover_cd: 0.0,
                cover_travel: 0.0,
                react: 0.8,
                strafe: (1.0, 0.0),
                calm: 99.0,
                idle: 0.0,
                stuck_spot: Vec3::ZERO,
                stuck_count: 0,
                goal_kind: "none",
                goal_letter: None,
                nav_ok: true,
                resets: 0,
                bias: 0,
                spot: feet,
                anchor: None,
                anchor_best: f32::MAX,
                anchor_t: 0.0,
                stalls: 0,
                brain: brain::Brain { nades: data.grenade.ammo.max(0), ..Default::default() },
            },
        ));
    }
    info!("bots: {nu} USA + {nk} KVA, strength {:.2} / {:.2} (bots/brain.rs level)", brain::level(TEAM_USA), brain::level(TEAM_KVA));
}

/// Goal of one bot (not from data: the MP game had no bots). The team spreads over the active
/// line: every third bot guards an objective the team holds, the others take the objectives it
/// does not hold, distributed by `slot` = the bot's index within its team (+ bias after an idle
/// reset; bot_ai). With every objective held
/// the attackers hunt the nearest enemy instead of standing in the held areas.
/// Returns (position, kind, objective letter).
fn goal_for(obj: &Objectives, team: u8, from: Vec3, slot: usize, enemies: &[Vec3], keep: Option<usize>) -> Option<(Vec3, &'static str, Option<usize>)> {
    let list: Vec<&crate::objectives::Objective> = obj.list.iter().filter(|o| o.line == obj.active_line).collect();
    let pos = |o: &crate::objectives::Objective| Vec3::new(o.location[0], o.location[2], o.location[1]) * 0.01;
    fn sorted<'a>(mut v: Vec<&'a crate::objectives::Objective>, from: Vec3) -> Vec<&'a crate::objectives::Objective> {
        let pos = |o: &crate::objectives::Objective| Vec3::new(o.location[0], o.location[2], o.location[1]) * 0.01;
        v.sort_by(|a, b| pos(a).distance(from).partial_cmp(&pos(b).distance(from)).unwrap());
        v
    }
    let open = sorted(list.iter().copied().filter(|o| o.owner != Some(team)).collect(), from);
    let held = sorted(list.iter().copied().filter(|o| o.owner == Some(team)).collect(), from);
    let guard = slot % 3 == 2;
    // a held objective the enemy is taking: the guards and every second other bot go back to
    // defend it, as players do (NOT from data)
    if let Some(o) = held.iter().copied().find(|o| o.capturing_team.is_some_and(|t| t != team) && o.progress > 0.0) {
        if guard || slot % 2 == 0 {
            return Some((pos(o), "guard", Some(o.letter)));
        }
    }
    if !held.is_empty() && guard {
        let o = held[(slot / 3) % held.len()];
        return Some((pos(o), "guard", Some(o.letter)));
    }
    if !open.is_empty() {
        // keep the objective the bot is already heading for while it is still open: the list is
        // sorted by the bot's current distance, so `slot % len` picked another objective after
        // a few metres and the attackers flipped A <-> B every replan (4-6 s) without arriving
        // (user's bot log 2026-10-19: KVA "goal attack A/B" alternating at 17-36 m, no capture)
        let o = keep.and_then(|k| open.iter().copied().find(|o| o.letter == k)).unwrap_or(open[slot % open.len()]);
        return Some((pos(o), if o.owner.is_some() { "attack" } else { "capture" }, Some(o.letter)));
    }
    // everything held: hunt the nearest enemy, else roam between the held objectives
    if let Some(e) = enemies.iter().min_by(|a, b| a.distance(from).partial_cmp(&b.distance(from)).unwrap()) {
        return Some((*e, "hunt", None));
    }
    // TDM (no objectives) and no enemy alive: roam to one of the far TDM spawn points, where the
    // enemies come back (NOT from data: the 5 farthest, by the bot's index)
    if list.is_empty() && !obj.roam.is_empty() {
        let mut far: Vec<Vec3> = obj.roam.clone();
        far.sort_by(|a, b| b.distance(from).partial_cmp(&a.distance(from)).unwrap_or(std::cmp::Ordering::Equal));
        return Some((far[slot % far.len().min(5)], "roam", None));
    }
    let all = sorted(list, from);
    all.get(slot % all.len().max(1)).map(|o| (pos(o), "roam", Some(o.letter)))
}

/// The active line's objective with this letter.
fn active_objective(obj: &Objectives, letter: Option<usize>) -> Option<&crate::objectives::Objective> {
    let l = letter?;
    obj.list.iter().find(|o| o.line == obj.active_line && o.letter == l)
}

/// Cylinder centre (UE units) of a standing pawn on a Bevy floor point: what
/// objectives::update_objectives tests against the GameMode_FrontlineVolume.
fn stand_centre_ue(floor: Vec3) -> [f32; 3] {
    (feet_ue(floor) + Vec3::Z * crate::combat::CYL_HALF).to_array()
}

/// Walk-grid component a bot at `feet` plans in (plan_path_r: from an island onto the main one).
fn walk_comp(grid: &crate::nav::NavGrid, feet: Vec3) -> Option<u32> {
    let a = grid.nearest(feet)?;
    if grid.comp_len(a) < ISLAND {
        if let Some(m) = grid.main_comp().filter(|&m| grid.nearest_in_comp(feet, m, 15.0).is_some()) {
            return Some(m);
        }
    }
    Some(grid.comp[a as usize])
}

/// Nodes with more missing neighbours (of 8) than this are wall / ledge nodes: the capture spot
/// is taken among the others while there are any (NOT from data).
const ZONE_WALL_MAX: f32 = 2.0 * 0.4;

/// Random walk-grid node of component `comp` where a standing bot is inside the objective's
/// GameMode_FrontlineVolume (the brush the capture counts in, Crossroads 7-15 m wide). The goal
/// was the objective's Location + 1.5-6 m and then any node within 4 m of that: up to 10 m off
/// centre, often outside the brush (C line 2 is 8.7 x 7.0 m), or a point the bot could not reach
/// ("stalled 4 s ... path 0/1" next to the point, then off to another objective).
fn zone_node(grid: &crate::nav::NavGrid, o: &crate::objectives::Objective, comp: u32, seed: u32) -> Option<Vec3> {
    let c = Vec3::new(o.location[0], o.location[2], o.location[1]) * 0.01;
    let r = o.volume.polys.iter().flatten().map(|q| (q[0] * 0.01 - c.x).abs().max((q[1] * 0.01 - c.z).abs())).fold(0.0f32, f32::max).min(30.0);
    let (mut open, mut edge) = (Vec::new(), Vec::new());
    for k in grid.nodes_within(c, r) {
        if grid.comp[k as usize] != comp || !o.volume.contains(stand_centre_ue(grid.nodes[k as usize])) {
            continue;
        }
        if grid.wall.get(k as usize).copied().unwrap_or(0.0) <= ZONE_WALL_MAX {
            open.push(k);
        } else {
            edge.push(k);
        }
    }
    let pick = if open.is_empty() { &edge } else { &open };
    if pick.is_empty() {
        return None;
    }
    let mut s = seed | 1;
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    Some(grid.nodes[pick[s as usize % pick.len()] as usize])
}

/// Nodes below which a nav component counts as an island (roof, car top, ledge): a bot on one
/// first walks to the main walkable area (NOT from data).
const ISLAND: u32 = 200;

/// Nav path from `feet` to `goal`, smoothed: keep a node only where the straight line from the
/// last kept one would be blocked. The goal node is taken in the bot's own component (nearest
/// to the goal when the random one is elsewhere); from an island the bot first steps onto the
/// main component. Returns the waypoints and whether a nav path was found (false: straight
/// line to the goal).
fn plan_path_ex(nav: &Nav, col: &CollisionWorld, feet: Vec3, goal: Vec3, seed: u32, buf: &mut Vec<u32>) -> (Vec<Vec3>, bool) {
    plan_path_r(nav, col, feet, goal, 4.0, seed, buf)
}

/// As plan_path_ex, the goal node taken at random within `r` metres of `goal` (0: the node at
/// the goal itself, for goals that already are walk-grid nodes: cover spots, capture-zone nodes).
/// The random 4 m let cover paths end up to 4 m from the cover node, where the bot never counted
/// as "in cover" and stood until it died (user's bot logs 2026-10-20: bots frozen 60-131 s).
fn plan_path_r(nav: &Nav, col: &CollisionWorld, feet: Vec3, goal: Vec3, r: f32, seed: u32, buf: &mut Vec<u32>) -> (Vec<Vec3>, bool) {
    let _perf = crate::perf::scope("bot_plan");
    let mut out = Vec::new();
    let mut ok = false;
    if let Some(grid) = nav.grid.as_ref() {
        if let Some(mut a) = grid.nearest(feet) {
            let mut anchor = feet;
            if grid.comp_len(a) < ISLAND {
                if let Some(k) = grid.main_comp().and_then(|m| grid.nearest_in_comp(feet, m, 15.0)) {
                    a = k;
                    anchor = grid.nodes[k as usize];
                    out.push(anchor);
                }
            }
            let ca = grid.comp[a as usize];
            // goal node in the bot's component: near the goal, else the component's closest node
            // to it (a goal the component does not reach: as close as the bot can get)
            let z = grid.node_near(goal, r, seed).filter(|&z| grid.comp[z as usize] == ca).or_else(|| grid.nearest_in_comp(goal, ca, 30.0)).or_else(|| grid.closest_in_comp(goal, ca));
            if let Some(p) = z.and_then(|z| grid.path(a, z, 150_000, &nav.penalty)) {
                ok = true;
                let pts: Vec<Vec3> = p.iter().map(|&k| grid.nodes[k as usize]).collect();
                let mut i = 0;
                while i < pts.len() {
                    let mut j = (i + 12).min(pts.len() - 1);
                    while j > i && !crate::nav::walkable_line(col, anchor, pts[j], buf) {
                        j -= 1;
                    }
                    out.push(pts[j]);
                    anchor = pts[j];
                    i = j + 1;
                }
            }
        }
    }
    if !ok {
        out.push(goal);
    }
    (out, ok)
}

fn plan_path(nav: &Nav, col: &CollisionWorld, feet: Vec3, goal: Vec3, seed: u32, buf: &mut Vec<u32>) -> Vec<Vec3> {
    plan_path_ex(nav, col, feet, goal, seed, buf).0
}

/// Nearest nav node within 12 m that hides a crouching bot (chest at 1.0 m) from `threat`
/// (Bevy eye position), not from data.
fn find_cover(nav: &Nav, col: &CollisionWorld, feet: Vec3, threat: Vec3, buf: &mut Vec<u32>) -> Option<Vec3> {
    let grid = nav.grid.as_ref()?;
    let here = grid.nearest(feet)?;
    let comp = grid.comp[here as usize];
    let _perf = crate::perf::scope("bot_cover");
    let mut best: Option<(f32, Vec3)> = None;
    // the columns within 12 m (all ~10^5 nodes of the map were scanned per call)
    for k in grid.nodes_within(feet, 12.0) {
        let (k, n) = (k as usize, grid.nodes[k as usize]);
        let d = n.distance(feet);
        if !(1.5..=12.0).contains(&d) || grid.comp[k] != comp || best.is_some_and(|b| b.0 <= d) {
            continue;
        }
        // farther from the threat than it is close to us, and the line of fire blocked
        if n.distance(threat) < 6.0 {
            continue;
        }
        let chest = n + Vec3::Y * 1.0;
        let dir = (chest - threat).normalize_or_zero();
        if col.raycast(threat, dir, threat.distance(chest) - 0.3, buf).is_some() {
            best = Some((d, n));
        }
    }
    best.map(|b| b.1)
}

/// How long a team remembers where it saw / heard an enemy (NOT from data; BotMemoryConfig is
/// per bot and not decoded).
const INTEL_SECS: f32 = 8.0;

/// Enemy positions a team's bots saw or heard (Bevy metres, time), per team index.
#[derive(Default)]
pub(crate) struct TeamIntel {
    known: [Vec<(Vec3, f32)>; 2],
}

impl TeamIntel {
    /// A sighting / noise: replaces an older one within 4 m (the same enemy moving).
    fn note(&mut self, team: usize, p: Vec3, now: f32) {
        let k = &mut self.known[team & 1];
        if let Some(x) = k.iter_mut().find(|x| x.0.distance(p) < 4.0) {
            *x = (p, now);
        } else if k.len() < 32 {
            k.push((p, now));
        }
    }

    fn positions(&self, team: usize) -> Vec<Vec3> {
        self.known[team & 1].iter().map(|x| x.0).collect()
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn bot_ai(
    time: Res<Time>,
    flow: Res<MatchFlow>,
    assets: Option<Res<BotAssets>>,
    data: Option<Res<CombatData>>,
    col: Res<CollisionWorld>,
    mut nav: ResMut<Nav>,
    obj: Res<Objectives>,
    pawns: Query<&Pawn>,
    mut bots: Query<(Entity, &mut Bot, &mut Soldier)>,
    mut queue: ResMut<DamageQueue>,
    (mut wsfx, mut fx, mut sfx, impacts): (ResMut<crate::sfx::WeaponSfx>, ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>, Option<Res<crate::impacts::Impacts>>),
    mut buf: Local<(Vec<u32>, Vec<Vec3>)>,
    (mut log, dbg, level, mut last_line): (ResMut<BotLog>, Res<BotDebug>, Option<Res<crate::objectives::LevelData>>, Local<Option<usize>>),
    mut intel: Local<TeamIntel>,
) {
    let _perf = crate::perf::scope("bot_ai");
    if flow.phase != Phase::Playing && flow.phase != Phase::Spawn {
        return;
    }
    let (Some(assets), Some(data)) = (assets, data) else { return };
    let dt = time.delta_secs().min(0.1);
    // snapshot of everyone for perception
    let player = pawns.single().ok().filter(|p| !p.dead && flow.phase == Phase::Playing).map(|p| feet_ue(p.feet) + Vec3::Z * crate::combat::CYL_HALF);
    // the points a bot must see of the player: head and chest at his real (crouched) height;
    // the standing centre + 20 / 45 saw over low cover a crouched player hid behind (user
    // 2026-10-22 22:02 "they see me through walls"). NOT from data.
    let player_look: Option<[f32; 2]> = pawns.single().ok().map(|p| {
        let feet_z = feet_ue(p.feet).z;
        let eye = p.eye * 100.0;
        let c = feet_z + crate::combat::CYL_HALF;
        [feet_z + eye - 4.0 - c, feet_z + eye * 0.6 - c]
    });
    let drone_c = crate::drone::target();
    // the bots' drones (botdrones.rs): targets for the other team
    let bot_drones = crate::botdrones::targets();
    // (the bot the nuke's Fulton pickup takes is no target: nuke.rs held)
    let others: Vec<(Entity, Vec3, u8, bool)> = bots.iter().map(|(e, _, s)| (e, s.center_ue(), s.team, s.alive && !crate::nuke::held(e))).collect();
    let mut plans_this_frame = 0u32;
    let mut plan_time = std::time::Duration::ZERO;
    let now = time.elapsed_secs();
    // a new round moves the front: every bot plans for the new line at once
    let line_changed = last_line.is_some_and(|l| l != obj.active_line);
    if line_changed {
        log.event(now, format!("front line {} -> {}: all bots replan", last_line.unwrap_or(0), obj.active_line));
    }
    *last_line = Some(obj.active_line);
    // enemies per team (Bevy positions) for hunting, the active line's spawns per team
    let to_bevy = |c: Vec3| Vec3::new(c.x, c.z, c.y) * 0.01;
    let mut enemy_pos: [Vec<Vec3>; 2] = [Vec::new(), Vec::new()];
    for &(_, c, team, alive) in &others {
        if alive {
            enemy_pos[(team as usize + 1) & 1].push(to_bevy(c));
        }
    }
    if let Some(c) = player {
        enemy_pos[(crate::combat::player_team() as usize + 1) & 1].push(to_bevy(c));
    }
    // what each team knows (TeamIntel): hunting goes to these, not to every enemy's live position
    // (user 2026-10-22 22:02: "once they saw me they always know where I'm at"). The team's UAV
    // sweep (botextras.rs) shows all enemies, as the UAV does for the player.
    for t in 0..2usize {
        let now_t = time.elapsed_secs();
        intel.known[t].retain(|k| now_t - k.1 < INTEL_SECS);
        if crate::botextras::team_uav(t as u8) {
            for &p in &enemy_pos[t] {
                intel.note(t, p, now_t);
            }
        }
    }
    let line_sp = [TEAM_USA, TEAM_KVA].map(|t| crate::objectives::line_spawns(level.as_deref(), &obj, t, "GMST_Infantry"));
    // the enemy side per team (UE): centre of the other team's spawns on the active line, where
    // idle bots look (bots/brain.rs idle_yaw)
    let enemy_side: [Option<Vec3>; 2] = [1usize, 0].map(|k| (!line_sp[k].is_empty()).then(|| line_sp[k].iter().map(|p| feet_ue(p.0)).sum::<Vec3>() / line_sp[k].len() as f32));
    // first slot of each team (main.rs: slots 0-6 USA, 7-14 KVA): goal_for distributes by the
    // bot's index within its team. With the global slot index the "every third bot guards" rule
    // made 3 of 8 KVA bots guards (slots 8, 11, 14) but only 2 of 7 USA bots (2, 5), and the
    // open objectives were split differently per team.
    let team_first = [TEAM_USA, TEAM_KVA].map(|t| assets.slots.iter().position(|sl| sl.team == t).unwrap_or(0));
    // snapshot of every bot in the log file every 2 s
    if log.enabled && log.file.is_some() && now - log.snap_t >= 2.0 {
        log.snap_t = now;
        const L: [&str; 4] = ["A", "B", "C", "D"];
        let tn = |t: u8| if t == TEAM_USA { "USA" } else { "KVA" };
        let lines: Vec<String> = bots
            .iter()
            .map(|(e, b, s)| {
                let r = b.row(e, s);
                // the capture volume the bot stands in (what objectives.rs counts) and cover
                let zone = obj.list.iter().find(|o| s.alive && o.line == obj.active_line && o.volume.contains(s.center_ue().to_array())).map(|o| L[o.letter.min(3)]).unwrap_or("-");
                format!("  {:<10} team {} {} hp {:3.0} at ({:7.1} {:6.1} {:7.1}) goal {:<10} {:5.1} m nav {} idle {:4.1} resets {} target {} zone {} cover {} kd {}/{} crouch {} ads {:.1}", r.name, r.team, if r.alive { "alive" } else { "dead " }, r.health, r.feet.x, r.feet.y, r.feet.z, r.goal, r.goal_dist, r.nav_ok, r.idle, r.resets, r.engaging, zone, b.cover.is_some(), s.kills, s.deaths, s.crouched, b.brain.ads)
            })
            .collect();
        // owners and capture progress of the active line (the logs had no objective state)
        let state: Vec<String> = obj
            .list
            .iter()
            .filter(|o| o.line == obj.active_line)
            .map(|o| {
                let own = o.owner.map(tn).unwrap_or("-");
                match o.capturing_team {
                    Some(t) if o.progress > 0.0 => format!("{}:{own}<{} {:.0}/{:.0}>", L[o.letter.min(3)], tn(t), o.progress, o.cap_time),
                    _ => format!("{}:{own}", L[o.letter.min(3)]),
                }
            })
            .collect();
        log.write(&format!("[{now:8.2}] snapshot (line {}) {}", obj.active_line, state.join(" ")));
        for l in lines {
            log.write(&l);
        }
    }
    for (e, mut b, mut s) in &mut bots {
        let b = &mut *b;
        // death / respawn
        if !s.alive {
            match b.state {
                State::Alive => {
                    b.state = State::Dead(0.0);
                    b.death = 1 + (rnd(&mut b.seed) * 10.0) as usize % 10;
                    b.target = None;
                }
                State::Dead(t) => {
                    let t = t + dt;
                    b.state = State::Dead(t);
                    if t >= BOT_RESPAWN {
                        if let Some((p, yaw)) = pick_start(&col, &line_sp[s.team as usize & 1], s.team, &mut b.seed) {
                            s.feet = p - Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
                            b.yaw = yaw;
                        }
                        s.alive = true;
                        s.health = crate::combat::HEALTH_MAX;
                        s.crouched = false;
                        b.cover = None;
                        b.state = State::Alive;
                        b.ragdoll = None;
                        b.vel = Vec3::ZERO;
                        b.path.clear();
                        b.goal = None;
                        b.idle = 0.0;
                        b.anchor = None;
                        b.stalls = 0;
                        b.spot = s.feet;
                        b.clip = data.weapons.get(assets.slots[b.slot].weapon_cfg).map(|w| w.clip).unwrap_or(30);
                        b.brain = brain::Brain { nades: data.grenade.ammo.max(0), ..Default::default() };
                        log.event(now, format!("{} respawned at ({:.1} {:.1} {:.1})", s.name, s.feet.x, s.feet.y, s.feet.z));
                    }
                }
            }
            continue;
        }
        // botextras.rs / botvehicles.rs drive this bot (in a vehicle, launcher out): no infantry
        // movement or rifle fire; the path is planned again once it is back on foot
        // (nuke.rs held: the bot the Fulton pickup takes stands still / hangs on the line)
        if crate::botextras::controlled(e) || crate::nuke::held(e) {
            b.vel = Vec3::ZERO;
            b.target = None;
            b.path.clear();
            b.goal = None;
            b.cover = None;
            continue;
        }
        let Some(w) = data.weapons.get(assets.slots[b.slot].weapon_cfg) else { continue };
        if dbg.frozen {
            b.vel = Vec3::ZERO;
            continue;
        }
        if line_changed {
            b.goal = None;
            b.repath = 0.0;
            b.path.clear();
            b.cover = None;
            b.bias = 0;
        }
        s.since_damage += dt;
        if s.since_damage >= 5.0 && s.health < crate::combat::HEALTH_MAX {
            s.health = (s.health + 10.0 * dt).min(crate::combat::HEALTH_MAX);
        }
        let me = s.center_ue();
        let eye = me + Vec3::Z * 35.5;
        // perception every 0.25 s
        b.think -= dt;
        if b.think <= 0.0 {
            b.think = 0.25;
            // view cone +-60 deg around the facing, all round within 5 m or right after being
            // hit; seen when the head or the chest is in the open (NOT from data). Among the
            // seen enemies the best by BotTargetScoringConfig (bots/brain.rs target_score), not
            // simply the nearest; only the 5 nearest candidates are traced.
            let lv = brain::level(s.team);
            let alert = s.since_damage < 2.0;
            let facing = Vec2::new(b.yaw.cos(), b.yaw.sin());
            // user 2026-10-22 22:04 "they can see me even when I'm not in their view": all-round
            // awareness only within 2.5 m (was 5 m) and the sight range shorter in the dark
            // (45 % at night; weather.rs daylight, 1 while the weather is off). NOT from data.
            let sight = SIGHT * (0.45 + 0.55 * crate::weather::daylight().clamp(0.0, 1.0));
            let mut cands: Vec<(f32, Target, Vec3)> = Vec::new();
            {
                let mut add = |t: Target, c: Vec3| {
                    let d = c.distance(me);
                    if d > sight {
                        return;
                    }
                    let to = Vec2::new(c.x - me.x, c.y - me.y).normalize_or_zero();
                    if !alert && d > 250.0 && to.dot(facing) < 0.5 {
                        return;
                    }
                    cands.push((d, t, c));
                };
                if s.team != crate::combat::player_team() {
                    if let Some(c) = player {
                        add(Target::Player, c);
                    }
                    // the player's drone (deployed or parked) is a target too (user 2026-10-19)
                    if let Some(c) = drone_c {
                        add(Target::Drone, c);
                    }
                }
                for &(oe, c, team, alive) in &others {
                    if oe != e && alive && team != s.team {
                        add(Target::Bot(oe), c);
                    }
                }
                for &(de, c, team) in &bot_drones {
                    if team != s.team {
                        add(Target::BotDrone(de), c);
                    }
                }
            }
            cands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let shooter = match s.last_attacker {
                Some(Actor::Player) if s.since_damage < 2.0 => Some(Target::Player),
                Some(Actor::Bot(a)) if s.since_damage < 2.0 => Some(Target::Bot(a)),
                _ => None,
            };
            let o = Vec3::new(eye.x, eye.z, eye.y) * 0.01;
            let mut best: Option<(f32, Target, Vec3)> = None;
            for &(d, t, c) in cands.iter().take(5) {
                let zs = match (t, player_look) {
                    (Target::Player, Some(z)) => z,
                    _ => [20.0, 45.0],
                };
                let seen = zs.iter().any(|z| {
                    let tgt = c + Vec3::Z * *z;
                    let tb = Vec3::new(tgt.x, tgt.z, tgt.y) * 0.01;
                    let dir = (tb - o).normalize_or_zero();
                    col.raycast(o, dir, o.distance(tb), &mut buf.0).is_none()
                });
                if !seen {
                    continue;
                }
                let sc = brain::target_score(d, shooter == Some(t), (b.target == Some(t)).then_some(b.brain.tgt_time));
                if best.is_none_or(|x| sc > x.0) {
                    best = Some((sc, t, c));
                }
            }
            let new = best.map(|b| b.1);
            if let Some((_, _, c)) = best {
                intel.note(s.team as usize & 1, Vec3::new(c.x, c.z, c.y) * 0.01, now);
            }
            if new != b.target {
                b.seen_for = 0.0;
                if let Some((_, _, c)) = best {
                    // reaction before the first shot at a new target (bots/brain.rs reaction):
                    // faster where the bot expected it (heard fire / lost a target there within
                    // 30 deg), slower at the edge of the view
                    use std::f32::consts::{PI, TAU};
                    let to = (c.y - me.y).atan2(c.x - me.x);
                    let off = |a: f32| {
                        let x = (a - to).rem_euclid(TAU);
                        if x > PI { x - TAU } else { x }
                    };
                    let near = |p: Vec3| off((p.y - me.y).atan2(p.x - me.x)).abs() < 0.52;
                    let expected = b.brain.heard.is_some_and(|h| now - h.1 < 3.0 && near(h.0)) || b.brain.lkp.is_some_and(|l| now - l.1 < 4.0 && near(l.0));
                    let flick = off(b.yaw);
                    b.react = brain::reaction(lv, &mut b.seed, expected, flick.abs() > 0.52);
                    brain::acquire(&mut b.brain, lv, flick, &mut b.seed);
                    b.brain.tgt_last = None;
                    b.brain.tgt_vel = Vec3::ZERO;
                    b.brain.lkp = None;
                } else if let Some(l) = b.brain.tgt_last.take() {
                    // lost sight: remember where (looked at for 4 s, grenades)
                    b.brain.lkp = Some((l, now));
                }
            }
            b.target = new;
            // hearing: enemy fire within NoiseProcessesDistanceMax 3000 uu, noticed after
            // NoiseProcessesTime 0.1-0.4 s by distance (BotMemoryConfig)
            if b.target.is_none() {
                if let Some((p, d)) = brain::hear(me, s.team, now) {
                    intel.note(s.team as usize & 1, Vec3::new(p.x, p.z, p.y) * 0.01, now);
                    if b.brain.heard.is_none_or(|h| now - h.1 > 1.0) {
                        b.brain.heard = Some((p, now + 0.1 + 0.3 * (d / 3000.0).min(1.0)));
                    }
                }
            }
        }
        let target_pos = match b.target {
            Some(Target::Player) => player,
            Some(Target::Drone) => drone_c,
            Some(Target::BotDrone(d)) => bot_drones.iter().find(|x| x.0 == d).map(|x| x.1),
            Some(Target::Bot(t)) => others.iter().find(|o| o.0 == t && o.3).map(|o| o.1),
            None => None,
        };
        if target_pos.is_none() {
            b.target = None;
        }
        // under fire: some bots break off to the nearest cover from the attacker, crouch there
        // for a few seconds and come out again (not from data)
        let feet = s.feet;
        b.cover_cd -= dt;
        if s.since_damage < dt * 1.5 && b.cover.is_none() && b.cover_cd <= 0.0 {
            b.cover_cd = 6.0;
            let threat = match s.last_attacker {
                Some(Actor::Player) => player,
                Some(Actor::Bot(a)) => others.iter().find(|o| o.0 == a).map(|o| o.1),
                _ => None,
            }
            .map(|c| Vec3::new(c.x, c.z + 35.5, c.y) * 0.01);
            // CombatInCover fDuckAfterShotByBot/PlayerChance, less while pushing, more when hurt
            let chance = brain::cover_chance(matches!(s.last_attacker, Some(Actor::Player)), matches!(b.goal_kind, "capture" | "attack"), s.health);
            if let Some(t) = threat.filter(|_| rnd(&mut b.seed) < chance) {
                if let Some(cp) = find_cover(&nav, &col, feet, t, &mut buf.0) {
                    // the path ends on the cover node itself (plan_path's random 4 m made it end
                    // beside it, the bot never counted as in cover and stood there for good)
                    b.path = plan_path_r(&nav, &col, feet, cp, 0.0, b.seed, &mut buf.0).0;
                    let cp = b.path.last().copied().unwrap_or(cp);
                    b.cover = Some((cp, brain::cover_time(s.health, &mut b.seed)));
                    b.brain.peek = (false, 1.0 + rnd(&mut b.seed));
                    b.cover_travel = 0.0;
                    b.goal = Some(cp);
                }
            }
        }
        let mut in_cover = false;
        if let Some((cp, left)) = b.cover {
            // at the spot, or the path is done (as close as the walker got)
            if Vec3::new(cp.x - feet.x, 0.0, cp.z - feet.z).length() < 0.8 || b.path.is_empty() {
                in_cover = true;
                let left = left - dt;
                if left <= 0.0 {
                    b.cover = None;
                    b.repath = 0.0;
                } else {
                    b.cover = Some((cp, left));
                }
            } else {
                // not there after COVER_TRAVEL_MAX: give the cover up and plan the goal again;
                // with the cover set nothing else replanned and the stall watchdog was off
                b.cover_travel += dt;
                if b.cover_travel > COVER_TRAVEL_MAX {
                    b.cover = None;
                    b.repath = 0.0;
                    b.goal = None;
                    b.path.clear();
                }
            }
        }
        // in cover: hide crouched (reload there) and peek standing; in a fight standing still
        // at range (> 12 m): crouch in some engagements (bots/brain.rs, NOT from data)
        let peeking = in_cover && brain::peek(b, dt);
        s.crouched = (in_cover && !peeking) || (!in_cover && b.target.is_some() && b.brain.crouch_fight && b.vel.length() < 0.3 && target_pos.is_some_and(|t| t.distance(me) > 1200.0));
        if in_cover && !peeking && b.reload <= 0.0 && b.clip < w.clip * 6 / 10 {
            b.reload = w.reload_time;
        }
        // movement goal
        b.repath -= dt;
        // at most two goal plans per frame (A* over the walk grid): the rest wait a frame, so a
        // respawn wave does not stall one frame (NOT from data)
        // and once 2 ms went to planning this frame, the next plans wait for the next frame
        // (bot_ai p99 13 ms in the user's profile 2026-10-19; NOT from data)
        if b.cover.is_none() && (b.goal.is_none() || b.repath <= 0.0) && plans_this_frame < 2 && plan_time.as_secs_f32() < 0.002 {
            plans_this_frame += 1;
            let t_plan = std::time::Instant::now();
            b.repath = 4.0 + rnd(&mut b.seed) * 2.0;
            let local = b.slot.saturating_sub(team_first[s.team as usize & 1]);
            if let Some((g, kind, letter)) = goal_for(&obj, s.team, feet, local + b.bias, &intel.positions(s.team as usize & 1), if matches!(b.goal_kind, "capture" | "attack") { b.goal_letter } else { None }) {
                // a walk-grid node inside the objective's capture volume (a hunted enemy: its
                // position); without one (no grid yet, zone off the bot's component) a point
                // near the objective as before
                let mut exact = false;
                let goal = if kind == "hunt" {
                    // follow a moving target: plan again sooner
                    b.repath = 2.0 + rnd(&mut b.seed);
                    g
                } else if let Some(n) = nav.grid.as_ref().and_then(|gr| {
                    let o = active_objective(&obj, letter)?;
                    zone_node(gr, o, walk_comp(gr, feet)?, b.seed ^ (now.to_bits()))
                }) {
                    exact = true;
                    n
                } else {
                    let ang = rnd(&mut b.seed) * std::f32::consts::TAU;
                    let r = 1.5 + rnd(&mut b.seed) * 4.5;
                    g + Vec3::new(ang.cos() * r, 0.0, ang.sin() * r)
                };
                let end_r = if exact { 0.0 } else { 4.0 };
                b.goal = Some(goal);
                // a new objective (not the next point in the same area): progress counts anew
                if b.anchor.is_none_or(|a| a.distance(g) > 5.0) {
                    b.anchor = Some(g);
                    b.anchor_best = f32::MAX;
                    b.anchor_t = 0.0;
                }
                // far goals: a detour through a point beside the direct line, the side and the
                // distance by slot, so the team does not run one lane (NOT from data)
                let span = Vec3::new(goal.x - feet.x, 0.0, goal.z - feet.z);
                let (path, ok) = if span.length() > 30.0 {
                    let side = Vec3::new(-span.z, 0.0, span.x).normalize_or_zero();
                    let k = if b.slot % 2 == 0 { 1.0 } else { -1.0 } * (8.0 + (b.slot % 4) as f32 * 5.0 + rnd(&mut b.seed) * 6.0);
                    let via = feet + span * (0.35 + rnd(&mut b.seed) * 0.3) + side * k;
                    let mut p = plan_path(&nav, &col, feet, via, b.seed, &mut buf.0);
                    let from = *p.last().unwrap_or(&feet);
                    let (rest, ok) = plan_path_r(&nav, &col, from, goal, end_r, b.seed ^ 0x5bd1e995, &mut buf.0);
                    p.extend(rest);
                    (p, ok)
                } else {
                    plan_path_r(&nav, &col, feet, goal, end_r, b.seed, &mut buf.0)
                };
                b.path = path;
                if kind != b.goal_kind || letter != b.goal_letter || !ok {
                    let l = letter.map(|l| ["A", "B", "C", "D"][l.min(3)]).unwrap_or("-");
                    log.event(now, format!("{} goal {kind} {l}: {:.0} m, {} waypoints{}", s.name, span.length(), b.path.len(), if ok { "" } else { ", NO nav path (straight line)" }));
                }
                b.goal_kind = kind;
                b.goal_letter = letter;
                b.nav_ok = ok;
                // the walk grid is still being built (first seconds of a match): plan again soon
                // instead of following the straight line for 4-6 s
                if !ok && nav.grid.is_none() {
                    b.repath = b.repath.min(1.0);
                }
            }
            plan_time += t_plan.elapsed();
        }
        // standing in the capture volume of the objective it heads for: the goal is reached
        // (capturing / holding happens there), no stall or no-progress reset that sent it off
        // to another objective, side steps stay inside
        let zone = if matches!(b.goal_kind, "capture" | "attack" | "guard") { active_objective(&obj, b.goal_letter) } else { None };
        let in_zone = zone.is_some_and(|o| o.volume.contains(s.center_ue().to_array()));
        // steering
        let mut dir = Vec3::ZERO;
        while let Some(&wp) = b.path.first() {
            let d = Vec3::new(wp.x - feet.x, 0.0, wp.z - feet.z);
            if d.length() < 0.6 && b.path.len() > 1 {
                b.path.remove(0);
                continue;
            }
            // the last point counts as reached within 0.8 m: nodes next to walls could not be
            // reached exactly and the bot pushed against the wall ("stuck" 0.5 m from its goal,
            // user's bot log 2026-10-18; NOT from data)
            if b.path.len() == 1 && d.length() < 0.8 {
                b.path.clear();
                break;
            }
            if d.length() > 0.3 {
                dir = d.normalize();
            }
            break;
        }
        let engaging = b.target.is_some();
        b.calm = if engaging { 0.0 } else { b.calm + dt };
        // engaging a close target: side steps instead of the path (not from data)
        let close = target_pos.is_some_and(|t| t.distance(me) < 2500.0);
        // objective first (user 2026-10-18): a bot on its way to capture / attack a point keeps
        // walking its path while it shoots, unless the enemy is within 8 m or it already stands
        // at the objective (NOT from data)
        let push = matches!(b.goal_kind, "capture" | "attack")
            && b.anchor.is_some_and(|a| Vec3::new(a.x - feet.x, 0.0, a.z - feet.z).length() > 8.0)
            && !target_pos.is_some_and(|t| t.distance(me) < 800.0);
        if in_cover {
            dir = Vec3::ZERO;
        } else if engaging && close && b.cover.is_none() && !push {
            b.strafe.1 -= dt;
            if b.strafe.1 <= 0.0 {
                b.strafe = (if rnd(&mut b.seed) < 0.5 { -1.0 } else { 1.0 }, 0.8 + rnd(&mut b.seed) * 1.5);
                if rnd(&mut b.seed) < 0.35 {
                    b.strafe.0 = 0.0;
                }
            }
            if let Some(t) = target_pos {
                let to = Vec3::new(t.x - me.x, 0.0, t.y - me.y).normalize_or_zero();
                dir = Vec3::new(-to.z, 0.0, to.x) * b.strafe.0;
                // no side step into a wall or off a ledge: the other side, else stand
                let free = |d: Vec3, buf: &mut Vec<u32>| {
                    let ahead = feet + d * 1.0;
                    crate::nav::clear(&col, feet, ahead, buf) && col.floor(ahead + Vec3::Y * 0.3, 0.6, buf).is_some() && (!in_zone || zone.is_none_or(|o| o.volume.contains(stand_centre_ue(ahead))))
                };
                if dir != Vec3::ZERO && !free(dir, &mut buf.0) {
                    b.strafe.0 = -b.strafe.0;
                    dir = -dir;
                    if !free(dir, &mut buf.0) {
                        dir = Vec3::ZERO;
                        b.strafe.0 = 0.0;
                    }
                }
            }
        }
        // zig-zag on the way while under fire or pushing through a fight (not inside the
        // objective volume; bots/brain.rs weave, NOT from data)
        if dir != Vec3::ZERO && b.cover.is_none() && !in_zone && (s.since_damage < 2.5 || (engaging && push)) {
            let wv = brain::weave(b, dir, dt);
            if crate::nav::clear(&col, feet, feet + wv * 1.0, &mut buf.0) {
                dir = wv;
            }
        }
        // sprint to cover and on stretches > 15 m with no enemy seen for 1.5 s (NOT from data)
        let far = b.goal.is_some_and(|g| Vec3::new(g.x - feet.x, 0.0, g.z - feet.z).length() > 15.0);
        b.sprint = ((!engaging && far && b.seen_for == 0.0 && b.calm > 1.5) || b.cover.is_some()) && dir != Vec3::ZERO && b.path.len() > 1 && !in_cover;
        let speed = crate::player::BOT_GROUND_SPEED * w.move_speed * if b.sprint {
            crate::player::BOT_SPRINT_PCT
        } else if engaging {
            // aiming down the sights: ADSMovementPercent; close-range side steps at most 0.6
            let ads = 1.0 + (w.ads_move.clamp(0.3, 1.0) - 1.0) * b.brain.ads;
            if close && !push { ads.min(0.6) } else { ads }
        } else {
            1.0
        };
        let mut on_ground = b.on_ground;
        let mut vel = b.vel;
        let mut f = s.feet;
        crate::player::walk(&col, &mut f, &mut vel, &mut on_ground, false, dir, speed, false, dt, &mut buf);
        s.feet = f;
        b.vel = vel;
        b.on_ground = on_ground;
        // stuck: new path
        if dir != Vec3::ZERO {
            if f.distance(b.last_feet) < 0.3 * dt {
                b.stuck += dt;
            } else {
                b.stuck = 0.0;
            }
            let last = b.path.last().copied();
            let near_end = b.path.len() <= 1 && last.is_some_and(|l| Vec3::new(l.x - f.x, 0.0, l.z - f.z).length() < 2.0);
            if b.stuck > 1.0 && (near_end || in_zone) {
                // blocked right before the last point, or inside the objective's capture volume:
                // take it as reached
                b.stuck = 0.0;
                b.path.clear();
                b.repath = b.repath.min(1.0 + rnd(&mut b.seed) * 2.0);
            } else if b.stuck > 1.0 {
                // stuck on something: the spot costs more for every bot's later paths, walk to a
                // free node a few metres away first, then plan the goal again (NOT from data)
                b.stuck = 0.0;
                b.resets += 1;
                let pen = nav.penalize(f, dir);
                log.event(now, format!("{} stuck at ({:.1} {:.1} {:.1}) heading for {}, node {:?} penalty {:.0}", s.name, f.x, f.y, f.z, b.goal_kind, pen, pen.and_then(|k| nav.penalty.get(k as usize)).copied().unwrap_or(0.0)));
                if f.distance(b.stuck_spot) < 1.0 {
                    b.stuck_count += 1;
                } else {
                    b.stuck_spot = f;
                    b.stuck_count = 1;
                }
                // trapped (third stuck at the same spot, the escape node unreachable too: a bot
                // stood 150 s in one place in the user's bot log 2026-10-19): onto the cheapest
                // node 2-8 m away (NOT from data)
                if b.stuck_count >= 3 {
                    b.stuck_count = 0;
                    if let Some(g) = nav.grid.clone() {
                        let main = g.main_comp();
                        let mut best: Option<(f32, u32)> = None;
                        for dj in -8i32..=8 {
                            for di in -8i32..=8 {
                                let p = f + Vec3::new(di as f32, 0.0, dj as f32);
                                let Some(k) = g.nearest(p) else { continue };
                                let q = g.nodes[k as usize];
                                let d = Vec3::new(q.x - f.x, 0.0, q.z - f.z).length();
                                if !(2.0..=8.0).contains(&d) || Some(g.comp[k as usize]) != main || (q.y - f.y).abs() > 1.5 {
                                    continue;
                                }
                                let cost = d + nav.penalty.get(k as usize).copied().unwrap_or(0.0) + g.wall.get(k as usize).copied().unwrap_or(0.0) * 2.0;
                                if best.is_none_or(|bb| cost < bb.0) {
                                    best = Some((cost, k));
                                }
                            }
                        }
                        if let Some((_, k)) = best {
                            let n = g.nodes[k as usize];
                            s.feet = n + Vec3::Y * 0.05;
                            b.vel = Vec3::ZERO;
                            b.goal = None;
                            b.repath = 0.0;
                            b.path.clear();
                            log.event(now, format!("{} trapped at ({:.1} {:.1} {:.1}) -> moved to ({:.1} {:.1} {:.1})", s.name, f.x, f.y, f.z, n.x, n.y, n.z));
                            continue;
                        }
                    }
                }
                let away = nav.grid.as_ref().and_then(|g| g.node_near(f, 4.0, b.seed ^ 0x2545_f491).map(|k| g.nodes[k as usize]));
                b.seed = b.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match away {
                    Some(a) => {
                        b.path = plan_path(&nav, &col, f, a, b.seed, &mut buf.0);
                        b.goal = Some(a);
                        b.repath = 1.5;
                    }
                    None => {
                        b.repath = 0.0;
                        b.goal = None;
                    }
                }
            }
        }
        // stall watchdog (NOT from data). A bot that wants to go somewhere (goal > 2 m away, not
        // in cover) is stalled when it stays within 1.5 m of one spot (standing, or pushing /
        // sliding against geometry: per-frame speed said "moving" there) for 4 s (8 s while
        // fighting), or when it has not come 2 m closer to its objective in 15 s. Stalled: a
        // free node 3-8 m away first, then a new goal (next objective by bias); the third stall
        // in a row puts it on the nearest node of the walkable map (stuck inside geometry, user's
        // screenshots: container entrance, truck under the bridge). Reaching the goal plans the
        // next point within 1-3 s, so guards and capturers keep moving in the area.
        let free = b.cover.is_none() && !in_cover;
        let gd = b.goal.map(|g| Vec3::new(g.x - f.x, 0.0, g.z - f.z).length()).unwrap_or(0.0);
        if Vec3::new(f.x - b.spot.x, 0.0, f.z - b.spot.z).length() > 1.5 || !free || gd <= 2.0 || in_zone {
            b.spot = f;
            b.idle = 0.0;
        } else {
            b.idle += dt;
        }
        if in_zone {
            b.stalls = 0;
        }
        // progress = the remaining walk along the planned path (then on to the objective), not
        // the straight distance: on Angel Island the nav path from the USA spawns to C first
        // leads away from it (bot log 1792581662: 67 m -> 100 m straight distance while walking
        // 2 m/s, "no progress" 3x at (18 -10 38)), the watchdog then gave up a working path
        let flat = |a: Vec3, c: Vec3| Vec3::new(a.x - c.x, 0.0, a.z - c.z).length();
        let ad = b.anchor.map(|a| match (b.nav_ok, b.path.first(), b.path.last()) {
            (true, Some(&p0), Some(&pn)) => flat(p0, f) + b.path.windows(2).map(|w| flat(w[1], w[0])).sum::<f32>() + flat(a, pn),
            _ => flat(a, f),
        });
        match ad {
            // not while fighting: bots that stopped to fight on the way counted as "no
            // progress" and were sent to another objective (bot log 1792678337: 20 resets)
            Some(d) if free && d > 6.0 && !in_zone && !engaging => {
                if d < b.anchor_best - 2.0 {
                    b.anchor_best = d;
                    b.anchor_t = 0.0;
                } else {
                    b.anchor_t += dt;
                }
            }
            _ => {
                b.anchor_best = ad.unwrap_or(f32::MAX);
                b.anchor_t = 0.0;
            }
        }
        if free && gd < 1.5 && b.repath > 3.0 && !engaging {
            // inside the objective volume a held spot for 2.5-5.5 s (looking over open sight
            // lines, bots/brain.rs idle_yaw) instead of shuffling every 1-3 s (NOT from data)
            b.repath = if in_zone { 2.5 + rnd(&mut b.seed) * 3.0 } else { 1.0 + rnd(&mut b.seed) * 2.0 };
        }
        let limit = if engaging { 8.0 } else { 4.0 };
        if b.idle > limit || b.anchor_t > 15.0 {
            let why = if b.idle > limit { format!("stalled {:.0} s", b.idle) } else { "no progress to the objective for 15 s".to_string() };
            b.idle = 0.0;
            b.anchor_t = 0.0;
            b.anchor_best = f32::MAX;
            b.resets += 1;
            b.bias += 1;
            b.stalls += 1;
            let grid = nav.grid.as_ref();
            if b.stalls >= 3 {
                // stuck inside geometry: onto the nearest node of the main walkable area
                b.stalls = 0;
                if let Some(k) = grid.and_then(|g| g.main_comp().and_then(|m| g.nearest_in_comp(f, m, 8.0))) {
                    let n = grid.unwrap().nodes[k as usize];
                    s.feet = n + Vec3::Y * 0.05;
                    b.vel = Vec3::ZERO;
                    log.event(now, format!("{} {why} at ({:.1} {:.1} {:.1}), third time -> put on the walk grid at ({:.1} {:.1} {:.1})", s.name, f.x, f.y, f.z, n.x, n.y, n.z));
                } else {
                    log.event(now, format!("{} {why} at ({:.1} {:.1} {:.1}), third time, no walk grid node within 8 m", s.name, f.x, f.y, f.z));
                }
                b.goal = None;
                b.repath = 0.0;
                b.path.clear();
            } else {
                // a free node 3-8 m away, then the goal again
                let away = grid.and_then(|g| {
                    let c = g.nearest(f).map(|k| g.comp[k as usize]);
                    (0..8u32).find_map(|i| g.node_near(f, 8.0, b.seed ^ (i * 0x9e37_79b9)).filter(|&k| g.nodes[k as usize].distance(f) > 3.0 && c.is_none_or(|c| g.comp[k as usize] == c))).map(|k| g.nodes[k as usize])
                });
                b.seed = b.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                log.event(now, format!("{} {why} at ({:.1} {:.1} {:.1}), goal {} {:?}, nav {}, path {} -> {}", s.name, f.x, f.y, f.z, b.goal_kind, b.goal_letter, b.nav_ok, b.path.len(), if away.is_some() { "step aside, new goal" } else { "new goal" }));
                match away {
                    Some(a) => {
                        b.path = plan_path(&nav, &col, f, a, b.seed, &mut buf.0);
                        b.goal = Some(a);
                        b.goal_kind = "unstuck";
                        b.goal_letter = None;
                        b.repath = 2.5;
                    }
                    None => {
                        b.goal = None;
                        b.goal_letter = None;
                        b.repath = 0.0;
                        b.path.clear();
                    }
                }
            }
        }
        if b.idle == 0.0 && b.anchor_t == 0.0 && gd > 2.0 && free && Vec3::new(f.x - b.spot.x, 0.0, f.z - b.spot.z).length() > 1.4 {
            // moving well again
            b.stalls = 0;
        }
        b.last_feet = f;
        // facing: the target, else the movement direction
        // facing: the target, else heard fire / last known position / walking direction /
        // a scan over open sight lines (bots/brain.rs idle_yaw)
        // sprinting (to cover, too): facing the running direction, the sprint animation only
        // runs forward (mp_sprint_F); before, a bot sprinting to cover faced its target and slid
        // sideways (user 2026-10-22 20:43). NOT from data.
        let want_yaw = match target_pos {
            _ if b.sprint && dir != Vec3::ZERO => dir.z.atan2(dir.x),
            Some(t) => (t.y - me.y).atan2(t.x - me.x),
            None => brain::idle_yaw(b, me, eye, dir, now, dt, enemy_side[s.team as usize & 1], &col, &mut buf.0),
        };
        let mut dyaw = (want_yaw - b.yaw).rem_euclid(std::f32::consts::TAU);
        if dyaw > std::f32::consts::PI {
            dyaw -= std::f32::consts::TAU;
        }
        let turn = brain::turn_rate(brain::level(s.team)) * dt;
        b.yaw += dyaw.clamp(-turn, turn);
        // weapon
        b.fire_cd = (b.fire_cd - dt).max(0.0);
        // flash bang (combat::tactical_blast): no shots while blinded
        s.blinded = (s.blinded - dt).max(0.0);
        if b.reload > 0.0 {
            b.reload -= dt;
            if b.reload <= 0.0 {
                b.clip = w.clip;
            }
        }
        if let Some(tp) = target_pos.filter(|_| !dbg.passive) {
            b.seen_for += dt;
            let lv = brain::level(s.team);
            let dist = tp.distance(me);
            let moving = b.vel.length() > 0.5;
            // aim settling / target velocity / spread / recoil / ADS beyond 6 m and not
            // sprinting (bots/brain.rs track; NOT from data: the 6 m)
            brain::track(&mut b.brain, w, lv, dt, tp, moving, s.crouched, dist > 600.0 && !b.sprint, &mut b.seed);
            // reaction time before the first shot
            let aimed = dyaw.abs() < 0.15 && b.seen_for > b.react && !b.sprint;
            // bursts by range (GCBotConfig Weapon0 Stationary / Moving, bots/brain.rs)
            if b.pause > 0.0 {
                b.pause -= dt;
                if b.pause <= 0.0 {
                    b.burst = brain::burst_len(w, dist, moving, &mut b.seed);
                }
            } else if b.burst <= 0.0 {
                b.burst = brain::burst_len(w, dist, moving, &mut b.seed);
            }
            if aimed && s.blinded <= 0.0 && b.reload <= 0.0 && b.pause <= 0.0 && b.fire_cd <= 0.0 {
                if b.clip <= 0 {
                    b.reload = w.reload_time;
                } else {
                    b.clip -= 1;
                    let iv = brain::interval(w, lv);
                    b.fire_cd = iv;
                    b.burst -= iv;
                    if b.burst <= 0.0 {
                        b.pause = brain::burst_pause(moving, lv, &mut b.seed);
                    }
                    if let Some(si) = &w.sound_info {
                        wsfx.shot(&format!("bot{}", e.index()), si, 1, Some(eye), w.fire_interval + 0.06);
                    }
                    brain::noise(eye, s.team, now);
                    let d = (tp + Vec3::Z * b.brain.aim_h - eye).normalize_or_zero();
                    // hold fire while a team mate is in the line of fire
                    if let ShotHit::Soldier(te, _, _) = crate::combat::trace_shot(&col, &others, None, eye, d, eye.distance(tp), Some(e), true, &mut buf.0) {
                        if others.iter().any(|o| o.0 == te && o.2 == s.team) {
                            b.fire_cd = 0.3;
                            continue;
                        }
                    }
                    if let Some(mf) = &w.muzzle[1] {
                        // 3P muzzle: rifle held at chest height ahead of the eye
                        let at = eye + d * 55.0 - Vec3::Z * 18.0;
                        fx.fire(mf, at, Quat::from_rotation_z(d.y.atan2(d.x)) * Quat::from_rotation_y(-d.z.asin()));
                    }
                    // the shot: aim point with tracking error, uncompensated recoil and the
                    // weapon's cone as for the player (bots/brain.rs shot_dir; replaces the old
                    // "1.2 + 4 x settle + moving + Spread.Stand.Min / 2" degrees)
                    let dd = brain::shot_dir(&b.brain, lv, eye, tp, &mut b.seed);
                    brain::fired(&mut b.brain, w, lv, s.crouched, &mut b.seed);
                    let pl = player.map(|c| (c, true));
                    let shot = crate::combat::trace_shot(&col, &others, pl, eye, dd, w.range * 2.0, Some(e), s.team == crate::combat::player_team(), &mut buf.0);
                    // shooting at the drone: a hit when the bullet passes within its collision
                    // cylinder (CollisionRadius) before any world surface
                    if b.target == Some(Target::Drone) {
                        if let Some(c) = drone_c {
                            let along = (c - eye).dot(dd);
                            let miss = (eye + dd * along).distance(c);
                            let world = match &shot {
                                ShotHit::World(t, _, _) => *t,
                                _ => f32::MAX,
                            };
                            if along > 0.0 && miss < DRONE_HIT_RADIUS && along < world {
                                let (dmg, _) = crate::combat::bullet_damage(&data.grid, w, c + Vec3::Z * 100.0, c, along);
                                queue.0.push(Hit { target: Target::Drone, amount: dmg, attacker: Actor::Bot(e), attacker_team: s.team, headshot: false, explosive: false, from: eye, icon: w.ui_icon.clone(), dmg_type: None });
                            }
                        }
                    }
                    // the bots' drones (botdrones.rs): their boxes stop the bullet; an enemy one
                    // takes the hit (body damage as the player's drone above)
                    if let Some((de, t)) = crate::botdrones::bullet_hit(eye, dd, &shot, w.range * 2.0, s.team) {
                        let c = bot_drones.iter().find(|x| x.0 == de).map(|x| x.1).unwrap_or(eye + dd * t);
                        let (dmg, _) = crate::combat::bullet_damage(&data.grid, w, c + Vec3::Z * 100.0, eye + dd * t, t);
                        queue.0.push(Hit { target: Target::BotDrone(de), amount: dmg, attacker: Actor::Bot(e), attacker_team: s.team, headshot: false, explosive: false, from: eye, icon: w.ui_icon.clone(), dmg_type: Some(w.dmg_type.clone()) });
                    }
                    // 3P tracer (View3P Day WeaponEffectTracer), every Frequency-th shot on average
                    if let Some(tc) = w.tracer[1].as_ref() {
                        if rnd(&mut b.seed) * tc.frequency as f32 <= 1.0 {
                            let dist = match &shot {
                                ShotHit::Player(t, _) | ShotHit::Soldier(_, t, _) | ShotHit::World(t, _, _) => *t,
                                _ => w.range * 2.0,
                            };
                            let at = eye + d * 55.0 - Vec3::Z * 18.0;
                            let end = eye + dd * dist;
                            let dir = (end - at).normalize_or_zero();
                            let rot = Quat::from_rotation_z(dir.y.atan2(dir.x)) * Quat::from_rotation_y(-dir.z.clamp(-1.0, 1.0).asin());
                            // combat::fire_tracer: random yellow / red / green and a light (user request)
                            crate::combat::fire_tracer(&mut fx, tc, at, rot, dir, end.distance(at) / tc.speed.max(1.0));
                        }
                    }
                    // impact effect (impacts.rs)
                    if let Some(im) = impacts.as_ref() {
                        let center = match &shot {
                            ShotHit::Soldier(te, ..) => others.iter().find(|o| o.0 == *te).map(|o| o.1),
                            ShotHit::Player(..) => player,
                            _ => None,
                        };
                        im.shot(&shot, eye, dd, &w.dmg_type, center, &mut fx, &mut sfx);
                    }
                    match shot {
                        ShotHit::Player(t, hit) if s.team != crate::combat::player_team() => {
                            let (dmg, head) = crate::combat::bullet_damage(&data.grid, w, player.unwrap_or(hit), hit, t);
                            queue.0.push(Hit { target: Target::Player, amount: dmg, attacker: Actor::Bot(e), attacker_team: s.team, headshot: head, explosive: false, from: eye, icon: w.ui_icon.clone(), dmg_type: Some(w.dmg_type.clone()) });
                        }
                        ShotHit::Soldier(te, t, hit) => {
                            if let Some(o) = others.iter().find(|o| o.0 == te) {
                                if o.2 != s.team {
                                    let (dmg, head) = crate::combat::bullet_damage(&data.grid, w, o.1, hit, t);
                                    queue.0.push(Hit { target: Target::Bot(te), amount: dmg, attacker: Actor::Bot(e), attacker_team: s.team, headshot: head, explosive: false, from: eye, icon: w.ui_icon.clone(), dmg_type: None });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        } else {
            b.seen_for = 0.0;
            b.brain.ads = (b.brain.ads - dt / w.ads_down.max(0.05)).max(0.0);
            if b.clip < w.clip / 2 && b.reload <= 0.0 {
                b.reload = w.reload_time;
            }
        }
        let _ = TEAM_KVA;
    }
    if std::env::var_os("HB_BOTDBG").is_some() {
        static LAST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let t = time.elapsed_secs() as u32;
        if LAST.swap(t, std::sync::atomic::Ordering::Relaxed) != t && t % 3 == 0 {
            for (_, b, s) in &bots {
                info!("bot {} team {} hp {:.0} feet {:?} path {} goal {:?} target {:?} anim {} vel {:.2}", s.name, s.team, s.health, s.feet, b.path.len(), b.goal, b.target, b.anim.0, b.vel.length());
            }
            if let Some(p) = player {
                info!("player centre {:?} nav {}", p, nav.grid.is_some());
            }
        }
    }
}

fn pack(v: Vec3, w: u8) -> [u8; 4] {
    let c = |x: f32| ((x.clamp(-1.0, 1.0) + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
    [c(v.x), c(v.y), c(v.z), w]
}

/// Animation for the bot's state: (sequence, looped, playback rate).
fn choose_anim(b: &Bot, s: &Soldier) -> (&'static str, bool, f32) {
    if let State::Dead(_) = b.state {
        const DEATHS: [&str; 10] = ["Death_1", "Death_2", "Death_3", "Death_4", "Death_5", "Death_6", "Death_7", "Death_8", "Death_9", "Death_10"];
        return (DEATHS[(b.death - 1) % 10], false, 1.0);
    }
    let v = Vec3::new(b.vel.x, 0.0, b.vel.z);
    let speed = v.length();
    if speed < 0.3 {
        return (if s.crouched { "crouch_idle" } else { "stand_idle" }, true, 1.0);
    }
    if b.sprint {
        return ("mp_sprint_F", true, (speed / (2.6 * 1.33)).clamp(0.5, 1.5));
    }
    // movement direction relative to facing (UE yaw; Bevy x = UE x, z = UE y)
    let ang = v.z.atan2(v.x) - b.yaw;
    let a = ang.rem_euclid(std::f32::consts::TAU);
    let sector = ((a / (std::f32::consts::TAU / 8.0)) + 0.5).floor() as i32 % 8;
    let name = match sector {
        0 => "run_front_ads_us",
        1 => "run_frontright_ads_us",
        2 => "run_right_ads_us",
        3 => "run_backright_ads_us",
        4 => "run_back_ads_us",
        5 => "run_backleft_ads_us",
        6 => "run_left_ads_us",
        _ => "run_frontleft_ads_us",
    };
    (name, true, (speed / 2.6).clamp(0.5, 1.5))
}

/// HB_BOTCAM=<slot> (tests), read once: it was looked up for every re-skinned bot each frame.
static BOTCAM: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();

#[allow(clippy::too_many_arguments)]
fn render_bots(
    mut commands: Commands,
    time: Res<Time>,
    flow: Res<MatchFlow>,
    (pawns, pc, mut kill_cam, mut body_init, col): (Query<&crate::player::Pawn>, Res<crate::combat::PlayerCombat>, Local<Option<(usize, f32, Vec3, bool)>>, Local<bool>, Res<CollisionWorld>),
    assets: Option<Res<BotAssets>>,
    mut bots: Query<(&mut Bot, &Soldier)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ents: Query<(Entity, &Mesh3d, &mut Visibility, &mut Transform), Without<Camera3d>>,
    mut cams: Query<&mut Transform, (With<Camera3d>, Without<Mesh3d>, Without<crate::minimap::MinimapCam>)>,
    (mut active, mut player_rag, mut rag_buf, mut for_assets): (Local<Vec<bool>>, Local<Option<crate::ragdoll::Ragdoll>>, Local<Vec<u32>>, Local<Option<AssetId<Mesh>>>),
    me: Res<crate::MeshEntities>,
    mut frame_no: Local<u32>,
) {
    let _perf = crate::perf::scope("render_bots");
    *frame_no = frame_no.wrapping_add(1);
    let Some(assets) = assets else { return };
    // rig debug (F3) slows or freezes the animation clock
    let dt = time.delta_secs().min(0.1) * crate::rigdebug::anim_scale();
    // first run, or the next match's bodies: everything is hidden below (spawned visible at the
    // origin)
    let id = assets.slots.first().and_then(|s| s.body.meshes.first()).map(|h| h.id());
    if active.len() != assets.slots.len() || *for_assets != id {
        *active = vec![true; assets.slots.len()];
        *for_assets = id;
        // the player's own body (kill cam) is hidden once per asset set too
        *body_init = false;
    }
    let mut now = vec![false; assets.slots.len()];
    // skinning budget (NOT from data): bots behind the camera keep their last mesh, bots beyond
    // 40 m are re-skinned every second frame
    let cam = cams.iter().next().map(|t| (t.translation, t.forward().as_vec3()));
    if flow.phase == Phase::Playing || flow.phase == Phase::Spawn {
        for (mut b, s) in &mut bots {
            let b = &mut *b;
            // in a vehicle (botvehicles.rs): the body is not drawn (NOT modelled: occupants)
            if s.alive && crate::botextras::hidden_slot(b.slot) {
                continue;
            }
            now[b.slot] = true;
            let slot = &assets.slots[b.slot];
            let rig = &assets.rigs[slot.rig];
            // dead: ragdoll from the pose of the moment of death, pushed away from the hit
            if let State::Dead(_) = b.state {
                if b.ragdoll.is_none() {
                    let local = if b.last_local.is_empty() { rig.local(&assets.lib, "stand_idle", 0.0, true) } else { b.last_local.clone() };
                    let pose = rig.compose(&local);
                    let origin = feet_ue(s.feet) + Vec3::Z * (crate::combat::CYL_HALF + MESH_Z);
                    let vel = ragdoll_push(s.last_hit, s.center_ue());
                    b.ragdoll = Some(Box::new(crate::ragdoll::Ragdoll::new(&rig.skeleton, &pose, origin, Quat::from_rotation_z(b.yaw), vel)));
                }
                let rd = b.ragdoll.as_mut().unwrap();
                let awake = !rd.asleep();
                if awake || !b.shown {
                    rd.step(&col, dt, &mut rag_buf);
                    let pose = rd.pose();
                    let tr = Vec3::new(rd.origin.x, rd.origin.z, rd.origin.y) * 0.01;
                    for part in std::iter::once(&slot.body).chain(slot.weapon.iter()) {
                        skin_part(part, &pose, rd.rot, rd.origin, &mut meshes);
                        for e in crate::mesh_ents(&me, &part.meshes) {
                            if let Ok((_, _, _, mut tf)) = ents.get_mut(e) {
                                if tf.translation != tr {
                                    tf.translation = tr;
                                }
                            }
                        }
                    }
                    b.shown = true;
                }
                continue;
            }
            // animation state with a 0.2 s cross-fade
            // nuke.rs bot mode: the caller hangs on the Fulton line (place, rotation and sequence
            // from nuke.rs towed_pose; NOT from data)
            let towed = crate::nuke::towed_pose(b.slot);
            let (name, looped, mut rate) = towed.map(|tp| (tp.anim, tp.looped, 1.0)).unwrap_or_else(|| choose_anim(b, s));
            // playback rate so the feet match the ground speed: the sequence's own root motion
            // per cycle (uu) against the pawn's speed
            if looped {
                if let Some((dist, len)) = assets.lib.root_cycle(name) {
                    let speed = Vec3::new(b.vel.x, 0.0, b.vel.z).length() * 100.0;
                    if speed > 30.0 {
                        rate = (speed * len / dist).clamp(0.4, 2.0);
                    }
                }
            }
            if b.anim.0 != name {
                if !b.last_local.is_empty() {
                    b.blend_from = Some((b.last_local.clone(), 0.0));
                }
                b.anim = (name, 0.0, looped);
            }
            b.anim.1 += dt * rate;
            let mut local = rig.local(&assets.lib, b.anim.0, b.anim.1, b.anim.2);
            if let Some((from, t)) = b.blend_from.as_mut() {
                *t += dt / 0.2;
                let k = t.min(1.0);
                for (l, f) in local.iter_mut().zip(from.iter()) {
                    l.0 = f.0.slerp(l.0, k);
                    l.1 = f.1.lerp(l.1, k);
                }
            }
            if b.blend_from.as_ref().is_some_and(|x| x.1 >= 1.0) {
                b.blend_from = None;
            }
            b.last_local = local.clone();
            let pos_b = s.feet + Vec3::Y * 0.9;
            let skip = towed.is_none() && cam.is_some_and(|(cp, cf)| {
                let d = pos_b - cp;
                let dist = d.length();
                // behind the camera only while standing still: a moving bot kept its old mesh
                // and jumped to its position when the view turned to it ("teleporting")
                let still = s.feet.distance(b.skinned_at) < 0.05;
                // re-skin cadence by distance: every frame within 15 m, every 2nd frame to 40 m,
                // every 3rd beyond (render_bots 5.1 ms per frame in the user's profile
                // 2026-10-18; NOT from data)
                let every = if dist < 15.0 { 1 } else if dist < 40.0 { 2 } else { 3 };
                // outside the view cone (75 deg half angle, wider than the view's diagonal): skip
                // even while moving, the mesh only follows by its Transform below (allocate_and_
                // free_meshes 3.7 ms for 160 re-skinned meshes per frame in the user's profile
                // 2026-10-19; NOT from data)
                let _ = still;
                (dist > 3.0 && d.dot(cf) < 0.25 * dist && b.shown) || (b.shown && (*frame_no + b.slot as u32) % every != 0)
            });
            if skip {
                now[b.slot] = true;
                // moved since the last skin: the old pose follows (no "teleport" once in view)
                if s.feet != b.skinned_at {
                    let d = s.feet - b.skinned_at;
                    let last = feet_ue(b.skinned_at) + Vec3::Z * (crate::combat::CYL_HALF + MESH_Z);
                    let tr = Vec3::new(last.x, last.z, last.y) * 0.01 + d;
                    for part in std::iter::once(&slot.body).chain(slot.weapon.iter()) {
                        for e in crate::mesh_ents(&me, &part.meshes) {
                            if let Ok((_, _, _, mut tf)) = ents.get_mut(e) {
                                if tf.translation != tr {
                                    tf.translation = tr;
                                }
                            }
                        }
                    }
                }
                continue;
            }
            let pose = rig.compose(&local);
            // component -> world: mesh (z, -x, -y) is (forward, right, up); yaw about Z
            let (origin, rot) = match towed {
                Some(tp) => (tp.feet_ue + tp.rot * (Vec3::Z * (crate::combat::CYL_HALF + MESH_Z)), tp.rot),
                None => (feet_ue(s.feet) + Vec3::Z * (crate::combat::CYL_HALF + MESH_Z), Quat::from_rotation_z(b.yaw)),
            };
            let tr = Vec3::new(origin.x, origin.z, origin.y) * 0.01;
            for part in std::iter::once(&slot.body).chain(slot.weapon.iter()) {
                skin_part(part, &pose, rot, origin, &mut meshes);
                for e in crate::mesh_ents(&me, &part.meshes) {
                    if let Ok((_, _, _, mut tf)) = ents.get_mut(e) {
                        if tf.translation != tr {
                            tf.translation = tr;
                        }
                    }
                }
            }
            b.shown = true;
            b.skinned_at = s.feet;
            // HB_BOTCAM=<slot>: chase camera behind that bot (tests)
            if *BOTCAM.get_or_init(|| std::env::var("HB_BOTCAM").ok().and_then(|v| v.parse::<usize>().ok())) == Some(b.slot) {
                if let Ok(mut ct) = cams.single_mut() {
                    let head = s.feet + Vec3::Y * 1.6;
                    let back = Vec3::new(b.yaw.cos(), 0.0, b.yaw.sin());
                    ct.translation = head - back * 3.0 + Vec3::Y * 0.6;
                    ct.look_at(head, Vec3::Y);
                }
            }
        }
    }
    // kill cam (PC_U_KillCamScreen): the player's body plays a death sequence where it fell (the
    // game's ragdoll is physics, NOT modelled: Death_Running_3P sequences instead), seen from
    // behind and above (screenshot: camera ~3 m back, ~2.5 m up, looking down at the body)
    let dead = (flow.phase == Phase::Playing) && pawns.single().is_ok_and(|p| p.dead && !p.in_vehicle);
    if let (Some(body), Ok(p)) = (assets.player.as_ref(), pawns.single()) {
        if dead {
            let k = kill_cam.get_or_insert_with(|| {
                let pick = (time.elapsed_secs() * 7.31) as usize % 10;
                (pick, 0.0, p.feet, true)
            });
            k.1 += dt;
            let rig = &assets.rigs[body.rig];
            let yaw = p.yaw / 65536.0 * std::f32::consts::TAU;
            // ragdoll (ragdoll.rs) from the standing pose where the player fell
            if player_rag.is_none() {
                let local = rig.local(&assets.lib, "stand_idle", 0.0, true);
                let pose = rig.compose(&local);
                let origin = feet_ue(k.2) + Vec3::Z * (crate::combat::CYL_HALF + MESH_Z);
                let center = feet_ue(k.2) + Vec3::Z * crate::combat::CYL_HALF;
                let vel = ragdoll_push(pc.killer.as_ref().map(|x| (x.2, pc.death_explosive)), center);
                *player_rag = Some(crate::ragdoll::Ragdoll::new(&rig.skeleton, &pose, origin, Quat::from_rotation_z(yaw), vel));
            }
            let rd = player_rag.as_mut().unwrap();
            rd.step(&col, dt, &mut rag_buf);
            if std::env::var_os("HB_RAGDBG").is_some() {
                info!("RAGDBG t {:.2} {}", rd.t, rd.debug());
            }
            let pose = rd.pose();
            let (rrot, origin) = (rd.rot, rd.origin);
            let tr = Vec3::new(origin.x, origin.z, origin.y) * 0.01;
            for part in std::iter::once(&body.body).chain(body.weapon.iter()) {
                skin_part(part, &pose, rrot, origin, &mut meshes);
                for e in crate::mesh_ents(&me, &part.meshes) {
                    if let Ok((e, _, mut v, mut tf)) = ents.get_mut(e) {
                        tf.translation = tr;
                        if *v != Visibility::Visible {
                            *v = Visibility::Visible;
                            commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                        }
                    }
                }
            }
            // (not while the nuke's white-out / far view holds the camera)
            if let Ok(mut ct) = cams.single_mut().map_err(|_| ()).and_then(|c| if crate::nuke::camera_owned() { Err(()) } else { Ok(c) }) {
                let target = k.2 + Vec3::Y * 0.5;
                let back = Vec3::new(yaw.cos(), 0.0, yaw.sin());
                let mut want = target - back * 3.0 + Vec3::Y * 2.5;
                // keep the camera out of walls: stop short of the first hit from the body
                let d = want - target;
                let len = d.length();
                let mut rb = Vec::new();
                if let Some((t, _)) = col.raycast(target, d / len.max(1e-3), len, &mut rb) {
                    want = target + d / len.max(1e-3) * (t - 0.25).max(0.3);
                }
                ct.translation = want;
                ct.look_at(target, Vec3::Y);
            }
            k.3 = true;
        } else if kill_cam.take().is_some() || !*body_init {
            *player_rag = None;
            *body_init = true;
            let ids: Vec<AssetId<Mesh>> = std::iter::once(&body.body).chain(body.weapon.iter()).flat_map(|p| p.meshes.iter().map(|h| h.id())).collect();
            for (_, mh, mut v, _) in &mut ents {
                if ids.contains(&mh.0.id()) {
                    *v = Visibility::Hidden;
                }
            }
        }
    }
    let _ = &pc;
    for (i, slot) in assets.slots.iter().enumerate() {
        if now[i] == active[i] {
            continue;
        }
        active[i] = now[i];
        let ids: Vec<AssetId<Mesh>> = std::iter::once(&slot.body).chain(slot.weapon.iter()).flat_map(|p| p.meshes.iter().map(|h| h.id())).collect();
        for (e, mh, mut v, _) in &mut ents {
            if ids.contains(&mh.0.id()) {
                *v = if now[i] { Visibility::Visible } else { Visibility::Hidden };
                commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
            }
        }
    }
}

/// Start velocity of a ragdoll (UE uu/s) away from the killing hit: 250 for bullets, 600 + 350
/// up for explosions (NOT from data: KCharacterRagdollImpulse / KDeathUpKick of the damage
/// types act on the PhysX masses, which are not modelled).
pub fn ragdoll_push(hit: Option<(Vec3, bool)>, center: Vec3) -> Vec3 {
    let Some((from, explosive)) = hit else { return Vec3::ZERO };
    let mut d = center - from;
    d.z = d.z.max(0.0);
    let d = d.normalize_or_zero();
    if explosive {
        d * 600.0 + Vec3::Z * 350.0
    } else {
        Vec3::new(d.x, d.y, 0.0).normalize_or_zero() * 250.0
    }
}

/// CPU skinning of a 3P part into world space (positions relative to `origin`).
pub fn skin_part(part: &SkinPart, pose: &[(Quat, Vec3)], rot: Quat, origin: Vec3, meshes: &mut Assets<Mesh>) {
    skin_part_scaled(part, pose, rot, origin, meshes, None)
}

/// `skin_part` with a scale per pose bone (0 collapses a bone's vertices onto its pivot: the
/// drones' GCSkelControl_HeliRotorSwap BoneScale).
pub fn skin_part_scaled(part: &SkinPart, pose: &[(Quat, Vec3)], rot: Quat, origin: Vec3, meshes: &mut Assets<Mesh>, scales: Option<&[f32]>) {
    crate::perf::count("bot_parts_skinned", 1);
    let skin: Vec<(Quat, Vec3)> = part
        .mesh
        .bones
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let cur = part.bone_map[i].and_then(|d| pose.get(d).copied()).unwrap_or(part.ref_comp[i]);
            let (rr, rp) = part.ref_comp[i];
            let inv = rr.conjugate();
            (cur.0 * inv, cur.1 - cur.0 * (inv * rp))
        })
        .collect();
    // per bone: world rotation * (UE mesh -> component axes) * skin rotation as one matrix,
    // translation alike (rel = sum w (A v + c)); vertices in parallel chunks
    let comp = Mat3A::from_cols(Vec3A::new(0.0, -1.0, 0.0), Vec3A::new(0.0, 0.0, -1.0), Vec3A::new(1.0, 0.0, 0.0));
    let rm = Mat3A::from_quat(rot) * comp;
    let mats: Vec<(Mat3A, Vec3A)> = skin
        .iter()
        .enumerate()
        .map(|(i, (r, t))| {
            let sc = scales.and_then(|sv| part.bone_map[i].and_then(|d| sv.get(d).copied())).unwrap_or(1.0);
            if sc != 1.0 {
                // scaled about the bone pivot: v -> pivot + s * (R v + T - pivot)
                let pivot = pose.get(part.bone_map[i].unwrap_or(0)).map(|p| Vec3A::from(p.1)).unwrap_or(Vec3A::ZERO);
                (rm * (Mat3A::from_quat(*r) * sc), rm * (pivot + (Vec3A::from(*t) - pivot) * sc))
            } else {
                (rm * Mat3A::from_quat(*r), rm * Vec3A::from(*t))
            }
        })
        .collect();
    let ident = (rm, Vec3A::ZERO);
    let o = Vec3A::from(origin);
    use bevy::tasks::{ComputeTaskPool, ParallelSlice};
    const CHUNK: usize = 1024;
    let chunks = part.mesh.lod.positions.par_chunk_map(ComputeTaskPool::get(), CHUNK, |ci, chunk| {
        let mut ue = Vec::with_capacity(chunk.len());
        let mut bv = Vec::with_capacity(chunk.len());
        let mut tx = Vec::with_capacity(chunk.len());
        let mut tz = Vec::with_capacity(chunk.len());
        for (j, p) in chunk.iter().enumerate() {
            let vi = ci * CHUNK + j;
            let v = Vec3A::from(*p);
            let (bi, wt) = part.mesh.influences[vi];
            let (mut rel, mut ra, mut rz) = (Vec3A::ZERO, Vec3A::ZERO, Vec3A::ZERO);
            let (t0, t1) = (Vec3A::from(part.tx[vi]), Vec3A::from(part.tz[vi].0));
            for k in 0..4 {
                if wt[k] == 0 {
                    continue;
                }
                let wf = wt[k] as f32 / 255.0;
                let (m, c) = mats.get(bi[k] as usize).copied().unwrap_or(ident);
                rel += (m * v + c) * wf;
                ra += (m * t0) * wf;
                rz += (m * t1) * wf;
            }
            ue.push((o + rel).to_array());
            bv.push([rel.x * 0.01, rel.z * 0.01, rel.y * 0.01]);
            tx.push(pack(Vec3::from(ra).normalize_or_zero(), 255));
            tz.push(pack(Vec3::from(rz).normalize_or_zero(), part.tz[vi].1));
        }
        (ue, bv, tx, tz)
    });
    let n = part.mesh.lod.positions.len();
    let (mut ue, mut bv, mut tx, mut tz) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for (a, b, c, d) in chunks {
        ue.extend(a);
        bv.extend(b);
        tx.extend(c);
        tz.extend(d);
    }
    for (k, h) in part.meshes.iter().enumerate() {
        crate::viewmodel::write_skinned(meshes, h, part.maps.get(k), &bv, &ue, &tx, &tz);
    }
}

/// Skeleton bones without a track in animation set `si` (rig debug).
pub(crate) fn rig_missing_tracks(r: &BodyRig, si: usize) -> Vec<String> {
    r.skeleton.iter().enumerate().filter(|(i, _)| r.tracks.get(si).and_then(|t| t[*i]).is_none()).map(|(_, b)| b.name.clone()).collect()
}

/// Rig debug view of a bot (the pose of its last animation update, or its ragdoll).
pub(crate) fn rig_view(assets: &BotAssets, b: &Bot, s: &Soldier) -> crate::rigdebug::RigView {
    let slot = &assets.slots[b.slot];
    let rig = &assets.rigs[slot.rig];
    let parents: Vec<i32> = rig.skeleton.iter().map(|x| x.parent).collect();
    let (comp, rot, origin, mode) = match (&b.state, b.ragdoll.as_ref()) {
        (State::Dead(_), Some(rd)) => (rd.pose(), rd.rot, rd.origin, format!("ragdoll: {}", rd.debug())),
        _ => {
            let local = if b.last_local.len() == rig.skeleton.len() { b.last_local.clone() } else { rig.local(&assets.lib, "stand_idle", 0.0, true) };
            (rig.compose(&local), Quat::from_rotation_z(b.yaw), feet_ue(s.feet) + Vec3::Z * (crate::combat::CYL_HALF + MESH_Z), "animated".to_string())
        }
    };
    let local = crate::rigdebug::local_from_comp(&parents, &comp);
    let map = |p: Vec3| {
        let ue = origin + rot * Vec3::new(p.z, -p.x, -p.y);
        Vec3::new(ue.x, ue.z, ue.y) * 0.01
    };
    let world = comp.iter().map(|c| map(c.1)).collect();
    let axes = comp.iter().map(|&(r, t)| [map(t + r * Vec3::X * 6.0), map(t + r * Vec3::Y * 6.0), map(t + r * Vec3::Z * 6.0)]).collect();
    let found = assets.lib.find(b.anim.0);
    let track = match found {
        Some((si, _)) => rig.tracks[si].clone(),
        None => vec![None; rig.skeleton.len()],
    };
    let seq = found.map(|(si, qi)| &assets.lib.sets[si].seqs[qi]);
    let seq_t = seq.map(|q| if b.anim.2 { b.anim.1.rem_euclid(q.length.max(1e-3)) } else { b.anim.1.min(q.length) }).unwrap_or(b.anim.1);
    let info = vec![
        format!("{} team {} slot {} rig {} ({} bones), {:?}, {mode}", s.name, s.team, b.slot, slot.rig, rig.skeleton.len(), b.state),
        format!(
            "anim {} t {:.3} (raw {:.3}) looped {}  set {}  len {:.3} rate {:.2} frames {}  root cycle {:?}",
            b.anim.0,
            seq_t,
            b.anim.1,
            b.anim.2,
            found.map(|(si, _)| assets.lib.sets[si].path.as_str()).unwrap_or("-"),
            seq.map(|q| q.length).unwrap_or(0.0),
            seq.map(|q| q.rate).unwrap_or(0.0),
            seq.map(|q| q.frames).unwrap_or(0),
            assets.lib.root_cycle(b.anim.0)
        ),
        format!(
            "blend {}  yaw {:.1}° pitch {:.1}°  vel {:.2} m/s  sprint {}  feet {:.2} {:.2} {:.2}",
            b.blend_from.as_ref().map(|x| format!("{:.2}", x.1)).unwrap_or("-".into()),
            b.yaw.to_degrees(),
            b.pitch.to_degrees(),
            Vec3::new(b.vel.x, 0.0, b.vel.z).length(),
            b.sprint,
            s.feet.x,
            s.feet.y,
            s.feet.z
        ),
    ];
    let mut parts = vec![crate::viewmodel::part_info(&slot.body, "body", &rig.skeleton)];
    if let Some(w) = &slot.weapon {
        parts.push(crate::viewmodel::part_info(w, "weapon", &rig.skeleton));
    }
    crate::rigdebug::RigView {
        label: format!("Bot {}", s.name),
        names: rig.skeleton.iter().map(|x| x.name.clone()).collect(),
        parents,
        reference: rig.skeleton.iter().map(|x| (q(x.rot), Vec3::from(x.pos))).collect(),
        local,
        comp,
        track,
        world,
        axes,
        seq: b.anim.0.to_string(),
        seq_t,
        info,
        parts,
    }
}


/// HB_NAVTEST=<n>: once the walk grid exists, n bot walks between random nodes of the main
/// component (20-80 m apart) are simulated with the bots' walker and steering; logs how many
/// arrived, got stuck, or ran out of time. HB_NAVTEST_OLD=1 smooths the paths the old way
/// (centre line only) for comparison.
fn nav_selftest(nav: Res<Nav>, col: Res<CollisionWorld>, mut done: Local<bool>) {
    let Ok(n) = std::env::var("HB_NAVTEST").map(|v| v.parse::<usize>().unwrap_or(100)) else { return };
    if *done {
        return;
    }
    let Some(grid) = nav.grid.clone() else { return };
    *done = true;
    let Some(main) = grid.main_comp() else { return };
    let nodes: Vec<u32> = (0..grid.nodes.len() as u32).filter(|&k| grid.comp[k as usize] == main).collect();
    let mut seed = 0x1234_5678u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let old = std::env::var_os("HB_NAVTEST_OLD").is_some();
    let (mut arrived, mut stuck_runs, mut timeouts, mut stuck_events, mut total_t) = (0, 0, 0, 0, 0.0f32);
    let mut buf = (Vec::new(), Vec::new());
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        let a = grid.nodes[nodes[rnd() as usize % nodes.len()] as usize];
        let mut b = a;
        for _ in 0..50 {
            let c = grid.nodes[nodes[rnd() as usize % nodes.len()] as usize];
            let d = Vec2::new(c.x - a.x, c.z - a.z).length();
            if (20.0..80.0).contains(&d) {
                b = c;
                break;
            }
        }
        if b == a {
            continue;
        }
        let (mut path, ok) = if old {
            // centre-line smoothing (before 2026-10-19)
            let za = grid.nearest(a).unwrap();
            let zb = grid.nearest(b).unwrap();
            // the wall cost cancelled: the old A* costs
            let unwall: Vec<f32> = grid.wall.iter().map(|w| -w).collect();
            let p = grid.path(za, zb, 150_000, &unwall).unwrap_or_default();
            let pts: Vec<Vec3> = p.iter().map(|&k| grid.nodes[k as usize]).collect();
            let mut out = Vec::new();
            let mut anchor = a;
            let mut i = 0;
            while i < pts.len() {
                let mut j = (i + 12).min(pts.len() - 1);
                while j > i && !crate::nav::clear(&col, anchor, pts[j], &mut buf.0) {
                    j -= 1;
                }
                out.push(pts[j]);
                anchor = pts[j];
                i = j + 1;
            }
            (out, !p.is_empty())
        } else {
            plan_path_ex(&nav, &col, a, b, rnd(), &mut buf.0)
        };
        if !ok {
            continue;
        }
        let mut feet = a + Vec3::Y * 0.05;
        let (mut vel, mut on_ground) = (Vec3::ZERO, true);
        let dt = 1.0 / 60.0;
        let limit = Vec2::new(b.x - a.x, b.z - a.z).length() / crate::player::BOT_GROUND_SPEED * 3.0 + 5.0;
        let (mut t, mut stuck, mut last, mut events) = (0.0f32, 0.0f32, feet, 0);
        let mut result = 0;
        while t < limit {
            let mut dir = Vec3::ZERO;
            while let Some(&wp) = path.first() {
                let d = Vec3::new(wp.x - feet.x, 0.0, wp.z - feet.z);
                if d.length() < 0.6 && path.len() > 1 {
                    path.remove(0);
                    continue;
                }
                if path.len() == 1 && d.length() < 0.8 {
                    path.clear();
                    break;
                }
                dir = d.normalize_or_zero();
                break;
            }
            if path.is_empty() {
                result = 1;
                break;
            }
            crate::player::walk(&col, &mut feet, &mut vel, &mut on_ground, false, dir, crate::player::BOT_GROUND_SPEED, false, dt, &mut buf);
            if feet.distance(last) < 0.3 * dt {
                stuck += dt;
            } else {
                stuck = 0.0;
            }
            last = feet;
            if stuck > 1.0 {
                events += 1;
                stuck = 0.0;
                if events >= 3 {
                    result = 2;
                    if std::env::var_os("HB_NAVTEST_V").is_some() {
                        info!("navtest stuck at {:.1} {:.1} {:.1} next {:?} left {}", feet.x, feet.y, feet.z, path.first(), path.len());
                    }
                    break;
                }
            }
            t += dt;
        }
        total_t += t;
        stuck_events += events;
        match result {
            1 => arrived += 1,
            2 => stuck_runs += 1,
            _ => timeouts += 1,
        }
    }
    info!(
        "navtest ({}): {n} walks: arrived {arrived}, stuck (3x) {stuck_runs}, timeout {timeouts}, stuck events {stuck_events}, sim {:.0} s in {:.1} s",
        if old { "old smoothing" } else { "walkable smoothing + wall cost" },
        total_t,
        t0.elapsed().as_secs_f32()
    );
}

/// Material a body section had before thermal vision swapped it.
#[derive(Component)]
pub struct ThermalOrig(Handle<crate::gamemat::GameMat>);

/// Thermal vision (Hellfire feed, thermal goggles): the enemies' bodies are drawn with
/// GCObjectPool_Content CharacterThermalMaterial = CH_3P.M_Character_Thermal_White (unlit,
/// EmissiveColor = VectorParameter Color 1,1,1), which M_Thermal01 shows as white heat.
fn thermal_bodies(
    fx: Res<crate::videofx::VideoFx>,
    assets: Option<Res<BotAssets>>,
    mut q: Query<(Entity, &Mesh3d, &mut MeshMaterial3d<crate::gamemat::GameMat>, Option<&ThermalOrig>)>,
    mut commands: Commands,
    mut was: Local<bool>,
) {
    if fx.thermal == *was {
        return;
    }
    let Some(assets) = assets else { return };
    let Some(white) = assets.thermal.clone() else { return };
    *was = fx.thermal;
    // only the enemies glow (user 2026-10-19: team mates without heat signature, NOT from data:
    // the game draws every character with the thermal material)
    let bodies: std::collections::HashSet<AssetId<Mesh>> = assets.slots.iter().filter(|s| s.team != crate::combat::player_team()).flat_map(|s| s.body.meshes.iter().map(|h| h.id())).collect();
    let mut n = 0;
    for (e, mesh, mut mat, orig) in &mut q {
        if !bodies.contains(&mesh.0.id()) {
            continue;
        }
        n += 1;
        match (fx.thermal, orig) {
            (true, None) => {
                commands.entity(e).insert(ThermalOrig(mat.0.clone()));
                mat.0 = white.clone();
            }
            (false, Some(o)) => {
                mat.0 = o.0.clone();
                commands.entity(e).remove::<ThermalOrig>();
            }
            _ => {}
        }
    }
    info!("bots: thermal vision {} ({n} body sections)", if fx.thermal { "on" } else { "off" });
}
