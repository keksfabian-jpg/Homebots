//! Destroyed vehicles: the wreck ("ragdoll") the game leaves when a GCVehicle dies.
//!
//! Data (read per vehicle class over the content / script class defaults, like vehload.rs):
//! - GCVehicle RagdollDefinitions (RagdollMesh, RagdollMaterials, RagdollAnimTree,
//!   VehicleEventsToFire) and RagdollDefinitionListStationary / ...Moving: the script
//!   (GCVehicle.CalculateRagdollUseMovingList: VSizeSq(Velocity) > 50 * 50,
//!   CalculateRagdollInfoFromDefinitions: a random entry of the list) picks one definition; the
//!   wreck is the vehicle mesh with the definition's *_DEST materials (M_Vehicle_DEST: soot,
//!   panning embers) and the anim tree's skeletal controls (SkelControlSingleBone BoneScale 0:
//!   doors, windshield, the team variant boxes blown off).
//! - Events (GCVehicle.Died: VehicleEvent('VehicleDestroyed'); SwitchToRagDoll: the definition's
//!   VehicleEventsToFire, e.g. TurretLaunch / EngineFlip): the mesh sockets' References with
//!   that StartTag (particles such as KFX_Pyrotechnics.FX_Explosions.Vehicle_Civilian and
//!   KFX_VHC.FX_Jeep.Fire_WreckChassis, sound cues such as A2_Destruction_C.SC_FireLoop_Metal,
//!   which run until their EndTag event) and the class's VehicleKismet
//!   (GCSeqEvent_VehicleEvents outputs -> SeqAct_PlaySound, e.g. SC_VehicleHusk).
//! - Timeline (GCVehicle.SetBurnOut / DyingVehicle.BeginState / WreckExplosionTimer):
//!   LifeSpan = RagdollLifespan; SetTimer(LifeSpan - m_fWreckExplosionStartTime,
//!   'WreckExplosionTimer') -> VehicleEvent('WreckExplosion') (FX_VehicleRemoval_*), the
//!   vehicle audio setup's SCWreckExplosion; the wreck is gone at the end of its LifeSpan.
//!   BurnOutTime / BurnTime material parameters: the *_DEST materials have no such parameter
//!   (M_Vehicle_DEST), so nothing to animate.
//! - DyingVehicle.InitialVehicleExplosion: HurtRadius(ExplosionDamage, ExplosionRadius,
//!   ExplosionDamageType, ExplosionMomentum, Location, ExplosionRadiusFull) on the soldiers
//!   around (GCVehicle defaults 100 / 200 / GCDmgType_VehicleExplosion / 60000 / 50).
//!
//! The PhysX rigid body of the wreck (PA_*_wreck physics assets, AddImpulse of the killing
//! hit's TearOffMomentum / Mass) is replaced by a point mass with a fixed launch (NOT from
//! data, see LAUNCH_*), gravity (DefaultGravityZ 750) and a ground stop on the CollisionWorld.
use crate::combat::{Actor, CombatData, DamageQueue, Hit, Soldier, Target, player_team};
use crate::player::{CollisionWorld, Pawn};
use crate::vehicles::{Vehicle, VehicleAssets};
use crate::viewmodel::SkinPart;
use bevy::prelude::*;
use upk::library::Library;
use upk::props::{Props, Value};
use upk::Package;

pub struct DestructionPlugin;

impl Plugin for DestructionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, test_kill.before(crate::player::PlayerSet))
            .add_systems(Update, (start_wrecks, update_wrecks, render_wrecks).chain().after(crate::combat::CombatSet));
    }
}

/// Wreck instances per vehicle mesh (NOT from data: pool size).
const POOL: usize = 2;
/// Launch of the wreck at the explosion (UE units / s, rad / s). NOT from data: the game
/// applies TearOffMomentum / Mass of the killing hit to the PhysX wreck body (masses of the
/// physics asset bodies not decoded); these are chosen to give a visible hop.
const LAUNCH_UP: f32 = 350.0;
const LAUNCH_ALONG_HIT: f32 = 150.0;
const LAUNCH_SPIN: f32 = 0.6;
/// Launch of the detached physics-asset bodies (turret, tail, barrel ...) relative to the wreck
/// (UE units / s, rad / s). NOT from data: the game breaks their constraints (LinearBreakThreshold
/// 10) in PhysX under the death impulse; these give a visible tear-off.
pub(crate) const PIECE_UP: f32 = 450.0;
pub(crate) const PIECE_OUT: f32 = 250.0;
pub(crate) const PIECE_SPIN: f32 = 2.5;
/// Default__WorldInfo DefaultGravityZ (UE units / s^2).
pub(crate) const GRAVITY: f32 = 750.0;
/// Ground contact: bounce and friction (NOT from data).
pub(crate) const BOUNCE: f32 = 0.25;
pub(crate) const GROUND_FRICTION: f32 = 3.0;

/// One mesh socket reference started by a vehicle event.
#[derive(Clone, Debug)]
pub struct SocketRef {
    pub start: String,
    pub end: String,
    pub socket: String,
    pub path: String,
    pub sound: bool,
}

#[derive(Clone, Debug)]
pub struct WreckDef {
    /// VehicleEventsToFire (lower case)
    pub events: Vec<String>,
    /// per skeleton bone scale (RagdollAnimTree SkelControlSingleBone BoneScale, inherited by
    /// the children like UE3 bone scaling)
    pub scales: Vec<f32>,
    /// per skeleton bone: 0 = the wreck body, k = detached piece k - 1 (RagdollAsset bodies
    /// with a breakable constraint or none, with the bodies welded to them by unbreakable ones)
    pub bone_piece: Vec<u8>,
    pub pieces: Vec<WreckPiece>,
}

/// A physics-asset body that tears off the wreck.
#[derive(Clone, Debug)]
pub struct WreckPiece {
    /// body bone name (logs)
    pub bone: String,
    /// centre of its vertices and their lowest z (vehicle-local UE units, reference pose)
    pub center: Vec3,
    pub low: f32,
    /// contact points / inertia relative to `center` (vehicle-local axes; piecephys.rs)
    pub hull: crate::piecephys::Hull,
}

/// Motion state of a detached piece: its frame (where the wreck frame was when it tore off),
/// kept in step with its rigid body (piecephys.rs: centroid = pos + rot * center).
#[derive(Clone, Debug)]
pub struct PieceState {
    pub pos: Vec3,
    pub rot: Quat,
    pub body: crate::piecephys::Body,
}

pub struct WreckKind {
    pub class: String,
    /// wreck mesh instances (RagdollMesh with RagdollMaterials), CPU-skinned
    pub slots: Vec<SkinPart>,
    /// per slot: the *wreck attachments (wingleftwreck, Mirror_Left_wreck, attachleftwreck ...)
    pub attach: Vec<Vec<SkinPart>>,
    pub defs: Vec<WreckDef>,
    pub stationary: Vec<usize>,
    pub moving: Vec<usize>,
    /// RagdollLifespan, m_fWreckExplosionStartTime
    pub lifespan: f32,
    pub wreck_explosion_start: f32,
    /// ExplosionDamage, ExplosionRadius, ExplosionRadiusFull, ExplosionDamageType
    pub explosion: (f32, f32, f32, String),
    pub socket_refs: Vec<SocketRef>,
    /// VehicleKismet: event (lower case) -> SeqAct_PlaySound cue, or "event:<name>" for a
    /// GCSeqAct_FireVehicleEvent
    pub kismet_sounds: Vec<(String, String)>,
    /// GCVehicleAudio VehicleAudioSetups[0].SCWreckExplosion
    pub wreck_sound: String,
}

