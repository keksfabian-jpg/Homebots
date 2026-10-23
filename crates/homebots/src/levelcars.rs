//! The level's interactive ("jiggle") cars: GCInteractiveSkelMeshActor with a
//! GCInteractiveObjectInfo (VEH_Jiggle.cars.Prefabs.DSO_*_Blocking prefabs: DEST_CivilianCar01/02,
//! DEST_TruckCivilian03, DEST_utilityvan, DEST_Semi_Cab, Bulldozer_01, HumV/Buffalo_Destroyed;
//! 90 in FL-Crossroads, 17 in FL-AngelIsland). What the data and HOMEFRONT.exe define:
//!
//! - Per socket damage thresholds (GCInteractiveObjectInfo init, exe 0xd64cf0): for each mesh
//!   socket, the StartTime of its References with StartTag "Destroyed" is the DestroyThreshold,
//!   with StartTag "Cracked" the CrackThreshold (windows 110 / 150, head- and taillights - / 50).
//! - HandleBoneDamage(Damage, Bone) (exe 0xb1d750): every socket on the bone accumulates the
//!   damage; crossing DestroyThreshold plays the socket's "Destroyed" references, else crossing
//!   CrackThreshold its "Cracked" ones (PlaySocketEffectsTag, exe 0x9025f0: a reference with an
//!   object = particle system / sound at the socket; without one = the scalar material parameter
//!   EndTag set to EndTime, e.g. Window1Shatter = 1 on M_common_Glass_LF); "Destroyed" also hides
//!   the socket's bone (HideBone). Effects: KFX_VHC.FX_JiggleCar.WindowExplode_Side +
//!   A2_Destruction_C.debris.SC_Debris_Glass, HeadlightExplode / TaillightExplode + BulletGlass06.
//! - Point damage (exe 0x988290): HandleBoneDamage(Damage, hit bone); when no socket took it,
//!   PlayImpactEffectsEx(bone, HitLocation, Momentum) (GCDeferredPhysicsComponent).
//! - Radius damage (exe 0x6d7440): every bone within DamageRadius of HurtOrigin gets
//!   HandleBoneDamage(int(BaseDamage)) (no falloff), then PlayExplosionEffects (exe 0x9918a0): the
//!   root bone's sockets play "Explosion" (Car_GroundDust + A2_Destruction_S.Destrustion_Metal04),
//!   OnExplodeAnim "Explode" (root jolt, AS_TruckCivilian03), AddRadialImpulse on the physics bodies.
//! - Vehicle contact (exe 0x411060): faster than ExplodeSpeed 120 -> every socket destroyed +
//!   PlayExplosionEffects, faster than BlowOutSpeed 60 -> every socket destroyed.
//! - NOT in the data: the cars do not burn or explode, doors / hood / trunk do not come off
//!   (PA_DEST_*: RB_ConstraintSetup to Root, not breakable, swing / twist limited to 5 degrees,
//!   bNoCollision), no burnt material / mesh. That only exists in SP set pieces (GMS_ExplodingCars:
//!   InterpActor / KActor + Kismet + GCAftermathTrigger), not placed on the MP maps.
//!
//! Here: the cars are CPU-skinned (main.rs level loop; HB_STATICCARS=1 = reference-pose meshes),
//! collision = the reference-pose render triangles (main.rs, unchanged). Implemented from the
//! above: thresholds, accumulation, Destroyed / Cracked / Explosion effects, hidden bones, radius
//! damage per bone, OnExplodeAnim, OnShotAnim. NOT from data: the hit bone of a shot (nearest
//! bone of the current pose to the hit point -- the physics bodies' shapes are not decoded), the
//! car's damage = damage grid value of the damage type for PM_Default (the car bodies' physical
//! material not decoded), reached by a blast = bounding sphere within DamageRadius (UE3
//! VisibleCollidingActors, no line-of-sight trace here), OnShotAnim as PlayImpactEffectsEx (its
//! native body not decoded). Not implemented: the material parameter switch of "Cracked"
//! (no runtime scalar parameters in our material path), the radial impulse (doors jiggle 5
//! degrees), SoundNodeWave references (BulletGlass06, Destrustion_Metal04: the sound queue plays
//! cues; waves only 2D), the MinEffectsDisplayFactor 0.3 screen-size gate, vehicle contact.
//!
//! USER DECISION (2026-10-21, knowingly NOT original for MP): the placed MP cars also get the
//! campaign's exploding-car behaviour. Source = SP package GMS_ExplodingCars (prefabs
//! trucks.TruckCab1 / RV1-3 / NKTruck / NKGasTruck / buses.CityBus1: InterpActor "clean" vehicle,
//! hidden wreck KActor / ActorFactoryPhysicsAsset, Kismet, GCAftermathTrigger; dev tool
//! crates/upk/examples/kismetdump.rs). What is taken from there (marked "SP" below):
//! - health: SeqEvent_TakeDamage_1 (GCDamageType, DamageThreshold 1000, every prefab) -> explode;
//! - "Burndown": TruckCab1 SeqEvent_TakeDamage_138 -> SeqAct_Delay_27 (5 s) -> explode;
//! - explosion: GCActorFactoryGCEmitter_1 KFX_Pyrotechnics.FX_Explosions.Vehicle_Civilian2 at the
//!   vehicle, SeqAct_PlaySound A2_Destruction_C.SC_LightVehicleAnnihilation_SP,
//!   SeqAct_CauseDamageRadial (1000, radius 1024, bDamageFalloff, GCDmgType_Veh_HeliMissile,
//!   Momentum 1e6) -> combat.rs car_explosions (soldiers, player, drone, vehicles, other cars);
//! - burning wreck: GCActorFactoryGCEmitter_2 KFX_VHC.FX_LAV.Fire_WreckChassis attached to the
//!   wreck (RV / NK trucks: "Clean up Fire" not linked -> it keeps burning);
//! - wreck launch: ActorFactoryPhysicsAsset InitialVelocity (0, 0, 750) (RV / bus / NK trucks):
//!   here the still attached doors / hood / trunk fly up with it (the body stays: collision);
//! - the pieces: this mesh's PhysicsAsset (GCInteractiveObjectInfo PhysicsAsset, PA_DEST_*)
//!   RB_ConstraintSetup ConstraintBone1 (doors / hood / trunk joined to Root) with their child
//!   bones (windows);
//! - burnt: SP swaps the wreck's materials (SeqAct_SetMaterial "Burnt Material" *_Wreck / *_DEST
//!   MICs, TruckCab1 MaterialSetOverrideIndex 1); the jiggle cars have NO burnt material in the
//!   data (VEH_Jiggle has none; VEH_civilian M_carWreckedYellow_C belongs to another mesh's UVs)
//!   -> only a darkening (gamemat.rs objects[].z = 5, NOT from data).
//! NOT from data: stage thresholds (fire at half health, door off at 300 bone damage), the engine
//! fire template KFX_Pyrotechnics.FX_Fire.Fire_Car (exists in the data, no SP Kismet uses it; the
//! SP prefabs have no fire before the explosion) at the hood bone (else between the headlights),
//! its loop sound A2_Destruction_C.SC_FireLoop_Metal (MP vehicle wreck socket reference), the
//! piece physics (launch destruction.rs PIECE_*, rigid bodies piecephys.rs), the explosion light. Not
//! implemented: GCAftermathTrigger AM_VehicleExplosion, LensFlare LF_Radial_Heli_Crash,
//! RB_RadialImpulseActor (10000, radius 300) on other physics objects, the momentum on victims.
//!
//! Static level trucks (StaticTruck / TruckBed, update_trucks): the KPA cargo truck NK_truck
//! (StaticMeshActor on Angel Island) with the SP NKTruck prefab's data, see the notes above
//! TRUCK_EXPLOSION_FX.
use crate::viewmodel::SkinPart;
use bevy::prelude::*;
use bevy::render::{
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_asset::RenderAssets,
    renderer::RenderQueue,
    storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
    Render, RenderApp, RenderSet,
};
use std::sync::Mutex;
use upk::Package;

pub struct LevelCarsPlugin;

impl Plugin for LevelCarsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BurntUpload>().add_plugins(ExtractResourcePlugin::<BurntUpload>::default()).add_systems(Update, update_cars.after(crate::combat::CombatSet));
        if let Some(r) = app.get_sub_app_mut(RenderApp) {
            r.add_systems(Render, upload_burnt.in_set(RenderSet::PrepareResources));
        }
    }
}

