//! Ground assault drone killstreak (CFG_DRO_GAssault -> GCWeap_DRO_GAssault_C m_DroneVehClass
//! GCV_SH_GAssaultWalker_Content).
//!
//! Data: SVehicleMesh UCV_GroundDrones.SkeletalMeshes.SKV_UCV_GAssault (Translation z -15),
//! CollisionCylinder radius 25 / height 25, PHYS_Walking with GroundSpeed 275, MaxStepHeight 16,
//! Health 730; Seats[0] CameraOffset (0, 0, 50), GunClass GCVW_GAssault_Content ->
//! WeaponConfigs MP.Vehicle.CFG_GAssault (FireInterval 0.15, FireSpread 2, WeaponRange 5500,
//! FullDamageRange 2500, overheat 3 / 0.3 / 1.5, GCDmgType_GAssault_Bullet);
//! UCV_GroundDrones VehicleSetup.Fuel (MaxFuel 1000 from Default__GCVehicleFuelInfo, moving 10/s,
//! idle 1/s, 3 per shot); audio A2_Droids_C AudioSetup_GroundDrone_Assault.
//! Flow (user's screenshots): after buying, a preview of the drone stands in front of the player
//! with the hint "(LMT) ZUM ENTSENDEN EINER DROHNE" (UI_GameplayHints DeployDrone); the left
//! mouse button sets it down and the view changes to the drone (HUD VehiclePlacerAllDrones +
//! VehiclePlacerGroundAssault). The pawn stays where it was. Ends when the battery is empty, the
//! drone is destroyed, or with E (NOT from data: the exit key).
//! NOT from data: the drone turns with the view (no separate tread / turret simulation), the
//! preview distance 2 m, the feed's post effect (grey, scan lines, vignette) is not drawn.
use crate::combat::{Actor, CombatData, DamageQueue, Hit, ShotHit, Soldier, Target, player_team};
use crate::menu::{MatchFlow, Phase};
use crate::player::{CollisionWorld, Pawn};
use bevy::prelude::*;

pub struct DronePlugin;

impl Plugin for DronePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drone>().init_resource::<ReconMarks>().add_systems(Update, (update_drone.after(crate::combat::CombatSet), recon_hud.after(update_drone)));
    }
}

/// Enemies marked by the air recon drone (seconds left) and the ones in its box this frame.
#[derive(Resource, Default)]
pub struct ReconMarks {
    pub marked: std::collections::HashMap<Entity, f32>,
    pub candidates: Vec<Entity>,
}

/// GCUIObject_ReconDroneMarker PositionAdjustment (0, 0, 10): the marker sits this far above the
/// head
const MARKER_LIFT: f32 = 10.0;

/// Screen positions (1280x720 units) of the marked enemies (ReconDroneMarkers_Marked, every view)
/// and of the drone's candidates (ReconDroneMarkers_UnMarked, drone view only).
fn recon_hud(
    time: Res<Time>,
    mut marks: ResMut<ReconMarks>,
    mut flow: ResMut<MatchFlow>,
    soldiers: Query<(Entity, &Soldier)>,
    cams: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
) {
    let dt = time.delta_secs().min(0.1);
    marks.marked.retain(|e, t| {
        *t -= dt;
        *t > 0.0 && soldiers.get(*e).is_ok_and(|(_, s)| s.alive)
    });
    flow.hud.recon.clear();
    if flow.phase != Phase::Playing {
        marks.marked.clear();
        return;
    }
    // HB_RECONTEST=1: every enemy marked (HUD tests)
    if std::env::var_os("HB_RECONTEST").is_some() {
        for (e, s) in &soldiers {
            if s.alive && s.team != player_team() {
                marks.marked.insert(e, 10.0);
            }
        }
    }
    let Ok((cam, gt)) = cams.single() else { return };
    let vp = cam.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
    let k = Vec2::new(1280.0 / vp.x.max(1.0), 720.0 / vp.y.max(1.0));
    let candidates = marks.candidates.clone();
    for (e, s) in &soldiers {
        let marked = marks.marked.contains_key(&e);
        if !s.alive || !(marked || candidates.contains(&e)) {
            continue;
        }
        let c = s.center_ue() + Vec3::Z * (crate::combat::HEAD_OFFSET + MARKER_LIFT);
        if let Ok(p) = cam.world_to_viewport(gt, Vec3::new(c.x, c.z, c.y) * 0.01) {
            flow.hud.recon.push((p * k, marked));
        }
    }
}

/// First-person cockpit (CockpitMesh, SDPG_Foreground): drawn at the camera socket in the
/// first-person view.
pub struct Cockpit {
    pub part: crate::viewmodel::SkinPart,
    pub pose: Vec<(Quat, Vec3)>,
    /// reference position of the camera socket's bone in the cockpit mesh (mesh space)
    pub anchor: Vec3,
    /// CockpitMesh Translation (UE units, view axes)
    pub translation: Vec3,
}

/// The ground drones by kind (main.rs DRONE_KINDS: 0 assault, 1 anti tank).
#[derive(Resource)]
pub struct DroneKinds(pub Vec<DroneAssets>);

impl DroneAssets {
    /// How much higher the mesh stands on its treads than on the cylinder (UE units).
    pub fn lift(&self) -> f32 {
        self.tread_low.map(|t| t - (self.half_height + self.mesh_z)).unwrap_or(0.0)
    }
}

/// A further drawable copy of a drone kind for the bots' drones (botdrones.rs): body, mirror
/// attachments and tread sections (left, right).
pub struct BotInstance {
    pub part: crate::viewmodel::SkinPart,
    pub attach: Vec<crate::viewmodel::SkinPart>,
    pub tread: [Vec<crate::viewmodel::TreadPart>; 2],
}