/// Wreck data per VehicleAssets kind (same order).
#[derive(Resource, Default)]
pub struct WreckAssets {
    pub kinds: Vec<Option<WreckKind>>,
}

impl WreckAssets {
    /// Particle systems the wrecks fire (pooled by main.rs with the vehicle effects).
    pub fn templates(&self) -> Vec<String> {
        let mut v: Vec<String> = self.kinds.iter().flatten().flat_map(|k| k.socket_refs.iter().filter(|r| !r.sound).map(|r| r.path.clone())).collect();
        v.sort();
        v.dedup();
        v
    }
}

/// Spawned by vehicles.rs when a vehicle's health reaches 0 (state of the vehicle then).
#[derive(Component, Clone, Debug)]
pub struct WreckSpawn {
    pub kind: usize,
    /// UE units, radians (vehicles.rs body_rot convention)
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// UE units / s
    pub vel: Vec3,
    /// where the killing hit came from (UE), its attacker and team
    pub from: Vec3,
    pub attacker: Actor,
    pub team: u8,
}

#[derive(Component)]
pub struct Wreck {
    pub kind: usize,
    pub slot: usize,
    pub def: usize,
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub vel: Vec3,
    /// pitch / roll rates (rad / s)
    pub spin: Vec2,
    pub age: f32,
    pub resting: bool,
    pub wreck_exploded: bool,
    /// running effects: (socket ref index, particle id or None for a looping sound)
    pub running: Vec<(usize, Option<(usize, f64)>)>,
    pub serial: u32,
    /// detached pieces of the definition (WreckDef::pieces)
    pub pieces: Vec<PieceState>,
}

fn body_rot(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_rotation_z(yaw) * Quat::from_rotation_y(-pitch) * Quat::from_rotation_x(roll)
}

/// Mesh (bone) space -> UE vehicle axes (vehicles.rs mesh_to_ue).
fn mesh_to_ue(m: Vec3) -> Vec3 {
    Vec3::new(m.z, -m.x, -m.y)
}

// ---- loading -------------------------------------------------------------------------------

fn find(p: &Package, path: &str) -> Option<usize> {
    p.exports.iter().enumerate().position(|(i, _)| p.object_path(i as i32 + 1) == path)
}

/// Export of a class by full object path in any loaded package (cooked into a map: the full
/// path; its own package: the path below the package name).
fn locate<'a>(pkgs: &'a [Package], path: &str, class: &str) -> Option<(&'a Package, usize)> {
    let rest = path.split_once('.').map(|x| x.1).unwrap_or(path);
    pkgs.iter().find_map(|p| {
        let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == class && {
            let op = p.object_path(i as i32 + 1);
            op.eq_ignore_ascii_case(path) || op.eq_ignore_ascii_case(rest)
        })?;
        Some((p, i))
    })
}

fn class_chain(p: &Package, class: &str) -> (Vec<String>, Option<String>) {
    let mut out = Vec::new();
    let mut ci = p.exports.iter().position(|e| p.object_name_of(e) == class && p.export_class(e) == "Class").map(|x| x as i32 + 1).unwrap_or(0);
    for _ in 0..16 {
        if ci <= 0 {
            break;
        }
        out.push(p.object_name(ci));
        ci = p.exports[ci as usize - 1].super_index;
    }
    ((out), (ci < 0).then(|| p.object_name(ci)))
}

/// Class defaults over the chain: (content props most derived first, script props after).
/// Object references are resolved to paths right away (the props come from two packages).
fn defaults(content: &Package, gcgame: &Package, class: &str) -> Vec<(Props, bool)> {
    let (names, import) = class_chain(content, class);
    let mut v = Vec::new();
    for n in &names {
        if let Some(pr) = find(content, &format!("Default__{n}")).and_then(|i| content.export_props(i)) {
            v.push((pr, true));
        }
    }
    if let Some(script) = import {
        for n in class_chain(gcgame, &script).0 {
            if let Some(pr) = find(gcgame, &format!("Default__{n}")).and_then(|i| gcgame.export_props(i)) {
                v.push((pr, false));
            }
        }
    }
    v
}

fn first_float(d: &[(Props, bool)], name: &str) -> Option<f32> {
    d.iter().find_map(|(p, _)| p.float(name))
}

fn first_obj(d: &[(Props, bool)], content: &Package, gcgame: &Package, name: &str) -> Option<String> {
    d.iter().find_map(|(p, c)| p.object(name).filter(|&o| o != 0).map(|o| if *c { content.object_path(o) } else { gcgame.object_path(o) }))
}

fn names(p: &Package, v: Option<&Value>) -> Vec<String> {
    let Some(Value::Raw { data, .. }) = v else { return Vec::new() };
    let n = data.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).unwrap_or(0);
    (0..n)
        .filter_map(|k| {
            let b = data.get(4 + k * 8..12 + k * 8)?;
            Some(p.name(upk::FName { index: i32::from_le_bytes(b[0..4].try_into().unwrap()), number: i32::from_le_bytes(b[4..8].try_into().unwrap()) }))
        })
        .collect()
}

fn int_list(p: &Package, v: Option<&Value>) -> Vec<usize> {
    v.map(|v| p.array_objects(v).into_iter().filter(|&i| i >= 0).map(|i| i as usize).collect()).unwrap_or_default()
}

/// RagdollAnimTree: bone -> BoneScale of the SkelControlSingleBone controls on it.
fn tree_scales(pkgs: &[Package], tree: &str, skeleton: &[upk::skeletal::Bone]) -> Vec<f32> {
    let mut s = vec![1.0f32; skeleton.len()];
    let Some((p, ti)) = locate(pkgs, tree, "AnimTree") else {
        warn!("wreck: anim tree {tree} not found");
        return s;
    };
    let Some(tp) = p.export_props(ti) else { return s };
    for l in p.struct_array(&tp, "SkelControlLists") {
        let Some(b) = l.enum_name("BoneName").and_then(|n| skeleton.iter().position(|x| x.name.eq_ignore_ascii_case(n))) else { continue };
        let mut c = l.object("ControlHead").unwrap_or(0);
        for _ in 0..16 {
            if c <= 0 {
                break;
            }
            let Some(cp) = p.export_props(c as usize - 1) else { break };
            if let Some(sc) = cp.float("BoneScale") {
                s[b] = sc;
            }
            c = cp.object("NextControl").unwrap_or(0);
        }
    }
    // children inherit their parent's scale (UE3 bone scaling is part of the bone matrix)
    for i in 1..skeleton.len() {
        let par = skeleton[i].parent;
        if par >= 0 && (par as usize) < i && s[par as usize] == 0.0 {
            s[i] = 0.0;
        }
    }
    s
}