/// SP SeqEvent_TakeDamage_1 DamageThreshold (GMS_ExplodingCars, every prefab): the car explodes
/// when the accumulated damage reaches it.
pub const CAR_HEALTH: f32 = 1000.0;
/// SP TruckCab1 SeqAct_Delay_27 "Burndown" (s): burning -> explosion.
pub const BURNDOWN: f32 = 5.0;
/// NOT from data: the engine catches fire at this share of CAR_HEALTH.
const FIRE_AT: f32 = 0.5;
/// NOT from data: a door / hood / trunk bone comes off at this accumulated bone damage.
const DETACH_AT: i32 = 300;
/// SP TruckCab1 GCActorFactoryGCEmitter_1 / SeqAct_PlaySound_0.
const EXPLOSION_FX: &str = "KFX_Pyrotechnics.FX_Explosions.Vehicle_Civilian2";
const EXPLOSION_SOUND: &str = "A2_Destruction_C.SC_LightVehicleAnnihilation_SP";
/// SP TruckCab1 GCActorFactoryGCEmitter_2 (the wreck's fire).
const WRECK_FIRE: &str = "KFX_VHC.FX_LAV.Fire_WreckChassis";
/// NOT from SP Kismet: the engine fire before the explosion (KFX_Pyrotechnics.FX_Fire.Fire_Car)
/// and its loop (A2_Destruction_C.SC_FireLoop_Metal, an MP vehicle wreck socket reference).
const ENGINE_FIRE: &str = "KFX_Pyrotechnics.FX_Fire.Fire_Car";
const FIRE_LOOP: &str = "A2_Destruction_C.SC_FireLoop_Metal";
/// SP SeqAct_CauseDamageRadial_1 (TruckCab1 / RV / bus / NK trucks).
pub const EXPLOSION_DAMAGE: f32 = 1000.0;
pub const EXPLOSION_RADIUS: f32 = 1024.0;
pub const EXPLOSION_DMG_TYPE: &str = "GCDmgType_Veh_HeliMissile";
/// SP ActorFactoryPhysicsAsset InitialVelocity z (the wreck KAsset; uu/s).
const WRECK_UP: f32 = 750.0;

// ---- static level trucks (KPA cargo truck VEH_military.trucks.Statics.NK_truck) ----
// Data: on Angel Island (the only MP map with them) the truck is a plain StaticMeshActor
// (14 placements: Barracks 2, Command 2, Communication 4, Hospital 5, Maintenance 1) of the
// StaticMesh NK_truck (cab + chassis, front = -Y, glass section M_NK_truck_glass_C), most with
// a separate StaticMeshActor bed on top: NK_truck_back_01 (4), NK_truck_back_02 (2),
// truckMissiles (7). No bones, sockets, damage thresholds or destroyed mesh in the MP data.
// SP GMS_ExplodingCars trucks.NKTruck is the same mesh as an exploding set piece (clean
// InterpActor NKTruck_9Arc0 with StaticMesh VEH_military.trucks.Statics.NK_truck); taken from it:
// health 1000 (SeqEvent_TakeDamage_1), explosion KFX_Pyrotechnics.FX_Explosions.Vehicle_Jeep at
// NKTruck_22Arc0, SC_LightVehicleAnnihilation_SP, CauseDamageRadial 1000 / 1024 / falloff /
// GCDmgType_Veh_HeliMissile (= the cars' EXPLOSION_*), wreck fire Fire_WreckChassis attached to
// the wreck KAsset (NKTruck_21Arc0) bone Main_Root + (0, 0, 12.75). The wreck itself
// (SP_NK_MilTrucks SKV_NK_MilTruck / PA_MilTruck_wreck, *_Wreck MICs, MI_CH_Z10_glass_DEST) is
// NOT in the shipped data (cooked into the missing SP maps) -> no mesh swap, the truck stays
// (its collision too) and is darkened like the cars.
/// SP NKTruck GCActorFactoryEmitter_1.
const TRUCK_EXPLOSION_FX: &str = "KFX_Pyrotechnics.FX_Explosions.Vehicle_Jeep";
/// SP NKTruck spawn points in the clean truck's frame (prefab locations rotated by the
/// InterpActor's yaw 10112): explosion NKTruck_22Arc0 (-128.9, 89.8, 86.0) -> (1.1, 157.1, 86.0);
/// wreck NKTruck_21Arc0 (-142.7, 97.9, 38.0) -> (0.0, 173.0, 38.0) + AttachToActor RelativeOffset
/// z 12.75 (assuming the wreck's Main_Root at its origin: the mesh is not in the data).
const TRUCK_EXPLOSION_AT: Vec3 = Vec3::new(1.1, 157.1, 86.0);
const TRUCK_WRECK_FIRE_AT: Vec3 = Vec3::new(0.0, 173.0, 50.75);
/// NOT from this truck's data: its glass shatters at this damage taken near the glass (the
/// jiggle cars' window socket DestroyThreshold, VEH_Jiggle DEST_* Window_*), with their effects.
const TRUCK_GLASS_AT: f32 = 150.0;
const GLASS_FX: &str = "KFX_VHC.FX_JiggleCar.WindowExplode_Side";
const GLASS_SOUND: &str = "A2_Destruction_C.debris.SC_Debris_Glass";
/// NOT from data: a bed placement belongs to the nearest truck within this distance (uu; the
/// placed beds sit 210-230 uu from their truck's origin).
const TRUCK_LINK_DIST: f32 = 450.0;

/// Which static meshes are destructible trucks / their beds (main.rs static placements).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StaticKind {
    Truck,
    Bed,
}

pub fn static_kind(mesh_path: &str) -> Option<StaticKind> {
    let p = mesh_path.to_ascii_lowercase();
    if p.ends_with("trucks.statics.nk_truck") {
        Some(StaticKind::Truck)
    } else if ["trucks.statics.nk_truck_back_01", "trucks.statics.nk_truck_back_02", "trucks.statics.truckmissiles"].iter().any(|s| p.ends_with(s)) {
        Some(StaticKind::Bed)
    } else {
        None
    }
}

/// Mesh-space (UE) shape of a static truck: bounds, glass bounds, the part handles.
pub struct StaticShape {
    lo: Vec3,
    hi: Vec3,
    glass: Option<(Vec3, Vec3)>,
    glass_meshes: Vec<Handle<Mesh>>,
    meshes: Vec<Handle<Mesh>>,
}

/// The shape from the built section meshes (Bevy-axis UE units: (x, z, y)); glass = sections
/// whose mesh material path contains "glass" (NK_truck: M_NK_truck_glass_C).
pub fn static_shape(p: &Package, meshes: &Assets<Mesh>, parts: &[(Handle<Mesh>, i32)]) -> StaticShape {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let (mut glo, mut ghi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut glass_meshes = Vec::new();
    for (h, mat) in parts {
        let is_glass = *mat != 0 && p.object_path(*mat).to_ascii_lowercase().contains("glass");
        if is_glass {
            glass_meshes.push(h.clone());
        }
        let Some(m) = meshes.get(h) else { continue };
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        // the section meshes carry all LOD vertices (main.rs build_game_mesh): the section's
        // own ones through its indices
        let Some(bevy::render::mesh::Indices::U32(idx)) = m.indices() else { continue };
        for &i in idx {
            let Some(q) = pos.get(i as usize) else { continue };
            let v = Vec3::new(q[0], q[2], q[1]);
            lo = lo.min(v);
            hi = hi.max(v);
            if is_glass {
                glo = glo.min(v);
                ghi = ghi.max(v);
            }
        }
    }
    if lo.x > hi.x {
        lo = Vec3::ZERO;
        hi = Vec3::ZERO;
    }
    StaticShape { lo, hi, glass: (glo.x <= ghi.x).then_some((glo, ghi)), glass_meshes, meshes: parts.iter().map(|x| x.0.clone()).collect() }
}

/// A destructible static truck (see the notes above the constants).
#[derive(Component)]
pub struct StaticTruck {
    pos: Vec3,
    rot: Quat,
    scale: Vec3,
    shape: StaticShape,
    /// engine fire point (mesh space; NOT from data)
    engine: Vec3,
    tag: u32,
    /// MeshTags of its beds (burnt with it)
    beds: Vec<u32>,
    linked: bool,
    damage: f32,
    glass_dmg: f32,
    glass_broken: bool,
    burning: Option<f32>,
    fire: Option<(usize, f64)>,
    exploded: bool,
    wreck_fire: Option<(usize, f64)>,
    serial: u32,
}

/// A truck's bed placement (NK_truck_back_01 / _02, truckMissiles).
#[derive(Component)]
pub struct TruckBed {
    pos: Vec3,
    tag: u32,
}

/// main.rs static placements: a truck or bed, `tag` = the MeshTag spawn_mesh gave it.
pub fn spawn_static(commands: &mut Commands, kind: StaticKind, shape: StaticShape, tag: u32, location: [f32; 3], rotation: [i32; 3], scale: [f32; 3]) {
    let pos = Vec3::from(location);
    if kind == StaticKind::Bed {
        commands.spawn((crate::LevelEntity, TruckBed { pos, tag }));
        return;
    }
    // engine fire (NOT from data): the cab end of the long horizontal axis (the end the glass
    // is on; else -Y as NK_truck), 18 % in, 45 % up
    let (lo, hi) = (shape.lo, shape.hi);
    let mid = (lo + hi) * 0.5;
    let along_y = hi.y - lo.y >= hi.x - lo.x;
    let gc = shape.glass.map(|(a, b)| (a + b) * 0.5);
    let front_neg = gc.map(|g| if along_y { g.y < mid.y } else { g.x < mid.x }).unwrap_or(true);
    let mut engine = Vec3::new(mid.x, mid.y, lo.z + 0.45 * (hi.z - lo.z));
    if along_y {
        engine.y = if front_neg { lo.y + 0.18 * (hi.y - lo.y) } else { hi.y - 0.18 * (hi.y - lo.y) };
    } else {
        engine.x = if front_neg { lo.x + 0.18 * (hi.x - lo.x) } else { hi.x - 0.18 * (hi.x - lo.x) };
    }
    static SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1 << 20);
    if std::env::var_os("HB_CARDBG").is_some() {
        info!("static truck at {pos:?}: bounds {lo:?}..{hi:?}, glass {:?}, engine {engine:?}", shape.glass);
    }
    commands.spawn((
        crate::LevelEntity,
        StaticTruck {
            pos,
            rot: ue_rotation_matrix(rotation),
            scale: Vec3::from(scale),
            shape,
            engine,
            tag,
            beds: Vec::new(),
            linked: false,
            damage: 0.0,
            glass_dmg: 0.0,
            glass_broken: false,
            burning: None,
            fire: None,
            exploded: false,
            wreck_fire: None,
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        },
    ));
}

