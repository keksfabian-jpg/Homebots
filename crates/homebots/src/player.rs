//! The local player: spawn at a GameMode_PlayerStart, first-person camera, UE3-style walking
//! (PHYS_Walking / PHYS_Falling) against the level geometry.
//!
//! Values (UE units, converted to metres here):
//! - Default__GCChar_Player.CollisionCylinder: CollisionRadius 15, CollisionHeight 41.5;
//!   BaseEyeHeight 35.5, CrouchHeight 32.5, CrouchBaseEyeHeight 22.5
//! - Default__GCCharacter: GroundSpeed 260, AccelRate 450, JumpZ 375, SprintingPct 1.33,
//!   CrouchedPct 0.4, MaxStepHeight 26, CustomGravityScaling 1.15
//! - Engine Default__Pawn WalkableFloorZ 0.7, Default__WorldInfo DefaultGravityZ -750,
//!   PhysicsVolume GroundFriction 8
//! - GCInput_deu.ini GCPlayerInput DefaultMouseSensitivity 45 (rotation units per mouse count)
//! - field of view: trace t3 frames 1960/2100 (1366x768 in game): projection x scale 1.1401,
//!   y scale 2.0278 -> vertical FOV 52.5 deg
//! - team USA spawns on the "_High" PlayerStart groups (trace t3: first spawn camera at
//!   GameMode_PlayerStart_111, group VehicleSpawnsHigh)
use crate::menu::{MatchFlow, Phase};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use std::collections::HashMap;

const UU: f32 = 0.01;
const RADIUS: f32 = 15.0 * UU;
const HALF_HEIGHT: f32 = 41.5 * UU;
const EYE: f32 = 35.5 * UU;
const CROUCH_HALF_HEIGHT: f32 = 32.5 * UU;
const CROUCH_EYE: f32 = 22.5 * UU;
pub const GROUND_SPEED: f32 = 260.0 * UU;
const ACCEL_RATE: f32 = 450.0 * UU;
const JUMP_Z: f32 = 375.0 * UU;
const SPRINT_PCT: f32 = 1.33;
const CROUCH_PCT: f32 = 0.4;
const STEP: f32 = 26.0 * UU;
const WALKABLE_Z: f32 = 0.7;
const GRAVITY: f32 = 750.0 * 1.15 * UU;
const FRICTION: f32 = 8.0;
const MOUSE_SENS: f32 = 45.0;
pub const MOUSE_SENS_PUB: f32 = MOUSE_SENS;
pub const VFOV_DEG: f32 = 52.5;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CollisionWorld>().add_systems(Update, (spawn_player, player_control).chain().in_set(PlayerSet));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;

/// Static level geometry for movement queries (triangles in Bevy metres), bucketed in a 2 m
/// grid over X/Z. Filled by the level loader (static meshes and terrain).
#[derive(Resource, Default, Clone)]
pub struct CollisionWorld {
    pub tris: Vec<[Vec3; 3]>,
    /// per triangle: COL_PLAYER (BlockNonZeroExtent: pawn movement) / COL_TRACE (BlockZeroExtent:
    /// shots, line checks)
    mask: Vec<u8>,
    /// mask given to triangles added from now on (default both)
    pub add_mask: u8,
    /// per triangle: physical material id (impacts.rs, 1-based), 0 = none
    phys: Vec<u16>,
    /// physical material id given to triangles added from now on
    pub add_phys: u16,
    grid: HashMap<(i32, i32), Vec<u32>>,
    /// GameMode_PlayerStart actors of the loaded maps: (group, location, yaw radians)
    pub starts: Vec<(String, Vec3, f32)>,
    /// triangle index ranges of road surfaces (road spline meshes, highway road-set meshes;
    /// main.rs level load), ascending: pa.rs keeps its poles off the road
    roads: Vec<std::ops::Range<u32>>,
    /// per triangle: where it comes from (SRC_*, debug overlay coldebug.rs)
    src: Vec<u8>,
    /// source given to triangles added from now on
    pub add_src: u8,
    /// per triangle: index into `owners` (placement / volume name), 0 = none
    owner: Vec<u32>,
    /// owner index given to triangles added from now on (`new_owner`)
    pub add_owner: u32,
    /// names of the triangles' owners (index 0 = "")
    pub owners: Vec<String>,
    /// geometry shown by the debug overlay only (vehicle blocking volumes): triangle, SRC_*, owner
    pub extra: Vec<([Vec3; 3], u8, u32)>,
}