pub struct DroneAssets {
    /// cost key (cfg_dro_gassault / cfg_dro_gantitank)
    pub key: &'static str,
    pub part: crate::viewmodel::SkinPart,
    /// mirror attachments (vehload::attachment_parts), shown with the body
    pub attach: Vec<crate::viewmodel::SkinPart>,
    /// GCVeh_RCDroneTank treads: TexPan2UnrealScale and the (left, right) tread section meshes
    /// with their reference UV0 (viewmodel::scroll_tread)
    pub tread: Option<(f32, [Vec<crate::viewmodel::TreadPart>; 2])>,
    /// treads' lowest vertex below the mesh origin (UE units): the mesh origin stands this
    /// high above the feet
    pub tread_low: Option<f32>,
    pub pose: Vec<(Quat, Vec3)>,
    pub mesh_z: f32,
    pub radius: f32,
    pub half_height: f32,
    pub speed: f32,
    /// bCanFly (air drones: AirSpeed instead of GroundSpeed)
    pub flying: bool,
    /// sqrt(m_nDetectionRadiusSq): enemies this close to the drone show on the minimap
    pub detect_radius: f32,
    /// the vehicle class and its parents in the package (HUD placers by VehicleTypes)
    pub classes: Vec<String>,
    pub health: f32,
    pub camera_z: f32,
    /// Seats[0] CameraOffset -x: the view sits this far behind the camera socket
    pub camera_back: f32,
    pub fuel_max: f32,
    pub fuel_moving: f32,
    pub fuel_idle: f32,
    pub fuel_shot: f32,
    pub gun: crate::vehicles::GunCfg,
    /// SCFireSound per fire mode (A2_Droids_C audio setup)
    pub mode_sounds: Vec<String>,
    /// Seat0FireMode<m>Socket<k> locations on Turret_Pitch per fire mode (UE units)
    pub fire_sockets: Vec<Vec<[f32; 3]>>,
    /// Seats[0] BehindCameraMaxOffset: third-person camera from the camera socket (UE, view axes)
    pub behind: Vec3,
    /// air recon drone: marks enemies in its box within this distance (UE) for these seconds
    pub mark: Option<(f32, f32)>,
    /// gun LockOn BigRectangle (1280x720 units around the centre), SmallRectangle material corners,
    /// MaxLockDist
    pub lock_box: Option<([f32; 2], [f32; 2])>,
    pub lock_small: Option<([f32; 2], [f32; 2])>,
    pub lock_dist: f32,
    pub cockpit: Option<Cockpit>,
    pub full_range: f32,
    pub skeleton: Vec<upk::skeletal::Bone>,
    pub rig: DroneRig,
    /// copies for the bots (botdrones.rs; their meshes are separate from the player's drone)
    pub bots: Vec<BotInstance>,
    /// hit box (UE, body axes: x forward, z up) around the mesh origin: the mesh's bounds (the
    /// bot drones' box for shots / explosions; NOT from data: a box instead of the collision)
    pub hit_box: (Vec3, Vec3),
}

/// Bones and values of the drone's AnimTree AT_UCV_GAssault_Walking (main.rs load_drone_rig).
#[derive(Debug, Clone)]
pub struct DroneRig {
    /// GCSkelControl_TurretConstrained (yaw only) on Turret_Rotate
    pub turret_yaw: Option<usize>,
    /// GCSkelControl_TurretConstrained (pitch only) on Turret_Pitch
    pub turret_pitch: Option<usize>,
    /// GCSkelControl_LookAtVelocity on TREADS_BASE (yaw only, bAllowRotationZ)
    pub treads: Option<usize>,
    pub rest_velocity: f32,
    pub wheels_l: Vec<usize>,
    pub wheels_r: Vec<usize>,
    /// GCVehicleWheel WheelRadius / RollFactor (NewWHL_L0)
    pub wheel_radius: f32,
    pub roll_factor: f32,
    /// half the distance between the tread wheels (WHL_R0 bone y)
    pub track_half: f32,
    pub antenna_bot: Option<usize>,
    pub antenna_top: Option<usize>,
    /// GCSkelControl_Spring: stiffness, damping, MaxDisplacement
    pub spring: (f32, f32, f32),
    /// GCSkelControl_HeliRotorSwap: the spinning state's bones (bone, BoneScale, BoneRotates) and
    /// vMaxRotationsPerSecond (bSpinWhenNotDriven: always at full speed)
    pub rotors: Vec<(Vec<(usize, f32, bool)>, Vec3)>,
}

impl Default for DroneRig {
    fn default() -> Self {
        DroneRig { turret_yaw: None, turret_pitch: None, treads: None, rest_velocity: 100.0, wheels_l: Vec::new(), wheels_r: Vec::new(), wheel_radius: 3.0, roll_factor: 1.0, track_half: 20.0, antenna_bot: None, antenna_top: None, spring: (50.0, 4.0, 0.0), rotors: Vec::new() }
    }
}

/// Animation state of the drone's skeletal controls.
#[derive(Default)]
pub struct DroneAnim {
    /// treads yaw relative to the body (radians)
    treads: f32,
    /// wheel roll angles left / right (radians)
    roll: (f32, f32),
    /// antenna spring: tip position and velocity (UE world units)
    tip: Option<(Vec3, Vec3)>,
    last_yaw: f32,
    /// rotor turns (fraction of a revolution per rotor)
    rotor: Vec<f32>,
    /// air drones: body tilt (pitch, roll in radians) following the acceleration
    pub tilt: Vec2,
}

impl DroneAnim {
    /// A drone set down facing `yaw` (radians).
    pub fn new(yaw: f32) -> DroneAnim {
        DroneAnim { last_yaw: yaw, ..default() }
    }

    /// Wheel roll angles left / right (radians).
    pub fn roll(&self) -> (f32, f32) {
        self.roll
    }
}

/// Adds a rotation given in the reference pose's component frame (pose space: z forward, -y up,
/// -x right; skin_part maps it to UE axes) at bone `k`: the bone and its children turn about
/// the bone's pivot.
pub(crate) fn add_comp(local: &mut [(Quat, Vec3)], skeleton: &[upk::skeletal::Bone], ref_comp: &[(Quat, Vec3)], k: Option<usize>, r: Quat) {
    let Some(k) = k.filter(|&k| k < local.len()) else { return };
    let pa = skeleton[k].parent;
    if k == 0 || pa < 0 || pa as usize >= k {
        local[k].0 = r * local[k].0;
    } else {
        let p = ref_comp[pa as usize].0;
        local[k].0 = p.conjugate() * r * p * local[k].0;
    }
}