impl StaticTruck {
    fn world(&self, v: Vec3) -> Vec3 {
        self.pos + self.rot * (self.scale * v)
    }

    /// Distance (uu) from a world point to the box `lo..hi` (mesh space).
    fn box_distance(&self, at: Vec3, lo: Vec3, hi: Vec3) -> f32 {
        let s = self.scale.abs().max(Vec3::splat(1e-3));
        let q = self.rot.conjugate() * (at - self.pos) / s;
        let d = q - q.clamp(lo, hi);
        (d * s).length()
    }

    fn sphere(&self) -> (Vec3, f32) {
        let (lo, hi) = (self.shape.lo, self.shape.hi);
        (self.world((lo + hi) * 0.5), ((hi - lo) * self.scale.abs()).length() * 0.5)
    }

    fn break_glass(&mut self, commands: &mut Commands, me: &crate::MeshEntities, tags: &Query<&bevy::render::mesh::MeshTag>, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue) {
        if self.glass_broken {
            return;
        }
        self.glass_broken = true;
        // the panes: this placement's glass section entities (the section meshes are shared
        // by all placements of the mesh: only those with this MeshTag)
        for e in crate::mesh_ents(me, &self.shape.glass_meshes) {
            if tags.get(e).is_ok_and(|t| t.0 == self.tag) {
                commands.entity(e).insert(Visibility::Hidden);
            }
        }
        if let Some((a, b)) = self.shape.glass {
            let at = self.world((a + b) * 0.5);
            fx.fire(GLASS_FX, at, self.rot);
            sfx.play(GLASS_SOUND, Some(at));
        }
    }
}

/// The static trucks' part of update_cars: shots and blasts, glass, engine fire, explosion,
/// burnt (the cars' stages with the SP NKTruck data, see the notes above the constants).
#[allow(clippy::too_many_arguments)]
fn update_trucks(
    trucks: &mut Query<&mut StaticTruck>,
    beds: &Query<&TruckBed>,
    tags: &Query<&bevy::render::mesh::MeshTag>,
    hits: &[(Vec3, Vec3, String)],
    blasts: &[(Vec3, f32, f32, bool)],
    data: Option<&crate::combat::CombatData>,
    dt: f32,
    commands: &mut Commands,
    me: &crate::MeshEntities,
    fx: &mut crate::particles::Particles,
    sfx: &mut crate::sfx::SfxQueue,
    dbg: bool,
    list: &mut Vec<(Vec3, f32)>,
    burnt: &mut Vec<u32>,
    gbuf: &mut Option<Handle<ShaderStorageBuffer>>,
    mat_of: &Query<&MeshMaterial3d<crate::gamemat::GameMat>>,
    game_mats: &Assets<crate::gamemat::GameMat>,
) {
    // beds -> the nearest truck (NOT from data: TRUCK_LINK_DIST)
    if trucks.iter().any(|t| !t.linked) {
        let pos: Vec<Vec3> = trucks.iter().map(|t| t.pos).collect();
        let mut own: Vec<Vec<u32>> = vec![Vec::new(); pos.len()];
        for b in beds.iter() {
            if let Some((k, d)) = pos.iter().enumerate().map(|(k, p)| (k, p.distance(b.pos))).min_by(|a, b| a.1.total_cmp(&b.1)) {
                if d <= TRUCK_LINK_DIST {
                    own[k].push(b.tag);
                }
            }
        }
        for (k, mut t) in trucks.iter_mut().enumerate() {
            if !t.linked {
                t.beds = std::mem::take(&mut own[k]);
                t.linked = true;
            }
        }
    }
    for mut t in trucks.iter_mut() {
        let t = &mut *t;
        list.push(t.sphere());
        let before = t.damage;
        let (lo, hi) = (t.shape.lo, t.shape.hi);
        // shots: on the truck's box (8 uu margin, NOT from data: the hit component is not
        // passed on), damage = grid value for PM_Default as the cars
        for (at, _dir, dtype) in hits {
            if t.box_distance(*at, lo, hi) > 8.0 {
                continue;
            }
            let dmg = data.map(|d| d.grid.damage(dtype, CAR_PHYS_MAT, 0.0, 1.0, 1.0)).unwrap_or(0.0).max(0.0);
            t.damage += dmg;
            if let Some((ga, gb)) = t.shape.glass {
                // NOT from data: 30 uu around the glass counts for the panes
                if t.box_distance(*at, ga, gb) <= 30.0 {
                    t.glass_dmg += dmg;
                }
            }
            if dbg {
                info!("static truck at {:?}: shot {dtype} ({dmg:.0})", t.pos);
            }
        }
        // radius damage: once per explosion (UE3 TakeRadiusDamage per actor), distance to the box
        for &(at, radius, base, falloff) in blasts {
            let d = t.box_distance(at, lo, hi);
            if d > radius {
                continue;
            }
            t.damage += if falloff { base * (1.0 - d / radius.max(1.0)).clamp(0.0, 1.0) } else { base };
            if let Some((ga, gb)) = t.shape.glass {
                if t.box_distance(at, ga, gb) <= radius {
                    t.glass_dmg += base;
                }
            }
            if dbg {
                info!("static truck at {:?}: blast {base:.0} radius {radius:.0} at {d:.0} uu", t.pos);
            }
        }
        if !t.exploded {
            if t.damage > before && dbg {
                info!("static truck at {:?}: damage {:.0} / {CAR_HEALTH}", t.pos, t.damage);
            }
            if t.glass_dmg >= TRUCK_GLASS_AT {
                t.break_glass(commands, me, tags, fx, sfx);
            }
            // engine fire at FIRE_AT (NOT from data), explosion at CAR_HEALTH (SP NKTruck
            // TakeDamage_1) or BURNDOWN after the fire (SP TruckCab1; NKTruck has no burndown)
            if t.burning.is_none() && t.damage >= FIRE_AT * CAR_HEALTH {
                t.burning = Some(0.0);
                let at = t.world(t.engine);
                t.fire = fx.fire_tracked(ENGINE_FIRE, at, Quat::IDENTITY);
                if dbg {
                    info!("static truck at {:?}: engine fire at {at:?}", t.pos);
                }
            }
            if let Some(b) = t.burning.as_mut() {
                *b += dt;
            }
            if t.damage >= CAR_HEALTH || t.burning.is_some_and(|b| b >= BURNDOWN) {
                t.exploded = true;
                if let Some(id) = t.fire.take() {
                    fx.release(id);
                }
                t.break_glass(commands, me, tags, fx, sfx);
                let at = t.world(TRUCK_EXPLOSION_AT);
                fx.fire(TRUCK_EXPLOSION_FX, at, t.rot);
                sfx.play(EXPLOSION_SOUND, Some(at));
                // SP CauseDamageRadial (combat.rs car_explosions), from the explosion point
                if let Ok(mut v) = EXPLOSIONS.lock() {
                    v.push(at);
                }
                if t.burning.is_none() {
                    t.burning = Some(0.0);
                }
                t.wreck_fire = fx.fire_tracked(WRECK_FIRE, t.world(TRUCK_WRECK_FIRE_AT), t.rot);
                if dbg {
                    info!("static truck at {:?}: EXPLODES (damage {:.0} / {CAR_HEALTH}), beds burnt {:?}", t.pos, t.damage, t.beds);
                }
            }
        }
        if let Some(id) = t.fire {
            fx.track(id, t.world(t.engine), Quat::IDENTITY);
        }
        if t.burning.is_some() {
            let at = if t.exploded { t.world(TRUCK_WRECK_FIRE_AT) } else { t.world(t.engine) };
            sfx.looping(&format!("carfire{}", t.serial), FIRE_LOOP, Some(at));
        }
        if t.exploded {
            burnt.push(t.tag);
            burnt.extend(t.beds.iter().copied());
            if gbuf.is_none() {
                *gbuf = crate::mesh_ents(me, &t.shape.meshes).find_map(|e| mat_of.get(e).ok()).and_then(|m| game_mats.get(&m.0)).map(|m| m.globals.clone());
            }
        }
    }
}