/// RagdollAsset (PhysicsAsset): which bodies tear off. RB_BodySetup BoneName per body,
/// RB_ConstraintSetup ConstraintBone1 (child body) / ConstraintBone2 (parent body) with
/// bLinearBreakable / bAngularBreakable. A body is a separate piece when its constraint is
/// breakable (e.g. PA_US_M1A3_wreck0 TURRET_ROOT / TURRET_BARREL, LinearBreakThreshold 10) or
/// when no constraint holds it (and it is not the root body); bodies held by an unbreakable
/// constraint move with their parent body (NOT from data: rigidly, joint limits ignored).
/// Returns per skeleton bone the piece (0 = wreck body) and the piece bone names.
fn asset_pieces(pkgs: &[Package], asset: &str, skeleton: &[upk::skeletal::Bone]) -> (Vec<u8>, Vec<String>) {
    let mut bone_piece = vec![0u8; skeleton.len()];
    let Some((p, ai)) = locate(pkgs, asset, "PhysicsAsset") else {
        warn!("wreck: physics asset {asset} not found");
        return (bone_piece, Vec::new());
    };
    let Some(ap) = p.export_props(ai) else { return (bone_piece, Vec::new()) };
    let bidx = |n: &str| skeleton.iter().position(|b| b.name.eq_ignore_ascii_case(n));
    let bodies: Vec<usize> = ap.get("BodySetup").map(|v| p.array_objects(v)).unwrap_or_default().into_iter().filter(|&o| o > 0).filter_map(|o| p.export_props(o as usize - 1)?.enum_name("BoneName").and_then(|n| bidx(n))).collect();
    // child bone -> (parent bone, breakable)
    let mut cons: Vec<(usize, Option<usize>, bool)> = Vec::new();
    for o in ap.get("ConstraintSetup").map(|v| p.array_objects(v)).unwrap_or_default().into_iter().filter(|&o| o > 0) {
        let Some(cp) = p.export_props(o as usize - 1) else { continue };
        let Some(b1) = cp.enum_name("ConstraintBone1").and_then(|n| bidx(n)) else { continue };
        let b2 = cp.enum_name("ConstraintBone2").and_then(|n| bidx(n));
        let brk = cp.bool("bLinearBreakable").unwrap_or(false) || cp.bool("bAngularBreakable").unwrap_or(false);
        cons.push((b1, b2, brk));
    }
    let depth = |mut b: usize| {
        let mut d = 0;
        while skeleton[b].parent >= 0 && (skeleton[b].parent as usize) != b && d < 256 {
            b = skeleton[b].parent as usize;
            d += 1;
        }
        d
    };
    let Some(&root) = bodies.iter().min_by_key(|&&b| depth(b)) else { return (bone_piece, Vec::new()) };
    // group of each body: its own piece, or its parent body's group
    let mut names = Vec::new();
    let mut group: std::collections::HashMap<usize, u8> = std::collections::HashMap::new();
    group.insert(root, 0);
    let mut order = bodies.clone();
    order.sort_by_key(|&b| depth(b));
    for &b in &order {
        if b == root {
            continue;
        }
        let c = cons.iter().find(|c| c.0 == b);
        let g = match c {
            Some(&(_, Some(par), false)) => group.get(&par).copied().unwrap_or(0),
            _ if names.len() < 250 => {
                names.push(skeleton[b].name.clone());
                names.len() as u8
            }
            _ => 0,
        };
        group.insert(b, g);
    }
    // every bone: the group of its nearest body (itself or an ancestor)
    for (i, bp) in bone_piece.iter_mut().enumerate() {
        let mut b = i;
        for _ in 0..256 {
            if let Some(&g) = group.get(&b) {
                *bp = g;
                break;
            }
            if skeleton[b].parent < 0 || skeleton[b].parent as usize == b {
                break;
            }
            b = skeleton[b].parent as usize;
        }
    }
    (bone_piece, names)
}

/// Centre and lowest point of each piece's vertices (dominant influence, visible bones),
/// vehicle-local UE units in the reference pose.
fn piece_bounds(part: &SkinPart, bone_piece: &[u8], scales: &[f32], names: &[String]) -> Vec<WreckPiece> {
    let n = names.len();
    let (mut sum, mut cnt, mut low) = (vec![Vec3::ZERO; n], vec![0u32; n], vec![f32::MAX; n]);
    let mut verts: Vec<Vec<Vec3>> = vec![Vec::new(); n];
    for (vi, p) in part.mesh.lod.positions.iter().enumerate() {
        let Some(&(bi, wt)) = part.mesh.influences.get(vi) else { continue };
        let k = (0..4).max_by_key(|&k| wt[k]).unwrap_or(0);
        let Some(sb) = part.bone_map.get(bi[k] as usize).copied().flatten() else { continue };
        if scales.get(sb).is_some_and(|&s| s == 0.0) {
            continue;
        }
        let g = bone_piece.get(sb).copied().unwrap_or(0) as usize;
        if g == 0 || g > n {
            continue;
        }
        let u = mesh_to_ue(Vec3::from(*p));
        sum[g - 1] += u;
        cnt[g - 1] += 1;
        low[g - 1] = low[g - 1].min(u.z);
        verts[g - 1].push(u);
    }
    (0..n).map(|i| {
        let c = if cnt[i] > 0 { sum[i] / cnt[i] as f32 } else { Vec3::ZERO };
        let rel: Vec<Vec3> = verts[i].iter().map(|v| *v - c).collect();
        WreckPiece { bone: names[i].clone(), center: c, low: if cnt[i] > 0 { low[i] } else { c.z }, hull: crate::piecephys::Hull::from_vertices(&rel, Quat::IDENTITY) }
    }).collect()
}

/// The vehicle audio setup's SCWreckExplosion (GCVehicleAudio cooked into the maps).
fn wreck_audio(lib: &Library, setup_path: &str) -> String {
    let Some(pk) = setup_path.split('.').next() else { return String::new() };
    let mut cands: Vec<(std::sync::Arc<Package>, bool)> = lib.open(pk).map(|p| (std::sync::Arc::new(p), true)).into_iter().collect();
    cands.extend(crate::sfx::cooked_packages(lib, pk).into_iter().map(|p| (p, false)));
    for (ap, own) in cands {
        let inner = if own { setup_path.split_once('.').map(|x| x.1).unwrap_or("").to_string() } else { setup_path.to_string() };
        let Some(i) = ap.exports.iter().enumerate().position(|(i, e)| ap.export_class(e) == "GCVehicleAudio" && ap.object_path(i as i32 + 1).eq_ignore_ascii_case(&inner)) else { continue };
        let Some(pr) = ap.export_props(i) else { continue };
        let Some(setup) = ap.struct_array(&pr, "VehicleAudioSetups").into_iter().next() else { continue };
        return setup.object("SCWreckExplosion").filter(|&o| o != 0).map(|o| if own { format!("{pk}.{}", ap.object_path(o)) } else { ap.object_path(o) }).unwrap_or_default();
    }
    String::new()
}

