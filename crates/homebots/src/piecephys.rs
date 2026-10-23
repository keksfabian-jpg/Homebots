//! Rigid-body motion of detached parts: map-car doors / hood / trunk (levelcars.rs) and the
//! vehicle wreck pieces (destruction.rs). In the game these are PhysX bodies of the
//! PhysicsAsset (PA_DEST_* / RagdollAsset); here a small impulse solver stands in for PhysX.
//!
//! From data: gravity = Default__WorldInfo DefaultGravityZ -750 * RBPhysicsGravityScaling 2.0
//! (Engine.u; the FL-Crossroads / FL-Overpass WorldInfo override neither). PhysX applies the
//! scaled gravity to rigid bodies, pawns / projectiles use DefaultGravityZ alone.
//!
//! NOT from data (the PhysX shapes / materials of the pieces are not decoded): the contact shape
//! (up to 26 extreme vertices of the piece, see `Hull`), mass 1 with the inertia of the vertex
//! cloud, restitution, friction, damping, sleep thresholds, substep length and iteration count.
//!
//! Per substep: gravity, damping, explicit integration of position and rotation; contacts of
//! every hull point = (a) a sweep of the point from its last position (walls, fast falls, no
//! tunnelling) and (b) the floor below the point (resting contact, slopes); sequential impulses
//! with accumulated, clamped normal and Coulomb friction impulses (restitution only above a
//! closing speed); then the body is pushed out of the deepest penetration along the contact
//! normals. A part landing on an edge or corner gets the torque of the off-centre contact and
//! tips over until it lies flat; it sleeps once slow for a while.
use crate::player::CollisionWorld;
use bevy::math::Mat3;
use bevy::prelude::*;

/// Default__WorldInfo DefaultGravityZ (750) * RBPhysicsGravityScaling (2.0), UE units / s^2.
pub const RB_GRAVITY: f32 = 750.0 * 2.0;
/// Restitution above RESTITUTION_MIN closing speed (NOT from data).
pub const RESTITUTION: f32 = 0.3;
const RESTITUTION_MIN: f32 = 150.0;
/// Coulomb friction coefficient (NOT from data).
pub const FRICTION: f32 = 0.6;
/// Linear / angular damping per second (NOT from data).
const LIN_DAMP: f32 = 0.05;
const ANG_DAMP: f32 = 0.6;
/// Sleep: slower than these (uu/s, rad/s) while touching for SLEEP_TIME s (NOT from data).
const SLEEP_LIN: f32 = 15.0;
const SLEEP_ANG: f32 = 0.35;
const SLEEP_TIME: f32 = 0.5;
/// Hard stop after this many seconds of motion (NOT from data; keeps CPU skinning bounded).
const MAX_AWAKE: f32 = 15.0;
/// Solver (NOT from data).
const SUBSTEP: f32 = 1.0 / 120.0;
const ITERATIONS: usize = 8;
/// A point counts as touching the floor this close above it (UE units).
const SKIN: f32 = 0.5;
/// The floor probe starts this far above a point (UE units): deeper than one substep's fall.
const PROBE_UP: f32 = 40.0;

/// Contact shape and inertia of a piece, in its body frame relative to its centroid.
#[derive(Clone, Debug, Default)]
pub struct Hull {
    pub pts: Vec<Vec3>,
    /// inverse inertia tensor (mass 1), body frame
    pub inv_inertia: Mat3,
}

impl Hull {
    /// From the piece's vertices (body frame, relative to the centroid). `axes` turns the box
    /// directions into the body frame (the car / vehicle orientation, so that a door's corners
    /// are the extremes along the car's axes): the hull keeps the vertex farthest along each of
    /// the 26 directions (corners, edges, faces of a box).
    pub fn from_vertices(verts: &[Vec3], axes: Quat) -> Hull {
        if verts.is_empty() {
            return Hull { pts: vec![Vec3::ZERO], inv_inertia: Mat3::IDENTITY };
        }
        let mut pts: Vec<Vec3> = Vec::new();
        for x in -1i32..=1 {
            for y in -1i32..=1 {
                for z in -1i32..=1 {
                    if x == 0 && y == 0 && z == 0 {
                        continue;
                    }
                    let d = axes * Vec3::new(x as f32, y as f32, z as f32);
                    let best = verts.iter().copied().max_by(|a, b| a.dot(d).total_cmp(&b.dot(d))).unwrap_or(Vec3::ZERO);
                    if !pts.iter().any(|p| p.distance_squared(best) < 1.0) {
                        pts.push(best);
                    }
                }
            }
        }
        // inertia of the vertex cloud (unit total mass), regularised so a flat part stays invertible
        let mut i = Mat3::ZERO;
        for r in verts {
            i += Mat3::from_diagonal(Vec3::splat(r.length_squared())) - Mat3::from_cols(*r * r.x, *r * r.y, *r * r.z);
        }
        i *= 1.0 / verts.len() as f32;
        let tr = i.x_axis.x + i.y_axis.y + i.z_axis.z;
        i += Mat3::from_diagonal(Vec3::splat(tr * 0.02 + 1.0));
        Hull { pts, inv_inertia: i.inverse() }
    }
}