/// World hits of shots this frame (UE position, direction, damage type): impacts.rs Impacts::shot.
static WORLD_HITS: Mutex<Vec<(Vec3, Vec3, String)>> = Mutex::new(Vec::new());
/// Radius damage this frame (UE HurtOrigin, DamageRadius, BaseDamage, falloff for the car's
/// health): combat.rs explosions.
static BLASTS: Mutex<Vec<(Vec3, f32, f32, bool)>> = Mutex::new(Vec::new());
/// Car explosions this frame (UE centre) for combat.rs car_explosions (SP CauseDamageRadial).
static EXPLOSIONS: Mutex<Vec<Vec3>> = Mutex::new(Vec::new());

/// combat.rs: the cars that exploded since the last call (UE centre).
pub fn take_explosions() -> Vec<Vec3> {
    EXPLOSIONS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
}

/// Burnt cars for the render world: the GameMat globals buffer and (MeshTag, objects[].z).
#[derive(Resource, Default, Clone, ExtractResource)]
struct BurntUpload {
    buffer: Option<Handle<ShaderStorageBuffer>>,
    tags: Vec<u32>,
}

/// objects[tag].z = 5 (gamemat.rs burnt flag) in the GameMat globals buffer (Globals, then one
/// vec4 per MeshTag).
fn upload_burnt(up: Res<BurntUpload>, buffers: Res<RenderAssets<GpuShaderStorageBuffer>>, queue: Res<RenderQueue>) {
    let Some(b) = up.buffer.as_ref().and_then(|h| buffers.get(h)) else { return };
    if !b.buffer.usage().contains(bevy::render::render_resource::BufferUsages::COPY_DST) {
        return;
    }
    let base = std::mem::size_of::<crate::gamemat::Globals>() as u64;
    for &t in &up.tags {
        let off = base + t as u64 * 16 + 8;
        if off + 4 <= b.buffer.size() {
            queue.write_buffer(&b.buffer, off, bytemuck::cast_slice(&[5.0f32]));
        }
    }
}
/// The cars of the level (bounding-sphere centre and radius, UE), refreshed every frame
/// (HB_TEST_CARHITS).
static CARS: Mutex<Vec<(Vec3, f32)>> = Mutex::new(Vec::new());
/// HB_TEST_CARHITS state: own clock (s) and the next index.
static TEST: Mutex<(f32, usize)> = Mutex::new((0.0, 0));

/// impacts.rs: a shot with damage type `dmg` hit the level at `at`.
pub fn world_hit(at: Vec3, dir: Vec3, dmg: &str) {
    if let Ok(mut v) = WORLD_HITS.lock() {
        if v.len() < 256 {
            v.push((at, dir, dmg.to_string()));
        }
    }
}

/// combat.rs: an explosion (HurtRadius) at `at` with DamageRadius `radius`; `base` = the damage
/// type's grid value for the cars (PM_Default, see the module notes).
pub fn blast(at: Vec3, radius: f32, base: f32) {
    if let Ok(mut v) = BLASTS.lock() {
        if v.len() < 64 {
            v.push((at, radius, base, false));
        }
    }
}

/// combat.rs: an exploding car's CauseDamageRadial on the other cars (SP bDamageFalloff: the
/// car's health takes base * (1 - distance / radius); the bones as `blast`).
pub fn blast_falloff(at: Vec3, radius: f32, base: f32) {
    if let Ok(mut v) = BLASTS.lock() {
        if v.len() < 64 {
            v.push((at, radius, base, true));
        }
    }
}

/// Physical material column of the damage grid used for the cars (NOT from data: the car bodies'
/// PhysicalMaterial is not decoded; PM_Default is the grid's default column).
pub const CAR_PHYS_MAT: &str = "PM_Default";

/// HB_TEST_CARHITS=<s>,<s>,...: at those test-clock seconds (the HB_RECORD_PLAY clock when
/// recording, else seconds in the match) the point of a scripted rocket explosion at the level
/// car nearest to `player` (UE): on the car's bounding sphere towards the player, half way up
/// (test setting, NOT from data). combat.rs fires the explosion.
pub fn test_carhit(dt: f32, player: Vec3) -> Option<Vec3> {
    let list = std::env::var("HB_TEST_CARHITS").ok()?;
    let times: Vec<f32> = list.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    let mut st = TEST.lock().ok()?;
    st.0 = crate::record::play_clock().unwrap_or(st.0 + dt);
    let &t = times.get(st.1)?;
    if st.0 < t {
        return None;
    }
    st.1 += 1;
    let cars = CARS.lock().ok()?;
    // HB_TEST_CAR=<x>,<y> (UE): the car nearest that point instead of the one nearest the player
    let pick = std::env::var("HB_TEST_CAR").ok().and_then(|v| {
        let f: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (f.len() >= 2).then(|| Vec3::new(f[0], f[1], player.z))
    });
    let near = pick.unwrap_or(player);
    let flat_d = |c: Vec3| Vec2::new(c.x - near.x, c.y - near.y).length();
    let &(c, r) = cars.iter().min_by(|a, b| flat_d(a.0).total_cmp(&flat_d(b.0)))?;
    let flat = Vec3::new(player.x - c.x, player.y - c.y, 0.0).normalize_or_zero();
    let at = c + flat * r * 0.6;
    info!("test car hit {} at {:?} (car centre {:?}, {:.0} uu from the player)", st.1 - 1, at, c, at.distance(player));
    Some(at)
}

/// A placed interactive car found in a level package (for the main.rs level loop).
#[derive(Clone)]
pub struct CarActor {
    pub location: [f32; 3],
    pub rotation: [i32; 3],
    pub shot: String,
    pub explode: String,
    pub anim_set: Option<String>,
    /// GCInteractiveObjectInfo PhysicsAsset (PA_DEST_*): its joints name the detachable parts
    pub physics_asset: Option<String>,
}

/// GCInteractiveSkelMeshActor with an InteractiveInfo in the persistent level of `pkgs[pi]`.
pub fn actors(pkgs: &[Package], pi: usize) -> Vec<CarActor> {
    let p = &pkgs[pi];
    let mut out = Vec::new();
    for (i, e) in p.exports.iter().enumerate() {
        if p.export_class(e) != "GCInteractiveSkelMeshActor" || !p.object_path(i as i32 + 1).starts_with("TheWorld.PersistentLevel.") {
            continue;
        }
        let Some(ap) = p.export_props(i) else { continue };
        let Some(info) = ap.object("InteractiveInfo").filter(|&o| o != 0) else { continue };
        // the info: an export here, else the same path in another loaded package
        let ip = if info > 0 {
            p.export_props(info as usize - 1).map(|x| (x, p))
        } else {
            let path = p.object_path(info);
            pkgs.iter().find_map(|q| {
                let k = q.exports.iter().enumerate().position(|(k, e)| q.export_class(e) == "GCInteractiveObjectInfo" && q.object_path(k as i32 + 1).eq_ignore_ascii_case(&path))?;
                Some((q.export_props(k)?, q))
            })
        };
        let Some((ip, q)) = ip else { continue };
        out.push(CarActor {
            location: ap.vector("Location").unwrap_or([0.0; 3]),
            rotation: ap.rotator("Rotation").unwrap_or([0; 3]),
            shot: ip.enum_name("OnShotAnim").unwrap_or("").to_string(),
            explode: ip.enum_name("OnExplodeAnim").unwrap_or("").to_string(),
            anim_set: ip.object("AnimSet").filter(|&o| o != 0).map(|o| q.object_path(o)),
            physics_asset: ip.object("PhysicsAsset").filter(|&o| o != 0).map(|o| q.object_path(o)),
        });
    }
    out
}

/// One socket Reference (GC socket References struct): tags, times and the object.
#[derive(Clone)]
struct CarRef {
    start_tag: String,
    end_tag: String,
    start_time: f32,
    end_time: f32,
    /// (object path, class)
    reference: Option<(String, String)>,
}