/// VehicleKismet: GCSeqEvent_VehicleEvents (VehicleEvents[i] -> OutputLinks[i]) linked to
/// SeqAct_PlaySound.
fn kismet_sounds(pkgs: &[Package], seq: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in pkgs {
        let prefix = format!("{}.", seq.to_ascii_lowercase());
        for (i, e) in p.exports.iter().enumerate() {
            if p.export_class(e) != "GCSeqEvent_VehicleEvents" || !p.object_path(i as i32 + 1).to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }
            let Some(pr) = p.export_props(i) else { continue };
            let evs = names(p, pr.get("VehicleEvents"));
            for (k, ol) in p.struct_array(&pr, "OutputLinks").iter().enumerate() {
                let Some(ev) = evs.get(k) else { continue };
                for l in p.struct_array(ol, "Links") {
                    let Some(op) = l.object("LinkedOp").filter(|&o| o > 0) else { continue };
                    let Some(sp) = p.export_props(op as usize - 1) else { continue };
                    match p.export_class(&p.exports[op as usize - 1]).as_str() {
                        "SeqAct_PlaySound" => {
                            if let Some(cue) = sp.object("PlaySound").filter(|&o| o != 0) {
                                out.push((ev.to_ascii_lowercase(), p.object_path(cue)));
                            }
                        }
                        // GCSeqAct_FireVehicleEvent: further events (TurretLaunch -> WheelPop)
                        "GCSeqAct_FireVehicleEvent" => {
                            for n in names(p, sp.get("VehicleEvents")) {
                                out.push((ev.to_ascii_lowercase(), format!("event:{}", n.to_ascii_lowercase())));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// Read the wreck data of every loaded vehicle kind; `spawn` places one wreck mesh instance
/// (mesh path, materials by mesh material slot) and returns its meshes.
pub fn load(lib: &Library, pkgs: &[Package], kinds: &[crate::vehicles::VehicleKind], spawn: &mut dyn FnMut(&str, &[String]) -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)>) -> WreckAssets {
    let (Some(content), Some(gcgame)) = (lib.open("GCGameContent"), lib.open("GCGame")) else {
        warn!("wreck: GCGameContent / GCGame not readable");
        return WreckAssets::default();
    };
    let kinds = kinds.iter().map(|k| load_kind(lib, pkgs, &content, &gcgame, k, spawn)).collect();
    WreckAssets { kinds }
}

fn load_kind(lib: &Library, pkgs: &[Package], content: &Package, gcgame: &Package, k: &crate::vehicles::VehicleKind, spawn: &mut dyn FnMut(&str, &[String]) -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)>) -> Option<WreckKind> {
    let d = defaults(content, gcgame, &k.class);
    // RagdollDefinitions / lists: the most derived class that has them (content classes only)
    let (rp, _) = d.iter().find(|(p, c)| *c && p.get("RagdollDefinitions").is_some())?;
    let raw_defs = content.struct_array(rp, "RagdollDefinitions");
    let list = |n: &str| d.iter().find_map(|(p, c)| if *c { p.get(n).map(|v| int_list(content, Some(v))) } else { None }).unwrap_or_default();
    let (stationary, moving) = (list("RagdollDefinitionListStationary"), list("RagdollDefinitionListMoving"));
    let first = raw_defs.first()?;
    let mesh = first.object("RagdollMesh").filter(|&o| o != 0).map(|o| content.object_path(o))?;
    let mats: Vec<String> = first.get("RagdollMaterials").map(|v| content.array_objects(v).iter().map(|&o| if o == 0 { String::new() } else { content.object_path(o) }).collect()).unwrap_or_default();
    let mut defs = Vec::new();
    for rd in &raw_defs {
        if rd.object("RagdollMesh").map(|o| content.object_path(o)).as_deref() != Some(mesh.as_str()) {
            warn!("wreck {}: definition with another mesh (first mesh used)", k.class);
        }
        let tree = rd.object("RagdollAnimTree").filter(|&o| o != 0).map(|o| content.object_path(o));
        let scales = tree.map(|t| tree_scales(pkgs, &t, &k.skeleton)).unwrap_or_else(|| vec![1.0; k.skeleton.len()]);
        let (bone_piece, piece_names) = rd.object("RagdollAsset").filter(|&o| o != 0).map(|o| asset_pieces(pkgs, &content.object_path(o), &k.skeleton)).unwrap_or_else(|| (vec![0; k.skeleton.len()], Vec::new()));
        defs.push(WreckDef { events: names(content, rd.get("VehicleEventsToFire")).into_iter().map(|s| s.to_ascii_lowercase()).collect(), scales, bone_piece, pieces: piece_names.into_iter().map(|b| WreckPiece { bone: b, center: Vec3::ZERO, low: 0.0, hull: Default::default() }).collect() });
    }
    // wreck mesh instances: the vehicle mesh with the definition's materials
    let mut slots = Vec::new();
    let us = k.class.starts_with("GCV_US_");
    for _ in 0..POOL {
        let Some((h, m)) = spawn(&mesh, &mats) else { break };
        // the other team's variant hidden like the intact vehicle (vehload.rs)
        let hide: Vec<String> = m
            .bones
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                let mut b = *i;
                for _ in 0..64 {
                    let n = m.bones[b].name.to_ascii_uppercase();
                    if if us { n.starts_with("CH_") || n.starts_with("OA_CH_") } else { n.starts_with("US_") || n.starts_with("OA_US_") } {
                        return true;
                    }
                    if b == 0 || m.bones[b].parent < 0 {
                        break;
                    }
                    b = m.bones[b].parent as usize;
                }
                false
            })
            .map(|(_, b)| b.name.clone())
            .collect();
        let refs: Vec<&str> = hide.iter().map(|s| s.as_str()).collect();
        slots.push(SkinPart::new(h, m, &k.skeleton, &refs));
    }
    if slots.is_empty() {
        warn!("wreck {}: mesh {mesh} not spawned", k.class);
        return None;
    }
    for d in &mut defs {
        let names: Vec<String> = d.pieces.iter().map(|p| p.bone.clone()).collect();
        d.pieces = piece_bounds(&slots[0], &d.bone_piece, &d.scales, &names);
        if !d.pieces.is_empty() {
            info!("wreck {} {:?}: detached pieces {:?}", k.class, d.events, d.pieces.iter().map(|p| (&p.bone, p.center.round(), p.low.round())).collect::<Vec<_>>());
        }
    }
    // the destroyed-state attachments of the content classes (vehload.rs attachment specs)
    let content_chain: Vec<String> = class_chain(content, &k.class).0;
    let specs = locate(pkgs, &mesh, "SkeletalMesh").map(|(mp, mi)| crate::vehload::attachment_specs_sel(content, &content_chain, mp, mi, true)).unwrap_or_default();
    let attach: Vec<Vec<SkinPart>> = (0..slots.len()).map(|_| crate::vehload::attachment_parts(&specs, &k.skeleton, &mut |r: &str| spawn(r, &[]))).collect();
    if !specs.is_empty() {
        info!("wreck {}: attachments {:?} -> {} parts", k.class, specs.iter().map(|s| &s.0).collect::<Vec<_>>(), attach.first().map_or(0, |v| v.len()));
    }
    // socket references (particles and sound cues, third-person / all views)
    let mut socket_refs = Vec::new();
    if let Some((mp, mi)) = locate(pkgs, &mesh, "SkeletalMesh") {
        for s in mp.mesh_sockets(mi) {
            for e in s.effects.iter().filter(|e| e.view != "VIEW_1P" && e.start_tag != "None" && (e.class == "ParticleSystem" || e.class == "SoundCue")) {
                socket_refs.push(SocketRef { start: e.start_tag.to_ascii_lowercase(), end: e.end_tag.to_ascii_lowercase(), socket: s.name.to_ascii_lowercase(), path: e.reference.clone(), sound: e.class == "SoundCue" });
            }
        }
    }
    // only the events a wreck fires
    let kismet = first_obj(&d, content, gcgame, "VehicleKismet").map(|s| kismet_sounds(pkgs, &s)).unwrap_or_default();
    let mut evs: Vec<String> = vec!["vehicledestroyed".into(), "wreckexplosion".into()];
    evs.extend(kismet.iter().filter_map(|(_, c)| c.strip_prefix("event:").map(String::from)));
    evs.extend(defs.iter().flat_map(|d| d.events.iter().cloned()));
    socket_refs.retain(|r| evs.contains(&r.start));
    let wreck_sound = first_obj(&d, content, gcgame, "VehicleAudio").map(|s| wreck_audio(lib, &s)).unwrap_or_default();
    let kind = WreckKind {
        class: k.class.clone(),
        slots,
        attach,
        defs,
        stationary,
        moving,
        lifespan: first_float(&d, "RagdollLifespan").unwrap_or(7.0),
        wreck_explosion_start: first_float(&d, "m_fWreckExplosionStartTime").unwrap_or(3.0),
        explosion: (
            first_float(&d, "ExplosionDamage").unwrap_or(0.0),
            first_float(&d, "ExplosionRadius").unwrap_or(0.0),
            first_float(&d, "ExplosionRadiusFull").unwrap_or(0.0),
            first_obj(&d, content, gcgame, "ExplosionDamageType").map(|p| p.rsplit('.').next().unwrap_or("").to_string()).unwrap_or_default(),
        ),
        socket_refs,
        kismet_sounds: kismet,
        wreck_sound,
    };
    info!(
        "wreck {}: mesh {mesh}, {} definitions (events {:?}), stationary {:?} moving {:?}, lifespan {} wreck explosion at -{} s, explosion {:?}, {} socket refs, kismet {:?}, wreck sound {:?}",
        kind.class,
        kind.defs.len(),
        kind.defs.iter().map(|d| &d.events).collect::<Vec<_>>(),
        kind.stationary,
        kind.moving,
        kind.lifespan,
        kind.wreck_explosion_start,
        kind.explosion,
        kind.socket_refs.len(),
        kind.kismet_sounds,
        kind.wreck_sound
    );
    Some(kind)
}

/// Textures of the *_DEST / *_Damage vehicle materials (MAT_LIB M_Vehicle_DEST,
/// M_Vehicle_Damage) that are not in the data: imports of the package GLO_textures, which no
/// file of the install exports (checked with examples/expfind over every .upk / .map). Unbound,
/// they sampled as Bevy's white fallback: Embers = 1 lit the whole soot area with
/// EmissiveColor (5, 2, 0.4375) * lerp(1, 25, (PixelDepth / 2000)^3), up to 125 times
/// overbright at 20 m - the "glowing slab" of the 2026-10-21 test - and soot02_N = 1 bent every
/// normal by (3.333, 3.333, 0). Substitutes (NOT from data): Embers and soot02_N black (no
/// embers; the soot normal adds nothing), soot02_C / soot02_M keep the white fallback (soot
/// mask 1 = fully charred, soot colour 1 * 0.01 = near black, as the material's soot branch).
pub fn tex_substitutes() -> Vec<(String, String)> {
    ["glo_textures.embers", "glo_textures.soot02_n"].iter().map(|p| (p.to_string(), String::new())).collect()
}

/// main.rs: one wreck mesh instance (sections as world::Spawner::skinned_mesh, each with the
/// RagdollMaterials entry of its mesh material slot as the material override).
#[allow(clippy::too_many_arguments)]
pub fn spawn_wreck_mesh(
    sp: &mut crate::world::Spawner,
    a: &mut crate::world::Assets3,
    commands: &mut Commands,
    pi: usize,
    ei: usize,
    l2w: [[f32; 3]; 4],
    mats: &[String],
) -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)> {
    let pkg = &sp.pkgs[pi];
    let sm = pkg.read_skeletal_mesh(ei)?;
    let mut parts = Vec::new();
    let mut over = Vec::new();
    for s in &sm.lod.sections {
        let Some(map) = crate::section_vertex_map(&sm.lod, s) else { continue };
        let Some(m) = crate::build_skinned_section(&sm.lod, s, &map) else { continue };
        let slot = sm.lod0_material_map.get(s.material as usize).copied().unwrap_or(s.material as i32);
        let o = mats.get(slot.max(0) as usize).filter(|p| !p.is_empty()).and_then(|path| find_material(pkg, path));
        if o.is_none() && mats.get(slot.max(0) as usize).is_some_and(|p| !p.is_empty()) {
            warn!("wreck: material {:?} not in the mesh package", mats.get(slot as usize));
        }
        over.push(o.map(|i| i as i32 + 1).unwrap_or(0));
        parts.push((a.meshes.add(m), sm.section_material(s.material as i32)));
    }
    let n = sp.spawn_mesh(commands, a, pi, parts.clone(), pi, &over, Transform::IDENTITY, l2w, [0.0; 4], false, false);
    if n == 0 {
        warn!("wreck: no drawable section");
    }
    Some((parts.into_iter().map(|x| x.0).collect(), sm))
}

/// A material export by full object path (or the path below its package name).
fn find_material(pkg: &Package, path: &str) -> Option<usize> {
    let rest = path.split_once('.').map(|x| x.1).unwrap_or(path);
    pkg.exports.iter().enumerate().position(|(i, e)| {
        let c = pkg.export_class(e);
        (c == "MaterialInstanceConstant" || c == "Material") && {
            let op = pkg.object_path(i as i32 + 1);
            op.eq_ignore_ascii_case(path) || op.eq_ignore_ascii_case(rest)
        }
    })
}

/// main.rs: a wreck attachment ("SM|<static mesh>|<bone>|<19 numbers>|<materials ;>", built by
/// vehload::attachment_parts from attachment_specs_sel(.., true)) as a one-bone skinned mesh with
/// the component's Materials (*_DEST) per mesh element. NOT from data: element k = section k.
pub fn spawn_wreck_static(
    sp: &mut crate::world::Spawner,
    a: &mut crate::world::Assets3,
    commands: &mut Commands,
    locate: &dyn Fn(&str, &str) -> Option<(usize, usize)>,
    l2w: [[f32; 3]; 4],
    req: &str,
) -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)> {
    let mut it = req.split('|');
    let (smp, bone, nums) = (it.next()?, it.next()?, it.next()?);
    let mats: Vec<&str> = it.next().map(|m| m.split(';').collect()).unwrap_or_default();
    let f: Vec<f32> = nums.split(',').filter_map(|x| x.parse().ok()).collect();
    if f.len() != 19 {
        return None;
    }
    let (pi, ei) = locate(smp, "StaticMesh")?;
    let lin = bevy::math::Mat3::from_cols_slice(&f[0..9]);
    let b = upk::skeletal::Bone { name: bone.to_string(), rot: [f[12], f[13], f[14], f[15]], pos: [f[16], f[17], f[18]], parent: 0 };
    let (parts, mesh) = sp.static_as_skinned(a, pi, ei, lin, Vec3::new(f[9], f[10], f[11]), b)?;
    let over: Vec<i32> = (0..parts.len())
        .map(|k| {
            let m = mats.get(k).copied().unwrap_or("");
            if m.is_empty() {
                return 0;
            }
            match find_material(&sp.pkgs[pi], m) {
                Some(i) => i as i32 + 1,
                None => {
                    warn!("wreck attachment: material {m} not in the mesh package");
                    0
                }
            }
        })
        .collect();
    sp.spawn_mesh(commands, a, pi, parts.clone(), pi, &over, Transform::IDENTITY, l2w, [0.0; 4], false, false);
    Some((parts.into_iter().map(|x| x.0).collect(), mesh))
}

// ---- runtime -------------------------------------------------------------------------------

/// HB_TEST_VEHKILL=<s>: the player's vehicle loses all health after <s> seconds in it (tests).
fn test_kill(time: Res<Time>, mut vehicles: Query<&mut Vehicle>, mut t: Local<f32>) {
    let Some(after) = std::env::var("HB_TEST_VEHKILL").ok().and_then(|v| v.parse::<f32>().ok()) else { return };
    let Some(mut v) = vehicles.iter_mut().find(|v| v.player_seat.is_some()) else {
        *t = 0.0;
        return;
    };
    *t += time.delta_secs();
    // with HB_VEHTEST the scripted drive's clock (it starts after a real-time delay, so a fast
    // PC doesn't kill the vehicle while the scene is still loading)
    let tt = if std::env::var_os("HB_VEHTEST").is_some() { f32::from_bits(crate::vehicles::TEST_T.load(std::sync::atomic::Ordering::Relaxed)) } else { *t };
    if *t >= 0.0 && tt >= after && v.health > 0.0 {
        info!("HB_TEST_VEHKILL: vehicle health -> 0");
        v.health = 0.0;
        *t = f32::MIN;
    }
}

pub(crate) fn rand01(seed: &mut u32) -> f32 {
    let mut x = if *seed == 0 { 0x2545_f491 } else { *seed };
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    (x >> 8) as f32 / 16_777_216.0
}

/// World frame (UE origin, rotation) of a bone's part of the wreck: the wreck body or the
/// detached piece the bone belongs to.
fn bone_frame(wk: &WreckKind, w: &Wreck, bone: usize) -> (Vec3, Quat) {
    let g = wk.defs.get(w.def).and_then(|d| d.bone_piece.get(bone)).copied().unwrap_or(0) as usize;
    match w.pieces.get(g.wrapping_sub(1)) {
        Some(p) if g > 0 => (p.pos, p.rot),
        _ => (w.pos, body_rot(w.yaw, w.pitch, w.roll)),
    }
}

/// Socket world position (UE) of a wreck, reference pose (on its detached piece if any).
fn socket_world(k: &crate::vehicles::VehicleKind, wk: &WreckKind, w: &Wreck, name: &str) -> Option<Vec3> {
    let &(bi, loc) = k.sockets.get(name)?;
    let (r, t) = k.ref_pose.get(bi)?;
    let (o, rot) = bone_frame(wk, w, bi);
    Some(o + rot * mesh_to_ue(*r * loc + *t))
}

/// Socket rotation of a wreck (its part's frame).
fn socket_rot(k: &crate::vehicles::VehicleKind, wk: &WreckKind, w: &Wreck, name: &str) -> Quat {
    k.sockets.get(name).map(|&(bi, _)| bone_frame(wk, w, bi).1).unwrap_or_else(|| body_rot(w.yaw, w.pitch, w.roll))
}

/// Reference pose with the detached pieces moved to their frames, expressed in the wreck's
/// mesh space (render_wrecks skins everything with the wreck's rotation and origin).
fn wreck_pose(k: &crate::vehicles::VehicleKind, wk: &WreckKind, w: &Wreck) -> Vec<(Quat, Vec3)> {
    let mut pose = k.ref_pose.clone();
    let Some(d) = wk.defs.get(w.def) else { return pose };
    if w.pieces.is_empty() {
        return pose;
    }
    // mesh_to_ue as a rotation (det +1): m -> (m.z, -m.x, -m.y)
    let m2u = Quat::from_mat3(&Mat3::from_cols(Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 0.0)));
    let inv = body_rot(w.yaw, w.pitch, w.roll).conjugate();
    let xf: Vec<(Quat, Vec3)> = w.pieces.iter().map(|p| (m2u.conjugate() * inv * p.rot * m2u, m2u.conjugate() * (inv * (p.pos - w.pos)))).collect();
    for (b, x) in pose.iter_mut().enumerate() {
        let g = d.bone_piece.get(b).copied().unwrap_or(0) as usize;
        if let Some(&(q, t)) = xf.get(g.wrapping_sub(1)).filter(|_| g > 0) {
            *x = (q * x.0, q * x.1 + t);
        }
    }
    pose
}

