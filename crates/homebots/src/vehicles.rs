//! Vehicles bought in the spawn selection and driven / flown by the player: the M1151 HUMVEE
//! (GCV_US_M1114_Content, GCVCar), the LAV PIRANHA (GCV_US_LAV_Content, GCVAmphibious), the
//! M1A3 ABRAMS (GCV_US_M1A3_Content, GCVTank), the AH-700 SCOUT (GCV_US_MD600_Content) and the
//! AH-64 APACHE (GCV_US_AH64_Content, both GCVAttackHeli).
//!
//! Data (vehload.rs reads it per class, over the parent classes' defaults):
//! - SVehicleMesh SkeletalMesh / AnimTreeTemplate, the mesh sockets, Wheels (BoneName,
//!   SteerFactor, WheelRadius), the car sim (TorqueVSpeedCurve, MaxSteerAngleCurve,
//!   GravityFactorEngineOn), MaxSpeed / AirSpeed, MaxSprintSpeed / SprintTimeMax /
//!   SprintRechargeRate, Health;
//! - Seats: CameraTag + CameraOffset, BehindCameraMaxOffset, ViewMinLimit / ViewMaxLimit (pitch),
//!   TurretControls -> the AnimTree's SkelControlLists bones (yaw, pitch control),
//!   TurretControllerMinLimit / MaxLimit (pitch), fTurret<Yaw|Pitch>MaxSpeed x SpeedModifier,
//!   SeatFireModes Sockets, GunClass -> VehicleWeaponInfo (WeaponConfigs MP.Vehicle.CFG_*:
//!   instant-hit fire modes with overheating, projectile fire modes with clip / burst / reload);
//! - VehicleAudio (GCVehicleAudio): engine idle / shutdown / sprint, EMP, per seat and fire mode
//!   SCFireSound;
//! - the helicopters' sims (GCVSimChopper_*): MaxYawRate, fAHMaxPitch; the AnimTree's
//!   GCSkelControl_HeliRotorSwap (rotor bones, scales, vMaxRotationsPerSecond).
//!
//! Keys (GCInput): E use (enter / exit), F1 / F2 seats, six next seat, C behind view; on the
//! ground BindingsLand (W / S throttle, A / D steer, Shift sprint, LMB EKBC_Fire = fire mode 0,
//! RMB EKBC_AltFire = fire mode 1), in a helicopter BindingsHeli (W / Space ascend, S / Ctrl
//! descend, A / D yaw, LMB / RMB fire modes 0 / 1).
//!
//! The PhysX simulations are replaced by kinematic models (wheel / tread ground probes, speeds
//! from the data); their acceleration, turn and climb constants are NOT from data (marked).
use crate::combat::{Actor, CombatData, DamageQueue, Hit, ShotHit, Soldier, Target, player_team};
use crate::input::{down, hit, Set};
use crate::menu::{MatchFlow, Phase};
use crate::player::{CollisionWorld, Pawn};
use crate::viewmodel::SkinPart;
use bevy::prelude::*;
use std::collections::HashMap;

pub struct VehiclesPlugin;

impl Plugin for VehiclesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VehicleVolumes>()
            .init_resource::<VehicleDamage>()
            .add_systems(Update, (spawn_vehicle, test_vehicle_hits, drive, update_volumes).chain().after(crate::player::PlayerSet).before(crate::combat::CombatSet))
            .add_systems(Update, (empty_vehicle_damage, unpiloted_fall, damage_states).chain().after(crate::combat::CombatSet).before(render_vehicles))
            .add_systems(Update, render_vehicles.after(crate::combat::CombatSet));
    }
}

/// One drawable instance of a vehicle mesh (preloaded at level load).
pub struct VehicleSlot {
    pub part: SkinPart,
    /// m_CockpitMesh / m_CockpitMeshSecondary (interior meshes on the outer mesh's bones,
    /// shown in the first-person view of seats with bUseCockpitMesh1p / bUseSecondaryCockpitMesh1p)
    pub cockpit: [Option<SkinPart>; 2],
    /// tread section meshes (left, right) with their reference UV0 (viewmodel::scroll_tread)
    pub tread_uv: [Vec<crate::viewmodel::TreadPart>; 2],
    /// mirror attachments (vehload.rs: wings, fenders) rigid on the outer mesh's Attach socket
    /// bone, shown with the outer mesh
    pub attach: Vec<SkinPart>,
}

/// GCVehicleSimChopperBase (+ GCVSimChopper_AttackHeli / _MD600 defaults, the content's
/// PlayerSimObject over them): the player flight model of UpdateVehicle (EXE 0x0071bf45..).
#[derive(Clone, Debug, Default)]
pub struct HeliSim {
    pub passive_lift: f32,
    pub active_lift: f32,
    pub up_damping: f32,
    pub turn_torque: f32,
    pub turn_damping: f32,
    pub max_yaw_rate: f32,
    pub pitch_torque: f32,
    pub pitch_torque_max: f32,
    pub pitch_damping: f32,
    pub roll_upright: f32,
    pub pitch_upright: f32,
    pub roll_strafe: f32,
    pub roll_torque_max: f32,
    pub roll_damping: f32,
    pub collective_comp: f32,
    pub air_resistance: f32,
    /// sin of fReasonablyUpright / fAHVeryUpright / fAHMaxPitch
    pub reasonably_upright: f32,
    pub very_upright: f32,
    pub max_pitch: f32,
    pub collective_roll: f32,
    pub falling_mult: f32,
    pub inertia: Vec3,
    /// StayUpright constraint: pitch resist angle (rad) / stiffness, roll the same
    pub upright_pitch: (f32, f32),
    pub upright_roll: (f32, f32),
    /// sin limits: roll / pitch input cut-off (input further out is dropped past them)
    pub roll_limit: f32,
    pub pitch_limit: f32,
}

/// GCTreadedVehicle tread texture scroll: Left/RightTreadMaterialIndex, TexPan2UnrealScale and
/// the Left/RightPoweredWheel WheelRadius
#[derive(Clone, Debug)]
pub struct Tread {
    pub slots: [i32; 2],
    pub tex_pan: f32,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// four wheels, front / rear steering (GCVCar)
    Car,
    /// treads or the LAV's eight wheels on a tank sim (GCVTank, GCVAmphibious: GCVSimTank*):
    /// turns on the spot
    Tracked,
    /// GCVAttackHeli (GCVSimChopper_*)
    Heli,
}

#[derive(Clone, Debug)]
pub struct GunCfg {
    pub fire_interval: f32,
    pub range: f32,
    pub spread: f32,
    pub dmg_type: String,
    pub overheat_max: f32,
    pub overheat_add: f32,
    pub overheat_recover: f32,
    pub sound: String,
    /// the fire cue loops while the trigger is held (FireModes bLooping)
    pub sound_looping: bool,
    pub engine_idle: String,
    pub engine_shutdown: String,
    /// GCVehicleAudio SCEngineSprint
    pub engine_sprint: String,
    /// VehicleFireModes[0].Tracer
    pub tracer: Option<crate::weaponcfg::TracerCfg>,
    /// GCVehicleAudio SCEMPStart / SCEMPLoop
    pub emp_start: String,
    pub emp_loop: String,
    /// damage type class defaults SplashDamageClass, SplashDamageRadius, SplashDamageRadiusFull
    /// (GCDmgType_Veh_30mm: GCDmgType_Veh_30mmSplash; LAV 200 / 150)
    pub splash: Option<(String, f32, f32)>,
    /// FireModes FullDamageRange
    pub full_range: f32,
}

/// A fire mode of a seat's gun.
#[derive(Clone, Debug)]
pub enum FireKind {
    /// EWFT_InstantHit (machine guns, the LAV's 25 mm): sound and tracer inside
    Instant(GunCfg),
    /// EWFT_Projectile: the CombatData weapon entry (combat::Grenade rocket) and the mode
    Projectile(crate::weaponcfg::RocketMode, usize),
}