/// All sockets of a SkeletalMesh export with every Reference (also those without an object,
/// which upk mesh_sockets leaves out): (name, bone, RelativeLocation, RelativeRotation, refs).
fn socket_refs(p: &Package, mesh: usize) -> Vec<(String, String, [f32; 3], [i32; 3], Vec<CarRef>)> {
    let mut out = Vec::new();
    let Some(pr) = p.export_props(mesh) else { return out };
    let Some(upk::props::Value::Raw { data, .. }) = pr.get("Sockets") else { return out };
    if data.len() < 4 {
        return out;
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    for k in 0..n {
        let Some(b) = data.get(4 + k * 4..8 + k * 4) else { break };
        let o = i32::from_le_bytes(b.try_into().unwrap());
        if o <= 0 {
            continue;
        }
        let Some(sp) = p.export_props(o as usize - 1) else { continue };
        let refs = p
            .struct_array(&sp, "References")
            .iter()
            .map(|r| CarRef {
                start_tag: r.enum_name("StartTag").unwrap_or("None").to_string(),
                end_tag: r.enum_name("EndTag").unwrap_or("None").to_string(),
                start_time: r.float("StartTime").unwrap_or(0.0),
                end_time: r.float("EndTime").unwrap_or(0.0),
                reference: r.object("Reference").filter(|&o| o != 0).map(|o| (p.object_path(o), if o < 0 { p.import_class(o) } else { p.export_class(&p.exports[o as usize - 1]) })),
            })
            .collect();
        out.push((sp.enum_name("SocketName").unwrap_or("").to_string(), sp.enum_name("BoneName").unwrap_or("").to_string(), sp.vector("RelativeLocation").unwrap_or([0.0; 3]), sp.rotator("RelativeRotation").unwrap_or([0; 3]), refs));
    }
    out
}

struct CarSocket {
    name: String,
    bone: usize,
    loc: Vec3,
    rot: Quat,
    refs: Vec<CarRef>,
    /// GCInteractiveObjectInfo DamageThreshold (DestroyThreshold, CrackThreshold); None = the
    /// socket has no threshold entry (no DamageReceived slot)
    thresholds: Option<(f32, f32)>,
    /// GCInteractiveSkelMeshActor DamageReceived
    damage: i32,
}

#[derive(Component)]
pub struct Car {
    part: SkinPart,
    /// UE origin and rotation
    pos: Vec3,
    rot: Quat,
    /// the SkeletalMesh's own RotOrigin / Origin (UE3: RotationTranslation(RotOrigin, Origin)
    /// before the component transform, like the reference-pose path in main.rs)
    mrot: Quat,
    morg: Vec3,
    ref_pose: Vec<(Quat, Vec3)>,
    anims: Option<upk::anim::AnimSetData>,
    track_of_bone: Vec<Option<usize>>,
    shot: Option<usize>,
    explode: Option<usize>,
    sockets: Vec<CarSocket>,
    scales: Vec<f32>,
    /// bounding sphere (UE, world) of the mesh
    center: Vec3,
    radius: f32,
    /// sequence playing and its time; the last pose is held
    playing: Option<(usize, f32)>,
    pose: Vec<(Quat, Vec3)>,
    dirty: bool,
    /// MeshTag of the car's mesh entities (gamemat objects[] block; burnt flag)
    tag: u32,
    /// accumulated damage (SP SeqEvent_TakeDamage, CAR_HEALTH)
    damage: f32,
    /// per bone accumulated damage (detaching, DETACH_AT)
    bone_dmg: Vec<i32>,
    /// detachable parts: (joint bone, the bone with its descendants), from the PhysicsAsset
    detachable: Vec<(usize, Vec<usize>)>,
    /// flying / fallen parts
    pieces: Vec<CarPiece>,
    /// this car's triangles in the CollisionWorld (reference pose incl. the doors that came off):
    /// the parts' contacts ignore them, else they land on the ghost of their own door / the
    /// body's collision and hang in the air (user screenshot 2026-10-21 21:46)
    col_tris: std::ops::Range<u32>,
    /// engine fire point (mesh / component space)
    engine: Vec3,
    /// burning since (s) with the tracked engine fire
    burning: Option<f32>,
    fire: Option<(usize, f64)>,
    exploded: bool,
    wreck_fire: Option<(usize, f64)>,
    serial: u32,
}

/// A detached part (door, hood, trunk with its windows): a rigid motion of its bones as they
/// were posed when it came off.
struct CarPiece {
    bones: Vec<usize>,
    /// component-space pose of those bones at the detach
    base: Vec<(Quat, Vec3)>,
    /// world (UE) centroid at the detach
    c0: Vec3,
    /// rigid body (piecephys.rs): centroid now, rotation since the detach, velocities
    body: crate::piecephys::Body,
    /// contact points / inertia (world axes at the detach, relative to c0)
    hull: crate::piecephys::Hull,
}

/// bots::skin_part_scaled maps mesh space with the vehicle / character convention
/// m -> (m.z, -m.x, -m.y) (its `comp`); as a rotation, to undo it for these meshes.
fn skin_comp() -> Quat {
    Quat::from_mat3(&bevy::math::Mat3::from_cols(Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 0.0)))
}

/// UE3 FRotationMatrix(Pitch, Yaw, Roll) (row vectors: v' = v.x X + v.y Y + v.z Z). Check:
/// the vehicles' RotOrigin (0, -16384, 16384) gives exactly skin_comp() (m.z, -m.x, -m.y).
/// Used for the actor rotation too (same as main.rs rotator_to_quat, in UE axes): the former
/// Z(yaw) * Y(-pitch) * X(roll) had the roll the wrong way round, tilted cars (roll up to
/// 54 degrees in FL-Crossroads) leaned to the wrong side.
fn ue_rotation_matrix(r: [i32; 3]) -> Quat {
    let a = |u: i32| u as f32 / 65536.0 * std::f32::consts::TAU;
    let (sp, cp) = a(r[0]).sin_cos();
    let (sy, cy) = a(r[1]).sin_cos();
    let (sr, cr) = a(r[2]).sin_cos();
    let x = Vec3::new(cp * cy, cp * sy, sp);
    let y = Vec3::new(sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp);
    let z = Vec3::new(-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp);
    Quat::from_mat3(&bevy::math::Mat3::from_cols(x, y, z)).normalize()
}

/// The car meshes' particle systems of the tags Destroyed / Cracked / Explosion (pooled by main.rs).
pub fn templates(pkgs: &[Package]) -> Vec<String> {
    let mut v = Vec::new();
    for p in pkgs {
        let infos: Vec<i32> = p.exports.iter().enumerate().filter(|(_, e)| p.export_class(e) == "GCInteractiveObjectInfo").filter_map(|(i, _)| p.export_props(i)?.object("SkeletalMesh").filter(|&o| o > 0)).collect();
        for m in infos {
            for s in p.mesh_sockets(m as usize - 1) {
                v.extend(s.effects.iter().filter(|e| e.class == "ParticleSystem" && ["Destroyed", "Cracked", "Explosion"].iter().any(|t| e.start_tag.eq_ignore_ascii_case(t))).map(|e| e.reference.clone()));
            }
        }
    }
    // the exploding-car stages (SP GMS_ExplodingCars + the engine fire, see the module notes);
    // the static trucks' explosion (SP NKTruck) and glass
    v.extend([EXPLOSION_FX, WRECK_FIRE, ENGINE_FIRE, TRUCK_EXPLOSION_FX, GLASS_FX].iter().map(|s| s.to_string()));
    v.sort();
    v.dedup();
    v
}

/// The PhysicsAsset's joints to Root (RB_ConstraintSetup ConstraintBone1, PA_DEST_*: doors,
/// hood, trunk) as mesh bone indices, each with its descendants.
fn detachable_parts(pkgs: &[Package], path: &str, bones: &[upk::skeletal::Bone]) -> Vec<(usize, Vec<usize>)> {
    let rest = path.split_once('.').map(|x| x.1).unwrap_or(path);
    let Some((p, i)) = pkgs.iter().find_map(|p| {
        let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "PhysicsAsset" && {
            let op = p.object_path(i as i32 + 1);
            op.eq_ignore_ascii_case(path) || op.eq_ignore_ascii_case(rest)
        })?;
        Some((p, i))
    }) else {
        return Vec::new();
    };
    let Some(pr) = p.export_props(i) else { return Vec::new() };
    let mut out = Vec::new();
    for o in pr.get("ConstraintSetup").map(|v| p.array_objects(v)).unwrap_or_default() {
        if o <= 0 {
            continue;
        }
        let Some(c) = p.export_props(o as usize - 1) else { continue };
        let (Some(b1), b2) = (c.enum_name("ConstraintBone1"), c.enum_name("ConstraintBone2").unwrap_or("")) else { continue };
        let Some(bi) = bones.iter().position(|b| b.name.eq_ignore_ascii_case(b1)) else { continue };
        // only parts hinged to the body (Root), as all PA_DEST_* joints are
        if bi == 0 || !bones.first().is_some_and(|r| r.name.eq_ignore_ascii_case(b2)) {
            continue;
        }
        let mut set = vec![bi];
        for k in 0..bones.len() {
            // descendants: walk up the parents to the joint bone or the root
            let mut j = k;
            let below = loop {
                if j == bi {
                    break true;
                }
                if j == 0 || j >= bones.len() {
                    break false;
                }
                j = bones[j].parent as usize;
            };
            if below && k != bi {
                set.push(k);
            }
        }
        out.push((bi, set));
    }
    out
}