/// Component-space pose of the drone for this frame: the reference pose with the AnimTree's
/// controls applied. NOT from data: the angle sign conventions were checked against the bone
/// axes (Turret_Pitch: barrel along +X, pitch about Y), the treads turn at most 360 deg/s and
/// face backwards when the drone reverses, the wheel roll is distance / WheelRadius * RollFactor.
#[allow(clippy::too_many_arguments)]
pub fn drone_pose(a: &DroneAssets, st: &mut DroneAnim, yaw: f32, pitch: f32, vel: Vec3, origin: Vec3, dt: f32) -> Vec<(Quat, Vec3)> {
    let rig = &a.rig;
    let mut local: Vec<(Quat, Vec3)> = a.skeleton.iter().map(|b| (Quat::from_xyzw(b.rot[0], b.rot[1], b.rot[2], b.rot[3]).normalize(), Vec3::from(b.pos))).collect();
    let ref_comp = &a.pose;
    // velocity in UE units relative to the body (x forward, y right)
    let v_ue = Vec3::new(vel.x, vel.z, 0.0) * 100.0;
    let body = Quat::from_rotation_z(yaw);
    let rel = body.conjugate() * v_ue;
    let speed = Vec2::new(rel.x, rel.y).length();
    if speed > rig.rest_velocity {
        let mut want = rel.y.atan2(rel.x);
        let mut reverse = 1.0;
        if want.abs() > std::f32::consts::FRAC_PI_2 {
            want -= std::f32::consts::PI * want.signum();
            reverse = -1.0;
        }
        // the body turned with the view: the treads keep their world heading
        st.treads -= wrap(yaw - st.last_yaw);
        let d = wrap(want - st.treads);
        let max = std::f32::consts::TAU * dt;
        st.treads = wrap(st.treads + d.clamp(-max, max));
        // wheel roll: tread speed along the tread heading, plus the turn of the body
        let along = (rel.x * st.treads.cos() + rel.y * st.treads.sin()).abs() * reverse;
        let turn = wrap(yaw - st.last_yaw) / dt.max(1e-4) * rig.track_half;
        let roll = |s: f32| s * dt / rig.wheel_radius.max(0.1) * rig.roll_factor;
        st.roll.0 = wrap(st.roll.0 + roll(along + turn));
        st.roll.1 = wrap(st.roll.1 + roll(along - turn));
    } else {
        st.treads -= wrap(yaw - st.last_yaw);
        st.treads = wrap(st.treads);
    }
    st.last_yaw = yaw;
    // pose space: the treads turn about the up axis (-y), towards the right (-x) for a positive
    // angle; the wheels roll about the axle (x), forward = negative; the gun pitches about x,
    // up for a positive view pitch
    add_comp(&mut local, &a.skeleton, ref_comp, rig.treads, Quat::from_rotation_y(-st.treads));
    // GCAnimNodeVehicleWheel: WheelRollAxis AXIS_Y (the axle), bInvertWheelRoll
    for &k in &rig.wheels_l {
        add_comp(&mut local, &a.skeleton, ref_comp, Some(k), Quat::from_rotation_x(-st.roll.0));
    }
    for &k in &rig.wheels_r {
        add_comp(&mut local, &a.skeleton, ref_comp, Some(k), Quat::from_rotation_x(-st.roll.1));
    }
    // turret: the gun follows the view pitch (the body yaws with the view, so the turret yaw
    // relative to it stays 0)
    add_comp(&mut local, &a.skeleton, ref_comp, rig.turret_yaw, Quat::IDENTITY);
    add_comp(&mut local, &a.skeleton, ref_comp, rig.turret_pitch, Quat::from_rotation_x(pitch));
    // rotors: about the bone's own axes at vMaxRotationsPerSecond (NOT from data: the vector's
    // components taken as yaw / pitch / roll rates; ROTOR_MAIN (5, 0, 0) spins about its up
    // axis, ROTOR_TAIL (0, 3, 0) about its side axis)
    st.rotor.resize(rig.rotors.len(), 0.0);
    for (r, (bones, rps)) in rig.rotors.iter().enumerate() {
        st.rotor[r] = (st.rotor[r] + rps.length() * dt).fract();
        let axis = Vec3::new(rps.z, rps.y, rps.x).normalize_or_zero();
        if axis == Vec3::ZERO {
            continue;
        }
        let q = Quat::from_axis_angle(axis, st.rotor[r] * std::f32::consts::TAU);
        for &(k, _, rotates) in bones {
            if rotates && k < local.len() {
                local[k].0 *= q;
            }
        }
    }
    let mut comp = compose(&a.skeleton, &local);
    // antenna: the tip lags behind on a spring (SpringStiffness / SpringDamping, at most
    // MaxDisplacement), the lower segment turns towards it
    if let (Some(bot), Some(top)) = (rig.antenna_bot, rig.antenna_top) {
        let (k, c, maxd) = rig.spring;
        let rot = body;
        let rest_world = origin + rot * mesh_to_comp(comp[top].1);
        let (mut p, mut v) = st.tip.unwrap_or((rest_world, Vec3::ZERO));
        if dt > 0.0 {
            let acc = (rest_world - p) * k - v * c;
            v += acc * dt;
            p += v * dt;
        }
        let mut off = p - rest_world;
        if maxd > 0.0 && off.length() > maxd {
            off = off.normalize() * maxd;
            p = rest_world + off;
        }
        st.tip = Some((p, v));
        let off_mesh = comp_to_mesh(rot.conjugate() * off);
        let (bp, top_p) = (comp[bot].1, comp[top].1);
        let d0 = top_p - bp;
        let d1 = d0 + off_mesh;
        if d0.length() > 1e-3 && d1.length() > 1e-3 {
            let q = Quat::from_rotation_arc(d0.normalize(), d1.normalize());
            comp[bot].0 = q * comp[bot].0;
            comp[top] = (q * comp[top].0, bp + q * d0);
        }
    }
    comp
}

/// Per-bone scale of the rotor swap (0 hides the still / blurred rotor), 1 elsewhere.
pub fn rotor_scales(a: &DroneAssets) -> Vec<f32> {
    let mut v = vec![1.0; a.skeleton.len()];
    for (bones, _) in &a.rig.rotors {
        for &(k, sc, _) in bones {
            if let Some(x) = v.get_mut(k) {
                *x = sc;
            }
        }
    }
    v
}

fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(t) - std::f32::consts::PI
}

/// Mesh (bone) space -> the component axes skin_part uses ((z, -x, -y), bots.rs).
fn mesh_to_comp(p: Vec3) -> Vec3 {
    Vec3::new(p.z, -p.x, -p.y)
}

fn comp_to_mesh(c: Vec3) -> Vec3 {
    Vec3::new(-c.y, -c.z, c.x)
}