const CELL: f32 = 2.0;
/// triangle sources (debug overlay)
pub const SRC_OTHER: u8 = 0;
pub const SRC_TERRAIN: u8 = 1;
/// Homefront's collision mesh of a StaticMesh (from data)
pub const SRC_COLMESH: u8 = 2;
/// render triangles of a mesh with a BodySetup but no collision mesh (NOT from data)
pub const SRC_BODYSETUP: u8 = 3;
/// render triangles of a mesh whose LOD layout is not decoded (NOT from data)
pub const SRC_UNDECODED: u8 = 4;
/// render triangles by the mesh's own rule (per-poly collision, shots, UseSimpleBoxCollision=0)
pub const SRC_RENDER: u8 = 5;
pub const SRC_BLOCKVOL: u8 = 6;
/// skeletal meshes (map cars etc.)
pub const SRC_SKEL: u8 = 7;
/// GCVehicleBlockingVolume (vehicles only; overlay only, not in the collision)
pub const SRC_VEHVOL: u8 = 8;
pub const COL_PLAYER: u8 = 1;
pub const COL_TRACE: u8 = 2;

impl CollisionWorld {
    pub fn clear(&mut self) {
        self.tris.clear();
        self.mask.clear();
        self.phys.clear();
        self.grid.clear();
        self.starts.clear();
        self.roads.clear();
        self.src.clear();
        self.owner.clear();
        self.owners.clear();
        self.extra.clear();
        self.add_src = SRC_OTHER;
        self.add_owner = 0;
    }

    /// Registers an owner name for the triangles added from now on (debug overlay).
    pub fn new_owner(&mut self, name: String) {
        if self.owners.is_empty() {
            self.owners.push(String::new());
        }
        self.add_owner = self.owners.len() as u32;
        self.owners.push(name);
    }

    /// (mask, source, owner name) of a triangle (debug overlay).
    pub fn tri_info(&self, i: u32) -> (u8, u8, &str) {
        let i = i as usize;
        let o = self.owner.get(i).copied().unwrap_or(0) as usize;
        (self.mask.get(i).copied().unwrap_or(0), self.src.get(i).copied().unwrap_or(0), self.owners.get(o).map_or("", |s| s.as_str()))
    }

    /// Owner name by index (debug overlay, `extra`).
    pub fn owner_name(&self, o: u32) -> &str {
        self.owners.get(o as usize).map_or("", |s| s.as_str())
    }

    /// Indices of the triangles filed in the grid cells overlapping the box (x/z), deduplicated.
    pub fn tris_in(&self, mn: Vec3, mx: Vec3, out: &mut Vec<u32>) {
        self.candidates(mn, mx, out);
    }

    /// Marks the triangles added since index `from` as road surface.
    pub fn mark_road(&mut self, from: usize) {
        let r = from as u32..self.tris.len() as u32;
        if r.is_empty() {
            return;
        }
        match self.roads.last_mut() {
            Some(l) if l.end == r.start => l.end = r.end,
            _ => self.roads.push(r),
        }
    }

    /// Whether triangle `tri` belongs to a road surface (`mark_road`).
    pub fn is_road(&self, tri: u32) -> bool {
        let k = self.roads.partition_point(|r| r.end <= tri);
        self.roads.get(k).is_some_and(|r| r.contains(&tri))
    }