/// main.rs level loop: spawn one car as a skinned mesh (component materials `mats` of package
/// `mat_pi`) instead of the reference-pose mesh.
#[allow(clippy::too_many_arguments)]
/// `col_tris`: the CollisionWorld triangles of this car (reference pose), ignored by its
/// detached parts.
pub fn spawn(sp: &mut crate::world::Spawner, a: &mut crate::world::Assets3, commands: &mut Commands, mp: usize, mi: usize, mat_pi: usize, mats: &[i32], car: &CarActor, col_tris: std::ops::Range<u32>) -> bool {
    let Some((parts, mesh)) = sp.skinned_mesh(a, mp, mi) else { return false };
    let l2w = crate::gameshader::ue_l2w([0.0; 3], [0; 3], [1.0; 3]);
    sp.cast_shadows = true;
    // spawn_mesh's first object block is the mesh entities' MeshTag (burnt flag)
    let tag = sp.objects.len() as u32;
    sp.spawn_mesh(commands, a, mp, parts.clone(), mat_pi, mats, Transform::IDENTITY, l2w, [0.0; 4], false, false);
    let pkgs = sp.pkgs;
    // sockets with their thresholds (exe 0xd64cf0: StartTime of the "Destroyed" / "Cracked"
    // references, truncated to int; a later reference of the same tag wins)
    let sockets: Vec<CarSocket> = socket_refs(&pkgs[mp], mi)
        .into_iter()
        .filter_map(|(name, bone, loc, rot, refs)| {
            let bone = mesh.bones.iter().position(|b| b.name.eq_ignore_ascii_case(&bone))?;
            let (mut destroy, mut crack, mut any) = (0.0f32, 0.0f32, false);
            for r in &refs {
                if r.start_tag.eq_ignore_ascii_case("Destroyed") {
                    destroy = r.start_time.trunc();
                    any = true;
                } else if r.start_tag.eq_ignore_ascii_case("Cracked") {
                    crack = r.start_time.trunc();
                    any = true;
                }
            }
            Some(CarSocket { name, bone, loc: Vec3::from(loc), rot: ue_rotation_matrix(rot), refs, thresholds: any.then_some((destroy, crack)), damage: 0 })
        })
        .collect();
    let anims = car.anim_set.as_ref().and_then(|path| {
        let rest = path.split_once('.').map(|x| x.1).unwrap_or(path);
        pkgs.iter().find_map(|p| {
            let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "AnimSet" && {
                let op = p.object_path(i as i32 + 1);
                op.eq_ignore_ascii_case(path) || op.eq_ignore_ascii_case(rest)
            })?;
            p.read_anim_set(i)
        })
    });
    let track_of_bone = mesh.bones.iter().map(|b| anims.as_ref().and_then(|s| s.bones.iter().position(|t| t.eq_ignore_ascii_case(&b.name)))).collect();
    let seq = |n: &str| anims.as_ref().and_then(|s| s.seqs.iter().position(|q| q.name.eq_ignore_ascii_case(n)));
    let (shot, explode) = (seq(&car.shot), seq(&car.explode));
    let rot = ue_rotation_matrix(car.rotation);
    let pos = Vec3::from(car.location);
    // the car meshes are authored with their own RotOrigin (0, 32768, 16384) / Origin: UE3
    // LocalToWorld = RotationTranslation(RotOrigin, Origin) * actor (offline check
    // crates/upk/examples/carorient.rs: the world z extent is the car height for all placed cars)
    let (mrot, morg) = (ue_rotation_matrix(mesh.rot_origin), Vec3::from(mesh.origin));
    let pts: Vec<Vec3> = mesh.lod.positions.iter().map(|p| rot * (mrot * Vec3::from(*p) + morg)).collect();
    let (lo, hi) = pts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let c = (lo + hi) * 0.5;
    let radius = pts.iter().map(|p| p.distance(c)).fold(0.0f32, f32::max);
    let ref_pose = crate::drone::ref_pose(&mesh.bones);
    let detachable = car.physics_asset.as_ref().map(|pa| detachable_parts(pkgs, pa, &mesh.bones)).unwrap_or_default();
    // engine fire point (NOT from data): between the hood bone and the headlights, whichever
    // the mesh has; else the middle of the mesh
    let bone_pos = |pred: &dyn Fn(&str) -> bool| -> Option<Vec3> {
        let v: Vec<Vec3> = mesh.bones.iter().enumerate().filter(|(_, b)| pred(&b.name.to_ascii_lowercase())).filter_map(|(i, _)| ref_pose.get(i).map(|x| x.1)).collect();
        (!v.is_empty()).then(|| v.iter().copied().sum::<Vec3>() / v.len() as f32)
    };
    let marks: Vec<Vec3> = [bone_pos(&|n| n == "hood"), bone_pos(&|n| n.starts_with("headlight"))].into_iter().flatten().collect();
    let engine = if marks.is_empty() {
        mesh.lod.positions.iter().map(|p| Vec3::from(*p)).sum::<Vec3>() / mesh.lod.positions.len().max(1) as f32
    } else {
        marks.iter().copied().sum::<Vec3>() / marks.len() as f32
    };
    let skeleton = mesh.bones.clone();
    let part = SkinPart::new(parts.into_iter().map(|x| x.0).collect(), mesh, &skeleton, &[]);
    let n = skeleton.len();
    if std::env::var_os("HB_CARDBG").is_some() {
        let th: Vec<String> = sockets.iter().filter_map(|s| s.thresholds.map(|(d, c)| format!("{} {}/{}", s.name, c, d))).collect();
        info!("car at {:?}: world extents {:?} (z = height), thresholds crack/destroy {}, detachable {:?}", pos, (hi - lo).round(), th.join(", "), detachable.iter().map(|d| skeleton[d.0].name.as_str()).collect::<Vec<_>>());
    }
    static SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    commands.spawn((
        crate::LevelEntity,
        Car {
            part,
            pos,
            rot,
            mrot,
            morg,
            pose: ref_pose.clone(),
            ref_pose,
            anims,
            track_of_bone,
            shot,
            explode,
            sockets,
            scales: vec![1.0; n],
            center: pos + c,
            radius,
            playing: None,
            dirty: true,
            tag,
            damage: 0.0,
            bone_dmg: vec![0; n],
            detachable,
            pieces: Vec::new(),
            col_tris,
            engine,
            burning: None,
            fire: None,
            exploded: false,
            wreck_fire: None,
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        },
    ));
    true
}

impl Car {
    fn play(&mut self, seq: Option<usize>) {
        if let Some(s) = seq {
            self.playing = Some((s, 0.0));
        }
    }

    /// World (UE) position of a component-space point.
    fn world(&self, v: Vec3) -> Vec3 {
        self.pos + self.rot * (self.mrot * v + self.morg)
    }

    fn bone_world(&self, b: usize) -> Vec3 {
        self.world(self.pose.get(b).map(|p| p.1).unwrap_or(Vec3::ZERO))
    }

    fn socket_world(&self, s: &CarSocket) -> (Vec3, Quat) {
        let (r, t) = self.pose.get(s.bone).copied().unwrap_or((Quat::IDENTITY, Vec3::ZERO));
        (self.world(r * s.loc + t), (self.rot * self.mrot * r * s.rot).normalize())
    }

    /// PlaySocketEffectsTag (exe 0x9025f0): the socket's references with StartTag `tag`;
    /// "Destroyed" hides the socket's bone.
    fn socket_effects(&mut self, k: usize, tag: &str, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue, dbg: bool) {
        let (w, q) = self.socket_world(&self.sockets[k]);
        for r in self.sockets[k].refs.iter().filter(|r| r.start_tag.eq_ignore_ascii_case(tag)) {
            match &r.reference {
                Some((path, class)) if class == "ParticleSystem" => {
                    fx.fire(path, w, q);
                }
                Some((path, class)) if class == "SoundCue" => sfx.play(path, Some(w)),
                Some((path, class)) => {
                    // NOT implemented: SoundNodeWave references (the queue plays 3D cues only)
                    if dbg {
                        info!("car socket {}: {tag} {class} {path} not played", self.sockets[k].name);
                    }
                }
                None => {
                    // NOT implemented: material scalar parameter EndTag = EndTime (no runtime
                    // material parameters in our material path)
                    if dbg {
                        info!("car socket {}: {tag} material parameter {} = {} (not applied)", self.sockets[k].name, r.end_tag, r.end_time);
                    }
                }
            }
        }
        if tag.eq_ignore_ascii_case("Destroyed") {
            let b = self.sockets[k].bone;
            if let Some(x) = self.scales.get_mut(b) {
                *x = 0.0;
            }
            self.dirty = true;
        }
    }

    /// HandleBoneDamage (exe 0xb1d750): true when a socket with a damage slot sits on the bone.
    fn bone_damage(&mut self, damage: i32, bone: usize, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue, dbg: bool) -> bool {
        let mut handled = false;
        for k in 0..self.sockets.len() {
            if self.sockets[k].bone != bone {
                continue;
            }
            let Some((destroy, crack)) = self.sockets[k].thresholds else { continue };
            handled = true;
            let cur = self.sockets[k].damage;
            let (c, n) = (cur as f32, (cur + damage) as f32);
            let tag = if destroy > c && n >= destroy {
                Some("Destroyed")
            } else if crack > c && n >= crack {
                Some("Cracked")
            } else {
                None
            };
            if dbg {
                info!("car at {:?}: socket {} damage {cur} + {damage} (crack {crack}, destroy {destroy}) -> {}", self.pos, self.sockets[k].name, tag.unwrap_or("-"));
            }
            if let Some(t) = tag {
                self.socket_effects(k, t, fx, sfx, dbg);
            }
            self.sockets[k].damage = cur.saturating_add(damage);
        }
        handled
    }