/// Fire a vehicle event on a wreck: socket references with that StartTag start, those with
/// that EndTag stop; the Kismet sounds of the event play.
fn fire_event(ev: &str, k: &crate::vehicles::VehicleKind, wk: &WreckKind, w: &mut Wreck, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue) {
    fire_event_d(ev, k, wk, w, fx, sfx, 0);
}

fn fire_event_d(ev: &str, k: &crate::vehicles::VehicleKind, wk: &WreckKind, w: &mut Wreck, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue, depth: u32) {
    let dbg = std::env::var_os("HB_WRECKDBG").is_some();
    if dbg {
        info!("wreck {}: event {ev} at {:.1} s", wk.class, w.age);
    }
    // stop what this event ends
    let mut keep = Vec::new();
    for (ri, id) in std::mem::take(&mut w.running) {
        if wk.socket_refs[ri].end == ev {
            if let Some(id) = id {
                fx.release(id);
            }
        } else {
            keep.push((ri, id));
        }
    }
    w.running = keep;
    for (ri, r) in wk.socket_refs.iter().enumerate().filter(|(_, r)| r.start == ev) {
        let at = socket_world(k, wk, w, &r.socket).unwrap_or(w.pos);
        let rot = socket_rot(k, wk, w, &r.socket);
        if r.sound {
            if r.end == "none" {
                sfx.play(&r.path, Some(at));
            } else {
                // runs until its EndTag event (looping cue, kept alive every frame)
                w.running.push((ri, None));
            }
        } else if let Some(id) = fx.fire_tracked(&r.path, at, rot) {
            w.running.push((ri, Some(id)));
        } else if dbg {
            info!("wreck {}: effect {} not pooled", wk.class, r.path);
        }
    }
    let mut chained = Vec::new();
    for (_, cue) in wk.kismet_sounds.iter().filter(|(e, _)| e == ev) {
        match cue.strip_prefix("event:") {
            Some(next) => chained.push(next.to_string()),
            None => sfx.play(cue, Some(w.pos)),
        }
    }
    for next in chained {
        if next != ev && depth < 4 {
            fire_event_d(&next, k, wk, w, fx, sfx, depth + 1);
        }
    }
}