pub(crate) fn compose(skeleton: &[upk::skeletal::Bone], local: &[(Quat, Vec3)]) -> Vec<(Quat, Vec3)> {
    let mut out: Vec<(Quat, Vec3)> = Vec::with_capacity(local.len());
    for (i, b) in skeleton.iter().enumerate() {
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

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum DronePhase {
    #[default]
    Idle,
    Placing,
    Active,
    /// set down but not controlled (the player left it with E): it stays, can be taken over
    /// again with its slot key, keeps losing battery and can be shot
    Parked,
}

/// GCBattlePoints.ini Rewards (Id=PSR_PaintTarget, Points=20)
const PAINT_TARGET_KP: i32 = 20;

/// Air drones: speed easing (1/s) and the most the body leans (degrees). NOT from data.
pub(crate) const FLY_ACCEL: f32 = 2.5;
pub(crate) const FLY_TILT: f32 = 14.0;

#[derive(Resource, Default)]
pub struct Drone {
    pub phase: DronePhase,
    /// feet (Bevy metres), view yaw / pitch (UE rotation units like the pawn)
    pub feet: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    vel: Vec3,
    on_ground: bool,
    pub fuel: f32,
    pub health: f32,
    heat: f32,
    overheated: f32,
    fire_cd: f32,
    /// set by combat.rs when a drone is bought: its kind (DroneKinds index)
    pub request: Option<usize>,
    /// kind of the drone out (DroneKinds index) and its cost key
    pub kind: usize,
    pub key: &'static str,
    /// rocket drone: rounds left per fire mode, reload time left, the burst being fired
    /// (mode, shots left, time to the next), shots fired (socket rotation)
    ammo: [i32; 2],
    reload: [f32; 2],
    burst: Option<(usize, i32, f32)>,
    shots: usize,
    /// the cockpit's placement this frame (camera socket UE, view rotation) in the first-person view
    cockpit_at: Option<(Vec3, Quat)>,
    /// first-person view (EKBC_BehindView toggles; the drones start in the behind view as in the
    /// user's gameplay video)
    pub first_person: bool,
    seed: u32,
    shown: Option<(bool, usize, bool)>,
    /// seconds in the current phase (test hook HB_DRONETEST)
    phase_t: f32,
    anim: DroneAnim,
    /// take over the parked drone (combat.rs: the drone's slot key)
    pub reenter: bool,
    /// where the last hit came from (UE units)
    pub last_hit: Option<Vec3>,
}

impl Drone {
    /// Set down and not destroyed: a target for the enemy bots.
    pub fn deployed(&self) -> bool {
        matches!(self.phase, DronePhase::Active | DronePhase::Parked) && self.health > 0.0
    }
}

/// The drone's centre (UE units) while it is deployed, for the bots and explosions.
static TARGET: std::sync::Mutex<Option<Vec3>> = std::sync::Mutex::new(None);

pub fn target() -> Option<Vec3> {
    TARGET.lock().ok().and_then(|g| *g)
}

/// Health of the drone: 3x the player's (user 2026-10-19; NOT from data: the class default
/// GCV_SH_GAssaultWalker_Content Health is 730)
pub const DRONE_HEALTH: f32 = crate::combat::HEALTH_MAX * 3.0;

/// Component-space reference pose of a skeleton.
pub fn ref_pose(skeleton: &[upk::skeletal::Bone]) -> Vec<(Quat, Vec3)> {
    let mut out: Vec<(Quat, Vec3)> = Vec::with_capacity(skeleton.len());
    for (i, b) in skeleton.iter().enumerate() {
        let lr = Quat::from_xyzw(b.rot[0], b.rot[1], b.rot[2], b.rot[3]).normalize();
        let lp = Vec3::from(b.pos);
        if i == 0 || b.parent < 0 || b.parent as usize >= i {
            out.push((lr, lp));
        } else {
            let (pr, pp) = out[b.parent as usize];
            out.push((pr * lr, pp + pr * lp));
        }
    }
    out
}

fn ue(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y) * 100.0
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_drone(
    time: Res<Time>,
    (keys, _mouse, motion): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>, Res<bevy::input::mouse::AccumulatedMouseMotion>),
    mut flow: ResMut<MatchFlow>,
    kinds: Option<Res<DroneKinds>>,
    data: Option<Res<CombatData>>,
    mut drone: ResMut<Drone>,
    col: Res<CollisionWorld>,
    mut pawns: Query<&mut Pawn>,
    soldiers: Query<(Entity, &Soldier)>,
    mut cams: Query<(&Camera, &mut Transform), (With<Camera3d>, Without<crate::minimap::MinimapCam>, Without<Mesh3d>)>,
    (mut queue, mut sfx, mut wsfx, mut fx, impacts): (ResMut<DamageQueue>, ResMut<crate::sfx::SfxQueue>, ResMut<crate::sfx::WeaponSfx>, ResMut<crate::particles::Particles>, Option<Res<crate::impacts::Impacts>>),
    (mut meshes, mut ents, mut commands): (ResMut<Assets<Mesh>>, Query<(Entity, &Mesh3d, &mut Visibility, &mut Transform), Without<Camera3d>>, Commands),
    (mut buf, mut vm, mut gr, mut marks, dbgv): (Local<(Vec<u32>, Vec<Vec3>)>, Option<ResMut<crate::viewmodel::ViewModel>>, ResMut<crate::combat::Grenades>, ResMut<ReconMarks>, Query<(&InheritedVisibility, &ViewVisibility, &GlobalTransform, Option<&ChildOf>, Has<bevy::render::view::NoFrustumCulling>)>),
    (me, mut skin_cache): (Res<crate::MeshEntities>, Local<DroneSkinCache>),
) {
    let _perf = crate::perf::scope("drone");
    flow.hud.drone = None;
    flow.hud.hint = None;
    flow.hud.lock_box = None;
    marks.candidates.clear();
    let Some(kinds) = kinds else { return };
    if kinds.0.is_empty() {
        return;
    }
    // a new match loads the level again: new drone mesh entities (spawned visible, at the origin,
    // in UE units = the giant drone in the user's screenshot 2026-10-20 01:06); the cached shown
    // state still said "hidden", so they were never hidden
    if kinds.is_changed() {
        drone.shown = None;
        *skin_cache = DroneSkinCache::default();
    }
    // a new drone bought: its kind (a parked one of the other kind is given up); requests carry
    // the main.rs DRONE_KINDS index
    if let Some(k) = drone.request.and_then(|r| crate::DRONE_KINDS.get(r)).and_then(|key| kinds.0.iter().position(|a| a.key == *key)) {
        if drone.phase == DronePhase::Idle || drone.phase == DronePhase::Parked && drone.kind != k {
            drone.phase = DronePhase::Idle;
            drone.kind = k;
        }
    }
    let kind = drone.kind.min(kinds.0.len() - 1);
    let assets = &kinds.0[kind];
    drone.key = assets.key;
    let playing = flow.phase == Phase::Playing;
    let dt = time.delta_secs().min(0.1);
    let mut visible = false;
    drone.cockpit_at = None;
    if let Ok(mut p) = pawns.single_mut() {
        if !playing {
            if drone.phase == DronePhase::Active {
                p.remote = false;
            }
            drone.phase = DronePhase::Idle;
            drone.request = None;
        } else if p.dead {
            // the player died: a deployed drone stays where it is
            if drone.phase == DronePhase::Active {
                p.remote = false;
                drone.phase = DronePhase::Parked;
            } else if drone.phase == DronePhase::Placing {
                drone.phase = DronePhase::Idle;
            }
            drone.request = None;
        }
        // destroyed (shot / blown up) or the battery is empty: an explosion where it stood
        // (FX_C4_EXP, NOT from data: no destroy effect found in the vehicle's data)
        if drone.deployed() || drone.phase == DronePhase::Parked || drone.phase == DronePhase::Active {
            if drone.health <= 0.0 || drone.fuel <= 0.0 {
                let at = ue(drone.feet) + Vec3::Z * 30.0;
                if drone.health <= 0.0 {
                    fx.fire("KFX_Pyrotechnics.FX_C4.FX_C4_EXP", at, Quat::IDENTITY);
                    info!("drone destroyed at {:?}", drone.feet);
                }
                if drone.phase == DronePhase::Active {
                    p.remote = false;
                    if let Some(vm) = vm.as_mut() {
                        vm.lowered = false;
                        vm.play(crate::viewmodel::WeaponAnim::Draw);
                    }
                }
                drone.phase = DronePhase::Idle;
            }
        }
        // take over the parked drone again
        if std::mem::take(&mut drone.reenter) && drone.phase == DronePhase::Parked && !p.dead {
            drone.phase = DronePhase::Active;
            p.remote = true;
            if let Some(vm) = vm.as_mut() {
                vm.lowered = true;
            }
        }
        // HB_DRONETEST=start: the drone without buying it (rig tests); HB_DRONEKIND=1 the anti
        // tank drone
        if std::env::var("HB_DRONETEST").is_ok_and(|v| v == "start") && drone.phase == DronePhase::Idle && drone.fuel == 0.0 && playing && !p.dead {
            let k = std::env::var("HB_DRONEKIND").ok().and_then(|v| v.parse().ok()).unwrap_or(0usize);
            if let Some(i) = crate::DRONE_KINDS.get(k).and_then(|key| kinds.0.iter().position(|a| a.key == *key)) {
                drone.kind = i;
            }
            drone.request = Some(k);
            drone.fuel = -1.0;
        }
        if drone.request.take().is_some() && drone.phase == DronePhase::Idle {
            drone.phase = DronePhase::Placing;
            drone.phase_t = 0.0;
        }
        drone.phase_t += dt;
        // HB_DRONETEST: set down after 3 s, then drive forward and fire (tests)
        let test = std::env::var_os("HB_DRONETEST").is_some();
        let yaw_r = p.yaw / 65536.0 * std::f32::consts::TAU;
        let fwd = Vec3::new(yaw_r.cos(), 0.0, yaw_r.sin());
        match drone.phase {
            DronePhase::Idle => {}
            DronePhase::Parked => {
                visible = true;
                drone.fuel -= dt * assets.fuel_idle;
                if assets.flying {
                    // hovers where it was left (NOT from data)
                    drone.vel = Vec3::ZERO;
                } else {
                    // stands on the floor (gravity, no input)
                    let (mut feet, mut vel, mut on_ground) = (drone.feet, drone.vel, drone.on_ground);
                    crate::player::walk(&col, &mut feet, &mut vel, &mut on_ground, true, Vec3::ZERO, assets.speed * 0.01, false, dt, &mut buf);
                    drone.feet = feet;
                    drone.vel = vel;
                    drone.on_ground = on_ground;
                }
            }
            DronePhase::Placing => {
                // preview 2 m ahead on the floor; LMB (EKBC_Fire) sets it down
                let mut at = p.feet + fwd * 2.0;
                if let Some((y, _)) = col.floor(at + Vec3::Y * 1.0, 3.0, &mut buf.0) {
                    at.y = y;
                }
                // air drones: hovering at the player's eye height (NOT from data)
                if assets.flying {
                    at.y = p.feet.y + p.eye - assets.half_height * 0.01;
                }
                drone.feet = at;
                drone.yaw = p.yaw;
                visible = true;
                flow.hud.hint = Some("deploy_drone");
                if !flow.paused && (crate::input::hit(crate::input::Set::Foot, "EKBC_Fire") || (test && drone.phase_t > 0.5)) {
                    info!("drone {} deployed at {:?}", assets.key, drone.feet);
                    drone.phase = DronePhase::Active;
                    drone.pitch = 0.0;
                    drone.vel = Vec3::ZERO;
                    drone.on_ground = false;
                    drone.fuel = assets.fuel_max;
                    drone.health = DRONE_HEALTH;
                    drone.heat = 0.0;
                    drone.overheated = 0.0;
                    drone.fire_cd = 0.3;
                    // rocket drone: full clips
                    let modes = data.as_ref().and_then(|d| d.drone_rockets.get(assets.key)).map(|v| v.as_slice()).unwrap_or(&[]);
                    for (m, (rm, _)) in modes.iter().enumerate().take(2) {
                        drone.ammo[m] = rm.clip;
                        drone.reload[m] = 0.0;
                    }
                    drone.burst = None;
                    // HB_DRONEFP=1: start in the first-person view (tests)
                    drone.first_person = std::env::var_os("HB_DRONEFP").is_some();
                    p.remote = true;
                    if let Some(vm) = vm.as_mut() {
                        vm.lowered = true;
                    }
                }
            }
            DronePhase::Active => {
                visible = true;
                let input = !flow.paused;
                if input {
                    let (sx, sy) = crate::settings::mouse_scale_for("Land", motion.delta, false);
                    drone.yaw += motion.delta.x * crate::player::MOUSE_SENS_PUB * sx;
                    drone.pitch = (drone.pitch - motion.delta.y * crate::player::MOUSE_SENS_PUB * sy).clamp(-12000.0, 12000.0);
                }
                let yr = drone.yaw / 65536.0 * std::f32::consts::TAU;
                let pr = drone.pitch / 65536.0 * std::f32::consts::TAU;
                let f = Vec3::new(yr.cos(), 0.0, yr.sin());
                let r = Vec3::new(-yr.sin(), 0.0, yr.cos());
                let mut dir = Vec3::ZERO;
                if input {
                    // the drone is a vehicle: BindingsLand
                    use crate::input::{down, Set};
                    if down(Set::Land, "EKBC_MoveForward") {
                        dir += f;
                    }
                    if down(Set::Land, "EKBC_MoveBackward") {
                        dir -= f;
                    }
                    if down(Set::Land, "EKBC_StrafeRight") {
                        dir += r;
                    }
                    if down(Set::Land, "EKBC_StrafeLeft") {
                        dir -= r;
                    }
                    if test && drone.phase_t < 3.0 {
                        dir += f;
                        // HB_DRONEPITCH=<deg> / HB_DRONESTRAFE=1: pose tests of the rig
                        if let Some(d) = std::env::var("HB_DRONEPITCH").ok().and_then(|v| v.parse::<f32>().ok()) {
                            drone.pitch = d / 360.0 * 65536.0;
                        }
                        if std::env::var_os("HB_DRONESTRAFE").is_some() {
                            dir = r;
                        }
                    }
                    if test && std::env::var_os("HB_DRONESTRAFE").is_some() && drone.phase_t < 2.0 {
                        dir = r;
                    }
                    // HB_DRONEDRIVE=<s>: forward / backward in 2 s turns for that long (tread tests)
                    if let Some(t) = std::env::var("HB_DRONEDRIVE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|&t| test && drone.phase_t < t) {
                        let _ = t;
                        dir = if (drone.phase_t / 2.0) as i32 % 2 == 0 { f } else { -f };
                    }
                }
                if assets.flying {
                    // air drones (bCanFly, bCanStrafe) at AirSpeed: W / S along the view's heading,
                    // A / D strafe, Space / left Ctrl climb / sink (BindingsHeli EKBC_Ascend /
                    // EKBC_Descend keys; user request: not along the view pitch any more). NOT
                    // from data (the RC heli physics are native): the speed eases in / out
                    // (FLY_ACCEL) and the body leans into the motion (up to FLY_TILT)
                    let mut d3 = Vec3::ZERO;
                    if input {
                        use crate::input::{down, Set};
                        if down(Set::Land, "EKBC_MoveForward") || test && drone.phase_t < 3.0 {
                            d3 += f;
                        }
                        if down(Set::Land, "EKBC_MoveBackward") {
                            d3 -= f;
                        }
                        if down(Set::Land, "EKBC_StrafeRight") {
                            d3 += r;
                        }
                        if down(Set::Land, "EKBC_StrafeLeft") {
                            d3 -= r;
                        }
                        if keys.pressed(KeyCode::Space) {
                            d3 += Vec3::Y;
                        }
                        if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
                            d3 -= Vec3::Y;
                        }
                    }
                    let want = d3.normalize_or_zero() * assets.speed * 0.01;
                    let k = (FLY_ACCEL * dt).min(1.0);
                    let v0 = drone.vel;
                    drone.vel = v0 + (want - v0) * k;
                    // lean: nose down flying forward, banked into a strafe
                    let sp = (assets.speed * 0.01).max(0.1);
                    let (vf, vr) = (drone.vel.dot(f) / sp, drone.vel.dot(r) / sp);
                    let tilt = Vec2::new(-vf, -vr) * FLY_TILT.to_radians();
                    let kt = (FLY_ACCEL * 1.5 * dt).min(1.0);
                    let t0 = drone.anim.tilt;
                    drone.anim.tilt = t0 + (tilt - t0) * kt;
                    let step = drone.vel * dt;
                    let len = step.length();
                    if len > 1e-5 {
                        let centre = drone.feet + Vec3::Y * assets.half_height * 0.01;
                        let clear = assets.radius.max(assets.half_height) * 0.01;
                        let free = col.raycast(centre, step / len, len + clear, &mut buf.0).map(|(t, _)| (t - clear).max(0.0)).unwrap_or(len);
                        drone.feet += step / len * free.min(len);
                        if free < len {
                            drone.vel *= 0.3;
                        }
                    }
                    drone.on_ground = false;
                } else {
                    let (mut feet, mut vel, mut on_ground) = (drone.feet, drone.vel, drone.on_ground);
                    crate::player::walk(&col, &mut feet, &mut vel, &mut on_ground, true, dir.normalize_or_zero(), assets.speed * 0.01, false, dt, &mut buf);
                    drone.feet = feet;
                    drone.vel = vel;
                    drone.on_ground = on_ground;
                }
                let vel = drone.vel;
                let moving = vel.length() > 0.2;
                drone.fuel -= dt * if moving { assets.fuel_moving } else { assets.fuel_idle };
                // camera: Seats[0] CameraOffset above the cylinder centre
                let view = Quat::from_rotation_z(yr) * Quat::from_rotation_y(-pr);
                let socket_b = drone.feet + Vec3::Y * (assets.half_height + assets.camera_z + assets.lift()) * 0.01;
                let back = view * Vec3::X * assets.camera_back * 0.01;
                let eye_b = socket_b - Vec3::new(back.x, back.z, back.y);
                if input && crate::input::hit(crate::input::Set::Global, "EKBC_BehindView") {
                    drone.first_person = !drone.first_person;
                }
                // behind view: BehindCameraMaxOffset in the view's axes, pulled in before the
                // world (NOT from data: the pull-in margin)
                let cam_b = if drone.first_person {
                    eye_b
                } else {
                    let o = view * assets.behind * 0.01;
                    let off = Vec3::new(o.x, o.z, o.y);
                    let len = off.length();
                    let t = if len > 1e-4 { col.raycast(socket_b, off / len, len, &mut buf.0).map(|(t, _)| (t - 0.1).max(0.0)).unwrap_or(len) } else { 0.0 };
                    socket_b + off.normalize_or_zero() * t
                };
                if drone.first_person {
                    drone.cockpit_at = Some((ue(eye_b), view));
                }
                flow.hud.lock_box = assets.lock_box;
                if let Ok((camera, mut ct)) = cams.single_mut() {
                    let d = view * Vec3::X;
                    // air recon drone: the enemies inside the lock box, close enough and in sight;
                    // holding fire marks them
                    if let (Some((dist, secs)), Some((lo, hi))) = (assets.mark, assets.lock_box) {
                        let gt = GlobalTransform::from(Transform::from_translation(cam_b).looking_to(Vec3::new(d.x, d.z, d.y), Vec3::Y));
                        let vp = camera.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
                        let k = Vec2::new(1280.0 / vp.x.max(1.0), 720.0 / vp.y.max(1.0));
                        let me = ue(drone.feet) + Vec3::Z * assets.half_height;
                        for (e, s) in &soldiers {
                            if !s.alive || s.team == player_team() {
                                continue;
                            }
                            let c = s.center_ue();
                            if (c - me).length() > dist {
                                continue;
                            }
                            let cb = Vec3::new(c.x, c.z, c.y) * 0.01;
                            let Ok(sp) = camera.world_to_viewport(&gt, cb) else { continue };
                            let sp = sp * k - Vec2::new(640.0, 360.0);
                            if sp.x < lo[0] || sp.x > hi[0] || sp.y < lo[1] || sp.y > hi[1] {
                                continue;
                            }
                            let to = cb - cam_b;
                            let l = to.length();
                            if l > 0.01 && col.raycast(cam_b, to / l, l - 0.3, &mut buf.0).is_some() {
                                continue;
                            }
                            marks.candidates.push(e);
                        }
                        if input && (crate::input::down(crate::input::Set::Land, "EKBC_Fire") || test && drone.phase_t > 2.0) {
                            for e in marks.candidates.clone() {
                                // GCBattlePoints Rewards (Id=PSR_PaintTarget, Points=20) per newly
                                // marked enemy (GCGAMEEVENT_DRONE_PAINT_TARGET)
                                if marks.marked.insert(e, secs).is_none() {
                                    flow.hud.kp += PAINT_TARGET_KP;
                                    flow.hud.rewards.push((PAINT_TARGET_KP, "PSR_PaintTarget"));
                                }
                            }
                        } else if marks.candidates.iter().any(|e| !marks.marked.contains_key(e)) {
                            flow.hud.hint = Some("recon_mark");
                        }
                    }
                    ct.translation = cam_b;
                    ct.look_to(Vec3::new(d.x, d.z, d.y), Vec3::Y);
                    // HB_DRONECAM=1: outside view from the front left (rig tests)
                    if std::env::var_os("HB_DRONECAM").is_some() {
                        let fb = Vec3::new(d.x, 0.0, d.z).normalize_or_zero();
                        let side = Vec3::new(-fb.z, 0.0, fb.x);
                        ct.translation = drone.feet + fb * 1.6 - side * 1.2 + Vec3::Y * 0.9;
                        ct.look_at(drone.feet + Vec3::Y * 0.35, Vec3::Y);
                    }
                }
                // gun (CFG_GAssault): overheat, fuel per shot
                let g = &assets.gun;
                drone.fire_cd = (drone.fire_cd - dt).max(0.0);
                if drone.overheated > 0.0 {
                    drone.overheated -= dt;
                    if drone.overheated <= 0.0 {
                        drone.heat = 0.0;
                    }
                } else {
                    drone.heat = (drone.heat - dt).max(0.0);
                }
                let rockets = data.as_ref().is_some_and(|d| d.drone_rockets.contains_key(assets.key));
                if rockets {
                    // MP.Vehicle.CFG_GAntiTank: FireModes[0] (EKBC_Fire) the dumb-fire missile,
                    // FireModes[1] (EKBC_AltFire) a burst of FireBurstCount missiles; a clip of
                    // AmmoClip, ReloadTime when empty (bUseLockOn of mode 1: no lock-on here, the
                    // missiles fly straight)
                    let modes: Vec<(crate::weaponcfg::RocketMode, usize)> = data.as_ref().and_then(|d| d.drone_rockets.get(assets.key).cloned()).unwrap_or_default();
                    for m in 0..modes.len().min(2) {
                        if drone.reload[m] > 0.0 {
                            drone.reload[m] -= dt;
                            if drone.reload[m] <= 0.0 {
                                drone.ammo[m] = modes[m].0.clip;
                            }
                        }
                    }
                    if drone.burst.is_none() && input {
                        use crate::input::{hit, Set};
                        let want = if hit(Set::Land, "EKBC_Fire") || test && drone.phase_t > 1.0 && drone.ammo[0] > 0 && drone.shots == 0 {
                            Some(0)
                        } else if hit(Set::Land, "EKBC_AltFire") || test && drone.phase_t > 4.0 && drone.shots == 1 {
                            Some(1)
                        } else {
                            None
                        };
                        if let Some(m) = want.filter(|&m| m < modes.len() && drone.ammo[m] > 0 && drone.reload[m] <= 0.0) {
                            drone.burst = Some((m, modes[m].0.burst.max(1).min(drone.ammo[m]), 0.0));
                        }
                    }
                    if let Some((m, left, cd)) = drone.burst {
                        let cd = cd - dt;
                        if cd > 0.0 {
                            drone.burst = Some((m, left, cd));
                        } else {
                            let (rm, wi) = &modes[m];
                            // Seat0FireMode<m>Socket<k> on Turret_Pitch (the camera socket), in
                            // the view's frame
                            let sock = assets.fire_sockets.get(m).filter(|v| !v.is_empty()).map(|v| v[drone.shots % v.len()]).unwrap_or([45.0, 0.0, 0.0]);
                            let o_ue = ue(socket_b) + view * Vec3::new(sock[0], sock[1], sock[2]);
                            if drone.seed == 0 {
                                drone.seed = 0x2468_ace1;
                            }
                            let d = crate::combat::spread_dir(view * Vec3::X, rm.spread * 0.5, &mut drone.seed);
                            let trail = rm.missile.flight_fx.as_ref().and_then(|t| fx.fire_tracked(t, o_ue, crate::combat::dir_quat(d)));
                            // GCProj_Rocket LifeSpan 8
                            gr.list.push(crate::combat::Grenade { rocket: Some(*wi), pos: o_ue, vel: d * rm.missile.speed, fuse: 8.0, owner: Actor::Player, team: player_team(), resting: false, trail, tac: None, payload: None });
                            if let Some(snd) = assets.mode_sounds.get(m).filter(|s| !s.is_empty()) {
                                sfx.play(snd, None);
                            }
                            drone.fuel -= assets.fuel_shot;
                            if test {
                                info!("drone rocket: mode {m} from {o_ue:?} speed {}", rm.missile.speed);
                            }
                            drone.shots += 1;
                            drone.ammo[m] -= 1;
                            if drone.ammo[m] <= 0 {
                                drone.reload[m] = rm.reload;
                            }
                            drone.burst = (left > 1 && drone.ammo[m] > 0).then_some((m, left - 1, rm.interval));
                        }
                    }
                }
                // instant-hit gun (CFG_GAssault); the air recon drone (CFG_SH_ARecon EWFT_Custom) has none
                if !rockets && !g.dmg_type.is_empty() && input && (crate::input::down(crate::input::Set::Land, "EKBC_Fire") || test) && drone.overheated <= 0.0 && drone.fire_cd <= 0.0 {
                    drone.fire_cd = g.fire_interval;
                    drone.heat += g.overheat_add + g.fire_interval;
                    if drone.heat >= g.overheat_max {
                        drone.overheated = g.overheat_recover;
                    }
                    drone.fuel -= assets.fuel_shot;
                    if g.sound_looping {
                        wsfx.shot_cue("drone_gun", &g.sound, None, g.fire_interval + 0.06);
                    } else {
                        sfx.play(&g.sound, None);
                    }
                    if drone.seed == 0 {
                        drone.seed = 0x2468_ace1;
                    }
                    let o = ue(eye_b);
                    let d = crate::combat::spread_dir(view * Vec3::X, g.spread * 0.5, &mut drone.seed);
                    if let Some(data) = data.as_ref() {
                        let list: Vec<(Entity, Vec3, u8, bool)> = soldiers.iter().map(|(e, s)| (e, s.center_ue(), s.team, s.alive)).collect();
                        let shot = crate::combat::trace_shot(&col, &list, None, o, d, g.range, None, true, &mut buf.0);
                        if let Some(im) = impacts.as_ref() {
                            let center = if let ShotHit::Soldier(e, ..) = &shot { list.iter().find(|s| s.0 == *e).map(|s| s.1) } else { None };
                            im.shot(&shot, o, d, &g.dmg_type, center, &mut fx, &mut sfx);
                        }
                        // the bots' drones (botdrones.rs; their boxes stop the shot)
                        if let Some((de, t)) = crate::botdrones::bullet_hit(o, d, &shot, g.range, player_team()) {
                            let dmg = data.grid.damage(&g.dmg_type, "PMA_Soldier_Chest", t, assets.full_range, g.range);
                            queue.0.push(Hit { target: Target::BotDrone(de), amount: dmg, attacker: Actor::Player, attacker_team: player_team(), headshot: false, explosive: false, from: o, icon: None, dmg_type: Some(g.dmg_type.clone()) });
                        }
                        if let ShotHit::Soldier(e, t, hit) = shot {
                            if let Some(s) = list.iter().find(|s| s.0 == e).filter(|s| s.2 != player_team()) {
                                let (mat, head) = if hit.z > s.1.z + crate::combat::HEAD_OFFSET - crate::combat::HEAD_RADIUS { ("PMA_Head", true) } else { ("PMA_Soldier_Chest", false) };
                                let dmg = data.grid.damage(&g.dmg_type, mat, t, assets.full_range, g.range);
                                queue.0.push(Hit { target: Target::Bot(e), amount: dmg, attacker: Actor::Player, attacker_team: player_team(), headshot: head, explosive: false, from: o, icon: None, dmg_type: None });
                            }
                        }
                    }
                }
                if !g.engine_idle.is_empty() {
                    sfx.looping_params("drone_engine", &g.engine_idle, None, &[("Gas", moving as usize)], 1.0);
                }
                flow.hud.drone = Some(((drone.fuel / assets.fuel_max).clamp(0.0, 1.0), (drone.heat / g.overheat_max).clamp(0.0, 1.0)));
                flow.hud.drone_classes = assets.classes.clone();
                if input && crate::input::hit(crate::input::Set::Land, "EKBC_Use") {
                    // leave it standing (user 2026-10-19: it stays and can be taken over again)
                    drone.phase = DronePhase::Parked;
                    p.remote = false;
                    if let Some(vm) = vm.as_mut() {
                        vm.lowered = false;
                        vm.play(crate::viewmodel::WeaponAnim::Draw);
                    }
                }
            }
        }
    }
    if let Ok(mut g) = TARGET.lock() {
        *g = drone.deployed().then(|| ue(drone.feet) + Vec3::Z * assets.half_height);
    }
    // the drone mesh (reference pose), yaw with the view
    let yr = drone.yaw / 65536.0 * std::f32::consts::TAU;
    let origin = ue(drone.feet) + Vec3::Z * assets.tread_low.unwrap_or(assets.half_height + assets.mesh_z);
    let tr = Vec3::new(origin.x, origin.z, origin.y) * 0.01;
    // bDisable3pMeshIn1p: the body is hidden in the first-person view, the cockpit shown instead
    let body = visible && drone.cockpit_at.is_none();
    let mut cockpit_tr = Vec3::ZERO;
    if let (Some((at, view)), Some(c)) = (drone.cockpit_at, assets.cockpit.as_ref()) {
        let origin = at + view * c.translation - view * mesh_to_comp(c.anchor);
        // unchanged view / place (drone standing, view still): the mesh already holds this skin
        let key = (kind, view, origin);
        if skin_cache.cockpit != Some(key) {
            crate::bots::skin_part(&c.part, &c.pose, view, origin, &mut meshes);
            skin_cache.cockpit = Some(key);
        }
        cockpit_tr = Vec3::new(origin.x, origin.z, origin.y) * 0.01;
    }
    if body {
        let pr = if drone.phase == DronePhase::Active { drone.pitch / 65536.0 * std::f32::consts::TAU } else { 0.0 };
        let vel = drone.vel;
        let dr = &mut *drone;
        if dr.phase != DronePhase::Active {
            dr.anim = DroneAnim { last_yaw: yr, ..default() };
        }
        let mut pose = drone_pose(assets, &mut dr.anim, yr, pr, vel, origin, dt * crate::rigdebug::anim_scale());
        if env_flag(&ENV_DRONEREF, "HB_DRONEREF") {
            pose = assets.pose.clone();
        }
        let scales = rotor_scales(assets);
        // air drones lean into their acceleration (body tilt: pitch about the side axis, roll
        // about the forward axis)
        let body_rot = Quat::from_rotation_z(yr) * Quat::from_rotation_y(-dr.anim.tilt.x) * Quat::from_rotation_x(dr.anim.tilt.y);
        // the same pose / place / rotor scales as the last skin (parked or standing drone): the
        // meshes already hold it; re-skinning re-uploaded every drone mesh each frame (update_drone
        // up to 3.6 ms per frame in the user's profile 2026-10-22). Same result, NOT from data.
        let unchanged = skin_cache.body.as_ref().is_some_and(|(k, p, r, o, sc)| *k == kind && *r == body_rot && *o == origin && *p == pose && *sc == scales);
        if !unchanged {
            crate::bots::skin_part_scaled(&assets.part, &pose, body_rot, origin, &mut meshes, Some(&scales));
            for at in &assets.attach {
                crate::bots::skin_part(at, &pose, body_rot, origin, &mut meshes);
            }
            skin_cache.body = Some((kind, pose.clone(), body_rot, origin, scales));
        }
        if let Some((tex_pan, parts)) = &assets.tread {
            // the powered wheels' (NewWHL_L0 / R0) rotation: tread distance / WheelRadius (the
            // drawn wheel roll includes RollFactor; NOT from data: CurrentRotation without it)
            let rf = assets.rig.roll_factor.max(0.01);
            let key = (kind, dr.anim.roll.0, dr.anim.roll.1);
            if skin_cache.tread != Some(key) {
                crate::viewmodel::scroll_tread(&mut meshes, &parts[0], (dr.anim.roll.0 / rf).to_degrees(), *tex_pan);
                crate::viewmodel::scroll_tread(&mut meshes, &parts[1], (dr.anim.roll.1 / rf).to_degrees(), *tex_pan);
                skin_cache.tread = Some(key);
            }
        }
        if env_flag(&ENV_DRONEDBG, "HB_DRONEDBG") {
            let b = |k: Option<usize>| k.and_then(|k| pose.get(k)).map(|p| p.1);
            info!("DRONEDBG pitch {:.2} treads {:.2} roll {:?} pitch bone {:?} barrel {:?}", pr, dr.anim.treads, dr.anim.roll, b(assets.rig.turret_pitch), b(assets.skeleton.iter().position(|x| x.name == "TURRET_BARREL")));
        }
    }
    let fp = drone.cockpit_at.is_some();
    if drone.shown != Some((visible, kind, fp)) || visible {
        // the drone mesh entities through the mesh -> entity index (main.rs MeshEntities) instead
        // of a pass over every Mesh3d entity (~18 600) with a search of all drone meshes each
        // (the bulk of update_drone, 1.4 ms avg / 3.6 ms with a drone out, profile 2026-10-22).
        // Same order of precedence: a body / attachment mesh of any kind before a cockpit mesh.
        let list = &mut skin_cache.ents;
        list.clear();
        let mut complete = true;
        for (pass_cockpit, k, h) in kinds
            .0
            .iter()
            .enumerate()
            .flat_map(|(k, a)| a.part.meshes.iter().chain(a.attach.iter().flat_map(|p| p.meshes.iter())).map(move |h| (false, k, h)))
            .chain(kinds.0.iter().enumerate().flat_map(|(k, a)| a.cockpit.iter().flat_map(|c| c.part.meshes.iter()).map(move |h| (true, k, h))))
        {
            let Some(es) = me.0.get(&h.id()) else {
                // not indexed yet (spawned this frame): try again next frame
                complete = false;
                continue;
            };
            for &e in es {
                if !list.iter().any(|x| x.0 == e) {
                    list.push((e, k, pass_cockpit));
                }
            }
        }
        let list = std::mem::take(&mut skin_cache.ents);
        for &(e, k, is_cockpit) in &list {
            let Ok((e, _, mut v, mut tf)) = ents.get_mut(e) else { continue };
            {
                tf.translation = if is_cockpit { cockpit_tr } else { tr };
                let on = if is_cockpit { fp } else { body };
                let want = if on && k == kind { Visibility::Visible } else { Visibility::Hidden };
                if *v != want {
                    *v = want;
                    commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                }
            }
        }
        skin_cache.ents = list;
        if env_flag(&ENV_DRONEDBG, "HB_DRONEDBG") {
            let n = ents.iter().filter(|(_, mh, _, _)| kinds.0[kind].part.meshes.iter().any(|h| h.id() == mh.0.id())).map(|(e, _, v, tf)| format!("{:?} {:?} {:?}", *v, tf.translation, dbgv.get(e).ok().map(|d| (d.0.get(), d.1.get(), d.2.translation(), d.3.is_some(), d.4)))).collect::<Vec<_>>();
            let m0 = kinds.0[kind].part.meshes.first().and_then(|h| meshes.get(h)).and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()).map(|p| (p.len(), p.first().copied(), p.get(p.len() / 2).copied())));
            info!("DRONEDBG mesh0 {m0:?}");
            info!("DRONEDBG kind {kind} visible {visible} body {body} fp {fp} cam {:?} body ents {:?} part meshes {}", cams.single().map(|c| c.1.translation).ok(), n, kinds.0[kind].part.meshes.len());
        }
        if complete {
            drone.shown = Some((visible, kind, fp));
        }
    }
}

/// Last skinned drone state (update_drone): skip re-skinning / re-scrolling while unchanged, and
/// the drone mesh entities of this frame.
#[derive(Default)]
pub(crate) struct DroneSkinCache {
    body: Option<(usize, Vec<(Quat, Vec3)>, Quat, Vec3, Vec<f32>)>,
    cockpit: Option<(usize, Quat, Vec3)>,
    tread: Option<(usize, f32, f32)>,
    ents: Vec<(Entity, usize, bool)>,
}

static ENV_DRONEREF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static ENV_DRONEDBG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// An env test switch, read once (std::env::var_os per frame takes the environment lock).
fn env_flag(cell: &'static std::sync::OnceLock<bool>, name: &str) -> bool {
    *cell.get_or_init(|| std::env::var_os(name).is_some())
}