    /// PlayExplosionEffects (exe 0x9918a0): the root bone's sockets "Explosion", OnExplodeAnim.
    fn explosion_effects(&mut self, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue, dbg: bool) {
        for k in 0..self.sockets.len() {
            if self.sockets[k].bone == 0 {
                self.socket_effects(k, "Explosion", fx, sfx, dbg);
            }
        }
        let s = self.explode;
        self.play(s);
    }

    /// A detachable part comes off with velocity `vel` (UE units / s; NOT from data, see
    /// destruction.rs PIECE_*): centroid and lowest point of the vertices of its bones in the
    /// current pose.
    fn detach(&mut self, k: usize, vel: Vec3, seed: &mut u32) {
        if k >= self.detachable.len() {
            return;
        }
        let (_, bones) = self.detachable.remove(k);
        let mut verts: Vec<Vec3> = Vec::new();
        for (vi, p) in self.part.mesh.lod.positions.iter().enumerate() {
            let Some(&(bi, wt)) = self.part.mesh.influences.get(vi) else { continue };
            let kk = (0..4).max_by_key(|&k| wt[k]).unwrap_or(0);
            let pb = bi[kk] as usize;
            let Some(sb) = self.part.bone_map.get(pb).copied().flatten() else { continue };
            if !bones.contains(&sb) {
                continue;
            }
            let (Some(&(rq, rt)), Some(&(q, t))) = (self.part.ref_comp.get(pb), self.pose.get(sb)) else { continue };
            verts.push(self.world(q * (rq.conjugate() * (Vec3::from(*p) - rt)) + t));
        }
        if verts.is_empty() {
            // no geometry: just hide it
            for &b in &bones {
                if let Some(x) = self.scales.get_mut(b) {
                    *x = 0.0;
                }
            }
            self.dirty = true;
            return;
        }
        let c0 = verts.iter().copied().sum::<Vec3>() / verts.len() as f32;
        for v in verts.iter_mut() {
            *v -= c0;
        }
        // contact points along the car's axes (actor frame)
        let hull = crate::piecephys::Hull::from_vertices(&verts, self.rot);
        let axis = Vec3::new(crate::destruction::rand01(seed) - 0.5, crate::destruction::rand01(seed) - 0.5, crate::destruction::rand01(seed) - 0.5).normalize_or_zero();
        let base = bones.iter().map(|&b| self.pose.get(b).copied().unwrap_or((Quat::IDENTITY, Vec3::ZERO))).collect();
        self.pieces.push(CarPiece {
            bones,
            base,
            c0,
            body: crate::piecephys::Body::new(c0, Quat::IDENTITY, vel, axis * crate::destruction::PIECE_SPIN * (0.5 + crate::destruction::rand01(seed))),
            hull,
        });
        self.dirty = true;
    }

    /// Parts whose bones took DETACH_AT (NOT from data) come off, pushed away from the hit
    /// `at` (plus `push`, the shot direction; NOT from data: speeds, see destruction.rs PIECE_*).
    fn check_detach(&mut self, at: Vec3, push: Vec3, seed: &mut u32) {
        let mut k = 0;
        while k < self.detachable.len() {
            let hit = self.detachable[k].1.iter().map(|&b| self.bone_dmg.get(b).copied().unwrap_or(0)).max().unwrap_or(0);
            if hit < DETACH_AT {
                k += 1;
                continue;
            }
            let w = self.bone_world(self.detachable[k].0);
            let mut out = Vec3::new(w.x - at.x, w.y - at.y, 0.0).normalize_or_zero();
            if out == Vec3::ZERO {
                out = Vec3::new(w.x - self.center.x, w.y - self.center.y, 0.0).normalize_or_zero();
            }
            let r = crate::destruction::rand01(seed);
            let vel = out * crate::destruction::PIECE_OUT * (0.6 + 0.8 * r) + push.with_z(0.0).clamp_length_max(150.0) + Vec3::Z * crate::destruction::PIECE_UP * (0.4 + 0.3 * crate::destruction::rand01(seed));
            self.detach(k, vel, seed);
        }
    }

    /// Move the detached parts' bones by their rigid motions (in the component space that
    /// the skinning maps with the car's transform).
    fn apply_pieces(&mut self) {
        let rc = self.rot * self.mrot;
        let ic = rc.conjugate();
        let off = self.pos + self.rot * self.morg;
        for p in &self.pieces {
            for (i, &b) in p.bones.iter().enumerate() {
                let (q, t) = p.base[i];
                if let Some(x) = self.pose.get_mut(b) {
                    let (pc, pr) = (p.body.c, p.body.rot);
                    *x = ((ic * pr * rc * q).normalize(), ic * (pc + pr * (off + rc * t - p.c0) - off));
                }
            }
        }
    }