/// A destroyed vehicle becomes a wreck: definition, slot, launch, the death events and the
/// explosion's HurtRadius.
#[allow(clippy::too_many_arguments)]
fn start_wrecks(
    mut commands: Commands,
    spawns: Query<(Entity, &WreckSpawn)>,
    mut wrecks: Query<(Entity, &mut Wreck)>,
    assets: Option<Res<VehicleAssets>>,
    wassets: Option<Res<WreckAssets>>,
    data: Option<Res<CombatData>>,
    soldiers: Query<(Entity, &Soldier)>,
    pawns: Query<&Pawn>,
    col: Res<CollisionWorld>,
    (mut queue, mut fx, mut sfx): (ResMut<DamageQueue>, ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>),
    (mut seed, mut serial, mut buf): (Local<u32>, Local<u32>, Local<Vec<u32>>),
) {
    for (e, s) in &spawns {
        commands.entity(e).despawn();
        let (Some(assets), Some(wa)) = (assets.as_ref(), wassets.as_ref()) else { continue };
        let (Some(k), Some(Some(wk))) = (assets.kinds.get(s.kind), wa.kinds.get(s.kind)) else { continue };
        // a slot: a free one, else the oldest wreck of this kind goes
        let used: Vec<(Entity, usize, u32)> = wrecks.iter().filter(|(_, w)| w.kind == s.kind).map(|(e, w)| (e, w.slot, w.serial)).collect();
        let slot = match (0..wk.slots.len()).find(|i| !used.iter().any(|u| u.1 == *i)) {
            Some(i) => i,
            None => {
                let Some(&(oe, os, _)) = used.iter().min_by_key(|u| u.2) else { continue };
                if let Ok((_, mut ow)) = wrecks.get_mut(oe) {
                    for (_, id) in std::mem::take(&mut ow.running) {
                        if let Some(id) = id {
                            fx.release(id);
                        }
                    }
                }
                commands.entity(oe).despawn();
                os
            }
        };
        // CalculateRagdollUseMovingList: VSizeSq(Velocity) > 50 * 50
        let list = if s.vel.length_squared() > 50.0 * 50.0 { &wk.moving } else { &wk.stationary };
        let def = if list.is_empty() { 0 } else { list[((rand01(&mut seed) * list.len() as f32) as usize).min(list.len() - 1)] }.min(wk.defs.len() - 1);
        let mut away = Vec3::new(s.pos.x - s.from.x, s.pos.y - s.from.y, 0.0).normalize_or_zero();
        if away == Vec3::ZERO {
            away = Vec3::new(rand01(&mut seed) - 0.5, rand01(&mut seed) - 0.5, 0.0).normalize_or_zero();
        }
        *serial += 1;
        let mut w = Wreck {
            kind: s.kind,
            slot,
            def,
            pos: s.pos,
            yaw: s.yaw,
            pitch: s.pitch,
            roll: s.roll,
            vel: s.vel + away * LAUNCH_ALONG_HIT + Vec3::Z * LAUNCH_UP,
            spin: Vec2::new((rand01(&mut seed) - 0.5) * 2.0 * LAUNCH_SPIN, (rand01(&mut seed) - 0.5) * 2.0 * LAUNCH_SPIN),
            age: 0.0,
            resting: false,
            wreck_exploded: false,
            running: Vec::new(),
            serial: *serial,
            pieces: Vec::new(),
        };
        // the detached bodies start where they sit on the vehicle and fly off (NOT from data:
        // launch, see PIECE_*): up, away from the vehicle centre, random spin
        let r0 = body_rot(s.yaw, s.pitch, s.roll);
        for p in &wk.defs[def].pieces {
            let mut out = (r0 * Vec3::new(p.center.x, p.center.y, 0.0)).with_z(0.0).normalize_or_zero();
            if out == Vec3::ZERO {
                out = away;
            }
            let axis = Vec3::new(rand01(&mut seed) - 0.5, rand01(&mut seed) - 0.5, rand01(&mut seed) - 0.5).normalize_or_zero();
            let vel = s.vel + out * PIECE_OUT * (0.6 + 0.8 * rand01(&mut seed)) + away * LAUNCH_ALONG_HIT + Vec3::Z * PIECE_UP * (0.7 + 0.6 * rand01(&mut seed));
            w.pieces.push(PieceState {
                pos: s.pos,
                rot: r0,
                body: crate::piecephys::Body::new(s.pos + r0 * p.center, r0, vel, axis * PIECE_SPIN * (0.5 + rand01(&mut seed))),
            });
        }
        fire_event("vehicledestroyed", k, wk, &mut w, &mut fx, &mut sfx);
        for ev in wk.defs[def].events.clone() {
            fire_event(&ev, k, wk, &mut w, &mut fx, &mut sfx);
        }
        info!("vehicle {} wreck: definition {def} ({:?}), slot {slot}", k.name, wk.defs[def].events);
        // InitialVehicleExplosion: HurtRadius(ExplosionDamage, ExplosionRadius, ExplosionDamageType,
        // ExplosionMomentum, Location, ExplosionRadiusFull); full damage inside the full radius,
        // linear falloff to the radius (like combat.rs grenades); occluded victims are spared
        let (amount, radius, full, dtype) = &wk.explosion;
        if *amount > 0.0 && *radius > 0.0 {
            let mut targets: Vec<(Target, Vec3, u8)> = soldiers.iter().filter(|(_, s)| s.alive).map(|(e, s)| (Target::Bot(e), s.center_ue(), s.team)).collect();
            if let Ok(p) = pawns.single() {
                if !p.dead && !p.in_vehicle {
                    let c = p.feet + Vec3::Y * crate::player::PAWN_HALF_HEIGHT;
                    targets.push((Target::Player, Vec3::new(c.x, c.z, c.y) * 100.0, player_team()));
                }
            }
            // the damage grid's soldier value of the damage type scales ExplosionDamage (like
            // the infantry blast code); without a grid entry ExplosionDamage as is
            let grid = data.as_ref().map(|d| d.grid.damage(dtype, "PMA_Soldier", 0.0, 1.0, 1.0)).filter(|&g| g > 0.0);
            let base = grid.unwrap_or(*amount);
            let r = radius.max(*full);
            for (t, c, _team) in targets {
                let d = (c - s.pos).length();
                if d > r {
                    continue;
                }
                let ob = Vec3::new(s.pos.x, s.pos.z + 50.0, s.pos.y) * 0.01;
                let cb = Vec3::new(c.x, c.z, c.y) * 0.01;
                if col.raycast(ob, (cb - ob).normalize_or_zero(), ob.distance(cb), &mut buf).is_some() {
                    continue;
                }
                let dmg = if d <= *full { base } else { base * (1.0 - (d - full) / (r - full).max(1.0)) };
                if std::env::var_os("HB_DMGDBG").is_some() {
                    info!("vehicle explosion {dtype}: {t:?} at {d:.0} uu -> {dmg:.0}");
                }
                queue.0.push(Hit { target: t, amount: dmg, attacker: s.attacker, attacker_team: s.team, headshot: false, explosive: true, from: s.pos, icon: None, dmg_type: Some(dtype.clone()) });
            }
        }
        commands.spawn(w);
    }
}