#[derive(Clone, Debug)]
pub struct FireMode {
    pub kind: FireKind,
    /// SeatFireModes[m].Sockets (cycled per shot)
    pub sockets: Vec<String>,
    /// VehicleAudioSetups[0].Seats[s].FireModes[m] SCFireSound
    pub sound: String,
    /// SeatFireModes[m].FlashTags: vehicle events of a shot (socket References effects)
    pub flash_tags: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SeatCfg {
    /// CameraTag socket and CameraOffset (UE units, in the seat's view frame)
    pub camera_tag: String,
    pub camera_offset: Vec3,
    /// BehindCameraMaxOffset (third person, view frame from the pivot)
    pub behind: Vec3,
    /// BehindCameraMinOffset: the closest camera, used as the first-person offset from the
    /// CameraTag socket (the HUMVEE gunner's view: MGViewSocket + (0, 0, 20))
    pub behind_min: Vec3,
    /// ViewMinLimit / ViewMaxLimit X: view pitch limits (degrees)
    pub view_pitch: (f32, f32),
    /// TurretControls -> (yaw bone, pitch bone)
    pub turret: Option<(usize, usize)>,
    /// TurretControllerMinLimit / MaxLimit X over the turret yaw (degrees)
    pub turret_pitch: (Vec<(f32, f32)>, Vec<(f32, f32)>),
    /// fTurretYawMaxSpeed x fTurretYawSpeedModifier, the same for the pitch (taken as deg/s,
    /// NOT from data: the unit)
    pub turret_rate: (f32, f32),
    /// MP.Vehicle.CFG_<x> of the seat's gun ("" without a gun)
    pub weapon: String,
    pub modes: Vec<FireMode>,
    /// FOVZoom of the gun (aiming, RMB when the seat has no second fire mode)
    pub fov_zoom: f32,
    /// Seat<n>StartPosition: where the player gets out
    pub exit: String,
    /// CameraTag3p (behind view socket; "" = CameraTag)
    pub camera_tag3p: String,
    /// BehindCameraRotation pitch (radians, added to the behind view)
    pub behind_pitch: f32,
    /// BehindCameraMaxOffsetSprint (zero = none)
    pub behind_sprint: Vec3,
    pub fixed1p: bool,
    pub fixed3p: bool,
    /// ESeatAllowedViews: 0 first only, 1 third only, 2 both
    pub views: u8,
    /// 1P: 0 outer mesh socket, 1 cockpit mesh, 2 secondary cockpit mesh
    pub cockpit1p: u8,
    /// bDisable3pMeshIn1p
    pub hide_outer_1p: bool,
    /// bCameraDoesNotRoll3P
    pub no_roll_3p: bool,
    /// TurretParentSeatIndex (-1 = None): the seat whose turret carries this one (M1A3 / LAV
    /// seat 1: the MG_ROTATE bone is a child of Turret_Rotate)
    pub turret_parent: Option<usize>,
}

/// First-person view of a seat (ESeatAllowedViews; BOTH: the C toggle, the driver starts in the
/// behind view, gunners in their sight)
pub(crate) fn seat_first_person(sc: &SeatCfg, v: &Vehicle, seat: usize) -> bool {
    match sc.views {
        0 => true,
        1 => false,
        _ => {
            if seat == 0 {
                !v.behind_view
            } else {
                !v.gunner_3p
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VehicleAudio {
    pub engine_idle: String,
    pub engine_shutdown: String,
    pub engine_sprint: String,
    pub emp_start: String,
    pub emp_loop: String,
    /// SCDamageCritical (GCVehicleAudio; GCVehicle.DamageCriticalSound's component has no cue)
    pub damage_critical: String,
}

pub struct VehicleKind {
    pub class: String,
    pub name: String,
    /// the class and its parents (GCV_US_M1A3_Content, GCVTank, GCVehicle, ...): HUD placers by
    /// their VehicleTypes
    pub classes: Vec<String>,
    pub motion: Motion,
    pub skeleton: Vec<upk::skeletal::Bone>,
    /// reference component pose (drone.rs ref_pose)
    pub ref_pose: Vec<(Quat, Vec3)>,
    /// socket name -> (bone index, location in the bone's frame)
    pub sockets: HashMap<String, (usize, Vec3)>,
    /// (bone, SteerFactor, side: -1 left / 1 right) of the wheels (all WHL_* bones for treads)
    pub wheels: Vec<(usize, f32, f32)>,
    pub wheel_radius: f32,
    /// mesh origin above the ground (wheel centre + WheelRadius; helicopters: lowest vertex)
    pub ride_height: f32,
    /// ground probe points (UE, body frame: x forward, y right)
    pub probes: Vec<Vec2>,
    pub wheel_base: f32,
    pub track_half: f32,
    /// half length / width of the body for the wall spheres (UE)
    pub half_len: f32,
    pub half_width: f32,
    pub torque_curve: Vec<(f32, f32)>,
    pub steer_curve: Vec<(f32, f32)>,
    pub gravity_factor: f32,
    /// MaxSpeed / GroundSpeed (ground), AirSpeed (helicopter)
    pub max_speed: f32,
    /// (MaxSprintSpeed, SprintTimeMax, SprintRechargeRate)
    pub sprint: Option<(f32, f32, f32)>,
    /// tank sims: m_fTreadMaxSpeedSteering (tread speed difference while steering, uu/s)
    pub tread_steer: f32,
    /// helicopter sims: MaxYawRate (rad/s), fAHMaxPitch (degrees)
    pub yaw_rate: f32,
    pub max_pitch: f32,
    pub seats: Vec<SeatCfg>,
    pub slots: Vec<VehicleSlot>,
    pub audio: VehicleAudio,
    /// GCSkelControl_HeliRotorSwap spinning state: (bone, BoneScale, BoneRotates), rotations / s
    pub rotors: Vec<(Vec<(usize, f32, bool)>, Vec3)>,
    pub health: f32,
    /// the mesh's highest vertex above the origin (UE units): the hit box top
    pub top: f32,
    /// m_DefaultPhysicalMaterial name (PMA_Heavy / PMA_Medium / PMA_Light / PMA_Heli...)
    pub armor: String,
    pub tread: Option<Tread>,
    /// helicopters: GCVehicleSimChopperBase values of the player's sim object
    pub heli: Option<HeliSim>,
    /// cockpit meshes: component Translation (vehicle space) and their sockets on the outer
    /// skeleton's bones
    pub cockpit_tr: [Vec3; 2],
    pub cockpit_sockets: [HashMap<String, (usize, Vec3)>; 2],
    /// socket References effects (particle systems): (event tag, socket, template), lower case
    /// tag and socket
    pub socket_fx: Vec<(String, String, String)>,
    /// progressive damage before destruction (GCOverArmorInfo panels, damage smoke)
    pub damage: DamageCfg,
}

/// One GCOverArmorInfo DestructibleBoneInfo (vehload.rs over_armor): an armour panel / door /
/// hood skinned to its own bone, destroyed at 0 bone health (InitialOverArmorHealth).
#[derive(Clone, Debug, Default)]
pub struct ArmorPart {
    pub bone: usize,
    pub name: String,
    /// index of the Outer part (this one is hidden until the outer part is destroyed)
    pub outer: Option<usize>,
    /// DamagePassThrough: share of the hit that reaches the vehicle's health
    pub pass: f32,
    pub ignore: bool,
    /// VehicleEvents (on destruction) / VehicleEventsOnHidden (state hidden), lower case
    pub events: Vec<String>,
    pub events_hidden: Vec<String>,
    pub sound: String,
    /// bones collapsed when hidden (the bone and its descendants that are no other part)
    pub subtree: Vec<usize>,
    /// vehicle-space (UE) bounds of the part's vertices (reference pose)
    pub lo: Vec3,
    pub hi: Vec3,
}

/// A socket Reference that a damage event starts (lower case tags / socket).
#[derive(Clone, Debug, Default)]
pub struct DamageRef {
    pub start: String,
    pub end: String,
    pub socket: String,
    pub path: String,
    pub sound: bool,
}

/// Progressive damage of a vehicle class (data: GCVehicle defaults over the class chain,
/// ArmorInfo GCOverArmorInfo, the mesh's socket References).
#[derive(Clone, Debug, Default)]
pub struct DamageCfg {
    pub parts: Vec<ArmorPart>,
    /// InitialOverArmorHealth (GCVehicle 150; *_SP classes 1)
    pub part_health: f32,
    /// DamageSmokeThreshhold (0.666): VehicleEvent DamageSmoke below, NoDamageSmoke above
    pub smoke_threshold: f32,
    /// DamageCriticalThreshold (0.333): DamageCriticalSound for the local driver below
    pub critical_threshold: f32,
    pub critical_sound: String,
    /// socket References of the damage events (over-armour events, DamageSmoke)
    pub refs: Vec<DamageRef>,
}

#[derive(Resource)]
pub struct VehicleAssets {
    pub kinds: Vec<VehicleKind>,
}

impl VehicleAssets {
    pub fn kind_of(&self, class: &str) -> Option<usize> {
        self.kinds.iter().position(|k| k.class.eq_ignore_ascii_case(class))
    }
}

/// Front-line spawn type of a vehicle class (GameMode SpawnPoints Type). NOT from data: the
/// mapping by the vehicle's parent class (the game picks it natively).
pub fn spawn_type(classes: &[String]) -> &'static str {
    let has = |n: &str| classes.iter().any(|c| c.eq_ignore_ascii_case(n));
    if has("GCVTank") {
        "GMST_TreadedVehicle"
    } else if has("GCVAmphibious") {
        "GMST_AmphibiousVehicle"
    } else if has("GCV_SD_MD600_Content") {
        "GMST_LightHelicopter"
    } else if has("GCVAttackHeli") {
        "GMST_AttackHelicopter"
    } else {
        "GMST_Jeep"
    }
}

/// Per-seat gun state.
#[derive(Clone, Debug, Default)]
pub struct SeatState {
    pub fire_cd: f32,
    pub heat: f32,
    pub overheated: f32,
    /// per fire mode: rounds in the clip and reload seconds left (projectile modes)
    pub ammo: Vec<i32>,
    pub reload: Vec<f32>,
    /// burst in progress: (mode, shots left, seconds to the next)
    pub burst: Option<(usize, i32, f32)>,
    pub shots: u32,
}

#[derive(Component)]
pub struct Vehicle {
    pub kind: usize,
    pub slot: usize,
    /// origin (UE units), yaw / pitch / roll (radians)
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub speed: f32,
    /// helicopter: sideways and vertical speed (UE units / s)
    pub side: f32,
    pub vz: f32,
    pub steer: f32,
    /// wheel / tread roll (left, right)
    pub wheel_spin: (f32, f32),
    /// per seat: turret yaw / pitch relative to the body (radians)
    pub turrets: Vec<(f32, f32)>,
    /// seat of the player (None = empty)
    pub player_seat: Option<usize>,
    pub behind_view: bool,
    /// gunner seats: behind view toggled on (C)
    pub gunner_3p: bool,
    /// the camera ray's aim point of the last frame (UE world), the turret's target in the
    /// behind view
    pub aim_pt: Option<Vec3>,
    /// view relative to the body (driver of a car: free look; turret seats: the aim)
    pub cam_yaw: f32,
    pub cam_pitch: f32,
    pub guns: Vec<SeatState>,
    pub health: f32,
    /// sprint time used (0 = full), sprinting this frame
    pub sprint_used: f32,
    pub sprinting: bool,
    /// EMP blast: seconds left disabled (EMPBlastDuration)
    pub emp: f32,
    /// rotor turns (fraction) per rotor swap control
    pub rotor: Vec<f32>,
    pub seed: u32,
    /// helicopters: linear velocity (UE units / s) and angular velocity (rad / s, world axes)
    pub vel: Vec3,
    pub ang: Vec3,
    /// over-armour bone health per DamageCfg part, destroyed parts (bit per part, like
    /// GCArmorManager RepArmorState), damage events to fire (damage_states)
    pub armor: Vec<f32>,
    pub armor_bits: u32,
    pub dmg_events: Vec<String>,
}

/// Piecewise-linear InterpCurveFloat (points as stored).
pub fn curve(points: &[(f32, f32)], x: f32) -> f32 {
    if points.is_empty() {
        return 0.0;
    }
    if x <= points[0].0 {
        return points[0].1;
    }
    for w in points.windows(2) {
        if x <= w[1].0 {
            let f = (x - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
            return w[0].1 + (w[1].1 - w[0].1) * f;
        }
    }
    points[points.len() - 1].1
}

/// InterpCurveFloat raw bytes: count, then 24 bytes per point (InVal, OutVal, ArriveTangent,
/// LeaveTangent, InterpMode + padding), then InterpMethod (Default__GCVSimCar_M1114: 4 + 5 * 24 + 8
/// = 132 bytes).
pub fn parse_curve(data: &[u8]) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    if data.len() < 4 {
        return out;
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    for k in 0..n {
        let o = 4 + k * 24;
        let Some(b) = data.get(o..o + 8) else { break };
        out.push((f32::from_le_bytes(b[0..4].try_into().unwrap()), f32::from_le_bytes(b[4..8].try_into().unwrap())));
    }
    out
}

/// (InVal, OutVal X) points of an InterpCurveVector (count, then 48 bytes per point: InVal,
/// OutVal xyz, tangents, mode): the seats' pitch limits over the turret yaw (degrees).
pub fn curve_vec_x(data: &[u8]) -> Vec<(f32, f32)> {
    let Some(n) = data.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize) else { return Vec::new() };
    let f = |o: usize| data.get(o..o + 4).map(|b| f32::from_le_bytes(b.try_into().unwrap()));
    (0..n).filter_map(|k| Some((f(4 + k * 48)?, f(8 + k * 48)?))).collect()
}

/// Mesh (bone) space -> UE vehicle axes (x forward, y right, z up).
fn mesh_to_ue(m: Vec3) -> Vec3 {
    Vec3::new(m.z, -m.x, -m.y)
}

pub(crate) fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(t) - std::f32::consts::PI
}

impl VehicleKind {
    /// Component-space pose: wheel roll and steering, turrets, rotors (drone.rs add_comp: the
    /// rotations act about the component axes, up = -y, right = -x, forward = z).
    pub(crate) fn pose(&self, v: &Vehicle) -> Vec<(Quat, Vec3)> {
        let mut local: Vec<(Quat, Vec3)> = self.skeleton.iter().map(|b| (Quat::from_xyzw(b.rot[0], b.rot[1], b.rot[2], b.rot[3]).normalize(), Vec3::from(b.pos))).collect();
        let sk = &self.skeleton;
        let rp = &self.ref_pose;
        for &(k, sf, side) in &self.wheels {
            let spin = if side < 0.0 { v.wheel_spin.0 } else { v.wheel_spin.1 };
            // steering about the up axis, roll about the axle (forward = negative x rotation)
            let r = Quat::from_rotation_y(-v.steer * sf) * Quat::from_rotation_x(-spin);
            crate::drone::add_comp(&mut local, sk, rp, Some(k), r);
        }
        // is bone `b` below bone `a` in the skeleton?
        let below = |b: usize, a: usize| {
            let mut k = b;
            for _ in 0..64 {
                if k >= sk.len() || sk[k].parent < 0 || sk[k].parent as usize == k {
                    return false;
                }
                k = sk[k].parent as usize;
                if k == a {
                    return true;
                }
            }
            false
        };
        for (s, seat) in self.seats.iter().enumerate() {
            let Some((by, bp)) = seat.turret else { continue };
            let (mut ty, mut tp) = v.turrets.get(s).copied().unwrap_or((0.0, 0.0));
            // a seat on another seat's turret (TurretParentSeatIndex: M1A3 / LAV seat 1, its
            // MG_ROTATE a child of Turret_Rotate): the seat's yaw / pitch are aims relative to
            // the body (seat_dir, the camera), its bones turn relative to the parent turret's.
            // Before, the parent's yaw was added twice: the commander's MG and its interior
            // (Interior_US_M1A2_Gunner, MGViewSocket) turned away from the view (user
            // 2026-10-21 23:56, "F2 view broken")
            if let Some((pby, pbp)) = seat.turret_parent.and_then(|p| self.seats.get(p)).and_then(|p| p.turret) {
                let (py, pp) = v.turrets.get(seat.turret_parent.unwrap_or(0)).copied().unwrap_or((0.0, 0.0));
                if by == pby || below(by, pby) {
                    ty = wrap(ty - py);
                }
                if pbp != pby && (by == pbp || below(by, pbp)) {
                    tp -= pp;
                }
            }
            if by != bp {
                crate::drone::add_comp(&mut local, sk, rp, Some(by), Quat::from_rotation_y(-ty));
                crate::drone::add_comp(&mut local, sk, rp, Some(bp), Quat::from_rotation_x(tp));
            } else {
                crate::drone::add_comp(&mut local, sk, rp, Some(by), Quat::from_rotation_y(-ty) * Quat::from_rotation_x(tp));
            }
        }
        // rotors about the bone's own axes at vMaxRotationsPerSecond (drone.rs)
        for (r, (bones, rps)) in self.rotors.iter().enumerate() {
            let turn = v.rotor.get(r).copied().unwrap_or(0.0);
            let axis = Vec3::new(rps.z, rps.y, rps.x).normalize_or_zero();
            if axis == Vec3::ZERO {
                continue;
            }
            let q = Quat::from_axis_angle(axis, turn * std::f32::consts::TAU);
            for &(k, _, rotates) in bones {
                if rotates && k < local.len() {
                    local[k].0 *= q;
                }
            }
        }
        crate::drone::compose(sk, &local)
    }

    /// Per-bone scale (rotor swap: the still / blurred rotor), None without rotors.
    /// `scales` plus the hidden over-armour parts of this vehicle (scale 0).
    fn scales_for(&self, veh: &Vehicle) -> Option<Vec<f32>> {
        let hidden: Vec<usize> = (0..self.damage.parts.len()).filter(|&i| part_hidden(&self.damage, veh.armor_bits, i)).collect();
        if hidden.is_empty() {
            return self.scales();
        }
        let mut v = self.scales().unwrap_or_else(|| vec![1.0; self.skeleton.len()]);
        for i in hidden {
            for &b in &self.damage.parts[i].subtree {
                if let Some(x) = v.get_mut(b) {
                    *x = 0.0;
                }
            }
        }
        Some(v)
    }

    fn scales(&self) -> Option<Vec<f32>> {
        if self.rotors.is_empty() {
            return None;
        }
        let mut v = vec![1.0; self.skeleton.len()];
        for (bones, _) in &self.rotors {
            for &(k, sc, _) in bones {
                if let Some(x) = v.get_mut(k) {
                    *x = sc;
                }
            }
        }
        Some(v)
    }

    /// Socket position (UE, vehicle frame relative to the origin) and its bone's rotation.
    fn socket_local(&self, pose: &[(Quat, Vec3)], name: &str) -> Option<(Vec3, Quat)> {
        let &(bi, loc) = self.sockets.get(&name.to_ascii_lowercase())?;
        let (r, t) = pose.get(bi)?;
        Some((mesh_to_ue(*r * loc + *t), *r))
    }

    /// World position (UE) of a cockpit mesh socket (bones of the outer pose).
    fn cockpit_socket_world(&self, v: &Vehicle, pose: &[(Quat, Vec3)], which: usize, name: &str) -> Option<Vec3> {
        let &(bi, loc) = self.cockpit_sockets.get(which)?.get(&name.to_ascii_lowercase())?;
        let (r, t) = pose.get(bi)?;
        Some(v.pos + body_rot(v) * (mesh_to_ue(*r * loc + *t) + self.cockpit_tr[which]))
    }

    /// World position (UE) of a socket.
    pub(crate) fn socket_world(&self, v: &Vehicle, pose: &[(Quat, Vec3)], name: &str) -> Option<Vec3> {
        let (l, _) = self.socket_local(pose, name)?;
        Some(v.pos + body_rot(v) * l)
    }
}

pub(crate) fn body_rot(v: &Vehicle) -> Quat {
    Quat::from_rotation_z(v.yaw) * Quat::from_rotation_y(-v.pitch) * Quat::from_rotation_x(v.roll)
}

/// HB_VEHTEST: real time of the first match frame, script time (f32 bits) for record.rs
static TEST_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
pub static TEST_T: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Kinematic constants (NOT from data: PhysX forces / torques are not reproduced).
const ACCEL: f32 = 350.0;
const BRAKE: f32 = 900.0;
const COAST: f32 = 120.0;
/// tanks: acceleration and braking (uu/s^2)
const TANK_ACCEL: f32 = 260.0;
/// helicopters: climb speed (uu/s), horizontal acceleration at full tilt (uu/s^2), drag (1/s)
const HELI_CLIMB: f32 = 450.0;
const HELI_ACCEL: f32 = 900.0;
const HELI_DRAG: f32 = 0.6;

#[allow(clippy::too_many_arguments)]
fn spawn_vehicle(
    mut commands: Commands,
    mut flow: ResMut<MatchFlow>,
    assets: Option<Res<VehicleAssets>>,
    data: Option<Res<CombatData>>,
    mut pawns: Query<&mut Pawn>,
    vehicles: Query<(Entity, &Vehicle)>,
    mut serial: Local<u32>,
    (col, mut buf): (Res<CollisionWorld>, Local<Vec<u32>>),
    mut cheats: ResMut<crate::combat::Cheats>,
) {
    if flow.phase == Phase::Front {
        for (e, _) in &vehicles {
            commands.entity(e).despawn();
        }
        // the next match counts its spawns from 0 again (see combat.rs)
        *serial = u32::MAX;
        return;
    }
    // debug menu: a free vehicle at the player once the menu is closed (the player's current
    // vehicle goes)
    // HB_TEST_DEBUGVEH=<class>: that debug-menu vehicle 3 s into the match (tests)
    if let Ok(c) = std::env::var("HB_TEST_DEBUGVEH") {
        static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if flow.phase == Phase::Playing && *serial != u32::MAX && *serial > 0 && flow.hud.time_left > 0.0 && TEST_START.get().is_none_or(|t| t.elapsed().as_secs_f32() > 3.0) && !DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
            cheats.vehicle = Some(c);
        }
    }
    let cheat = if flow.phase == Phase::Playing && !flow.debug { cheats.vehicle.take() } else { None };
    if cheat.is_none() && (flow.phase != Phase::Playing || flow.spawn_serial == *serial) {
        return;
    }
    let (Some(assets), Some(data)) = (assets, data) else { return };
    let Ok(mut p) = pawns.single_mut() else { return };
    if p.dead {
        return;
    }
    let is_cheat = cheat.is_some();
    let class = if let Some(c) = cheat {
        for (e, v) in &vehicles {
            if v.player_seat.is_some() {
                // the new one stands where the old one was
                p.feet = Vec3::new(v.pos.x, v.pos.z - k_ride(&assets, v.kind), v.pos.y) * 0.01;
                commands.entity(e).despawn();
            }
        }
        p.in_vehicle = false;
        c
    } else {
        // wait for spawn_player to place the pawn (the request can still be pending when the
        // menu ran after PlayerSet: the vehicle then came at the old pawn position)
        if flow.spawn_requested {
            return;
        }
        *serial = flow.spawn_serial;
        // HOMEBOTS_TEST_VEHICLE=1 (the HUMVEE) or =<class>: spawn in a vehicle (tests)
        let test = std::env::var("HOMEBOTS_TEST_VEHICLE").ok().map(|v| if v.starts_with("GCV_") { v } else { "GCV_US_M1114_Content".to_string() });
        let Some(class) = flow.vehicle.clone().or(test) else { return };
        if flow.vehicle.is_some() {
            info!("vehicle: bought {class}, KP {}", flow.hud.kp);
        }
        class
    };
    let free = is_cheat || flow.vehicle.is_none();
    let Some(kind) = assets.kind_of(&class) else {
        info!("vehicle {class} not available here, infantry spawn");
        return;
    };
    let k = &assets.kinds[kind];
    // (the test and debug vehicles are free)
    let cost = if free { 0 } else { data.costs.get(&class.to_ascii_lowercase()).map(|c| c.0).unwrap_or(0) };
    if flow.hud.kp < cost {
        warn!("vehicle: {class} costs {cost}, only {} KP", flow.hud.kp);
        return;
    }
    // a free slot of the kind (the oldest one of the kind is recycled)
    let used: Vec<usize> = vehicles.iter().filter(|(_, v)| v.kind == kind).map(|(_, v)| v.slot).collect();
    // (a vehicle the bots use is recycled last: botvehicles.rs)
    let slot = (0..k.slots.len()).find(|s| !used.contains(s)).or_else(|| {
        vehicles.iter().filter(|(_, v)| v.kind == kind).min_by_key(|(e, _)| crate::botvehicles::is_bot_vehicle(*e)).map(|(e, v)| {
            commands.entity(e).despawn();
            v.slot
        })
    });
    let Some(slot) = slot else { return };
    flow.hud.kp -= cost;
    // on the ground below the start point (vehicle starts can sit well above it)
    let mut feet = p.feet;
    if let Some((y, _)) = col.floor(feet + Vec3::Y * 1.5, 60.0, &mut buf) {
        feet.y = y;
    }
    let mut pos = Vec3::new(feet.x, feet.z, feet.y) * 100.0 + Vec3::Z * k.ride_height;
    let mut yaw = p.yaw / 65536.0 * std::f32::consts::TAU;
    // HB_VEHSLOPE=<deg>: start at the foot of a real slope of about that angle near the spawn,
    // facing uphill (tests, with HB_VEHTEST's throttle)
    if let Some(deg) = std::env::var("HB_VEHSLOPE").ok().and_then(|v| v.parse::<f32>().ok()) {
        match find_slope(&col, feet, deg, &mut buf) {
            Some((at, a, got)) => {
                pos = Vec3::new(at.x, at.z, at.y) * 100.0 + Vec3::Z * k.ride_height;
                yaw = a;
                info!("vehslope: start {pos:?} yaw {:.0} slope {got:.1} deg", a.to_degrees());
            }
            None => warn!("vehslope: no {deg} deg slope found"),
        }
    }
    p.in_vehicle = true;
    commands.spawn(new_vehicle(k, kind, slot, pos, yaw));
    info!("vehicle: {} ({:?}) spawned for {cost} KP at {pos:?} (pawn feet {:?})", k.name, k.motion, p.feet);
}

/// Aim direction (UE) of a seat: the body's heading plus the seat's view.
pub(crate) fn seat_dir(v: &Vehicle, k: &VehicleKind, seat: usize) -> Vec3 {
    let has_turret = k.seats.get(seat).is_some_and(|s| s.turret.is_some());
    let (yaw, pitch) = if has_turret {
        let (ty, tp) = v.turrets.get(seat).copied().unwrap_or((0.0, 0.0));
        (v.yaw + ty, v.pitch + tp)
    } else if k.motion == Motion::Heli {
        // fixed guns / launchers along the nose
        (v.yaw, v.pitch)
    } else {
        (v.yaw + v.cam_yaw, v.cam_pitch)
    };
    Vec3::new(yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), pitch.sin())
}

#[allow(clippy::too_many_arguments)]
fn drive(
    time: Res<Time>,
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    mut flow: ResMut<MatchFlow>,
    assets: Option<Res<VehicleAssets>>,
    data: Option<Res<CombatData>>,
    col: Res<CollisionWorld>,
    mut pawns: Query<&mut Pawn>,
    mut vehicles: Query<(Entity, &mut Vehicle)>,
    soldiers: Query<(Entity, &Soldier)>,
    mut cams: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    (mut queue, mut sfx, mut vm, mut wsfx, mut fx, mut grenades, impacts): (ResMut<DamageQueue>, ResMut<crate::sfx::SfxQueue>, Option<ResMut<crate::viewmodel::ViewModel>>, ResMut<crate::sfx::WeaponSfx>, ResMut<crate::particles::Particles>, ResMut<crate::combat::Grenades>, Option<Res<crate::impacts::Impacts>>),
    mut buf: Local<(Vec<u32>, Vec<Vec3>)>,
    mut test_t: Local<f32>,
    (mut commands, mut vhits): (Commands, ResMut<crate::combat::VehicleHits>),
) {
    if flow.phase != Phase::Playing {
        *test_t = 0.0;
        return;
    }
    let Some(assets) = assets else { return };
    let dt = time.delta_secs().min(0.1);
    // HB_VEHTEST=<s>: a scripted drive (tests), starting <s> real seconds into the match (the
    // software renderer compiles the vehicle's pipelines first): ground vehicles drive, turn,
    // aim the turret round and fire both modes; helicopters climb, fly nose down, turn and fire
    let script_delay: Option<f32> = std::env::var("HB_VEHTEST").ok().and_then(|v| v.parse().ok());
    let script = script_delay.is_some_and(|d| {
        let start = *TEST_START.get_or_init(std::time::Instant::now);
        start.elapsed().as_secs_f32() >= d
    });
    if script {
        *test_t += dt;
        TEST_T.store(test_t.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
    let tt = *test_t;
    // EMP blasts (combat::tactical_blast): every vehicle in EMPDamageRadius is dead for
    // EMPBlastDuration (engine off, guns silent); the rotors keep turning
    let emps = grenades.emp.clone();
    for (_, mut v) in &mut vehicles {
        v.emp = (v.emp - dt).max(0.0);
        for &(at, r, d) in &emps {
            if v.pos.distance(at) <= r {
                v.emp = v.emp.max(d);
                sfx.play(&assets.kinds[v.kind].audio.emp_start, Some(v.pos));
            }
        }
        let k = &assets.kinds[v.kind];
        for r in 0..k.rotors.len().min(v.rotor.len()) {
            v.rotor[r] = (v.rotor[r] + k.rotors[r].1.length() * dt).fract();
        }
    }
    flow.hud.vehicle_crosshair = None;
    flow.hud.vehicle_range = None;
    flow.hud.vehicle_first_person = false;
    flow.hud.vehicle_seat = None;
    flow.hud.vehicle_gauges = None;
    flow.hud.vehicle_classes.clear();
    let Ok(mut p) = pawns.single_mut() else { return };
    let input = !flow.paused && !p.dead;
    // enter: E near a vehicle (within its body + 150 uu)
    if !p.in_vehicle {
        if input && hit(Set::Foot, "EKBC_Use") {
            let me = Vec3::new(p.feet.x, p.feet.z, p.feet.y) * 100.0;
            let reach = |v: &Vehicle| assets.kinds[v.kind].half_len.max(200.0) + 150.0;
            if let Some((_, mut v)) = vehicles.iter_mut().filter(|(_, v)| v.player_seat.is_none() && v.pos.distance(me) < reach(v)).min_by(|a, b| a.1.pos.distance(me).partial_cmp(&b.1.pos.distance(me)).unwrap()) {
                v.player_seat = Some(0);
                v.cam_yaw = 0.0;
                p.in_vehicle = true;
            }
        }
        vhits.0.clear();
        return;
    }
    let Some((ve, mut v)) = vehicles.iter_mut().find(|(_, v)| v.player_seat.is_some()) else {
        p.in_vehicle = false;
        vhits.0.clear();
        return;
    };
    let k = &assets.kinds[v.kind];
    // hits on the vehicle (combat.rs forwards the player's): the damage grid's value for the
    // armour (m_DefaultPhysicalMaterial), scaled like the hit was against the soldier (range /
    // blast falloff)
    let mut last = None;
    for h in std::mem::take(&mut vhits.0) {
        let amount = match (&h.dmg_type, data.as_ref()) {
            (Some(t), Some(d)) => {
                let soldier = d.grid.damage(t, if h.explosive { "PMA_Soldier" } else if h.headshot { "PMA_Head" } else { "PMA_Soldier_Chest" }, 0.0, 1.0, 1.0);
                let armour = d.grid.damage(t, &k.armor, 0.0, 1.0, 1.0);
                if soldier > 0.0 { armour * (h.amount / soldier).min(1.0) } else { armour }
            }
            _ => h.amount,
        };
        // over-armour panels first (GCArmorManager), at the hit's location
        let at = hit_location(k, &v, h.from, h.explosive);
        let amount = armor_damage(k, &mut v, at, amount, h.explosive, &mut sfx);
        if std::env::var_os("HB_DMGDBG").is_some() {
            info!("vehicle hit {:?} {:.0} -> {amount:.0} ({}), health {:.0}", h.dmg_type, h.amount, k.armor, v.health - amount);
        }
        v.health -= amount;
        last = Some(h);
    }
    if v.health <= 0.0 {
        // destroyed: the explosion (GCVehicle ExplosionDamageType GCDmgType_VehicleExplosion's
        // effect for the surface), the occupant dies with it; the wreck (RagdollDefinitions,
        // death events, burn, WreckExplosion, HurtRadius) is destruction.rs
        let at = v.pos;
        if !crate::combat::explosion_fx_debris(impacts.as_deref(), &col, at, "GCDmgType_VehicleExplosion", None, &mut fx, &mut sfx, &mut buf.0) {
            fx.fire("KFX_Pyrotechnics.FX_Grenades.Frag_Metal", at, Quat::IDENTITY);
        }
        sfx.play(&k.audio.engine_shutdown, Some(at));
        p.in_vehicle = false;
        let (attacker, team, icon, from) = last.map(|h| (h.attacker, h.attacker_team, h.icon, h.from)).unwrap_or((Actor::World, 2, None, at));
        queue.0.push(Hit { target: Target::Player, amount: 100_000.0, attacker, attacker_team: team, headshot: false, explosive: true, from, icon, dmg_type: None });
        let vel = if v.vel != Vec3::ZERO { v.vel } else { body_rot(&v) * Vec3::X * v.speed + Vec3::Z * v.vz };
        commands.spawn(crate::destruction::WreckSpawn { kind: v.kind, pos: v.pos, yaw: v.yaw, pitch: v.pitch, roll: v.roll, vel, from, attacker, team });
        commands.entity(ve).despawn();
        info!("vehicle {} destroyed", k.name);
        if let Some(vm) = vm.as_mut() {
            vm.lowered = false;
        }
        return;
    }
    if let Some(vm) = vm.as_mut() {
        vm.lowered = true;
    }
    let heli = k.motion == Motion::Heli;
    let set = if heli { Set::Heli } else { Set::Land };
    let seat = v.player_seat.unwrap_or(0);
    let pose = k.pose(&v);
    // seat change / exit
    if input {
        // BindingsPCGlobal F1 / F2 EKBC_Seat1 / Seat2, six EKBC_NextVehicleSeat, C EKBC_BehindView;
        // BindingsLand / BindingsHeli E EKBC_Use
        let armed: Vec<usize> = (0..k.seats.len()).filter(|&s| s == 0 || !k.seats[s].weapon.is_empty()).collect();
        if hit(Set::Global, "EKBC_Seat1") {
            v.player_seat = Some(0);
        }
        if (hit(Set::Global, "EKBC_Seat2") || std::env::var_os("HOMEBOTS_TEST_SEAT").is_some() && (v.speed > 300.0 || std::env::var_os("HOMEBOTS_TEST_FORWARD").is_none())) && armed.contains(&1) {
            v.player_seat = Some(1);
        }
        // F3..F8 EKBC_Seat3..Seat8 (SwitchVehicleSeat 2..7): seats with a gun or a camera socket
        for (n, s) in (3..=8).zip(2usize..) {
            let usable = armed.contains(&s) || k.seats.get(s).is_some_and(|c| !c.camera_tag.is_empty());
            if usable && hit(Set::Global, &format!("EKBC_Seat{n}")) {
                v.player_seat = Some(s);
            }
        }
        if hit(Set::Global, "EKBC_NextVehicleSeat") {
            let i = armed.iter().position(|&s| s == seat).unwrap_or(0);
            v.player_seat = Some(armed[(i + 1) % armed.len()]);
        }
        // a new seat starts with its own view: a turret seat looks where its turret points,
        // the others straight ahead (the view of the old seat carried over, e.g. a turned gunner
        // view became the driver's first-person view, user 2026-10-20)
        if let Some(ns) = v.player_seat.filter(|&ns| ns != seat) {
            let (ty, tp) = if k.seats.get(ns).is_some_and(|s| s.turret.is_some()) { v.turrets.get(ns).copied().unwrap_or((0.0, 0.0)) } else { (0.0, 0.0) };
            v.cam_yaw = ty;
            v.cam_pitch = tp;
        }
        // HB_VEHVIEWS=1 (tests, with HB_VEHTEST): 1P at 3 s, the gunner seat at 6 s, its behind
        // view at 9 s
        if script && std::env::var_os("HB_VEHVIEWS").is_some() {
            let step = |a: f32| tt >= a && tt - dt < a;
            if step(3.0) {
                v.behind_view = false;
            }
            if step(6.0) && armed.contains(&1) {
                v.player_seat = Some(1);
            }
            if step(9.0) {
                v.gunner_3p = true;
            }
        }
        if hit(Set::Global, "EKBC_BehindView") {
            if seat == 0 {
                v.behind_view = !v.behind_view;
            } else {
                v.gunner_3p = !v.gunner_3p;
            }
        }
        if hit(set, "EKBC_Use") {
            // exit at the seat's start position socket (driver: left side)
            let exit = k.socket_world(&v, &pose, &k.seats.get(seat).map(|s| s.exit.clone()).unwrap_or_default())
                .or_else(|| k.socket_world(&v, &pose, "Seat0StartPosition"))
                .unwrap_or(v.pos + Quat::from_rotation_z(v.yaw) * Vec3::new(0.0, -(k.half_width + 80.0), 0.0));
            let mut feet = Vec3::new(exit.x, exit.z, exit.y) * 0.01;
            if let Some((y, _)) = col.floor(feet + Vec3::Y * 2.0, if heli { 60.0 } else { 4.0 }, &mut buf.0) {
                feet.y = y;
            }
            p.feet = feet;
            p.vel = Vec3::ZERO;
            p.in_vehicle = false;
            p.yaw = v.yaw / std::f32::consts::TAU * 65536.0;
            p.pitch = 0.0;
            v.player_seat = None;
            // GCVehicleAudio SCEngineShutdown when the occupant leaves
            sfx.play(&k.audio.engine_shutdown, Some(v.pos));
            if let Some(vm) = vm.as_mut() {
                vm.lowered = false;
                vm.play(crate::viewmodel::WeaponAnim::Draw);
            }
            return;
        }
    }
    let seat = v.player_seat.unwrap_or(0).min(k.seats.len().saturating_sub(1));
    let sc = k.seats.get(seat).cloned().unwrap_or_default();
    // HUD mode: the vehicle's placers and the seat's (hud.rs)
    flow.hud.vehicle_seat = Some(seat);
    flow.hud.vehicle_classes = k.classes.clone();
    // mouse: the driver of a car orbits the camera; turret seats turn the turret towards the
    // view (at most turret_rate); the helicopter pilot turns (yaw) and tilts (pitch) the
    // helicopter
    let sens = 45.0 / 65536.0 * std::f32::consts::TAU;
    let (mut mdx, mut mdy) = if input {
        let (sx, sy) = crate::settings::mouse_scale_for(if heli { "Heli" } else { "Land" }, motion.delta, false);
        (motion.delta.x * sens * sx, -motion.delta.y * sens * sy)
    } else {
        (0.0, 0.0)
    };
    if script {
        if heli {
            // nose down 4..8 s, level again after; turn right 8..10 s
            if (4.0..4.5).contains(&tt) {
                mdy = -0.5 * dt;
            }
            if (8.0..8.5).contains(&tt) {
                mdy = 0.5 * dt;
            }
            if (8.0..10.0).contains(&tt) {
                mdx = 0.8 * dt;
            }
        } else if tt > 6.0 {
            // turret round to the right and a little up
            mdx = 0.5 * dt;
            if tt < 6.5 {
                mdy = 0.15 * dt;
            }
        }
    }
    let emp_off = v.emp > 0.0;
    if emp_off {
        sfx.looping("vehicle_emp", &k.audio.emp_loop, None);
    }
    let pilot = heli && seat == 0;
    // the flight model (heli_step) takes the mouse and A/D itself
    let mut heli_yaw_key = 0.0f32;
    if pilot && k.heli.is_some() {
        v.cam_yaw = 0.0;
        v.cam_pitch = 0.0;
        if input && !emp_off {
            if down(set, "EKBC_YawLeft") {
                heli_yaw_key -= 1.0;
            }
            if down(set, "EKBC_YawRight") {
                heli_yaw_key += 1.0;
            }
        }
        if script && (8.0..10.0).contains(&tt) {
            heli_yaw_key = 1.0;
        }
    } else if pilot {
        // pitch: the nose follows the mouse (fAHMaxPitch), turns at MaxYawRate
        let maxp = k.max_pitch.to_radians();
        v.cam_pitch = (v.cam_pitch + mdy).clamp(-maxp, maxp);
        v.cam_yaw = 0.0;
        let yaw_in = (mdx / dt.max(1e-3)).clamp(-k.yaw_rate, k.yaw_rate) * dt;
        let mut key_yaw = 0.0;
        if input && !emp_off {
            if down(set, "EKBC_YawLeft") {
                key_yaw -= 1.0;
            }
            if down(set, "EKBC_YawRight") {
                key_yaw += 1.0;
            }
        }
        v.yaw += yaw_in + key_yaw * k.yaw_rate * 0.6 * dt;
    } else if sc.turret.is_some() {
        // turret: the view leads, the turret follows at its rate
        v.cam_yaw = wrap(v.cam_yaw + mdx);
        let (lo, hi) = sc.view_pitch;
        v.cam_pitch = (v.cam_pitch + mdy).clamp(lo.to_radians(), hi.to_radians());
        let (ty, tp) = v.turrets[seat];
        let (ry, rpch) = (sc.turret_rate.0.to_radians() * dt, sc.turret_rate.1.to_radians() * dt);
        // behind view: the turret turns to the camera's aim point (the camera sits above /
        // behind the gun), so the barrel points where the shots go
        let (want_yaw, want_pitch) = match (seat_first_person(&sc, &v, seat), v.aim_pt) {
            (false, Some(ap)) => {
                let from = sc.modes.first().and_then(|m| m.sockets.first()).and_then(|s| k.socket_world(&v, &pose, s)).unwrap_or(v.pos + Vec3::Z * 100.0);
                let d = (ap - from).normalize_or_zero();
                if d == Vec3::ZERO {
                    (v.cam_yaw, v.cam_pitch)
                } else {
                    (wrap(d.y.atan2(d.x) - v.yaw), d.z.clamp(-1.0, 1.0).asin() - v.pitch)
                }
            }
            _ => (v.cam_yaw, v.cam_pitch),
        };
        let ny = ty + wrap(want_yaw - ty).clamp(-ry, ry);
        let yaw_deg = ny.to_degrees();
        let (plo, phi) = (curve(&sc.turret_pitch.0, yaw_deg), curve(&sc.turret_pitch.1, yaw_deg));
        let np = (tp + (want_pitch - tp).clamp(-rpch, rpch)).clamp(plo.to_radians(), phi.to_radians().max(plo.to_radians()));
        v.turrets[seat] = (wrap(ny), np);
    } else {
        v.cam_yaw = wrap(v.cam_yaw + mdx);
        v.cam_pitch = (v.cam_pitch + mdy).clamp(-0.6, 0.5);
    }
    // driving (driver / pilot only)
    let mut throttle = 0.0;
    let mut steer_in = 0.0;
    let mut climb: f32 = 0.0;
    if script && seat == 0 && !emp_off {
        if heli {
            climb = if (1.0..4.0).contains(&tt) { 1.0 } else { 0.0 };
        } else {
            throttle = if (1.0..6.0).contains(&tt) { 1.0 } else { 0.0 };
            steer_in = if (3.5..5.5).contains(&tt) { 1.0 } else { 0.0 };
            // HB_VEHTEST_STILL=1: the vehicle stays where it spawned (damage tests: the
            // HB_VEHCAM view keeps its open ground; turret and guns still run the script)
            if std::env::var_os("HB_VEHTEST_STILL").is_some() {
                throttle = 0.0;
                steer_in = 0.0;
            }
            // HB_VEHSLOPE: straight up the slope for 12 s, logged every half second
            if std::env::var_os("HB_VEHSLOPE").is_some() {
                throttle = if (1.0..13.0).contains(&tt) { 1.0 } else { 0.0 };
                steer_in = 0.0;
                if ((tt - dt) * 2.0).floor() != (tt * 2.0).floor() {
                    info!("vehslope: t {tt:.1} pos {:?} speed {:.0} pitch {:.1}", v.pos, v.speed, v.pitch.to_degrees());
                }
            }
        }
    } else if input && seat == 0 && !emp_off {
        if heli {
            if down(set, "EKBC_Ascend") || std::env::var_os("HOMEBOTS_TEST_CLIMB").is_some() {
                climb += 1.0;
            }
            if down(set, "EKBC_Descend") {
                climb -= 1.0;
            }
        } else {
            if down(set, "EKBC_MoveForward") || std::env::var_os("HOMEBOTS_TEST_FORWARD").is_some() {
                throttle += 1.0;
            }
            if down(set, "EKBC_MoveBackward") {
                throttle -= 1.0;
            }
            if down(set, "EKBC_StrafeLeft") {
                steer_in -= 1.0;
            }
            if down(set, "EKBC_StrafeRight") || std::env::var_os("HOMEBOTS_TEST_STEER").is_some() {
                steer_in += 1.0;
            }
        }
    }
    let top = top_speed(k);
    // sprint (Shift, forward): MaxSprintSpeed for SprintTimeMax seconds, recharged at
    // SprintRechargeRate seconds per second (the native GCVehicle code is not decoded: the
    // speed cap is lifted)
    let (sprint_speed, sprint_time, sprint_recharge) = k.sprint.unwrap_or((top, 0.0, 0.0));
    let forward_input = if heli { v.cam_pitch < -0.05 } else { throttle > 0.0 };
    v.sprinting = input && seat == 0 && forward_input && down(set, "EKBC_Sprint") && v.sprint_used < sprint_time;
    if v.sprinting {
        v.sprint_used += dt;
    } else {
        v.sprint_used = (v.sprint_used - sprint_recharge * dt).max(0.0);
    }
    let cap = if v.sprinting { sprint_speed.max(top) } else { top };
    let mut pos = v.pos;
    // a bot drives / flies while the player rides in another seat (botvehicles.rs moves it)
    let bot_moves = seat != 0 && crate::botvehicles::piloted(ve);
    match k.motion {
        _ if bot_moves => {}
        Motion::Car | Motion::Tracked => {
            ground_step(k, &mut v, &mut pos, &col, &mut buf, throttle, steer_in, cap, top, dt);
        }
        Motion::Heli if k.heli.is_some() && seat == 0 => {
            let h = k.heli.as_ref().unwrap();
            let rot_units = 65536.0 / std::f32::consts::TAU;
            let rise = if emp_off { 0.0 } else { climb.clamp(-1.0, 1.0) };
            heli_step(h, k, &mut v, &mut pos, rise, heli_yaw_key, mdx * rot_units, mdy * rot_units, cap, dt, &col, &mut buf);
        }
        Motion::Heli => {
            // horizontal: the tilt accelerates along the heading (nose down = forward), drag
            // slows it; vertical: climb / descend; the pilot's view pitch is the tilt
            let fwd = Vec3::new(v.yaw.cos(), v.yaw.sin(), 0.0);
            let right = Vec3::new(-v.yaw.sin(), v.yaw.cos(), 0.0);
            let tilt = if pilot { -v.cam_pitch } else { 0.0 };
            v.speed += (tilt.sin() * HELI_ACCEL - v.speed * HELI_DRAG) * dt;
            v.side -= v.side * HELI_DRAG * dt;
            v.speed = v.speed.clamp(-cap * 0.5, cap);
            let want_vz = climb * HELI_CLIMB;
            v.vz += (want_vz - v.vz).clamp(-HELI_CLIMB * 2.0 * dt, HELI_CLIMB * 2.0 * dt);
            pos += (fwd * v.speed + right * v.side) * dt + Vec3::Z * v.vz * dt;
            // ground and walls: the fuselage as three spheres along the body (NOT from data)
            let r = (k.half_width * 0.8).clamp(60.0, 120.0);
            for fx in [-1.0f32, 0.0, 1.0] {
                let c = pos + fwd * (fx * (k.half_len * 0.5)) + Vec3::Z * (r - k.ride_height).max(0.0);
                let cb = Vec3::new(c.x, c.z, c.y) * 0.01;
                let (b0, b1) = &mut *buf;
                b1.clear();
                let corr = col.push_sphere(cb, r * 0.01, b0, b1);
                if corr.length() > 0.001 {
                    pos += Vec3::new(corr.x, corr.z, corr.y) * 100.0;
                    if b1.iter().any(|n| n.y < 0.6) {
                        v.speed *= 0.5_f32.powf(dt * 10.0);
                    }
                }
            }
            let probe = Vec3::new(pos.x, pos.z, pos.y) * 0.01 + Vec3::Y * 0.5;
            let landed = if let Some((gy, _)) = col.floor(probe, 0.5 + k.ride_height * 0.01 + 0.05, &mut buf.0) {
                let want = gy * 100.0 + k.ride_height;
                if pos.z <= want + 1.0 {
                    pos.z = want;
                    v.vz = v.vz.max(0.0);
                    v.speed *= 0.2_f32.powf(dt);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            // body: pitch with the tilt, roll into the turn (NOT from data: the angles' scale)
            let kk = (dt * 4.0).min(1.0);
            let tp = if landed { 0.0 } else { -tilt * 0.6 };
            v.pitch += (tp - v.pitch) * kk;
            let tr = if landed { 0.0 } else { (mdx / dt.max(1e-3)).clamp(-1.0, 1.0) * 0.35 };
            v.roll += (tr - v.roll) * kk;
        }
    }
    v.pos = pos;
    // the player rides along (objectives, bots' targeting)
    p.feet = Vec3::new(pos.x, pos.z - k.ride_height, pos.y) * 0.01;
    p.vel = Vec3::ZERO;
    // GCVehicleAudio SCEngineIdle while occupied: switch "Gas" = throttle held, "BypassStartup" 0,
    // loop pitch from the cue's modulator over 10..1000 uu/s (NOT from data: the native value
    // source); layer volumes follow the speed / throttle (NOT from data)
    let rev = ((v.speed.abs() - 10.0) / 990.0).clamp(0.0, 1.0);
    let gas = (throttle != 0.0 || climb != 0.0 || (heli && v.cam_pitch.abs() > 0.05)) as u8 as f32;
    let sp = (v.speed.abs() / top.max(1.0)).clamp(0.0, 1.0);
    if !emp_off {
        sfx.looping_layers(
            "vehicle_engine",
            &k.audio.engine_idle,
            None,
            &[("Gas", gas as usize), ("BypassStartup", 0), ("Sprinting", v.sprinting as usize)],
            1.0 + 0.2 * rev,
            &[("Tires", sp), ("JeepAccelerat", gas * (0.3 + 0.7 * sp)), ("HumV_Gas", gas)],
        );
    }
    if v.sprinting {
        sfx.looping("vehicle_sprint", &k.audio.engine_sprint, None);
    }
    let pose = k.pose(&v);
    // camera (before the guns: their shots aim at the camera's target)
    let mut aim_pt: Option<Vec3> = None;
    if let Ok((mut tf, mut proj)) = cams.single_mut() {
        let has_turret = sc.turret.is_some();
        // view direction: turret seats look where they aim (cam_yaw / cam_pitch relative to the
        // body), the pilot along the nose, the car driver free
        let (vyaw, vpitch) = if pilot { (v.yaw, if k.heli.is_some() { v.pitch } else { v.cam_pitch * 0.5 }) } else if has_turret { (v.yaw + v.cam_yaw, v.cam_pitch) } else { (v.yaw + v.cam_yaw, v.cam_pitch) };
        let look = Vec3::new(vyaw.cos() * vpitch.cos(), vyaw.sin() * vpitch.cos(), vpitch.sin());
        // GCVehicle VehicleCalcCameraThirdPerson / FirstPerson (script, decoded 2026-10-20):
        // ESeatAllowedViews picks the view (BOTH: C toggles).
        let third = !seat_first_person(&sc, &v, seat);
        let body = body_rot(&v);
        let (eye, look) = if third {
            // rotation: the socket's (bCameraFixedToSocket3P: taken as the seat's aim / the
            // body) or the view rotation, plus BehindCameraRotation
            let (byaw, bpitch) = (vyaw, vpitch);
            let rot = Quat::from_rotation_z(byaw) * Quat::from_rotation_y(-(bpitch + sc.behind_pitch));
            // pivot: fixed to the socket -> CameraTag3p / CameraTag socket + BehindCameraMinOffset
            // (vehicle space); else Location + (0, 0, BaseEyeHeight 50) + CameraOffset (vehicle space)
            let tag = if sc.camera_tag3p.is_empty() { &sc.camera_tag } else { &sc.camera_tag3p };
            let pivot = if sc.fixed3p && !tag.is_empty() {
                k.socket_world(&v, &pose, tag).unwrap_or(v.pos + body * sc.camera_offset) + body * sc.behind_min
            } else {
                v.pos + Vec3::Z * 50.0 + body * sc.camera_offset
            };
            // BehindCameraMaxOffset in the camera rotation (MaxOffsetSprint while sprinting)
            let max = if v.sprinting && sc.behind_sprint.length() > 1.0 { sc.behind_sprint } else { sc.behind };
            let max = if max.length() > 1.0 { max } else { Vec3::new(-400.0, 0.0, 50.0) };
            let mut cam = pivot + rot * max;
            // keep the camera out of walls (the game traces with a 20 unit extent)
            let pb = Vec3::new(pivot.x, pivot.z, pivot.y) * 0.01;
            let cb = Vec3::new(cam.x, cam.z, cam.y) * 0.01;
            let dl = (cb - pb).length();
            if let Some((t, _)) = col.raycast(pb, (cb - pb) / dl.max(1e-4), dl, &mut buf.0) {
                cam = pivot + (cam - pivot) * ((t - 0.2).max(0.1) / dl);
            }
            (cam, rot * Vec3::X)
        } else {
            // the CameraTag socket of the seat's cockpit mesh (bUseCockpitMesh1p /
            // bUseSecondaryCockpitMesh1p, on the outer mesh's bones), else of the outer mesh;
            // the view rotation (fixed seats: the socket's, taken as the aim / the body)
            let tag = if sc.camera_tag.is_empty() { "Seat0CharacterSocket".to_string() } else { sc.camera_tag.clone() };
            let at = if sc.cockpit1p > 0 { k.cockpit_socket_world(&v, &pose, sc.cockpit1p as usize - 1, &tag) } else { None }
                .or_else(|| k.socket_world(&v, &pose, &tag))
                .unwrap_or(v.pos + Vec3::Z * (sc.camera_offset.z + 50.0));
            // bCameraFixedToSocket1P turret seats (M1A3 seat 1 MGViewSocket on MG_PITCH, AH64
            // gunner): the view turns with the socket, i.e. the turret, which follows the mouse
            // at its rate; the seat's cockpit mesh hangs on the same bones and stays in frame
            // (before, the view led the turret and looked past the interior)
            let look = if sc.fixed1p && sc.turret.is_some() && !pilot { seat_dir(&v, k, seat) } else { look };
            (at, look)
        };
        // the aim point: the camera ray's first world hit (else 500 m out); the seat's shots fly
        // from the muzzle to it, so they land under the screen-centre crosshair also in the
        // behind view (its camera sits above / behind the gun and BehindCameraRotation tilts it).
        // UE3 vehicles aim the same way (camera trace for the weapon aim); the 500 m is NOT from
        // data
        {
            let eb = Vec3::new(eye.x, eye.z, eye.y) * 0.01;
            let lb = Vec3::new(look.x, look.z, look.y).normalize_or_zero();
            let t = col.raycast(eb, lb, 500.0, &mut buf.0).map(|(t, _)| t).unwrap_or(500.0);
            aim_pt = Some(eye + look.normalize_or_zero() * t * 100.0);
            v.aim_pt = aim_pt;
        }
        // HB_VEHCAM=<yaw deg>,<distance>,<height>: test view of the vehicle from outside
        // (relative to the body, looking at the origin)
        let (eye, look) = match std::env::var("HB_VEHCAM").ok().map(|s| s.split(',').filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>()) {
            Some(c) if c.len() == 3 => {
                let a = v.yaw + c[0].to_radians();
                let mut eye = v.pos + Vec3::new(a.cos(), a.sin(), 0.0) * c[1] + Vec3::Z * c[2];
                // pulled in front of the first wall / terrain between the vehicle and the eye
                // (like a camera spring arm), so a drive past a house does not put the view
                // inside it (test camera, the 30 uu margin is NOT from data)
                let tgt = v.pos + Vec3::Z * 60.0;
                let (ob, d) = (Vec3::new(tgt.x, tgt.z, tgt.y) * 0.01, eye - tgt);
                let db = Vec3::new(d.x, d.z, d.y).normalize_or_zero();
                if let Some((t, _)) = col.raycast(ob, db, d.length() * 0.01, &mut buf.0) {
                    eye = tgt + d.normalize_or_zero() * (t * 100.0 - 30.0).max(50.0);
                }
                (eye, (tgt - eye).normalize_or_zero())
            }
            _ => (eye, look),
        };
        tf.translation = Vec3::new(eye.x, eye.z, eye.y) * 0.01;
        tf.look_to(Vec3::new(look.x, look.z, look.y), Vec3::Y);
        flow.hud.vehicle_first_person = !third;
        // RangeToTarget: the camera ray's first hit (m)
        let lb = Vec3::new(look.x, look.z, look.y).normalize_or_zero();
        flow.hud.vehicle_range = col.raycast(tf.translation, lb, 500.0, &mut buf.0).map(|(t, _)| t);
        // the socket's roll: in the first-person view of the body-fixed seats, in the behind
        // view unless bCameraDoesNotRoll3P
        if (!third && (heli || sc.fixed1p)) || (third && !sc.no_roll_3p && heli) {
            tf.rotate_local_z(-v.roll);
        }
        if let Projection::Perspective(pp) = &mut *proj {
            pp.fov = crate::settings::vfov().to_radians();
        }
    }
    // ---- the seat's gun: fire mode 0 on EKBC_Fire, 1 on EKBC_AltFire
    let gun_heat_shown = sc.modes.iter().find_map(|m| if let FireKind::Instant(g) = &m.kind { Some(g.overheat_max) } else { None });
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
            if st.reload[m] > 0.0 {
                st.reload[m] -= dt;
                if st.reload[m] <= 0.0 {
                    if let FireKind::Projectile(rm, _) = &sc.modes[m].kind {
                        st.ammo[m] = rm.clip.max(1);
                    }
                }
            }
        }
    }
    if let Some(st) = v.guns.get(seat) {
        // the reload bar (GCUIObject_WeaponReloadOverheat): heat of the machine gun, else the
        // reload of the first projectile mode
        let reload = sc.modes.iter().enumerate().find_map(|(m, fm)| if let FireKind::Projectile(rm, _) = &fm.kind { Some((st.reload.get(m).copied().unwrap_or(0.0) / rm.reload.max(1e-3)).clamp(0.0, 1.0)) } else { None });
        let heat = gun_heat_shown.map(|mx| (st.heat / mx.max(1e-3)).clamp(0.0, 1.0)).or(reload).unwrap_or(0.0);
        let st_time = k.sprint.map(|s| s.1).unwrap_or(1.0);
        flow.hud.vehicle_gauges = Some((1.0 - (v.sprint_used / st_time.max(1e-3)).clamp(0.0, 1.0), v.sprinting, heat, st.overheated > 0.0));
    }
    // the gun's crosshair at its spread (same projection as the infantry hip crosshair)
    if let Some(g) = sc.modes.iter().find_map(|m| if let FireKind::Instant(g) = &m.kind { Some(g) } else { None }) {
        let half = (g.spread * 0.5).to_radians().tan();
        flow.hud.vehicle_crosshair = Some(360.0 * half / (crate::settings::vfov().to_radians() * 0.5).tan());
    } else if !sc.modes.is_empty() {
        flow.hud.vehicle_crosshair = Some(4.0);
    }
    let test_fire = std::env::var_os("HOMEBOTS_TEST_FIRE").is_some();
    // the script: fire mode 0 from 7 s (held 1.5 s), mode 1 at 10 s
    let script_fire = |m: usize| script && if m == 0 { (7.0..8.5).contains(&tt) } else { (10.0..10.3).contains(&tt) };
    let can_fire = input && !emp_off;
    let dir = seat_dir(&v, k, seat);
    // from the muzzle to the camera's aim point (within 45 degrees of the gun's direction)
    let aim_from = |o: Vec3| aim_pt.map(|p| (p - o).normalize_or_zero()).filter(|a| a.dot(dir) > 0.7).unwrap_or(dir);
    let list: Vec<(Entity, Vec3, u8, bool)> = soldiers.iter().map(|(e, s)| (e, s.center_ue(), s.team, s.alive)).collect();
    for (m, fm) in sc.modes.iter().enumerate().take(2) {
        let trig_name = if m == 0 { "EKBC_Fire" } else { "EKBC_AltFire" };
        match &fm.kind {
            FireKind::Instant(g) => {
                let st = &v.guns[seat];
                let trigger = can_fire && st.overheated <= 0.0 && (down(set, trig_name) || test_fire && m == 0 || script_fire(m));
                if !(trigger && st.fire_cd <= 0.0) {
                    continue;
                }
                let st = &mut v.guns[seat];
                st.fire_cd = g.fire_interval;
                st.heat += g.overheat_add + g.fire_interval;
                if st.heat >= g.overheat_max {
                    st.overheated = g.overheat_recover;
                }
                st.shots += 1;
                let shots = st.shots;
                let sock = if fm.sockets.is_empty() { None } else { Some(fm.sockets[shots as usize % fm.sockets.len()].clone()) };
                let o = sock.and_then(|s| k.socket_world(&v, &pose, &s)).unwrap_or(v.pos + Vec3::Z * 100.0);
                let mut seed = v.seed;
                let d = crate::combat::spread_dir(aim_from(o), g.spread * 0.5, &mut seed);
                v.seed = seed;
                let snd = if fm.sound.is_empty() { &g.sound } else { &fm.sound };
                if g.sound_looping {
                    wsfx.shot_cue(&format!("vehicle_gun{m}"), snd, None, g.fire_interval + 0.06);
                } else {
                    sfx.play(snd, None);
                }
                flash_fx(k, &v, &pose, &fm.flash_tags, d, &mut fx);
                let shot = crate::combat::trace_shot(&col, &list, None, o, d, g.range, None, true, &mut buf.0);
                // tracer (VehicleFireModes Tracer, every Frequency-th shot) to the impact point
                if let Some(tc) = g.tracer.as_ref().filter(|tc| shots % tc.frequency.max(1) == 0) {
                    let dist = match &shot {
                        ShotHit::Soldier(_, t, _) | ShotHit::World(t, _, _) => *t,
                        _ => g.range,
                    };
                    // combat::fire_tracer: random yellow / red / green and a light (user request)
                    crate::combat::fire_tracer(&mut fx, tc, o, crate::combat::dir_quat(d), d, dist / tc.speed.max(1.0));
                }
                if let Some(im) = impacts.as_ref() {
                    let center = if let ShotHit::Soldier(e, ..) = &shot { list.iter().find(|s| s.0 == *e).map(|s| s.1) } else { None };
                    im.shot(&shot, o, d, &g.dmg_type, center, &mut fx, &mut sfx);
                }
                let impact = match &shot {
                    ShotHit::Soldier(_, _, hp) => Some(*hp),
                    ShotHit::World(t, _, _) => Some(o + d * *t),
                    _ => None,
                };
                let direct = if let ShotHit::Soldier(e, ..) = &shot { Some(*e) } else { None };
                if let (ShotHit::Soldier(e, t, hp), Some(data)) = (shot, data.as_ref()) {
                    if let Some(s) = list.iter().find(|s| s.0 == e) {
                        if s.2 != player_team() {
                            let (mat, head) = if hp.z > s.1.z + crate::combat::HEAD_OFFSET - crate::combat::HEAD_RADIUS { ("PMA_Head", true) } else { ("PMA_Soldier_Chest", false) };
                            let dmg = data.grid.damage(&g.dmg_type, mat, t, g.full_range, g.range);
                            queue.0.push(Hit { target: Target::Bot(e), amount: dmg, attacker: Actor::Player, attacker_team: player_team(), headshot: head, explosive: false, from: o, icon: None, dmg_type: Some(g.dmg_type.clone()) });
                        }
                    }
                }
                // splash (SplashDamageClass within SplashDamageRadius, full inside
                // SplashDamageRadiusFull; NOT from data: linear falloff between, the directly hit
                // soldier left out)
                if let (Some((sc, radius, full)), Some(at), Some(data)) = (g.splash.as_ref(), impact, data.as_ref()) {
                    for s in list.iter().filter(|s| s.2 != player_team() && Some(s.0) != direct) {
                        let dist = s.1.distance(at);
                        if dist >= *radius {
                            continue;
                        }
                        let f = if dist <= *full { 1.0 } else { 1.0 - (dist - full) / (radius - full).max(1.0) };
                        let dmg = data.grid.damage(sc, "PMA_Soldier", 0.0, 1.0, 1.0) * f;
                        if dmg > 0.0 {
                            queue.0.push(Hit { target: Target::Bot(s.0), amount: dmg, attacker: Actor::Player, attacker_team: player_team(), headshot: false, explosive: true, from: at, icon: None, dmg_type: Some(sc.clone()) });
                        }
                    }
                }
            }
            FireKind::Projectile(rm, wi) => {
                // a clip of AmmoClip, FireBurstCount shots per pull FireInterval apart, ReloadTime
                // when empty (lock-on modes fly straight: no lock-on here)
                let st = &v.guns[seat];
                let pulled = can_fire && (hit(set, trig_name) || test_fire && m == 0 && st.shots == 0 || script_fire(m));
                if pulled && st.burst.is_none() && st.ammo[m] > 0 && st.reload[m] <= 0.0 {
                    let n = rm.burst.max(1).min(st.ammo[m]);
                    v.guns[seat].burst = Some((m, n, 0.0));
                }
                let Some((bm, left, cd)) = v.guns[seat].burst.filter(|b| b.0 == m) else { continue };
                let cd = cd - dt;
                if cd > 0.0 {
                    v.guns[seat].burst = Some((bm, left, cd));
                    continue;
                }
                let st = &mut v.guns[seat];
                st.shots += 1;
                let shots = st.shots;
                let sock = if fm.sockets.is_empty() { None } else { Some(fm.sockets[shots as usize % fm.sockets.len()].clone()) };
                let o = sock.and_then(|s| k.socket_world(&v, &pose, &s)).unwrap_or(v.pos + dir * 200.0 + Vec3::Z * 100.0);
                let mut seed = v.seed;
                let d = crate::combat::spread_dir(aim_from(o), rm.spread * 0.5, &mut seed);
                v.seed = seed;
                flash_fx(k, &v, &pose, &fm.flash_tags, d, &mut fx);
                let trail = rm.missile.flight_fx.as_ref().and_then(|t| fx.fire_tracked(t, o, crate::combat::dir_quat(d)));
                // GCProj_Rocket LifeSpan 8
                grenades.list.push(crate::combat::Grenade { rocket: Some(*wi), pos: o, vel: d * rm.missile.speed, fuse: 8.0, owner: Actor::Player, team: player_team(), resting: false, trail, tac: None, payload: None });
                if !fm.sound.is_empty() {
                    sfx.play(&fm.sound, None);
                }
                let st = &mut v.guns[seat];
                st.ammo[m] -= 1;
                if st.ammo[m] <= 0 {
                    st.reload[m] = rm.reload;
                }
                st.burst = (left > 1 && st.ammo[m] > 0).then_some((m, left - 1, rm.interval));
            }
        }
    }
}

fn render_vehicles(
    mut commands: Commands,
    assets: Option<Res<VehicleAssets>>,
    vehicles: Query<&Vehicle>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ents: Query<(Entity, &Mesh3d, &mut Visibility, &mut Transform), Without<Camera3d>>,
    (mut shown, mut for_assets): (Local<Vec<Vec<u8>>>, Local<Option<AssetId<Mesh>>>),
    me: Res<crate::MeshEntities>,
    mut last_state: Local<HashMap<(usize, usize), (u8, Vec<f32>)>>,
) {
    let _perf = crate::perf::scope("render_vehicles");
    let Some(assets) = assets else { return };
    // new vehicle assets (the next match loads them again, their mesh entities spawned
    // visible at the origin in UE units): everything counts as shown, so it gets hidden (the
    // flags of the last match kept them "hidden" and the new meshes stood in the level 100
    // times too large, user 2026-10-19). Per slot: bit 0 outer mesh, bits 1 / 2 the cockpit
    // meshes.
    let id = assets.kinds.first().and_then(|k| k.slots.first()).and_then(|s| s.part.meshes.first()).map(|h| h.id());
    if shown.len() != assets.kinds.len() || *for_assets != id {
        *shown = assets.kinds.iter().map(|k| vec![7u8; k.slots.len()]).collect();
        *for_assets = id;
    }
    let mut now: Vec<Vec<u8>> = assets.kinds.iter().map(|k| vec![0u8; k.slots.len()]).collect();
    for v in &vehicles {
        let k = &assets.kinds[v.kind];
        let Some(slot) = k.slots.get(v.slot) else { continue };
        // the player's first-person view: the seat's cockpit mesh, the outer mesh hidden with
        // bDisable3pMeshIn1p
        let fp = v.player_seat.and_then(|s| k.seats.get(s).map(|sc| (sc, s))).filter(|(sc, s)| seat_first_person(sc, v, *s)).map(|(sc, _)| sc);
        let cockpit = fp.map(|sc| sc.cockpit1p).unwrap_or(0) as usize;
        let mut bits = if fp.is_some_and(|sc| sc.hide_outer_1p) { 0 } else { 1 };
        if cockpit > 0 && slot.cockpit[cockpit - 1].is_some() {
            bits |= 1 << cockpit;
        }
        now[v.kind][v.slot] = bits;
        // a parked vehicle does not move: the skinned meshes keep last frame's vertices (the
        // CPU skinning of every parked vehicle cost ~2 ms a frame and re-uploaded its meshes,
        // perf log 2026-10-21); skinned again when anything of the pose or the view changes
        let mut state = vec![v.pos.x, v.pos.y, v.pos.z, v.yaw, v.pitch, v.roll, v.steer, v.wheel_spin.0, v.wheel_spin.1];
        state.extend(v.turrets.iter().flat_map(|t| [t.0, t.1]));
        state.extend(v.rotor.iter().copied());
        state.push(v.armor_bits as f32);
        let changed = last_state.get(&(v.kind, v.slot)).is_none_or(|(b, st)| *b != bits || *st != state) || shown.get(v.kind).and_then(|s| s.get(v.slot)).is_none_or(|&b| b != bits);
        if !changed {
            continue;
        }
        // only once the mesh entities are indexed (else their translation stays at the origin)
        let ready = crate::mesh_ents(&me, &slot.part.meshes).next().is_some() && slot.attach.iter().all(|a| crate::mesh_ents(&me, &a.meshes).next().is_some());
        if ready {
            last_state.insert((v.kind, v.slot), (bits, state));
        }
        let pose = k.pose(v);
        let rot = body_rot(v);
        let tr = Vec3::new(v.pos.x, v.pos.z, v.pos.y) * 0.01;
        if bits & 1 != 0 {
            let scales = k.scales_for(v);
            crate::bots::skin_part_scaled(&slot.part, &pose, rot, v.pos, &mut meshes, scales.as_deref());
            if let Some(t) = &k.tread {
                // powered wheel rotation from the tread roll (wheel_spin is per k.wheel_radius)
                for (side, spin) in [v.wheel_spin.0, v.wheel_spin.1].into_iter().enumerate() {
                    let deg = (spin * k.wheel_radius / t.radius.max(1.0)).to_degrees();
                    crate::viewmodel::scroll_tread(&mut meshes, &slot.tread_uv[side], deg, t.tex_pan);
                }
            }
            for e in crate::mesh_ents(&me, &slot.part.meshes) {
                if let Ok((_, _, _, mut tf)) = ents.get_mut(e) {
                    if tf.translation != tr {
                        tf.translation = tr;
                    }
                }
            }
            for at in &slot.attach {
                crate::bots::skin_part(at, &pose, rot, v.pos, &mut meshes);
                for e in crate::mesh_ents(&me, &at.meshes) {
                    if let Ok((_, _, _, mut tf)) = ents.get_mut(e) {
                        if tf.translation != tr {
                            tf.translation = tr;
                        }
                    }
                }
            }
        }
        if cockpit > 0 {
            if let Some(cp) = slot.cockpit[cockpit - 1].as_ref() {
                let origin = v.pos + rot * k.cockpit_tr[cockpit - 1];
                crate::bots::skin_part(cp, &pose, rot, origin, &mut meshes);
                let ctr = Vec3::new(origin.x, origin.z, origin.y) * 0.01;
                for e in crate::mesh_ents(&me, &cp.meshes) {
                    if let Ok((_, _, _, mut tf)) = ents.get_mut(e) {
                        if tf.translation != ctr {
                            tf.translation = ctr;
                        }
                    }
                }
            }
        }
    }
    for (ki, k) in assets.kinds.iter().enumerate() {
        for (i, slot) in k.slots.iter().enumerate() {
            if now[ki][i] == shown[ki][i] {
                continue;
            }
            let (old, new) = (shown[ki][i], now[ki][i]);
            shown[ki][i] = new;
            let parts: Vec<(&SkinPart, u8)> = std::iter::once((&slot.part, 1u8)).chain(slot.attach.iter().map(|p| (p, 1u8))).chain(slot.cockpit.iter().enumerate().filter_map(|(c, p)| p.as_ref().map(|p| (p, 2u8 << c)))).collect();
            for (part, bit) in parts {
                if old & bit == new & bit {
                    continue;
                }
                let vis_on = new & bit != 0;
                for (e, mh, mut vis, _) in &mut ents {
                    if part.meshes.iter().any(|h| h.id() == mh.0.id()) {
                        *vis = if vis_on { Visibility::Visible } else { Visibility::Hidden };
                        commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                    }
                }
            }
        }
    }
}


/// Wheels / treads on the ground for one frame (speed from the input, steering, wall spheres,
/// ground probes, body pitch / roll); `pos` is the vehicle origin (UE units). Shared by drive
/// and the HB_SLOPETEST bench.
#[allow(clippy::too_many_arguments)]
/// Face normal up component of drivable ground (about 53 degrees; the wall check before used
/// the same value on the contact normals). NOT from data.
const DRIVABLE_UP: f32 = 0.6;

pub(crate) fn ground_step(k: &VehicleKind, v: &mut Vehicle, pos_out: &mut Vec3, col: &CollisionWorld, buf: &mut (Vec<u32>, Vec<Vec3>), throttle: f32, steer_in: f32, cap: f32, top: f32, dt: f32) -> bool {
    let mut pos = *pos_out;
    let tracked = k.motion == Motion::Tracked;
    if throttle != 0.0 {
        if v.speed * throttle < -10.0 {
            v.speed += throttle * BRAKE * dt;
        } else if tracked {
            v.speed += throttle * TANK_ACCEL * dt;
        } else {
            let torque_max = k.torque_curve.iter().map(|p| p.1).fold(0.0f32, f32::max).max(1e-3);
            let tq = curve(&k.torque_curve, v.speed.abs() * throttle.signum()) / torque_max;
            v.speed += throttle * ACCEL * tq.max(0.0) * dt;
        }
    } else {
        let d = COAST * dt * if tracked { 2.0 } else { 1.0 };
        v.speed = if v.speed.abs() <= d { 0.0 } else { v.speed - d * v.speed.signum() };
    }
    if v.sprinting {
        v.speed += ACCEL * dt * 0.5;
    }
    v.speed = if v.speed > cap { (v.speed - BRAKE * dt).max(cap) } else { v.speed }.max(-top * 0.5);
    let yaw_rate = if tracked {
        // tread speed difference m_fTreadMaxSpeedSteering over the track width; turns on
        // the spot; reversing mirrors the steering
        let dir = if v.speed < -10.0 { -1.0 } else { 1.0 };
        v.steer = steer_in;
        steer_in * dir * k.tread_steer / (2.0 * k.track_half.max(10.0))
    } else {
        let max_steer = curve(&k.steer_curve, v.speed.abs()).to_radians();
        let target = steer_in * max_steer;
        let sr = 3.0 * dt;
        v.steer += (target - v.steer).clamp(-sr, sr);
        // bicycle model with the rear axle's SteerFactor
        let sf_front = k.wheels.iter().map(|w| w.1).fold(f32::MIN, f32::max).max(0.0);
        let sf_rear = k.wheels.iter().map(|w| w.1).fold(f32::MAX, f32::min).min(0.0);
        v.speed * ((v.steer * sf_front).tan() - (v.steer * sf_rear).tan()) / k.wheel_base.max(1.0)
    };
    v.yaw += yaw_rate * dt;
    let turn = yaw_rate * k.track_half;
    let roll_l = (v.speed + turn) / k.wheel_radius.max(1.0) * dt;
    let roll_r = (v.speed - turn) / k.wheel_radius.max(1.0) * dt;
    v.wheel_spin = (wrap(v.wheel_spin.0 + if tracked { roll_l } else { v.speed / k.wheel_radius.max(1.0) * dt }), wrap(v.wheel_spin.1 + if tracked { roll_r } else { v.speed / k.wheel_radius.max(1.0) * dt }));
    let fwd = Vec3::new(v.yaw.cos(), v.yaw.sin(), 0.0);
    let right = Vec3::new(-v.yaw.sin(), v.yaw.cos(), 0.0);
    pos += fwd * v.speed * dt;
    // walls: spheres at the corners (radius ~ half the width, at bumper height)
    let r = (k.half_width * 0.9).clamp(30.0, 90.0);
    let mut blocked = false;
    for (fx, sy) in [(1.0f32, -1.0f32), (1.0, 1.0), (-1.0, -1.0), (-1.0, 1.0)] {
        let c = pos + fwd * (fx * (k.half_len - r * 0.5)) + right * (sy * (k.half_width - r)) + Vec3::Z * (r - k.ride_height * 0.4).max(10.0);
        let cb = Vec3::new(c.x, c.z, c.y) * 0.01;
        let (b0, b1) = &mut *buf;
        b1.clear();
        // drivable ground (face normal up >= 0.6) is the wheels' business: only steeper
        // triangles count as walls
        let corr = col.push_sphere_steep(cb, r * 0.01, DRIVABLE_UP, b0, b1);
        let ch = Vec3::new(corr.x, 0.0, corr.z);
        if ch.length() > 0.005 && std::env::var_os("HB_SLOPEDBG").is_some() {
            eprintln!("blocked corner ({fx}, {sy}) centre {c:?} pos {pos:?} corr {corr:?} normals {:?}", b1);
        }
        if ch.length() > 0.005 {
            pos += Vec3::new(ch.x, ch.z, 0.0) * 100.0;
            blocked = true;
        }
    }
    if blocked {
        v.speed *= 0.5_f32.powf(dt * 10.0);
    }
    // ground: probes under the wheels / treads
    let hs: Vec<Option<f32>> = k
        .probes
        .iter()
        .map(|pr| {
            let w = pos + fwd * pr.x + right * pr.y;
            let probe = Vec3::new(w.x, w.z, w.y) * 0.01 + Vec3::Y * 1.2;
            col.floor(probe, 3.5, &mut buf.0).map(|(y, _)| y * 100.0)
        })
        .collect();
    let known: Vec<f32> = hs.iter().flatten().copied().collect();
    if known.is_empty() {
        v.vz -= 750.0 * k.gravity_factor * dt;
        pos.z += v.vz * dt;
    } else {
        let ground = known.iter().sum::<f32>() / known.len() as f32;
        let want = ground + k.ride_height;
        if pos.z > want + 2.0 {
            v.vz -= 750.0 * k.gravity_factor * dt;
            pos.z = (pos.z + v.vz * dt).max(want);
        } else {
            v.vz = 0.0;
            pos.z = want;
        }
        // body pitch / roll from the probes' front / rear and left / right averages
        let avg = |f: &dyn Fn(Vec2) -> bool| {
            let s: Vec<f32> = k.probes.iter().zip(hs.iter()).filter(|(p, _)| f(**p)).map(|(_, h)| h.unwrap_or(ground)).collect();
            if s.is_empty() { ground } else { s.iter().sum::<f32>() / s.len() as f32 }
        };
        let front = avg(&|p| p.x > 0.0);
        let rear = avg(&|p| p.x <= 0.0);
        let left = avg(&|p| p.y < 0.0);
        let rightv = avg(&|p| p.y >= 0.0);
        let tp = ((front - rear) / k.wheel_base.max(1.0)).atan();
        let tr = -((left - rightv) / (2.0 * k.track_half.max(10.0))).atan();
        let kk = (dt * 8.0).min(1.0);
        v.pitch += (tp - v.pitch) * kk;
        v.roll += (tr - v.roll) * kk;
    }
    *pos_out = pos;
    blocked
}

/// A vehicle at `pos` (UE units, origin) with heading `yaw`, the player in seat 0.
pub(crate) fn new_vehicle(k: &VehicleKind, kind: usize, slot: usize, pos: Vec3, yaw: f32) -> Vehicle {
    Vehicle {
        kind,
        slot,
        pos,
        yaw,
        pitch: 0.0,
        roll: 0.0,
        speed: 0.0,
        side: 0.0,
        vz: 0.0,
        steer: 0.0,
        wheel_spin: (0.0, 0.0),
        turrets: vec![(0.0, 0.0); k.seats.len()],
        player_seat: Some(0),
        behind_view: true,
        gunner_3p: false,
        aim_pt: None,
        cam_yaw: 0.0,
        cam_pitch: -0.1,
        guns: k.seats.iter().map(|s| SeatState { ammo: s.modes.iter().map(|m| if let FireKind::Projectile(rm, _) = &m.kind { rm.clip.max(1) } else { -1 }).collect(), reload: vec![0.0; s.modes.len()], ..Default::default() }).collect(),
        health: k.health,
        sprint_used: 0.0,
        sprinting: false,
        emp: 0.0,
        rotor: vec![0.0; k.rotors.len()],
        seed: 0x1357_9bdf ^ (slot as u32 * 7919),
        vel: Vec3::ZERO,
        ang: Vec3::ZERO,
        armor: vec![k.damage.part_health; k.damage.parts.len()],
        armor_bits: 0,
        dmg_events: Vec::new(),
    }
}

/// Speed cap without sprint: the car's TorqueVSpeedCurve end (torque <= 0.5), else MaxSpeed.
pub(crate) fn top_speed(k: &VehicleKind) -> f32 {
    match k.motion {
        Motion::Car => {
            let t = k.torque_curve.iter().filter(|p| p.0 > 0.0 && p.1 <= 0.5).map(|p| p.0).fold(f32::MAX, f32::min).min(2000.0);
            if t == f32::MAX { k.max_speed } else { t }
        }
        _ => k.max_speed,
    }
}

/// HB_VEHSLOPE: a 20 m run of drivable ground (all samples' normal up >= 0.6, three parallel
/// lines 3 m apart) rising at about `deg` within 150 m of `near` (Bevy metres): (foot, UE yaw,
/// mean angle).
fn find_slope(col: &CollisionWorld, near: Vec3, deg: f32, buf: &mut Vec<u32>) -> Option<(Vec3, f32, f32)> {
    let mut best: Option<(f32, Vec3, f32, f32)> = None;
    let h = |x: f32, z: f32, buf: &mut Vec<u32>| col.floor(Vec3::new(x, near.y + 60.0, z), 160.0, buf).filter(|(_, n)| n.y >= DRIVABLE_UP).map(|(y, _)| y);
    for ix in -37..=37 {
        for iz in -37..=37 {
            let (x0, z0) = (near.x + ix as f32 * 4.0, near.z + iz as f32 * 4.0);
            'dir: for d in 0..16 {
                let a = d as f32 / 16.0 * std::f32::consts::TAU;
                // Bevy x = UE x, Bevy z = UE y
                let (fx, fz) = (a.cos(), a.sin());
                let mut angles = Vec::new();
                let mut foot = None;
                for side in [-1.5f32, 0.0, 1.5] {
                    let (sx, sz) = (x0 - fz * side, z0 + fx * side);
                    let Some(mut last) = h(sx, sz, buf) else { continue 'dir };
                    if side == 0.0 {
                        foot = Some(Vec3::new(sx, last, sz));
                    }
                    for k in 1..=20 {
                        let Some(y) = h(sx + fx * k as f32, sz + fz * k as f32, buf) else { continue 'dir };
                        let seg = (y - last).atan2(1.0).to_degrees();
                        if (seg - deg).abs() > 10.0 {
                            continue 'dir;
                        }
                        angles.push(seg);
                        last = y;
                    }
                }
                let mean = angles.iter().sum::<f32>() / angles.len() as f32;
                let err = (mean - deg).abs();
                if best.as_ref().is_none_or(|b| err < b.0) {
                    best = Some((err, foot?, a, mean));
                }
            }
        }
    }
    best.filter(|b| b.0 < 3.0).map(|b| (b.1, b.2, b.3))
}

fn k_ride(assets: &VehicleAssets, kind: usize) -> f32 {
    assets.kinds.get(kind).map(|k| k.ride_height).unwrap_or(0.0)
}

/// Effects of a shot's FlashTags: the mesh sockets' References with that StartTag (muzzle
/// flashes FX_LAV_MF, MF_Tank_3P, 50Cal_3P ...), along the shot direction.
pub(crate) fn flash_fx(k: &VehicleKind, v: &Vehicle, pose: &[(Quat, Vec3)], tags: &[String], d: Vec3, fx: &mut crate::particles::Particles) {
    for tag in tags {
        let tl = tag.to_ascii_lowercase();
        for (t, sock, tpl) in k.socket_fx.iter().filter(|e| e.0 == tl) {
            let _ = t;
            if let Some(at) = k.socket_world(v, pose, sock) {
                fx.fire(tpl, at, crate::combat::dir_quat(d));
            }
        }
    }
}

/// Gravity for the flight model (UE units / s^2; NOT from data: the ground vehicles' value).
const HELI_G: f32 = 750.0;
/// NOT from data (PhysX body damping not decoded): linear and angular damping per second
/// on top of the sim's own terms, so the helicopter does not coast or spin forever.
const HELI_LIN_DAMP: f32 = 0.35;
const HELI_ANG_DAMP: f32 = 2.5;
const HELI_YAW_DAMP: f32 = 1.2;
/// Levelling outside the auto-level window (1/s^2 per radian of bank / pitch, NOT from data):
/// with HELI_ANG_DAMP about 0.8 x the angle per second back towards level
const HELI_LEVEL: f32 = 2.0;

/// Player helicopter for one frame: GCVehicleSimChopperBase::UpdateVehicle, the human-controlled
/// branch (EXE 0x0071bf45..0x0071d47d, reversed 2026-10-20) with the power level P = 1:
/// collective (Rise: W / S) along the lift direction A = normalize(Z.x, Z.y, Z.z - 0.4),
/// passive lift PassiveLiftFactor * A (+ 2.25 % hover margin), collective compensation,
/// air resistance AirSpeed * AirResistanceMultiplier, A / D yaw torque TurnTorqueFactor, mouse x
/// roll torque RollTorqueStrafeFactor, mouse y pitch torque PitchTorqueFactor (both per frame
/// in rotator units, clamped to the Max values), damping terms, auto-levelling with the
/// Upright factors and the StayUpright constraint springs. Forces scale as
/// a = F * g / (1.0225 * PassiveLiftFactor) (NOT from data: the PhysX mass / unit conversion;
/// chosen so the passive lift hovers), torques as T / InertiaTensorOverride.
#[allow(clippy::too_many_arguments)]
pub(crate) fn heli_step(h: &HeliSim, k: &VehicleKind, v: &mut Vehicle, pos: &mut Vec3, rise: f32, steer: f32, yaw_in: f32, pitch_in: f32, cap: f32, dt: f32, col: &CollisionWorld, buf: &mut (Vec<u32>, Vec<Vec3>)) {
    if dt <= 0.0 {
        return;
    }
    let q = body_rot(v);
    let (x, y, z) = (q * Vec3::X, q * Vec3::Y, q * Vec3::Z);
    let a = Vec3::new(z.x, z.y, z.z - 0.4).normalize_or(Vec3::Z);
    let hz = Vec3::new(a.x, a.y, 0.0);
    let t = hz.length();
    let hdir = hz.normalize_or_zero();
    let r_flat = Vec3::new(y.x, y.y, 0.0).normalize_or_zero();
    let up_ok = a.z > 0.0 && t < h.reasonably_upright;
    let vel = v.vel;
    let k_air = cap.max(1.0) * h.air_resistance;
    // forces
    let mut f = Vec3::ZERO;
    let rise_f = h.active_lift * rise;
    if rise_f < 0.0 {
        f.z += rise_f * (1.0 - t).max(0.6);
        if vel.z >= 20.0 {
            f.z -= h.up_damping * vel.z;
        }
    } else if a.z <= 0.0 {
        f += a * rise_f;
    } else {
        let mut c = 1.0;
        if 1.0 - t <= 0.8 {
            c = 1.0 - t;
            if c > 0.3 {
                c = (1.75 * (1.0 - t)).clamp(0.0, 0.8);
            }
        }
        f.z += rise_f * c.max(0.6);
        if up_ok && rise_f > 0.0 && vel.z < -50.0 {
            f.z += (h.falling_mult - 1.0) * rise_f;
        }
        f += hdir * (t * rise_f);
    }
    let lat = r_flat.dot(a).abs();
    f.z += (lat + 1.0) * h.collective_comp * h.collective_roll * t;
    if a.z > 0.0 {
        f.x -= vel.x * (1.0 - t) * k_air;
        f.y -= vel.y * (1.0 - t) * k_air;
    }
    f += a * h.passive_lift;
    f.z += 0.01 * (1.0 - t) * h.passive_lift * 2.25;
    if rise == 0.0 && up_ok && vel.z <= -10.0 {
        f.z -= vel.z * k_air * 0.75;
    }
    let acc = f * (HELI_G / (1.0225 * h.passive_lift.max(1.0))) - Vec3::Z * HELI_G;
    let mut nv = (vel + acc * dt) * (1.0 - HELI_LIN_DAMP * dt).max(0.0);
    // speed cap (AirSpeed / sprint)
    let hs = Vec2::new(nv.x, nv.y);
    if hs.length() > cap {
        let c = hs.normalize() * cap;
        nv.x = c.x;
        nv.y = c.y;
    }
    // torques in "angle-rate" terms: x = roll (mouse right rolls right), y = pitch (mouse up
    // lifts the nose), z = yaw (D turns right)
    let pm = x.z;
    let rm = y.z;
    let inv = if a.z < 0.0 { (0.25 * t).max(0.2) } else { 1.0 };
    // roll / pitch limits (EXE 0x0071cd36..0x0071cf8d, CollectiveMultiplier < 0.7): the input
    // is cut while the body is past lerp(sin fAHMin*, sin fAHMax*, CollectiveMultiplier) AND the
    // input pushes further out; input back towards level always acts. Here a positive roll /
    // pitch command raises rm = Y.z / pm = X.z. Before, the roll test had the sign flipped: it
    // cut the input BACK to level and never the one further out, so the heli rolled to ~80 deg
    // and could not be brought back (user 2026-10-21 23:56, AH-64 on its side); the pitch input
    // had no limit at all.
    let limits = crate::vehload::HELI_COLLECTIVE_MULT < 0.7;
    let roll_raw = (h.roll_strafe * yaw_in).clamp(-h.roll_torque_max, h.roll_torque_max);
    let roll_cut = limits && ((rm >= h.roll_limit && roll_raw > 0.0) || (rm <= -h.roll_limit && roll_raw < 0.0));
    let roll_cmd = if roll_cut { 0.0 } else { roll_raw * (1.0 - (0.7 * rm).abs()) * inv };
    let pitch_raw = (h.pitch_torque * pitch_in).clamp(-h.pitch_torque_max, h.pitch_torque_max);
    let pitch_cut = limits && ((pm >= h.pitch_limit && pitch_raw > 0.0) || (pm <= -h.pitch_limit && pitch_raw < 0.0));
    let pitch_cmd = if pitch_cut { 0.0 } else { pitch_raw * inv };
    let yaw_cmd = h.turn_torque * steer * inv;
    let mut tq = Vec3::new(roll_cmd, pitch_cmd, yaw_cmd);
    // auto-level while tilted (EXE: lift direction up, 0.05 < t < 0.85; roll only without roll
    // input or past sin 0.4; x 2 while CollectiveMultiplier <= 0.3: the player's 0.12)
    let um = if crate::vehload::HELI_COLLECTIVE_MULT <= 0.3 { 2.0 } else { 1.0 };
    if a.z > 0.0 && t > 0.05 && t < 0.85 {
        if yaw_in.abs() < 1e-8 || rm.abs() > 0.4 {
            tq.x -= rm.signum() * (rm.abs() + 0.5) * h.roll_upright * um;
        }
        tq.y -= pm.signum() * (pm.abs() + 0.5) * h.pitch_upright * um;
    }
    // NOT from data: past the EXE's auto-level window (lift direction tilted beyond t 0.85, about
    // 40 deg of bank or pitch: overshoot past the limits, collisions; PhysX's own body damping /
    // contacts are not rebuilt here) the upright torques act on as inside it and the axis
    // without input levels at HELI_LEVEL, so the heli always comes back towards level
    let outside = t >= 0.85 || a.z <= 0.0;
    let level_roll = outside && yaw_in.abs() < 1e-8;
    let level_pitch = outside && pitch_in.abs() < 1e-8;
    if level_roll {
        tq.x -= rm.signum() * (rm.abs() + 0.5) * h.roll_upright * um;
    }
    if level_pitch {
        tq.y -= pm.signum() * (pm.abs() + 0.5) * h.pitch_upright * um;
    }
    // StayUpright springs (pitch / roll beyond the resist angle)
    let (pa, ps) = h.upright_pitch;
    if ps > 0.0 && v.pitch.abs() > pa {
        tq.y -= v.pitch.signum() * (v.pitch.abs() - pa) * ps;
    }
    let (ra, rs) = h.upright_roll;
    if rs > 0.0 && v.roll.abs() > ra {
        tq.x -= v.roll.signum() * (v.roll.abs() - ra) * rs;
    }
    // damping, each at most what stops the motion this frame
    let i = h.inertia.max(Vec3::splat(1.0));
    // extra pitch damping 100 outside -0.9 s < pm < 0.9 s, s = 0 while the pitch input is cut
    // (EXE 0x0071cf93..: always while cut)
    let s = if pitch_cut { 0.0 } else { 1.0 };
    let extra = if -0.9 * s < pm && pm < 0.9 * s { 0.0 } else { 100.0 };
    let damp = |kd: f32, w: f32, ii: f32| (kd * w).abs().min(ii * w.abs() / dt) * w.signum();
    tq.x -= damp(h.roll_damping, v.ang.x, i.x);
    tq.y -= damp(h.pitch_damping + extra, v.ang.y, i.y);
    tq.z -= damp(h.turn_damping, v.ang.z, i.z);
    let mut w = v.ang + Vec3::new(tq.x / i.x, tq.y / i.y, tq.z / i.z) * dt;
    w.x *= (1.0 - HELI_ANG_DAMP * dt).max(0.0);
    w.y *= (1.0 - HELI_ANG_DAMP * dt).max(0.0);
    w.z *= (1.0 - HELI_YAW_DAMP * dt).max(0.0);
    w.z = w.z.clamp(-h.max_yaw_rate, h.max_yaw_rate);
    if level_roll {
        w.x -= v.roll * HELI_LEVEL * dt;
    }
    if level_pitch {
        w.y -= v.pitch * HELI_LEVEL * dt;
    }
    // ground: the old sphere / floor handling with the new velocity
    let mut p = *pos + nv * dt;
    let fwd = Vec3::new(v.yaw.cos(), v.yaw.sin(), 0.0);
    let r = (k.half_width * 0.8).clamp(60.0, 120.0);
    for fx in [-1.0f32, 0.0, 1.0] {
        let c = p + fwd * (fx * (k.half_len * 0.5)) + Vec3::Z * (r - k.ride_height).max(0.0);
        let cb = Vec3::new(c.x, c.z, c.y) * 0.01;
        let (b0, b1) = &mut *buf;
        b1.clear();
        let corr = col.push_sphere(cb, r * 0.01, b0, b1);
        if corr.length() > 0.001 {
            let cu = Vec3::new(corr.x, corr.z, corr.y) * 100.0;
            p += cu;
            // lose the velocity into the surface
            let n = cu.normalize_or_zero();
            let into = nv.dot(n);
            if into < 0.0 {
                nv -= n * into;
            }
        }
    }
    let probe = Vec3::new(p.x, p.z, p.y) * 0.01 + Vec3::Y * 0.5;
    if let Some((gy, _)) = col.floor(probe, 0.5 + k.ride_height * 0.01 + 0.05, &mut buf.0) {
        let want = gy * 100.0 + k.ride_height;
        if p.z <= want + 1.0 {
            p.z = want;
            nv.z = nv.z.max(0.0);
            // skids on the ground: friction, the body settles level
            nv.x *= 0.1_f32.powf(dt);
            nv.y *= 0.1_f32.powf(dt);
            w.x *= 0.05_f32.powf(dt);
            w.y *= 0.05_f32.powf(dt);
            let kk = (dt * 4.0).min(1.0);
            v.pitch -= v.pitch * kk;
            v.roll -= v.roll * kk;
        }
    }
    // integrate the attitude: roll / pitch / yaw rates on the Euler angles of body_rot (NOT
    // from data: the small-angle mapping of body rates to the angles)
    v.roll += w.x * dt;
    v.pitch += w.y * dt;
    v.yaw += w.z * dt;
    v.pitch = v.pitch.clamp(-1.4, 1.4);
    v.roll = wrap(v.roll);
    v.vel = nv;
    v.ang = w;
    v.speed = nv.dot(fwd);
    v.vz = nv.z;
    *pos = p;
}


/// Hit boxes of the vehicles (UE world): the body's oriented box from the mesh extent
/// (half_len / half_width; helicopters: the fuselage part of the collision cylinder), the
/// ground (origin - ride height) up to the mesh's highest vertex. Rockets, explosions and
/// shots hit vehicles through it (combat.rs); before only a vehicle with the player inside took
/// damage (the player's hits were passed on). NOT from data: a box instead of the PhysX body.
#[derive(Clone, Copy, Debug)]
pub struct VehVol {
    pub e: Entity,
    pub pos: Vec3,
    pub rot: Quat,
    pub min: Vec3,
    pub max: Vec3,
    pub player_inside: bool,
}

#[derive(Resource, Default)]
pub struct VehicleVolumes(pub Vec<VehVol>);

impl VehicleVolumes {
    /// First vehicle box along the ray (UE units), distance.
    pub fn ray(&self, o: Vec3, d: Vec3, max: f32, skip_player: bool) -> Option<(Entity, f32)> {
        let mut best: Option<(Entity, f32)> = None;
        for v in &self.0 {
            if skip_player && v.player_inside {
                continue;
            }
            let inv = v.rot.inverse();
            let lo = inv * (o - v.pos);
            let ld = inv * d;
            let (mut t0, mut t1) = (0.0f32, max);
            let mut ok = true;
            for a in 0..3 {
                let (oa, da, mn, mx) = (lo[a], ld[a], v.min[a], v.max[a]);
                if da.abs() < 1e-6 {
                    if oa < mn || oa > mx {
                        ok = false;
                        break;
                    }
                } else {
                    let (mut ta, mut tb) = ((mn - oa) / da, (mx - oa) / da);
                    if ta > tb {
                        std::mem::swap(&mut ta, &mut tb);
                    }
                    t0 = t0.max(ta);
                    t1 = t1.min(tb);
                    if t0 > t1 {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && best.is_none_or(|b| t0 < b.1) {
                best = Some((v.e, t0));
            }
        }
        best
    }

    /// Vehicles whose box is within `r` of the point: (entity, distance to the box, player inside).
    pub fn near(&self, at: Vec3, r: f32) -> Vec<(Entity, f32, bool)> {
        self.0
            .iter()
            .filter_map(|v| {
                let l = v.rot.inverse() * (at - v.pos);
                let d = (l - l.clamp(v.min, v.max)).length();
                (d <= r).then_some((v.e, d, v.player_inside))
            })
            .collect()
    }
}

/// The boxes of this frame for the instant-hit traces (combat::trace_shot: bullets stop at
/// vehicles and do no damage to them, user 2026-10-21).
pub static SHOT_VOLUMES: std::sync::Mutex<Vec<VehVol>> = std::sync::Mutex::new(Vec::new());

/// First vehicle box a shot from outside meets (boxes around the shot's origin are left out:
/// the vehicle's own guns): distance and face normal (UE).
pub fn shot_block(o: Vec3, d: Vec3, max: f32) -> Option<(f32, Vec3)> {
    let vols = SHOT_VOLUMES.lock().ok()?;
    let mut best: Option<(f32, Vec3)> = None;
    for v in vols.iter() {
        let inv = v.rot.inverse();
        let lo = inv * (o - v.pos);
        if lo.cmpge(v.min - Vec3::splat(20.0)).all() && lo.cmple(v.max + Vec3::splat(20.0)).all() {
            continue;
        }
        let ld = inv * d;
        let (mut t0, mut t1, mut axis, mut sign) = (0.0f32, max, 0usize, 0.0f32);
        let mut ok = true;
        for a in 0..3 {
            let (oa, da, mn, mx) = (lo[a], ld[a], v.min[a], v.max[a]);
            if da.abs() < 1e-6 {
                if oa < mn || oa > mx {
                    ok = false;
                    break;
                }
                continue;
            }
            let (ta, tb) = ((mn - oa) / da, (mx - oa) / da);
            let (tn, tf, sg) = if ta < tb { (ta, tb, -1.0) } else { (tb, ta, 1.0) };
            if tn > t0 {
                t0 = tn;
                axis = a;
                sign = sg;
            }
            t1 = t1.min(tf);
            if t0 > t1 {
                ok = false;
                break;
            }
        }
        if ok && best.is_none_or(|b| t0 < b.0) {
            let mut n = Vec3::ZERO;
            n[axis] = sign;
            best = Some((t0, v.rot * n));
        }
    }
    best
}

/// Hits on vehicles without the player inside (the player's vehicle takes the player's hits).
#[derive(Resource, Default)]
pub struct VehicleDamage(pub Vec<(Entity, Hit)>);

pub(crate) fn update_volumes(assets: Option<Res<VehicleAssets>>, vehicles: Query<(Entity, &Vehicle)>, mut vols: ResMut<VehicleVolumes>) {
    vols.0.clear();
    let Some(assets) = assets else {
        if let Ok(mut g) = SHOT_VOLUMES.lock() {
            g.clear();
        }
        return;
    };
    for (e, v) in &vehicles {
        let Some(k) = assets.kinds.get(v.kind) else { continue };
        vols.0.push(VehVol { e, pos: v.pos, rot: body_rot(v), min: Vec3::new(-k.half_len, -k.half_width, -k.ride_height), max: Vec3::new(k.half_len, k.half_width, k.top.max(60.0)), player_inside: v.player_seat.is_some() });
    }
    if let Ok(mut g) = SHOT_VOLUMES.lock() {
        g.clear();
        g.extend(vols.0.iter().copied());
    }
}

/// Damage to vehicles without the player: the damage grid's value for the armour (as for the
/// player's vehicle in `drive`), destroyed at 0 health (explosion, wreck: destruction.rs).
#[allow(clippy::too_many_arguments)]
pub(crate) fn empty_vehicle_damage(
    mut commands: Commands,
    assets: Option<Res<VehicleAssets>>,
    data: Option<Res<CombatData>>,
    col: Res<CollisionWorld>,
    mut dmg: ResMut<VehicleDamage>,
    mut vehicles: Query<(Entity, &mut Vehicle)>,
    (mut fx, mut sfx, impacts): (ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>, Option<Res<crate::impacts::Impacts>>),
    mut buf: Local<Vec<u32>>,
) {
    let hits = std::mem::take(&mut dmg.0);
    let Some(assets) = assets else { return };
    for (e, h) in hits {
        let Ok((_, mut v)) = vehicles.get_mut(e) else { continue };
        if v.player_seat.is_some() || v.health <= 0.0 {
            continue;
        }
        let Some(k) = assets.kinds.get(v.kind) else { continue };
        let amount = match (&h.dmg_type, data.as_ref()) {
            (Some(t), Some(d)) => {
                let soldier = d.grid.damage(t, if h.explosive { "PMA_Soldier" } else { "PMA_Soldier_Chest" }, 0.0, 1.0, 1.0);
                let armour = d.grid.damage(t, &k.armor, 0.0, 1.0, 1.0);
                if soldier > 0.0 { armour * (h.amount / soldier).min(1.0) } else { armour }
            }
            _ => h.amount,
        };
        let at = hit_location(k, &v, h.from, h.explosive);
        let amount = armor_damage(k, &mut v, at, amount, h.explosive, &mut sfx);
        if std::env::var_os("HB_DMGDBG").is_some() {
            info!("empty vehicle {} hit {:?} {:.0} -> {amount:.0} ({}), health {:.0}", k.name, h.dmg_type, h.amount, k.armor, v.health - amount);
        }
        v.health -= amount;
        if v.health > 0.0 {
            continue;
        }
        let at = v.pos;
        if !crate::combat::explosion_fx_debris(impacts.as_deref(), &col, at, "GCDmgType_VehicleExplosion", None, &mut fx, &mut sfx, &mut buf) {
            fx.fire("KFX_Pyrotechnics.FX_Grenades.Frag_Metal", at, Quat::IDENTITY);
        }
        let vel = if v.vel != Vec3::ZERO { v.vel } else { body_rot(&v) * Vec3::X * v.speed + Vec3::Z * v.vz };
        commands.spawn(crate::destruction::WreckSpawn { kind: v.kind, pos: v.pos, yaw: v.yaw, pitch: v.pitch, roll: v.roll, vel, from: h.from, attacker: h.attacker, team: h.attacker_team });
        commands.entity(e).despawn();
        info!("vehicle {} destroyed (empty)", k.name);
    }
}

/// A helicopter without a pilot (the player got out in the air) falls: gravity (HELI_G, the
/// sim's gravity), no lift, until it hits the ground, where it is wrecked when it came down
/// faster than HELI_CRASH_SPEED. NOT from data: the original's unoccupied sim (UnoccupiedSimObj)
/// is native; the crash speed and the tumble rate are not.
const HELI_CRASH_SPEED: f32 = 600.0;

fn unpiloted_fall(time: Res<Time>, assets: Option<Res<VehicleAssets>>, col: Res<CollisionWorld>, mut vehicles: Query<(Entity, &mut Vehicle)>, mut dmg: ResMut<VehicleDamage>, mut buf: Local<Vec<u32>>) {
    let Some(assets) = assets else { return };
    let dt = time.delta_secs().min(0.05);
    for (e, mut v) in &mut vehicles {
        let Some(k) = assets.kinds.get(v.kind) else { continue };
        // a bot pilot flies it (botvehicles.rs)
        if k.motion != Motion::Heli || v.player_seat.is_some() || v.health <= 0.0 || crate::botvehicles::piloted(e) {
            continue;
        }
        let probe = Vec3::new(v.pos.x, v.pos.z, v.pos.y) * 0.01 + Vec3::Y * 0.5;
        let ground = col.floor(probe, 200.0, &mut buf).map(|(gy, _)| gy * 100.0 + k.ride_height);
        let Some(g) = ground else { continue };
        if v.pos.z <= g + 1.0 {
            // resting on the ground
            v.vel = Vec3::ZERO;
            v.vz = 0.0;
            continue;
        }
        let mut vel = v.vel;
        vel.z -= HELI_G * dt;
        vel.x *= (1.0 - 0.3 * dt).max(0.0);
        vel.y *= (1.0 - 0.3 * dt).max(0.0);
        // the body tips over as it falls (NOT from data)
        v.roll += v.ang.x.signum().max(0.5) * 0.4 * dt;
        v.pitch -= 0.25 * dt;
        let mut p = v.pos + vel * dt;
        if p.z <= g {
            p.z = g;
            let impact = -vel.z;
            vel = Vec3::ZERO;
            if impact > HELI_CRASH_SPEED {
                dmg.0.push((e, Hit { target: Target::Player, amount: 100_000.0, attacker: Actor::World, attacker_team: 2, headshot: false, explosive: true, from: p, icon: None, dmg_type: None }));
            }
        }
        v.pos = p;
        v.vel = vel;
        v.vz = vel.z;
    }
}

/// GCArmorManager.UpdateOverArmorVisibility (EXE 0x6666c0, chains of OuterToInnerMap outer ->
/// inner): a part is hidden when it is destroyed, or while its Outer part is still intact (the
/// inner layer, e.g. the dented OA_WS2_* / *_DEST geometry, appears when the outer panel goes).
pub(crate) fn part_hidden(d: &DamageCfg, bits: u32, i: usize) -> bool {
    if bits >> i & 1 == 1 {
        return true;
    }
    match d.parts.get(i).and_then(|p| p.outer) {
        Some(o) if o != i && o < 32 => bits >> o & 1 == 0,
        _ => false,
    }
}

/// NOT from data: how far (UE units) a hit may land from a part's vertex bounds and still
/// damage it (GCArmorManager.HandleBoneDamage is native and not decoded).
const ARMOR_REACH: f32 = 120.0;
const ARMOR_REACH_BLAST: f32 = 200.0;

/// Damage on a vehicle through its over-armour (GCArmorManager.HandleBoneDamage): the hit goes
/// to the nearest exposed part (bRadialDamageAffectsSingleBoneOnly true: blasts too hit one
/// part); its bone health takes the damage, the vehicle's health DamagePassThrough of it plus
/// what the part could not absorb (out_UnHandledDamage). At 0 the part is destroyed (bit set):
/// VehicleEvents + DestructionSound, then VehicleEventsOnHidden; its inner part shows.
/// `at`: hit location (UE world). Returns the damage for the vehicle's health.
/// NOT from data: part choice by distance to the part's vertex bounds, the overflow rule.
pub(crate) fn armor_damage(k: &VehicleKind, v: &mut Vehicle, at: Vec3, amount: f32, explosive: bool, sfx: &mut crate::sfx::SfxQueue) -> f32 {
    let d = &k.damage;
    let local = body_rot(v).inverse() * (at - v.pos);
    let reach = if explosive { ARMOR_REACH_BLAST } else { ARMOR_REACH };
    let best = (0..d.parts.len())
        .filter(|&i| !d.parts[i].ignore && !part_hidden(d, v.armor_bits, i) && v.armor.get(i).is_some_and(|h| *h > 0.0))
        .map(|i| (i, (local - local.clamp(d.parts[i].lo, d.parts[i].hi)).length()))
        .filter(|x| x.1 <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((i, _)) = best else { return amount };
    let p = &d.parts[i];
    let before = v.armor[i];
    v.armor[i] -= amount;
    let mut to_vehicle = amount * p.pass;
    if v.armor[i] <= 0.0 && i < 32 {
        to_vehicle += (amount - before).max(0.0) * (1.0 - p.pass);
        v.armor_bits |= 1 << i;
        v.dmg_events.extend(p.events.iter().cloned());
        v.dmg_events.extend(p.events_hidden.iter().cloned());
        sfx.play(&p.sound, Some(at));
        if std::env::var_os("HB_DMGDBG").is_some() {
            info!("vehicle {}: over-armour {} destroyed, events {:?} / {:?}", k.name, p.name, p.events, p.events_hidden);
        }
    }
    if std::env::var_os("HB_DMGDBG").is_some() {
        info!("vehicle {}: hit {:.0} on {} ({:.0} -> {:.0}), to health {:.0}", k.name, amount, p.name, before, v.armor[i], to_vehicle);
    }
    to_vehicle
}

/// Hit location for the over-armour: blasts carry their point (`from` = the explosion), other
/// hits the point of the vehicle box nearest the attacker (NOT from data: Hit has no location).
pub(crate) fn hit_location(k: &VehicleKind, v: &Vehicle, from: Vec3, explosive: bool) -> Vec3 {
    if explosive {
        return from;
    }
    let r = body_rot(v);
    let l = r.inverse() * (from - v.pos);
    let c = l.clamp(Vec3::new(-k.half_len, -k.half_width, -k.ride_height), Vec3::new(k.half_len, k.half_width, k.top.max(60.0)));
    v.pos + r * c
}

#[derive(Default)]
struct DmgState {
    smoke: bool,
    /// running references: (DamageCfg.refs index, particle id)
    running: Vec<(usize, Option<(usize, f64)>)>,
}

/// Damage stages of the live vehicles: GCVehicle.CheckVehicleDamage (health below
/// DamageSmokeThreshhold * Health -> VehicleEvent DamageSmoke, above -> NoDamageSmoke),
/// CheckDamageCriticalAudio (below DamageCriticalThreshold with the local player inside ->
/// DamageCriticalSound), the over-armour events queued by armor_damage. Events start the
/// socket References with that StartTag and stop those with that EndTag (VehicleDestroyed:
/// the vehicle's despawn); the effects follow their sockets.
fn damage_states(
    assets: Option<Res<VehicleAssets>>,
    mut vehicles: Query<(Entity, &mut Vehicle)>,
    (mut fx, mut sfx): (ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>),
    mut st: Local<HashMap<Entity, DmgState>>,
) {
    let Some(assets) = assets else {
        st.clear();
        return;
    };
    let live: std::collections::HashSet<Entity> = vehicles.iter().map(|(e, _)| e).collect();
    st.retain(|e, s| {
        if live.contains(e) {
            return true;
        }
        for (_, id) in s.running.drain(..) {
            if let Some(id) = id {
                fx.release(id);
            }
        }
        false
    });
    for (e, mut v) in &mut vehicles {
        let Some(k) = assets.kinds.get(v.kind) else { continue };
        let d = &k.damage;
        let s = st.entry(e).or_default();
        let pct = v.health / k.health.max(1.0);
        let smoke = pct < d.smoke_threshold;
        if smoke != s.smoke {
            s.smoke = smoke;
            v.dmg_events.push(if smoke { "damagesmoke".into() } else { "nodamagesmoke".into() });
        }
        if pct < d.critical_threshold && v.player_seat.is_some() {
            sfx.looping(&format!("vehcrit{e:?}"), &d.critical_sound, Some(v.pos));
        }
        let events = std::mem::take(&mut v.dmg_events);
        if events.is_empty() && s.running.is_empty() {
            continue;
        }
        let pose = k.pose(&v);
        for ev in events {
            let ev = ev.to_ascii_lowercase();
            s.running.retain(|(ri, id)| {
                if d.refs[*ri].end != ev {
                    return true;
                }
                if let Some(id) = id {
                    fx.release(*id);
                }
                false
            });
            for (ri, r) in d.refs.iter().enumerate().filter(|(_, r)| r.start == ev) {
                let (at, rot) = (k.socket_local(&pose, &r.socket).map(|(l, _)| v.pos + body_rot(&v) * l).unwrap_or(v.pos), body_rot(&v));
                if r.sound {
                    if r.end == "none" {
                        sfx.play(&r.path, Some(at));
                    } else {
                        s.running.push((ri, None));
                    }
                } else if let Some(id) = fx.fire_tracked(&r.path, at, rot) {
                    s.running.push((ri, Some(id)));
                } else if std::env::var_os("HB_DMGDBG").is_some() {
                    info!("vehicle {}: damage effect {} not pooled", k.name, r.path);
                }
            }
        }
        // the running effects follow their sockets; looping cues stay requested
        for (ri, id) in &s.running {
            let r = &d.refs[*ri];
            let (at, rot) = (k.socket_local(&pose, &r.socket).map(|(l, _)| v.pos + body_rot(&v) * l).unwrap_or(v.pos), body_rot(&v));
            match id {
                Some(id) => fx.track(*id, at, rot),
                None => sfx.looping(&format!("vehdmg{e:?}{ri}"), &r.path, Some(at)),
            }
        }
    }
}

/// HB_TEST_VEHHITS=<s>,<s>,...: scripted hits on the player's vehicle (else the first vehicle)
/// at those match seconds, one full GCDmgType_ATW_PROX rocket each (damage grid value of the
/// armour), going round the body: left, right, rear, front, top (tests of the damage stages).
fn test_vehicle_hits(
    time: Res<Time>,
    flow: Res<MatchFlow>,
    assets: Option<Res<VehicleAssets>>,
    vehicles: Query<(Entity, &Vehicle)>,
    (mut vhits, mut vdmg): (ResMut<crate::combat::VehicleHits>, ResMut<VehicleDamage>),
    mut st: Local<(f32, usize)>,
) {
    let Ok(list) = std::env::var("HB_TEST_VEHHITS") else { return };
    if flow.phase != Phase::Playing {
        *st = (0.0, 0);
        return;
    }
    let Some(assets) = assets else { return };
    st.0 += time.delta_secs().min(0.1);
    // with HB_VEHTEST the scripted drive's clock (starts after its real-time delay)
    if std::env::var_os("HB_VEHTEST").is_some() {
        st.0 = f32::from_bits(TEST_T.load(std::sync::atomic::Ordering::Relaxed));
    }
    let times: Vec<f32> = list.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    let Some(&t) = times.get(st.1) else { return };
    if st.0 < t {
        return;
    }
    let n = st.1;
    st.1 += 1;
    let Some((e, v)) = vehicles.iter().find(|(_, v)| v.player_seat.is_some()).or_else(|| vehicles.iter().next()) else { return };
    let k = &assets.kinds[v.kind];
    let side = [Vec3::new(0.0, -k.half_width, 40.0), Vec3::new(0.0, k.half_width, 40.0), Vec3::new(-k.half_len, 0.0, 40.0), Vec3::new(k.half_len, 0.0, 40.0), Vec3::new(-k.half_len * 0.3, 0.0, k.top)][n % 5];
    let at = v.pos + body_rot(v) * side;
    info!("test hit {n} on {} at {:?} (health {:.0})", k.name, side, v.health);
    let h = crate::combat::Hit { target: crate::combat::Target::Player, amount: 1.0e6, attacker: crate::combat::Actor::World, attacker_team: 2, headshot: false, explosive: true, from: at, icon: None, dmg_type: Some("GCDmgType_ATW_PROX".into()) };
    if v.player_seat.is_some() {
        // the player's vehicle takes the forwarded hits (World attacks are not forwarded)
        vhits.0.push(crate::combat::Hit { attacker: crate::combat::Actor::Bot(e), ..h });
    } else {
        vdmg.0.push((e, h));
    }
}