    /// The car explodes (SP: explosion emitter + sound + CauseDamageRadial, clean vehicle
    /// replaced by the burnt wreck with its fire, the wreck KAsset thrown up).
    fn blow_up(&mut self, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue, seed: &mut u32, dbg: bool) {
        self.exploded = true;
        if let Some(id) = self.fire.take() {
            fx.release(id);
        }
        fx.fire(EXPLOSION_FX, self.center, Quat::IDENTITY);
        sfx.play(EXPLOSION_SOUND, Some(self.center));
        if let Ok(mut v) = EXPLOSIONS.lock() {
            v.push(self.center);
        }
        // every pane / light goes (the clean vehicle is gone in SP)
        for k in 0..self.sockets.len() {
            if let Some((destroy, _)) = self.sockets[k].thresholds {
                if (self.sockets[k].damage as f32) < destroy && destroy > 0.0 {
                    self.socket_effects(k, "Destroyed", fx, sfx, dbg);
                    self.sockets[k].damage = destroy as i32;
                }
            }
        }
        self.explosion_effects(fx, sfx, dbg);
        // the remaining doors / hood / trunk fly with the wreck's InitialVelocity (SP z 750),
        // out from the centre (NOT from data: PIECE_OUT, random spread)
        while !self.detachable.is_empty() {
            let b = self.detachable[0].0;
            let w = self.bone_world(b);
            let out = Vec3::new(w.x - self.center.x, w.y - self.center.y, 0.0).normalize_or_zero();
            let r = crate::destruction::rand01(seed);
            let vel = out * crate::destruction::PIECE_OUT * (0.6 + 0.8 * r) + Vec3::Z * WRECK_UP * (0.7 + 0.3 * crate::destruction::rand01(seed));
            self.detach(0, vel, seed);
        }
        // the burning wreck (SP Fire_WreckChassis, attached to the wreck): at the car's middle,
        // on the ground below the centre (NOT from data: position)
        let at = Vec3::new(self.center.x, self.center.y, self.center.z - self.radius * 0.25);
        self.wreck_fire = fx.fire_tracked(WRECK_FIRE, at, Quat::IDENTITY);
        if dbg {
            info!("car at {:?}: EXPLODES (damage {:.0} / {CAR_HEALTH})", self.pos, self.damage);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_cars(
    mut commands: Commands,
    time: Res<Time>,
    mut cars: Query<&mut Car>,
    data: Option<Res<crate::combat::CombatData>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ents: Query<&mut Transform, (With<Mesh3d>, Without<Camera3d>)>,
    me: Res<crate::MeshEntities>,
    (mut fx, mut sfx): (ResMut<crate::particles::Particles>, ResMut<crate::sfx::SfxQueue>),
    (col, mat_of, game_mats, mut upload): (Res<crate::player::CollisionWorld>, Query<&MeshMaterial3d<crate::gamemat::GameMat>>, Res<Assets<crate::gamemat::GameMat>>, ResMut<BurntUpload>),
    (mut seed, mut buf): (Local<u32>, Local<Vec<u32>>),
    (mut trucks, beds, mesh_tags): (Query<&mut StaticTruck>, Query<&TruckBed>, Query<&bevy::render::mesh::MeshTag>),
) {
    let dt = time.delta_secs().min(0.05);
    let hits: Vec<(Vec3, Vec3, String)> = WORLD_HITS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
    let blasts: Vec<(Vec3, f32, f32, bool)> = BLASTS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
    let dbg = std::env::var_os("HB_CARDBG").is_some();
    let mut list = Vec::new();
    let mut burnt_tags = Vec::new();
    let mut gbuf: Option<Handle<ShaderStorageBuffer>> = None;
    for mut car in &mut cars {
        list.push((car.center, car.radius));
        let car = &mut *car;
        let before = car.damage;
        // shots on the car (its bounding sphere): the nearest bone takes the damage
        for (at, dir, dtype) in &hits {
            if at.distance(car.center) > car.radius {
                continue;
            }
            let dmg = data.as_ref().map(|d| d.grid.damage(dtype, CAR_PHYS_MAT, 0.0, 1.0, 1.0)).unwrap_or(0.0) as i32;
            let bone = (0..car.pose.len()).min_by(|&a, &b| car.bone_world(a).distance(*at).total_cmp(&car.bone_world(b).distance(*at))).unwrap_or(0);
            if dbg {
                info!("car at {:?}: shot {dtype} ({dmg}) on bone {}", car.pos, car.part.mesh.bones.get(bone).map(|b| b.name.as_str()).unwrap_or(""));
            }
            if !car.bone_damage(dmg, bone, &mut fx, &mut sfx, dbg) && car.playing.is_none_or(|(s, _)| Some(s) != car.explode) {
                // PlayImpactEffectsEx (native, not decoded): the OnShotAnim
                let s = car.shot;
                car.play(s);
            }
            // exploding-car stages: health (SP TakeDamage), the bone's part
            car.damage += dmg.max(0) as f32;
            if let Some(x) = car.bone_dmg.get_mut(bone) {
                *x = x.saturating_add(dmg.max(0));
            }
            car.check_detach(*at, *dir * 150.0, &mut seed);
        }
        // explosions: every bone within DamageRadius takes int(BaseDamage), then the effects
        for &(at, radius, base, falloff) in &blasts {
            if at.distance(car.center) - car.radius > radius {
                continue;
            }
            let bones: Vec<usize> = (0..car.pose.len()).filter(|&b| car.bone_world(b).distance(at) <= radius).collect();
            if dbg {
                info!("car at {:?}: blast {base:.0} radius {radius:.0} at {:.0} uu, {} bones in reach", car.pos, at.distance(car.center), bones.len());
            }
            for &b in &bones {
                car.bone_damage(base as i32, b, &mut fx, &mut sfx, dbg);
                if let Some(x) = car.bone_dmg.get_mut(b) {
                    *x = x.saturating_add(base as i32);
                }
            }
            car.explosion_effects(&mut fx, &mut sfx, dbg);
            // health: once per explosion (UE3 TakeRadiusDamage per actor); an exploding car's
            // CauseDamageRadial falls off with the distance (SP bDamageFalloff)
            let d = (at.distance(car.center) - car.radius * 0.5).max(0.0);
            car.damage += if falloff { base * (1.0 - d / radius.max(1.0)).clamp(0.0, 1.0) } else { base };
            car.check_detach(at, Vec3::ZERO, &mut seed);
        }
        // stages: fire at FIRE_AT (NOT from data), explosion at CAR_HEALTH (SP) or BURNDOWN
        // seconds after the fire started (SP TruckCab1 "Burndown")
        if !car.exploded {
            if car.damage > before && dbg {
                info!("car at {:?}: damage {:.0} / {CAR_HEALTH}", car.pos, car.damage);
            }
            if car.burning.is_none() && car.damage >= FIRE_AT * CAR_HEALTH {
                car.burning = Some(0.0);
                let at = car.world(car.engine);
                car.fire = fx.fire_tracked(ENGINE_FIRE, at, Quat::IDENTITY);
                if dbg {
                    info!("car at {:?}: engine fire at {:?}", car.pos, at);
                }
            }
            if let Some(t) = car.burning.as_mut() {
                *t += dt;
            }
            if car.damage >= CAR_HEALTH || car.burning.is_some_and(|t| t >= BURNDOWN) {
                car.blow_up(&mut fx, &mut sfx, &mut seed, dbg);
                // the pieces leave the spawn-time bounds
                for e in crate::mesh_ents(&me, &car.part.meshes) {
                    commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                }
            }
        }
        // the fires: engine (until the explosion), then the wreck's; one loop sound
        if let Some(id) = car.fire {
            fx.track(id, car.world(car.engine), Quat::IDENTITY);
        }
        if car.burning.is_some() {
            let at = if car.exploded { car.center } else { car.world(car.engine) };
            sfx.looping(&format!("carfire{}", car.serial), FIRE_LOOP, Some(at));
        }
        if car.exploded {
            burnt_tags.push(car.tag);
            if gbuf.is_none() {
                gbuf = crate::mesh_ents(&me, &car.part.meshes).find_map(|e| mat_of.get(e).ok()).and_then(|m| game_mats.get(&m.0)).map(|m| m.globals.clone());
            }
        }
        // flying parts: rigid bodies (piecephys.rs) against the world without this car's own
        // collision (col_tris)
        if car.pieces.iter().any(|p| !p.body.resting) {
            // the parts leave the spawn-time bounds (as the wrecks in destruction.rs)
            if car.pieces.iter().any(|p| p.body.c == p.c0) {
                for e in crate::mesh_ents(&me, &car.part.meshes) {
                    commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                }
            }
            let mut probe = crate::piecephys::ColProbe { col: &col, buf: &mut buf, skip: car.col_tris.clone() };
            for p in car.pieces.iter_mut().filter(|p| !p.body.resting) {
                crate::piecephys::step(&mut p.body, &p.hull, &mut probe, dt);
                // fell through / out of the world (NOT from data): stop
                if p.body.c.z < p.c0.z - 5000.0 {
                    p.body.resting = true;
                }
            }
            car.dirty = true;
        }
        // animation (sequence time, RateScale), the last frame held
        if let Some((s, t)) = car.playing {
            let car = &mut *car;
            if let Some(seq) = car.anims.as_ref().and_then(|a| a.seqs.get(s)) {
                let t2 = t + dt * seq.rate.max(1e-3);
                let local: Vec<(Quat, Vec3)> = car
                    .part
                    .mesh
                    .bones
                    .iter()
                    .enumerate()
                    .map(|(i, b)| match car.track_of_bone[i].filter(|&tr| seq.tracks.get(tr).is_some_and(|k| k.pos.len() > 1 || k.rot.len() > 1)).and_then(|tr| seq.sample(tr, t2.min(seq.length))) {
                        Some((p, r)) => (Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize().conjugate(), Vec3::from(p)),
                        None => (Quat::from_xyzw(b.rot[0], b.rot[1], b.rot[2], b.rot[3]).normalize(), Vec3::from(b.pos)),
                    })
                    .collect();
                car.pose = crate::drone::compose(&car.part.mesh.bones, &local);
                car.playing = if t2 >= seq.length { None } else { Some((s, t2)) };
                car.dirty = true;
            } else {
                car.playing = None;
            }
        }
        if car.dirty && !car.pieces.is_empty() {
            car.apply_pieces();
        }
        if car.dirty {
            let car = &mut *car;
            // skinned vertices are relative to the mesh origin in the world (sorg below)
            let o = car.pos + car.rot * car.morg;
            let tr = Vec3::new(o.x, o.z, o.y) * 0.01;
            let mut found = false;
            for e in crate::mesh_ents(&me, &car.part.meshes) {
                found = true;
                if let Ok(mut tf) = ents.get_mut(e) {
                    if tf.translation != tr {
                        tf.translation = tr;
                    }
                }
            }
            // the mesh entities are indexed a frame after their spawn: skin once they are
            if found {
                // rotation handed over so that skin_part_scaled's rot * comp = rot * RotOrigin
                let srot = car.rot * car.mrot * skin_comp().conjugate();
                let sorg = car.pos + car.rot * car.morg;
                crate::bots::skin_part_scaled(&car.part, &car.pose, srot, sorg, &mut meshes, Some(&car.scales));
                car.dirty = false;
            }
        }
    }
    // the static level trucks (NK_truck): same shots / blasts
    update_trucks(&mut trucks, &beds, &mesh_tags, &hits, &blasts, data.as_deref(), dt, &mut commands, &me, &mut fx, &mut sfx, dbg, &mut list, &mut burnt_tags, &mut gbuf, &mat_of, &game_mats);
    if let Ok(mut c) = CARS.lock() {
        *c = list;
    }
    // burnt cars (gamemat.rs objects[].z = 5, written every frame by upload_burnt)
    if upload.tags != burnt_tags || (gbuf.is_some() && upload.buffer != gbuf) {
        upload.tags = burnt_tags;
        if gbuf.is_some() {
            upload.buffer = gbuf;
        }
    }
}

#[cfg(test)]
mod rot_tests {
    use super::*;
    #[test]
    fn rot_origin_matches_vehicle_comp() {
        let q = ue_rotation_matrix([0, -16384, 16384]);
        let c = skin_comp();
        for v in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(1.0, 2.0, 3.0)] {
            assert!((q * v - c * v).length() < 1e-4, "{v} {} {}", q * v, c * v);
        }
        // car RotOrigin (0, 32768, 16384): mesh -y is up, as the DEST_* vertex bounds (y 0..-115)
        assert!((ue_rotation_matrix([0, 32768, 16384]) * Vec3::NEG_Y - Vec3::Z).length() < 1e-4);
    }

    #[test]
    fn actor_rotation_is_frotationmatrix() {
        // main.rs rotator_to_quat (FRotationMatrix in Bevy axes) and ue_rotation_matrix agree
        for r in [[-490, 9097, 9778], [-3047, -8771, -12824], [0, 10464, 0], [162, -29131, -848]] {
            let q = ue_rotation_matrix(r);
            let b = crate::rotator_to_quat(r);
            for v in [Vec3::X, Vec3::Y, Vec3::Z] {
                let ue = q * v;
                let bv = b * Vec3::new(v.x, v.z, v.y);
                assert!((Vec3::new(ue.x, ue.z, ue.y) - bv).length() < 1e-4, "{r:?} {v}");
            }
        }
    }
}