/// Wreck motion, timeline (WreckExplosion, end of LifeSpan) and running effects.
#[allow(clippy::too_many_arguments)]
fn update_wrecks(
    mut commands: Commands,
    time: Res<Time>,
    flow: Res<crate::menu::MatchFlow>,
    mut wrecks: Query<(Entity, &mut Wreck)>,
    assets: Option<Res<VehicleAssets>>,
    wassets: Option<Res<WreckAssets>>,
    col: Res<CollisionWorld>,
    (mut fx, mut sfx): (ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>),
    mut buf: Local<Vec<u32>>,
) {
    let dt = if flow.paused { 0.0 } else { time.delta_secs().min(0.05) };
    let (Some(assets), Some(wa)) = (assets, wassets) else {
        for (e, _) in &wrecks {
            commands.entity(e).despawn();
        }
        return;
    };
    // new level: the wrecks of the last one go
    if wa.is_changed() {
        for (e, _) in &wrecks {
            commands.entity(e).despawn();
        }
        return;
    }
    for (e, mut w) in &mut wrecks {
        let (Some(k), Some(Some(wk))) = (assets.kinds.get(w.kind), wa.kinds.get(w.kind)) else {
            commands.entity(e).despawn();
            continue;
        };
        w.age += dt;
        // motion: ballistic until the origin is back at ride height over the floor
        if !w.resting {
            w.vel.z -= GRAVITY * dt;
            let step = w.vel * dt;
            w.pos += step;
            w.pitch += w.spin.x * dt;
            w.roll += w.spin.y * dt;
            let pb = Vec3::new(w.pos.x, w.pos.z, w.pos.y) * 0.01;
            if let Some((fy, _)) = col.floor(pb + Vec3::Y * 3.0, 30.0, &mut buf) {
                let rest = fy * 100.0 + k.ride_height;
                if w.pos.z <= rest {
                    w.pos.z = rest;
                    if w.vel.z < 0.0 {
                        w.vel.z = -w.vel.z * BOUNCE;
                    }
                    let f = (1.0 - GROUND_FRICTION * dt).max(0.0);
                    w.vel.x *= f;
                    w.vel.y *= f;
                    w.spin *= f;
                    if w.vel.z < 60.0 && w.vel.truncate().length() < 20.0 {
                        w.resting = true;
                        w.vel = Vec3::ZERO;
                        w.spin = Vec2::ZERO;
                    }
                }
            }
        }
        // detached pieces: rigid bodies (piecephys.rs: rotated contact points, tip over, lie
        // flat, walls). The wreck itself is not in the CollisionWorld, nothing to skip.
        if let Some(d) = wk.defs.get(w.def) {
            let mut probe = crate::piecephys::ColProbe { col: &col, buf: &mut buf, skip: 0..0 };
            let start = w.pos.z;
            for (pi, p) in w.pieces.iter_mut().enumerate() {
                let Some(pd) = d.pieces.get(pi) else { continue };
                if p.body.resting {
                    continue;
                }
                crate::piecephys::step(&mut p.body, &pd.hull, &mut probe, dt);
                // fell through / out of the world (NOT from data): stop
                if p.body.c.z < start - 5000.0 {
                    p.body.resting = true;
                }
                p.rot = p.body.rot;
                p.pos = p.body.c - p.body.rot * pd.center;
            }
        }
        // timeline
        let wreck_at = (wk.lifespan - wk.wreck_explosion_start).max(0.0);
        if !w.wreck_exploded && w.age >= wreck_at {
            w.wreck_exploded = true;
            fire_event("wreckexplosion", k, wk, &mut w, &mut fx, &mut sfx);
            if !wk.wreck_sound.is_empty() {
                sfx.play(&wk.wreck_sound, Some(w.pos));
            }
        }
        for &(ri, id) in &w.running {
            let r = &wk.socket_refs[ri];
            let at = socket_world(k, wk, &w, &r.socket).unwrap_or(w.pos);
            let rot = socket_rot(k, wk, &w, &r.socket);
            match id {
                Some(id) => fx.track(id, at, rot),
                None => sfx.looping(&format!("wreck{}_{ri}", w.serial), &r.path, Some(at)),
            }
        }
        if w.age >= wk.lifespan {
            for (_, id) in std::mem::take(&mut w.running) {
                if let Some(id) = id {
                    fx.release(id);
                }
            }
            commands.entity(e).despawn();
        }
    }
}