/// Motion state: centroid (world UE), rotation (body frame -> world), velocities.
#[derive(Clone, Debug)]
pub struct Body {
    pub c: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub ang: Vec3,
    pub resting: bool,
    still: f32,
    awake: f32,
}

impl Body {
    pub fn new(c: Vec3, rot: Quat, vel: Vec3, ang: Vec3) -> Body {
        Body { c, rot, vel, ang, resting: false, still: 0.0, awake: 0.0 }
    }
}

/// The world as the solver sees it (UE units, z up).
pub trait World {
    /// Highest surface at the vertical through `top`, from `top.z` down to `top.z - depth`:
    /// (z, upward normal).
    fn floor(&mut self, top: Vec3, depth: f32) -> Option<(f32, Vec3)>;
    /// First surface on the segment a -> b: (point, normal facing a).
    fn sweep(&mut self, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)>;
}

/// CollisionWorld (Bevy metres) with the pawn-blocking triangles, minus the triangles `skip`
/// (the car's own reference-pose collision: the pieces would land on the ghost of their own
/// door / the car's body, see levelcars.rs).
pub struct ColProbe<'a> {
    pub col: &'a CollisionWorld,
    pub buf: &'a mut Vec<u32>,
    pub skip: std::ops::Range<u32>,
}

fn ue_to_bevy_m(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y) * 0.01
}

fn bevy_dir_to_ue(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y)
}

impl World for ColProbe<'_> {
    fn floor(&mut self, top: Vec3, depth: f32) -> Option<(f32, Vec3)> {
        self.col.floor_skip(ue_to_bevy_m(top), depth * 0.01, self.buf, &self.skip).map(|(y, n)| (y * 100.0, bevy_dir_to_ue(n)))
    }

    fn sweep(&mut self, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
        let (pa, pb) = (ue_to_bevy_m(a), ue_to_bevy_m(b));
        let d = pb - pa;
        let len = d.length();
        if len < 1e-5 {
            return None;
        }
        self.col
            .raycast_hit_skip(pa, d / len, len, self.buf, crate::player::COL_PLAYER, &self.skip)
            .map(|(t, n, _)| (a + (b - a) * (t / len), bevy_dir_to_ue(n)))
    }
}

struct Contact {
    r: Vec3,
    n: Vec3,
    depth: f32,
    target: f32,
    jn: f32,
    jt: Vec3,
}

/// Advance a body by `dt` seconds (substeps of SUBSTEP).
pub fn step(b: &mut Body, h: &Hull, w: &mut dyn World, dt: f32) {
    if b.resting || dt <= 0.0 {
        return;
    }
    let n = (dt / SUBSTEP).ceil().clamp(1.0, 8.0) as usize;
    let sdt = dt / n as f32;
    for _ in 0..n {
        substep(b, h, w, sdt);
        if b.resting {
            break;
        }
    }
}