    pub fn add(&mut self, t: [Vec3; 3]) {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        if n.length_squared() < 1e-12 {
            return;
        }
        let i = self.tris.len() as u32;
        let (mn, mx) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
        let (x0, x1, z0, z1) = ((mn.x / CELL).floor() as i32, (mx.x / CELL).floor() as i32, (mn.z / CELL).floor() as i32, (mx.z / CELL).floor() as i32);
        if (x1 - x0 + 1) * (z1 - z0 + 1) > 4096 {
            return;
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.grid.entry((x, z)).or_default().push(i);
            }
        }
        self.tris.push(t);
        self.mask.push(if self.add_mask == 0 { COL_PLAYER | COL_TRACE } else { self.add_mask });
        self.phys.push(self.add_phys);
        self.src.push(self.add_src);
        self.owner.push(self.add_owner);
    }

    /// Physical material id of a triangle (0: none).
    /// Nuclear crater (crater.rs): terrain triangles within `reach` of `c` (Bevy m) move by
    /// `offset(r)`, every other triangle within `gone` is switched off. Returns how many went.
    pub fn crater(&mut self, c: Vec3, gone: f32, offset: impl Fn(f32) -> f32, reach: f32) -> usize {
        let mut n = 0;
        for i in 0..self.tris.len() {
            let t = self.tris[i];
            let m = (t[0] + t[1] + t[2]) / 3.0;
            let r = Vec2::new(m.x - c.x, m.z - c.z).length();
            if self.phys[i] == crate::impacts::TERRAIN {
                if r < reach + 10.0 {
                    for v in self.tris[i].iter_mut() {
                        v.y += offset(Vec2::new(v.x - c.x, v.z - c.z).length());
                    }
                }
            } else if r < gone && self.mask[i] != 0 {
                self.mask[i] = 0;
                n += 1;
            }
        }
        n
    }

    pub fn phys_of(&self, tri: u32) -> u16 {
        self.phys.get(tri as usize).copied().unwrap_or(0)
    }

    fn has(&self, i: u32, m: u8) -> bool {
        self.mask.get(i as usize).is_none_or(|x| x & m != 0)
    }

    fn candidates(&self, mn: Vec3, mx: Vec3, out: &mut Vec<u32>) {
        out.clear();
        for x in (mn.x / CELL).floor() as i32..=(mx.x / CELL).floor() as i32 {
            for z in (mn.z / CELL).floor() as i32..=(mx.z / CELL).floor() as i32 {
                if let Some(v) = self.grid.get(&(x, z)) {
                    out.extend_from_slice(v);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// Highest walkable-or-not surface below `p` within `depth`: (height, normal).
    pub fn floor(&self, p: Vec3, depth: f32, buf: &mut Vec<u32>) -> Option<(f32, Vec3)> {
        self.floor_skip(p, depth, buf, &(0..0))
    }

    /// `floor` ignoring the triangles with an index in `skip` (piecephys.rs: a car's own
    /// collision under its detached parts).
    pub fn floor_skip(&self, p: Vec3, depth: f32, buf: &mut Vec<u32>, skip: &std::ops::Range<u32>) -> Option<(f32, Vec3)> {
        self.candidates(p - Vec3::splat(0.01), p + Vec3::splat(0.01), buf);
        let mut best: Option<(f32, Vec3)> = None;
        for &i in buf.iter() {
            if !self.has(i, COL_PLAYER) || skip.contains(&i) {
                continue;
            }
            let t = &self.tris[i as usize];
            if let Some(y) = ray_down(p, t) {
                if y <= p.y + 1e-4 && y >= p.y - depth && best.is_none_or(|b| y > b.0) {
                    let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize();
                    if n.y < 0.0 {
                        n = -n;
                    }
                    best = Some((y, n));
                }
            }
        }
        best
    }

    /// All surfaces crossing the vertical line at (x, z): (height, |normal.y|), unsorted.
    pub fn column_hits(&self, x: f32, z: f32, out: &mut Vec<(f32, f32)>) {
        out.clear();
        let Some(v) = self.grid.get(&((x / CELL).floor() as i32, (z / CELL).floor() as i32)) else { return };
        let p = Vec3::new(x, 0.0, z);
        for &i in v {
            if !self.has(i, COL_PLAYER) {
                continue;
            }
            let t = &self.tris[i as usize];
            if let Some(y) = ray_down(p, t) {
                let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize();
                out.push((y, n.y.abs()));
            }
        }
    }

    /// Bounds of the triangles (Bevy metres).
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.tris.iter().flatten();
        let f = *it.next()?;
        Some(it.fold((f, f), |(a, b), v| (a.min(*v), b.max(*v))))
    }

    /// First hit of a segment (Bevy metres): (distance along `dir`, surface normal facing the ray).
    pub fn raycast(&self, o: Vec3, dir: Vec3, max: f32, buf: &mut Vec<u32>) -> Option<(f32, Vec3)> {
        self.raycast_mask(o, dir, max, buf, COL_TRACE)
    }

    /// `raycast` against the triangles with any bit of `m` (COL_PLAYER: movement checks).
    pub fn raycast_mask(&self, o: Vec3, dir: Vec3, max: f32, buf: &mut Vec<u32>, m: u8) -> Option<(f32, Vec3)> {
        self.raycast_hit(o, dir, max, buf, m).map(|(t, n, _)| (t, n))
    }

    /// `raycast_mask` with the triangle index hit.
    pub fn raycast_hit(&self, o: Vec3, dir: Vec3, max: f32, buf: &mut Vec<u32>, m: u8) -> Option<(f32, Vec3, u32)> {
        self.raycast_hit_skip(o, dir, max, buf, m, &(0..0))
    }

    /// `raycast_hit` ignoring the triangles with an index in `skip` (see `floor_skip`).
    pub fn raycast_hit_skip(&self, o: Vec3, dir: Vec3, max: f32, buf: &mut Vec<u32>, m: u8, skip: &std::ops::Range<u32>) -> Option<(f32, Vec3, u32)> {
        // Grid cells along the ray in order (2D DDA on x/z). Every triangle is filed in all
        // cells its bounds touch, so a hit at distance t lies in the cell the ray is in at t:
        // once the best hit is no farther than the current cell's exit, later cells cannot
        // give a nearer one. (Before: up to 4096 sampled cells deduplicated with a linear
        // search per sample - the bots' line-of-sight checks spent most of their time there.)
        buf.clear();
        let mut best: Option<(f32, Vec3, u32)> = None;
        let (mut cx, mut cz) = ((o.x / CELL).floor() as i32, (o.z / CELL).floor() as i32);
        let axis = |d: f32, p: f32, c: i32| -> (i32, f32, f32) {
            if d > 1e-12 {
                (1, ((c + 1) as f32 * CELL - p) / d, CELL / d)
            } else if d < -1e-12 {
                (-1, (c as f32 * CELL - p) / d, -CELL / d)
            } else {
                (0, f32::INFINITY, f32::INFINITY)
            }
        };
        let (sx, mut tx, dx) = axis(dir.x, o.x, cx);
        let (sz, mut tz, dz) = axis(dir.z, o.z, cz);
        for _ in 0..100_000 {
            if let Some(v) = self.grid.get(&(cx, cz)) {
                for &i in v {
                    if !self.has(i, m) || skip.contains(&i) {
                        continue;
                    }
                    let t = &self.tris[i as usize];
                    if let Some(d) = ray_tri(o, dir, t) {
                        if d >= 0.0 && d <= max && best.is_none_or(|b| d < b.0) {
                            let mut nrm = (t[1] - t[0]).cross(t[2] - t[0]).normalize();
                            if nrm.dot(dir) > 0.0 {
                                nrm = -nrm;
                            }
                            best = Some((d, nrm, i));
                        }
                    }
                }
            }
            let exit = tx.min(tz);
            if best.is_some_and(|b| b.0 <= exit) || exit > max {
                break;
            }
            if tx < tz {
                cx += sx;
                tx += dx;
            } else {
                cz += sz;
                tz += dz;
            }
        }
        best
    }

    /// Push a sphere out of the geometry; returns the total correction and the contact normals.
    pub fn push_sphere(&self, c: Vec3, r: f32, buf: &mut Vec<u32>, normals: &mut Vec<Vec3>) -> Vec3 {
        self.push_sphere_steep(c, r, f32::INFINITY, buf, normals)
    }

    /// `push_sphere` against the triangles whose face is steeper than `max_up` (|normal.y| <
    /// max_up): the vehicles' wall check, where the ground is the wheels' business. With all
    /// triangles a sphere dipping into a slope touched the internal edges of neighbouring
    /// ground triangles, whose closest-point normals lean sideways (HB_SLOPETEST: the HUMVEE
    /// stopped on 25 degree ramps).
    pub fn push_sphere_steep(&self, c: Vec3, r: f32, max_up: f32, buf: &mut Vec<u32>, normals: &mut Vec<Vec3>) -> Vec3 {
        self.candidates(c - Vec3::splat(r), c + Vec3::splat(r), buf);
        let mut p = c;
        for &i in buf.iter() {
            if !self.has(i, COL_PLAYER) {
                continue;
            }
            let t = &self.tris[i as usize];
            if max_up.is_finite() {
                let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                if n.y.abs() >= max_up {
                    continue;
                }
            }
            let q = closest_on_tri(p, t);
            let d = p - q;
            let l2 = d.length_squared();
            if l2 < r * r && l2 > 1e-10 {
                let l = l2.sqrt();
                let n = d / l;
                p += n * (r - l);
                normals.push(n);
            }
        }
        p - c
    }
}

/// Moller-Trumbore (both faces).
fn ray_tri(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f32> {
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let pv = d.cross(e2);
    let det = e1.dot(pv);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let tv = o - t[0];
    let u = tv.dot(pv) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = tv.cross(e1);
    let v = d.dot(qv) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(e2.dot(qv) * inv)
}

fn ray_down(p: Vec3, t: &[Vec3; 3]) -> Option<f32> {
    // barycentric in the XZ plane
    let (a, b, c) = (t[0], t[1], t[2]);
    let v0 = Vec2::new(b.x - a.x, b.z - a.z);
    let v1 = Vec2::new(c.x - a.x, c.z - a.z);
    let v2 = Vec2::new(p.x - a.x, p.z - a.z);
    let den = v0.x * v1.y - v1.x * v0.y;
    if den.abs() < 1e-12 {
        return None;
    }
    let v = (v2.x * v1.y - v1.x * v2.y) / den;
    let w = (v0.x * v2.y - v2.x * v0.y) / den;
    let u = 1.0 - v - w;
    if u < 0.0 || v < 0.0 || w < 0.0 {
        return None;
    }
    Some(u * a.y + v * b.y + w * c.y)
}

/// Ericson, Real-Time Collision Detection 5.1.5.
fn closest_on_tri(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The local player's pawn state.
#[derive(Component, Debug)]
pub struct Pawn {
    /// feet position (bottom of the collision cylinder), Bevy metres
    pub feet: Vec3,
    pub vel: Vec3,
    /// view rotation in UE rotation units (65536 = 360 degrees)
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub crouched: bool,
    pub sprinting: bool,
    /// eye height above the feet (blends between standing and crouched)
    pub eye: f32,
    /// weapon movement modifiers (combat.rs): MovementSpeedModifier, ADSMovementPercent
    pub speed_scale: f32,
    /// vertical field of view (degrees), lowered while aiming
    pub fov: f32,
    /// no sprinting (aiming, reloading)
    pub no_sprint: bool,
    /// dead: no input, camera stays
    pub dead: bool,
    /// in a vehicle (vehicles.rs drives the camera and the position)
    pub in_vehicle: bool,
    /// flying a remote UAV (airstrike.rs): no walking, no infantry weapon
    pub remote: bool,
}

impl Pawn {
    pub fn new(feet: Vec3, yaw_units: f32) -> Pawn {
        Pawn { feet, vel: Vec3::ZERO, yaw: yaw_units, pitch: 0.0, on_ground: false, crouched: false, sprinting: false, eye: EYE + HALF_HEIGHT, speed_scale: 1.0, fov: crate::settings::vfov(), no_sprint: false, dead: false, in_vehicle: false, remote: false }
    }

    /// Eye position in UE units.
    pub fn eye_ue(&self) -> Vec3 {
        let e = self.feet + Vec3::Y * self.eye;
        Vec3::new(e.x, e.z, e.y) / UU
    }

    /// View direction in UE axes.
    pub fn dir_ue(&self) -> Vec3 {
        let (y, p) = (ue_rot(self.yaw), ue_rot(self.pitch));
        Vec3::new(y.cos() * p.cos(), y.sin() * p.cos(), p.sin())
    }
}

/// Cylinder walking shared by the player and the bots (UnPhysic.cpp CalcVelocity, PHYS_Walking /
/// PHYS_Falling against the collision triangles). `accel_dir` is a unit horizontal direction
/// (or zero); returns true when a jump started.
#[allow(clippy::too_many_arguments)]
pub fn walk(col: &CollisionWorld, feet: &mut Vec3, vel: &mut Vec3, on_ground: &mut bool, crouched: bool, accel_dir: Vec3, max_speed: f32, jump: bool, dt: f32, buf: &mut (Vec<u32>, Vec<Vec3>)) -> bool {
    let mut jumped = false;
    // substeps keep each move below the collision radius
    let steps = ((dt * 4.0 / RADIUS).ceil() as usize).clamp(1, 16);
    let h = dt / steps as f32;
    let (bufv, normals) = buf;
    for _ in 0..steps {
        let mut v = *vel;
        let mut horiz = Vec3::new(v.x, 0.0, v.z);
        if *on_ground {
            if accel_dir != Vec3::ZERO {
                let size = horiz.length();
                horiz -= (horiz - accel_dir * size) * (h * FRICTION).min(1.0);
                horiz += accel_dir * ACCEL_RATE * h;
                if horiz.length() > max_speed {
                    horiz = horiz.normalize() * max_speed;
                }
            } else {
                let dec = horiz * (2.0 * h * FRICTION).min(1.0);
                horiz -= dec;
                if horiz.length() < 0.01 {
                    horiz = Vec3::ZERO;
                }
            }
            v = Vec3::new(horiz.x, 0.0, horiz.z);
            if jump && !jumped && !crouched {
                v.y = JUMP_Z;
                *on_ground = false;
                jumped = true;
            }
        } else {
            v.y -= GRAVITY * h;
            // air control (UE3 AirControl 0.05 of the acceleration)
            v += accel_dir * ACCEL_RATE * 0.05 * h;
        }
        let half = if crouched { CROUCH_HALF_HEIGHT } else { HALF_HEIGHT };
        let mut f = *feet + v * h;
        // body: spheres from step height up to the top of the cylinder
        let body_bottom = STEP + RADIUS;
        let top = 2.0 * half - RADIUS;
        let mut k = body_bottom;
        let mut hit_ceiling = false;
        while k <= top + 1e-4 {
            normals.clear();
            let c = f + Vec3::Y * k;
            let corr = col.push_sphere(c, RADIUS, bufv, normals);
            // horizontal push only for the body (the floor is handled below)
            let corr_h = Vec3::new(corr.x, corr.y.min(0.0), corr.z);
            f += corr_h;
            for n in normals.iter() {
                let n_h = Vec3::new(n.x, 0.0, n.z);
                if n_h.length_squared() > 1e-6 {
                    let nn = n_h.normalize();
                    let into = v.dot(nn);
                    if into < 0.0 {
                        v -= nn * into;
                    }
                }
                if n.y < -0.5 {
                    hit_ceiling = true;
                }
            }
            if top - body_bottom < 1e-4 {
                break;
            }
            k += ((top - body_bottom) / 2.0).max(RADIUS);
        }
        if hit_ceiling && v.y > 0.0 {
            v.y = 0.0;
        }
        // floor: highest surface under the feet within the step height (up) / snap distance (down)
        let probe = f + Vec3::Y * STEP;
        let snap = if *on_ground && v.y <= 0.0 { STEP + 0.05 } else { (-v.y * h).max(0.0) + 0.01 };
        match col.floor(probe, STEP + snap, bufv) {
            Some((y, n)) if n.y >= WALKABLE_Z && v.y <= 0.0 => {
                f.y = y;
                v.y = 0.0;
                *on_ground = true;
            }
            Some((y, n)) if f.y < y => {
                // steep: stand on it but slide off
                f.y = y;
                let nh = Vec3::new(n.x, 0.0, n.z);
                v += nh * GRAVITY * h;
                v.y = v.y.max(0.0);
                *on_ground = false;
            }
            _ => {
                *on_ground = false;
            }
        }
        *feet = f;
        *vel = v;
    }
    jumped
}

/// Movement constants for the bots.
pub const BOT_GROUND_SPEED: f32 = GROUND_SPEED;
pub const BOT_SPRINT_PCT: f32 = SPRINT_PCT;
pub const PAWN_HALF_HEIGHT: f32 = HALF_HEIGHT;
pub const PAWN_RADIUS: f32 = RADIUS;
pub const PAWN_EYE: f32 = EYE;

pub(crate) fn ue_rot(units: f32) -> f32 {
    units / 65536.0 * std::f32::consts::TAU
}

fn spawn_player(
    mut commands: Commands,
    mut flow: ResMut<MatchFlow>,
    col: Res<CollisionWorld>,
    mut pawns: Query<(Entity, &mut Pawn)>,
    (level, obj): (Option<Res<crate::objectives::LevelData>>, Res<crate::objectives::Objectives>),
    vassets: Option<Res<crate::vehicles::VehicleAssets>>,
) {
    if flow.phase != Phase::Playing {
        // left the match: remove the pawn
        if flow.phase == Phase::Front {
            for (e, _) in &pawns {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    if !flow.spawn_requested {
        return;
    }
    flow.spawn_requested = false;
    // team USA: Infantry_High starts, KVA the _Low ones (fallback when the front line has no
    // SpawnPointCollection); the one nearest the team's vehicle spawns (base side)
    let usa = crate::combat::player_team() == crate::combat::TEAM_USA;
    let veh_group = if usa { "VehicleSpawnsHigh" } else { "VehicleSpawnsLow" };
    let group = if flow.vehicle.is_some() || std::env::var_os("HOMEBOTS_TEST_VEHICLE").is_some() { veh_group } else if usa { "Infantry_High" } else { "Infantry_Low" };
    let base: Vec<Vec3> = col.starts.iter().filter(|s| s.0 == veh_group).map(|s| s.1).collect();
    let centre = if base.is_empty() { Vec3::ZERO } else { base.iter().copied().sum::<Vec3>() / base.len() as f32 };
    // helicopters: the vehicle starts with an AirVehicle camera transition (main.rs "#air")
    let heli = flow.vehicle.as_ref().or(std::env::var("HOMEBOTS_TEST_VEHICLE").ok().as_ref()).and_then(|c| vassets.as_ref().and_then(|a| a.kind_of(c).map(|k| a.kinds[k].motion == crate::vehicles::Motion::Heli))).unwrap_or(false);
    let air_group = format!("{veh_group}#air");
    let start_group = if heli { air_group.as_str() } else { group };
    let mut cands: Vec<&(String, Vec3, f32)> = col.starts.iter().filter(|s| s.0 == start_group).collect();
    cands.sort_by(|a, b| a.1.distance(centre).partial_cmp(&b.1.distance(centre)).unwrap());
    cands.truncate(8);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) as usize;
    // the active front line's SpawnPointCollection (objectives::Frontline): infantry, or the
    // jeep points with a HUMVEE; after a round the spawns move with the front
    // the vehicle's spawn type (vehicles.rs spawn_type: GMST_Jeep / GMST_TreadedVehicle /
    // GMST_AmphibiousVehicle / GMST_LightHelicopter / GMST_AttackHelicopter)
    let veh_class = flow.vehicle.clone().or_else(|| std::env::var("HOMEBOTS_TEST_VEHICLE").ok().map(|v| if v.starts_with("GCV_") { v } else { "GCV_US_M1114_Content".into() }));
    let veh_type = veh_class.as_ref().and_then(|c| vassets.as_ref().and_then(|a| a.kind_of(c).map(|k| crate::vehicles::spawn_type(&a.kinds[k].classes)))).unwrap_or("GMST_Jeep");
    let mut line = crate::objectives::line_spawns(level.as_deref(), &obj, crate::combat::player_team(), if group == veh_group { veh_type } else { "GMST_Infantry" });
    if line.is_empty() && group == veh_group && veh_type != "GMST_Jeep" {
        line = crate::objectives::line_spawns(level.as_deref(), &obj, crate::combat::player_team(), "GMST_Jeep");
    }
    let line_pick = (!line.is_empty()).then(|| {
        let (l, y) = line[nanos % line.len()];
        ("line".to_string(), l, y)
    });
    let pick = if line_pick.is_some() {
        line_pick.as_ref()
    } else if cands.is_empty() {
        col.starts.first()
    } else {
        Some(cands[nanos % cands.len()])
    };
    // HOMEBOTS_SPAWN_AT=x,y,z[,yaw[,pitch]] (UE units, cylinder centre; degrees, pitch up > 0):
    // tests (tests/scenarios.txt, positions from crates/upk/examples/viewspot.rs)
    let spawn_at: Option<Vec<f32>> = std::env::var("HOMEBOTS_SPAWN_AT").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect());
    let forced = spawn_at.as_ref().and_then(|f| (f.len() >= 3).then(|| ("test".to_string(), Vec3::new(f[0], f[2], f[1]) * UU, f.get(3).copied().unwrap_or(0.0).to_radians())));
    let forced_pitch = spawn_at.as_ref().filter(|_| forced.is_some()).and_then(|f| f.get(4).copied());
    let Some((g, loc, yaw)) = forced.or_else(|| pick.cloned()) else {
        warn!("player: no PlayerStart");
        return;
    };
    info!("player: spawn at {g} {loc:?}");
    // PlayerStart location = cylinder centre
    let feet = loc - Vec3::Y * HALF_HEIGHT;
    let yaw_units = yaw / std::f32::consts::TAU * 65536.0;
    let mut p = Pawn::new(feet, yaw_units);
    if let Some(deg) = forced_pitch {
        p.pitch = deg / 360.0 * 65536.0;
    }
    if let Ok((_, mut old)) = pawns.single_mut() {
        *old = p;
    } else {
        commands.spawn(p);
    }
}

#[allow(clippy::too_many_arguments)]
fn player_control(
    time: Res<Time>,
    motion: Res<AccumulatedMouseMotion>,
    flow: Res<MatchFlow>,
    col: Res<CollisionWorld>,
    mut pawns: Query<&mut Pawn>,
    mut cams: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut buf: Local<(Vec<u32>, Vec<Vec3>)>,
    mut prev_motion: Local<Vec2>,
) {
    use crate::input::{down, hit, Set};
    let playing = flow.phase == Phase::Playing;
    if let Ok(w) = windows.single() {
        crate::settings::set_aspect(w.width() / w.height().max(1.0));
    }
    let active = playing && !flow.paused && pawns.single().is_ok_and(|p| !p.dead);
    if let Ok(mut w) = windows.single_mut() {
        let (grab, vis) = if active { (CursorGrabMode::Locked, false) } else { (CursorGrabMode::None, true) };
        if (playing || w.cursor_options.grab_mode == CursorGrabMode::Locked) && w.cursor_options.grab_mode != grab {
            w.cursor_options.grab_mode = grab;
            w.cursor_options.visible = vis;
        }
    }
    if !playing {
        return;
    }
    let Ok(mut p) = pawns.single_mut() else { return };
    if p.in_vehicle || p.remote {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    if active {
        // UE3 PlayerInput: aMouseX *= MouseSensitivity, yaw += aTurn (rotation units); the
        // options' sensitivity, yaw / pitch factors, ADS scale, invert, smoothing (settings.rs)
        let mut d = motion.delta;
        if crate::settings::flag("InputActionToKey:Infantry.ToggleMouseSmoothing") {
            let avg = (d + *prev_motion) * 0.5;
            *prev_motion = d;
            d = avg;
        } else {
            *prev_motion = d;
        }
        let zoomed = p.fov < crate::settings::vfov() - 0.5;
        let (sx, sy) = crate::settings::mouse_scale(d, zoomed);
        p.yaw += d.x * MOUSE_SENS * sx;
        p.pitch = (p.pitch - d.y * MOUSE_SENS * sy).clamp(-16000.0, 16000.0);
        // HB_TEST_TURN=<degrees per second>: keep turning (flicker tests)
        if let Some(r) = std::env::var("HB_TEST_TURN").ok().and_then(|v| v.parse::<f32>().ok()) {
            p.yaw += r / 360.0 * 65536.0 * time.delta_secs();
        }
    }
    let (sy, cy) = ue_rot(p.yaw).sin_cos();
    // UE yaw: X forward, Y right (left-handed); Bevy: x = UE x, z = UE y
    let fwd = Vec3::new(cy, 0.0, sy);
    let right = Vec3::new(-sy, 0.0, cy);
    let mut input = Vec3::ZERO;
    if active {
        // HOMEBOTS_TEST_FORWARD=1: hold W (automated tests)
        if down(Set::Foot, "EKBC_MoveForward") || std::env::var_os("HOMEBOTS_TEST_FORWARD").is_some() {
            input += fwd;
        }
        if down(Set::Foot, "EKBC_MoveBackward") {
            input -= fwd;
        }
        if down(Set::Foot, "EKBC_StrafeRight") {
            input += right;
        }
        if down(Set::Foot, "EKBC_StrafeLeft") {
            input -= right;
        }
        // BindingsFoot LeftControl EKBC_ChangePosture (Y EKBC_Prone: no prone in HomeBots)
        if hit(Set::Foot, "EKBC_ChangePosture") {
            p.crouched = !p.crouched;
        }
    }
    let forward_only = down(Set::Foot, "EKBC_MoveForward") && !down(Set::Foot, "EKBC_MoveBackward");
    p.sprinting = active && down(Set::Foot, "EKBC_Sprint") && forward_only && p.on_ground && !p.no_sprint;
    if p.sprinting {
        p.crouched = false;
    }
    let max_speed = GROUND_SPEED * p.speed_scale * if p.sprinting { SPRINT_PCT } else if p.crouched { CROUCH_PCT } else { 1.0 };
    let accel_dir = input.normalize_or_zero();
    let jump = active && hit(Set::Foot, "EKBC_Jump");
    let p = &mut *p;
    walk(&col, &mut p.feet, &mut p.vel, &mut p.on_ground, p.crouched, accel_dir, max_speed, jump, dt, &mut buf);
    if std::env::var_os("HB_PLAYERDBG").is_some() {
        info!("pawn feet {:?} vel {:?} ground {} tris {}", p.feet, p.vel, p.on_ground, col.tris.len());
    }
    // eye height blends like UE3's EyeHeight (smooth crouch)
    let want = if p.crouched { CROUCH_EYE + CROUCH_HALF_HEIGHT } else { EYE + HALF_HEIGHT };
    let k = (dt * 10.0).min(1.0);
    p.eye += (want - p.eye) * k;
    if let Ok((mut tf, mut proj)) = cams.single_mut() {
        tf.translation = p.feet + Vec3::Y * p.eye;
        // UE rotator (yaw about Z, pitch up) -> Bevy camera looking along fwd
        let yaw = ue_rot(p.yaw);
        let pitch = ue_rot(p.pitch);
        let dir = Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos());
        tf.look_to(dir, Vec3::Y);
        if let Projection::Perspective(pp) = &mut *proj {
            let f = p.fov.to_radians();
            if (pp.fov - f).abs() > 1e-6 {
                pp.fov = f;
            }
        }
    }
}

#[cfg(test)]
mod ray_tests {
    use super::*;
    #[test]
    fn dda_matches_brute_force() {
        let mut col = CollisionWorld::default();
        let mut seed = 12345u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        for _ in 0..3000 {
            let c = Vec3::new(rnd() * 200.0 - 100.0, rnd() * 20.0, rnd() * 200.0 - 100.0);
            let r = || Vec3::new(0.0, 0.0, 0.0);
            let _ = r;
            let a = c + Vec3::new(rnd() * 6.0 - 3.0, rnd() * 6.0 - 3.0, rnd() * 6.0 - 3.0);
            let b = c + Vec3::new(rnd() * 6.0 - 3.0, rnd() * 6.0 - 3.0, rnd() * 6.0 - 3.0);
            let d = c + Vec3::new(rnd() * 6.0 - 3.0, rnd() * 6.0 - 3.0, rnd() * 6.0 - 3.0);
            col.add([a, b, d]);
        }
        let mut buf = Vec::new();
        for _ in 0..2000 {
            let o = Vec3::new(rnd() * 200.0 - 100.0, rnd() * 20.0, rnd() * 200.0 - 100.0);
            let dir = Vec3::new(rnd() * 2.0 - 1.0, rnd() * 0.4 - 0.2, rnd() * 2.0 - 1.0).normalize();
            let max = rnd() * 150.0;
            let fast = col.raycast_hit(o, dir, max, &mut buf, COL_TRACE).map(|h| h.0);
            let mut brute: Option<f32> = None;
            for t in &col.tris {
                if let Some(d) = ray_tri(o, dir, t) {
                    if d >= 0.0 && d <= max && brute.is_none_or(|b| d < b) {
                        brute = Some(d);
                    }
                }
            }
            match (fast, brute) {
                (Some(a), Some(b)) => assert!((a - b).abs() < 1e-4, "{a} vs {b}"),
                (None, None) => {}
                x => panic!("mismatch {x:?}"),
            }
        }
    }
}