/// Skin the wreck meshes in use, hide the others.
fn render_wrecks(
    mut commands: Commands,
    wrecks: Query<&Wreck>,
    assets: Option<Res<VehicleAssets>>,
    wassets: Option<Res<WreckAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ents: Query<(&mut Visibility, &mut Transform), Without<Camera3d>>,
    me: Res<crate::MeshEntities>,
    (mut shown, mut for_assets): (Local<Vec<Vec<bool>>>, Local<Option<AssetId<Mesh>>>),
) {
    let (Some(assets), Some(wa)) = (assets, wassets) else { return };
    // new assets (next match): every slot counts as shown so it gets hidden once
    let id = wa.kinds.iter().flatten().next().and_then(|k| k.slots.first()).and_then(|s| s.meshes.first()).map(|h| h.id());
    if shown.len() != wa.kinds.len() || *for_assets != id {
        *shown = wa.kinds.iter().map(|k| vec![true; k.as_ref().map_or(0, |k| k.slots.len())]).collect();
        *for_assets = id;
    }
    let mut now: Vec<Vec<bool>> = wa.kinds.iter().map(|k| vec![false; k.as_ref().map_or(0, |k| k.slots.len())]).collect();
    for w in &wrecks {
        let (Some(k), Some(Some(wk))) = (assets.kinds.get(w.kind), wa.kinds.get(w.kind)) else { continue };
        let Some(part) = wk.slots.get(w.slot) else { continue };
        // HB_NOWRECKMESH=1: no wreck mesh (effects, sounds and splash damage stay)
        if std::env::var_os("HB_NOWRECKMESH").is_some() {
            continue;
        }
        now[w.kind][w.slot] = true;
        let mut scales = wk.defs[w.def].scales.clone();
        // rotor swap scales of the intact vehicle (the blurred / still rotor meshes)
        for (bones, _) in &k.rotors {
            for &(b, sc, _) in bones {
                if let Some(x) = scales.get_mut(b) {
                    *x *= sc;
                }
            }
        }
        let rot = body_rot(w.yaw, w.pitch, w.roll);
        let pose = wreck_pose(k, wk, w);
        crate::bots::skin_part_scaled(part, &pose, rot, w.pos, &mut meshes, Some(&scales));
        let extra: &[SkinPart] = wk.attach.get(w.slot).map(|v| v.as_slice()).unwrap_or(&[]);
        for ap in extra {
            crate::bots::skin_part_scaled(ap, &pose, rot, w.pos, &mut meshes, Some(&scales));
        }
        let tr = Vec3::new(w.pos.x, w.pos.z, w.pos.y) * 0.01;
        for e in crate::mesh_ents(&me, &part.meshes).chain(extra.iter().flat_map(|ap| crate::mesh_ents(&me, &ap.meshes))) {
            if let Ok((_, mut tf)) = ents.get_mut(e) {
                if tf.translation != tr {
                    tf.translation = tr;
                }
            }
        }
    }
    for (ki, k) in wa.kinds.iter().enumerate() {
        let Some(k) = k else { continue };
        for (si, part) in k.slots.iter().enumerate() {
            if now[ki][si] == shown[ki][si] {
                continue;
            }
            let vis = if now[ki][si] { Visibility::Visible } else { Visibility::Hidden };
            let extra: &[SkinPart] = k.attach.get(si).map(|v| v.as_slice()).unwrap_or(&[]);
            // the mesh entities are indexed a frame after their spawn (MeshEntities): the state
            // counts as applied only once they were found (else the wreck meshes stayed visible
            // at the origin in UE units, 100 times too large)
            let mut found = false;
            for e in crate::mesh_ents(&me, &part.meshes).chain(extra.iter().flat_map(|ap| crate::mesh_ents(&me, &ap.meshes))) {
                found = true;
                if let Ok((mut v, _)) = ents.get_mut(e) {
                    *v = vis;
                    // the skinned vertices leave the spawn-time bounds (pieces fly off), like
                    // the vehicles in vehicles.rs render_vehicles
                    commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                }
            }
            if found {
                shown[ki][si] = now[ki][si];
            }
        }
    }
}