fn substep(b: &mut Body, h: &Hull, w: &mut dyn World, dt: f32) {
    let prev: Vec<Vec3> = h.pts.iter().map(|p| b.c + b.rot * *p).collect();
    b.vel.z -= RB_GRAVITY * dt;
    b.vel *= (1.0 - LIN_DAMP * dt).max(0.0);
    b.ang *= (1.0 - ANG_DAMP * dt).max(0.0);
    b.c += b.vel * dt;
    b.rot = (Quat::from_scaled_axis(b.ang * dt) * b.rot).normalize();
    b.awake += dt;

    // contacts
    let rm = Mat3::from_quat(b.rot);
    let iinv = rm * h.inv_inertia * rm.transpose();
    let mut cs: Vec<Contact> = Vec::new();
    for (i, p) in h.pts.iter().enumerate() {
        let cur = b.c + b.rot * *p;
        let (n, depth) = if let Some((hit, n)) = w.sweep(prev[i], cur) {
            (n, (hit - cur).dot(n).max(0.0))
        } else if let Some((fz, n)) = w.floor(cur + Vec3::Z * PROBE_UP, PROBE_UP + 2.0 * SKIN) {
            if cur.z > fz + SKIN {
                continue;
            }
            (n, ((fz - cur.z) * n.z).max(0.0))
        } else {
            continue;
        };
        let r = cur - b.c;
        let vn = (b.vel + b.ang.cross(r)).dot(n);
        let target = if vn < -RESTITUTION_MIN { -RESTITUTION * vn } else { 0.0 };
        cs.push(Contact { r, n, depth, target, jn: 0.0, jt: Vec3::ZERO });
    }
    if cs.is_empty() {
        b.still = 0.0;
    } else {
        // sequential impulses (unit mass)
        let k_of = |r: Vec3, d: Vec3| 1.0 + d.dot((iinv * r.cross(d)).cross(r));
        for _ in 0..ITERATIONS {
            for c in cs.iter_mut() {
                let v = b.vel + b.ang.cross(c.r);
                let vn = v.dot(c.n);
                let k = k_of(c.r, c.n);
                let new = (c.jn + (c.target - vn) / k).max(0.0);
                let dj = new - c.jn;
                c.jn = new;
                let imp = c.n * dj;
                b.vel += imp;
                b.ang += iinv * c.r.cross(imp);
                // friction, limited by the accumulated normal impulse
                let v = b.vel + b.ang.cross(c.r);
                let vt = v - c.n * v.dot(c.n);
                let lt = vt.length();
                if lt > 1e-4 {
                    let t = vt / lt;
                    let want = c.jt - t * (lt / k_of(c.r, t));
                    let new = want.clamp_length_max(FRICTION * c.jn);
                    let d = new - c.jt;
                    c.jt = new;
                    b.vel += d;
                    b.ang += iinv * c.r.cross(d);
                }
            }
        }
        // push out of the penetration
        let mut corr = Vec3::ZERO;
        for _ in 0..3 {
            for c in &cs {
                let d = c.depth - corr.dot(c.n);
                if d > 0.0 {
                    corr += c.n * d;
                }
            }
        }
        b.c += corr;
        if b.vel.length() < SLEEP_LIN && b.ang.length() < SLEEP_ANG {
            b.still += dt;
        } else {
            b.still = 0.0;
        }
    }
    if b.still >= SLEEP_TIME || b.awake >= MAX_AWAKE {
        b.resting = true;
        b.vel = Vec3::ZERO;
        b.ang = Vec3::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat ground at z = 0 and a wall x = 300 (normal -x).
    struct Plane;
    impl World for Plane {
        fn floor(&mut self, top: Vec3, depth: f32) -> Option<(f32, Vec3)> {
            (top.z >= 0.0 && top.z - depth <= 0.0).then_some((0.0, Vec3::Z))
        }
        fn sweep(&mut self, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
            let mut best: Option<(f32, Vec3)> = None;
            if a.z >= 0.0 && b.z < 0.0 {
                best = Some((a.z / (a.z - b.z), Vec3::Z));
            }
            if a.x <= 300.0 && b.x > 300.0 {
                let t = (300.0 - a.x) / (b.x - a.x);
                if best.is_none_or(|x| t < x.0) {
                    best = Some((t, -Vec3::X));
                }
            }
            best.map(|(t, n)| (a + (b - a) * t, n))
        }
    }

    /// A door-like plate: 100 x 8 x 120 uu (x, y, z), vertices on a grid.
    fn door() -> Vec<Vec3> {
        let mut v = Vec::new();
        for i in 0..=10 {
            for k in 0..=12 {
                for j in [-4.0, 4.0] {
                    v.push(Vec3::new(i as f32 * 10.0 - 50.0, j, k as f32 * 10.0 - 60.0));
                }
            }
        }
        v
    }

    fn run(b: &mut Body, h: &Hull, secs: f32) {
        let mut t = 0.0;
        while t < secs && !b.resting {
            step(b, h, &mut Plane, 1.0 / 60.0);
            t += 1.0 / 60.0;
        }
    }

    #[test]
    fn upright_door_tips_over_and_lies_flat() {
        let h = Hull::from_vertices(&door(), Quat::IDENTITY);
        // standing on its edge 1 m up, slight tilt and spin
        let mut b = Body::new(Vec3::new(0.0, 0.0, 160.0), Quat::from_rotation_x(0.15), Vec3::new(30.0, 0.0, 0.0), Vec3::new(0.5, 0.2, 0.0));
        run(&mut b, &h, 12.0);
        assert!(b.resting, "settles");
        // lying flat: the thin axis (body y) is vertical, centroid at half the thickness
        let thin = (b.rot * Vec3::Y).z.abs();
        assert!(thin > 0.9, "lies flat, thin axis z = {thin}");
        assert!(b.c.z > 2.0 && b.c.z < 8.0, "on the ground, centroid z = {}", b.c.z);
    }

    #[test]
    fn falling_piece_does_not_float_or_sink() {
        let h = Hull::from_vertices(&door(), Quat::IDENTITY);
        let mut b = Body::new(Vec3::new(0.0, 0.0, 300.0), Quat::from_rotation_y(0.7) * Quat::from_rotation_x(1.2), Vec3::new(0.0, 0.0, 750.0), Vec3::new(2.0, -1.5, 1.0));
        run(&mut b, &h, 12.0);
        assert!(b.resting);
        let low = h.pts.iter().map(|p| (b.c + b.rot * *p).z).fold(f32::MAX, f32::min);
        assert!(low > -2.0 && low < 2.0, "lowest point on the ground: {low}");
    }

    #[test]
    fn wall_stops_piece() {
        let h = Hull::from_vertices(&door(), Quat::IDENTITY);
        let mut b = Body::new(Vec3::new(100.0, 0.0, 100.0), Quat::IDENTITY, Vec3::new(1500.0, 0.0, 300.0), Vec3::ZERO);
        run(&mut b, &h, 12.0);
        let maxx = h.pts.iter().map(|p| (b.c + b.rot * *p).x).fold(f32::MIN, f32::max);
        assert!(maxx <= 302.0, "stays in front of the wall: {maxx}");
    }
}
