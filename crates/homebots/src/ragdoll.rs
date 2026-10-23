//! Ragdoll for dead soldiers (bots and the player's kill-cam body) instead of a death sequence.
//!
//! The game drives its ragdolls with PhysX bodies of the PhysicsAsset (PA_*), which are NOT
//! decoded here: this is a point-mass (Verlet) skeleton of the main bones with fixed bone
//! lengths, a rigid torso, no-fold limits on knees / elbows, gravity (GCProjectile-like
//! 750 uu/s^2 used elsewhere) and ground contact against the level collision. The start
//! velocity comes from the killing hit's direction (NOT from data: the damage types'
//! KCharacterRagdollImpulse / KDeathUpKick act on PhysX masses).
//!
//! Positions are simulated in UE world units; the pose handed to the skinning is in the
//! skeleton's component space for the origin / yaw the body had when it died.

use crate::player::CollisionWorld;
use bevy::prelude::*;

const GRAVITY: f32 = 750.0;
/// point radius against the ground (uu)
const RADIUS: f32 = 4.0;
const ITER: usize = 10;
/// seconds after which the body stops simulating
const SLEEP: f32 = 6.0;

/// bones simulated as points; the rest follow their parent rigidly
const SIM: &[&str] = &[
    "Hips", "Spine1", "Spine2", "Neck", "Head", "LeftArm", "LeftForeArm", "LeftHand", "RightArm", "RightForeArm", "RightHand", "LeftUpLeg", "LeftLeg", "LeftFoot", "RightUpLeg", "RightLeg", "RightFoot",
];
/// rigid torso (all pairwise distances kept)
const TORSO: &[&str] = &["Hips", "Spine1", "Spine2", "Neck", "LeftArm", "RightArm", "LeftUpLeg", "RightUpLeg"];

/// mesh component space <-> UE (skin_part: rel = rot * comp(pos))
fn comp(d: Vec3) -> Vec3 {
    Vec3::new(d.z, -d.x, -d.y)
}
fn comp_inv(u: Vec3) -> Vec3 {
    Vec3::new(-u.y, -u.z, u.x)
}

pub struct Ragdoll {
    pub origin: Vec3,
    pub rot: Quat,
    parents: Vec<i32>,
    /// component-space pose at death
    rest: Vec<(Quat, Vec3)>,
    /// bone -> point index
    point_of: Vec<Option<usize>>,
    /// bone -> point whose direction orients the bone
    aim: Vec<Option<usize>>,
    pos: Vec<Vec3>,
    prev: Vec<Vec3>,
    /// (a, b, length, min only)
    links: Vec<(usize, usize, f32, bool)>,
    /// hips frame: (spine point, left thigh point, right thigh point)
    hips_frame: Option<(usize, usize, usize, usize)>,
    pub t: f32,
}

impl Ragdoll {
    /// `pose`: component-space pose of `skeleton` at death; `vel`: start velocity (UE uu/s).
    pub fn new(skeleton: &[upk::skeletal::Bone], pose: &[(Quat, Vec3)], origin: Vec3, rot: Quat, vel: Vec3) -> Ragdoll {
        let n = skeleton.len();
        let parents: Vec<i32> = skeleton.iter().map(|b| b.parent).collect();
        let mut point_of = vec![None; n];
        let mut pos = Vec::new();
        let world = |p: Vec3| origin + rot * comp(p);
        let find = |name: &str| skeleton.iter().position(|b| b.name.eq_ignore_ascii_case(name));
        for name in SIM {
            if let Some(i) = find(name) {
                point_of[i] = Some(pos.len());
                pos.push(world(pose[i].1));
            }
        }
        // nearest simulated ancestor of each simulated bone
        let sim_parent = |i: usize| -> Option<usize> {
            let mut k = parents[i];
            for _ in 0..64 {
                if k < 0 || k as usize == i || k as usize >= n {
                    return None;
                }
                if point_of[k as usize].is_some() {
                    return Some(k as usize);
                }
                if k == 0 {
                    return None;
                }
                k = parents[k as usize];
            }
            None
        };
        let mut links = Vec::new();
        let mut aim = vec![None; n];
        for i in 0..n {
            let Some(pi) = point_of[i] else { continue };
            if let Some(a) = sim_parent(i) {
                let pa = point_of[a].unwrap();
                links.push((pa, pi, pos[pa].distance(pos[pi]), false));
                if aim[a].is_none() {
                    aim[a] = Some(pi);
                }
            }
        }
        let torso: Vec<usize> = TORSO.iter().filter_map(|n| find(n)).filter_map(|i| point_of[i]).collect();
        for (k, &a) in torso.iter().enumerate() {
            for &b in &torso[k + 1..] {
                links.push((a, b, pos[a].distance(pos[b]), false));
            }
        }
        // stiff neck, no folding knees / elbows
        let pt = |name: &str| find(name).and_then(|i| point_of[i]);
        if let (Some(h), Some(s)) = (pt("Head"), pt("Spine2")) {
            links.push((h, s, pos[h].distance(pos[s]), false));
        }
        for (a, b, k) in [("LeftUpLeg", "LeftFoot", 0.6), ("RightUpLeg", "RightFoot", 0.6), ("LeftArm", "LeftHand", 0.45), ("RightArm", "RightHand", 0.45)] {
            if let (Some(a), Some(b)) = (pt(a), pt(b)) {
                links.push((a, b, pos[a].distance(pos[b]) * k, true));
            }
        }
        let hips_frame = match (pt("Hips"), pt("Spine2"), pt("LeftUpLeg"), pt("RightUpLeg")) {
            (Some(h), Some(s), Some(l), Some(r)) => Some((h, s, l, r)),
            _ => None,
        };
        // the upper body gets an extra tip over (at least 150 uu/s horizontally, in the push
        // direction, else backwards) so a body pushed straight up does not stay standing
        let mut tip = Vec3::new(vel.x, vel.y, 0.0);
        if tip.length() < 1.0 {
            tip = rot * Vec3::new(-1.0, 0.0, 0.0);
        }
        let tip = tip.normalize() * 150.0;
        let hips_z = pt("Hips").map(|h| pos[h].z).unwrap_or(origin.z);
        let dt0 = 1.0 / 60.0;
        let prev = pos
            .iter()
            .map(|p| {
                let k = ((p.z - hips_z) / 60.0).clamp(0.0, 1.0);
                *p - (vel + tip * k) * dt0
            })
            .collect();
        Ragdoll { origin, rot, parents, rest: pose.to_vec(), point_of, aim, pos, prev, links, hips_frame, t: 0.0 }
    }

    pub fn debug(&self) -> String {
        format!("pts {} first {:?} last {:?}", self.pos.len(), self.pos.first(), self.pos.last())
    }

    pub fn asleep(&self) -> bool {
        self.t >= SLEEP
    }

    pub fn step(&mut self, col: &CollisionWorld, dt: f32, buf: &mut Vec<u32>) {
        self.step_with(dt, |p| {
            let b = Vec3::new(p.x, p.z, p.y) * 0.01;
            col.floor(b + Vec3::Y * 0.6, 2.5, buf).map(|(y, _)| y * 100.0)
        });
    }

    /// One step with a ground-height query (UE position -> ground z under it).
    pub fn step_with(&mut self, dt: f32, mut ground_at: impl FnMut(Vec3) -> Option<f32>) {
        if self.asleep() || dt <= 0.0 {
            return;
        }
        self.t += dt;
        let dt = dt.min(1.0 / 30.0);
        for (p, q) in self.pos.iter_mut().zip(self.prev.iter_mut()) {
            let v = (*p - *q) * 0.995;
            *q = *p;
            *p += v + Vec3::new(0.0, 0.0, -GRAVITY) * dt * dt;
        }
        // ground under each point (Bevy metres in the collision world)
        let ground: Vec<Option<f32>> = self.pos.iter().map(|p| ground_at(*p)).collect();
        for _ in 0..ITER {
            for &(a, b, len, min_only) in &self.links {
                let d = self.pos[b] - self.pos[a];
                let l = d.length();
                if l < 1e-4 || (min_only && l >= len) {
                    continue;
                }
                let c = d * ((l - len) / l * 0.5);
                self.pos[a] += c;
                self.pos[b] -= c;
            }
            for (k, g) in ground.iter().enumerate() {
                if let Some(g) = g {
                    if self.pos[k].z < g + RADIUS {
                        self.pos[k].z = g + RADIUS;
                    }
                }
            }
        }
        // ground friction
        for (k, g) in ground.iter().enumerate() {
            if let Some(g) = g {
                if self.pos[k].z <= g + RADIUS + 0.5 {
                    let p = self.pos[k];
                    let q = &mut self.prev[k];
                    q.x += (p.x - q.x) * 0.4;
                    q.y += (p.y - q.y) * 0.4;
                }
            }
        }
    }

    /// Component-space pose for skin_part(.., self.rot, self.origin, ..).
    pub fn pose(&self) -> Vec<(Quat, Vec3)> {
        let rinv = self.rot.inverse();
        let to_comp = |w: Vec3| comp_inv(rinv * (w - self.origin));
        let cur: Vec<Vec3> = self.pos.iter().map(|w| to_comp(*w)).collect();
        let n = self.rest.len();
        let mut out: Vec<(Quat, Vec3)> = Vec::with_capacity(n);
        for i in 0..n {
            let (r0, p0) = self.rest[i];
            let parent = self.parents[i];
            let has_parent = i > 0 && parent >= 0 && (parent as usize) < i;
            let p = match self.point_of[i] {
                Some(k) => cur[k],
                None if has_parent => {
                    let (pr0, pp0) = self.rest[parent as usize];
                    let (pr, pp) = out[parent as usize];
                    pp + pr * (pr0.inverse() * (p0 - pp0))
                }
                None => p0,
            };
            let is_hips = self.hips_frame.is_some_and(|f| self.point_of[i] == Some(f.0));
            let r = if is_hips {
                // hips: full frame from the spine (up) and the thighs (side)
                let (h, s, l, rr) = self.hips_frame.unwrap();
                let frame = |up: Vec3, side: Vec3| {
                    let u = up.normalize_or_zero();
                    let f = u.cross(side).normalize_or_zero();
                    let s2 = f.cross(u);
                    Mat3::from_cols(u, s2, f)
                };
                let b0 = frame(self.rest_point(s) - self.rest_point(h), self.rest_point(l) - self.rest_point(rr));
                let b1 = frame(cur[s] - cur[h], cur[l] - cur[rr]);
                Quat::from_mat3(&(b1 * b0.transpose())).normalize() * r0
            } else if let (Some(_), Some(c)) = (self.point_of[i], self.aim[i]) {
                let k = self.point_of[i].unwrap();
                let d0 = (self.rest_point(c) - p0).normalize_or_zero();
                let d1 = (cur[c] - cur[k]).normalize_or_zero();
                if d0 == Vec3::ZERO || d1 == Vec3::ZERO {
                    r0
                } else {
                    Quat::from_rotation_arc(d0, d1) * r0
                }
            } else if has_parent {
                let (pr0, _) = self.rest[parent as usize];
                let (pr, _) = out[parent as usize];
                pr * (pr0.inverse() * r0)
            } else {
                r0
            };
            out.push((r, p));
        }
        out
    }

    /// Component-space rest position of a point.
    fn rest_point(&self, k: usize) -> Vec3 {
        let i = self.point_of.iter().position(|p| *p == Some(k)).unwrap_or(0);
        self.rest[i].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn falls_over() {
        let root = std::path::Path::new("/mnt/user-data/uploads/HOMEFRONT/GCGame/CookedPC/Characters_FL2");
        let Some(f) = std::fs::read_dir(root).ok().and_then(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n == "US_AST.upk"))) else { return };
        let p = upk::Package::open(&f).unwrap();
        let i = p.exports.iter().position(|e| p.object_name_of(e) == "SKC_MP_US_AST" && p.export_class(e) == "SkeletalMesh").unwrap();
        let m = p.read_skeletal_mesh(i).unwrap();
        let mut pose: Vec<(Quat, Vec3)> = Vec::new();
        for (k, b) in m.bones.iter().enumerate() {
            let lr = Quat::from_xyzw(b.rot[0], b.rot[1], b.rot[2], b.rot[3]).normalize();
            let lp = Vec3::from(b.pos);
            if k == 0 || b.parent < 0 {
                pose.push((lr, lp));
            } else {
                let (pr, pp) = pose[b.parent as usize];
                pose.push((pr * lr, pp + pr * lp));
            }
        }
        let mut rd = Ragdoll::new(&m.bones, &pose, Vec3::new(0.0, 0.0, 100.0), Quat::IDENTITY, Vec3::new(250.0, 0.0, 0.0));
        let zs = |rd: &Ragdoll| rd.pos.iter().map(|p| p.z).fold((f32::MAX, f32::MIN), |a, z| (a.0.min(z), a.1.max(z)));
        let (lo, hi) = zs(&rd);
        println!("start z {lo:.1}..{hi:.1}");
        let g = lo;
        for _ in 0..360 {
            rd.step_with(1.0 / 60.0, |_| Some(g - RADIUS));
        }
        let (lo2, hi2) = zs(&rd);
        println!("end z {lo2:.1}..{hi2:.1}");
        let pose2 = rd.pose();
        assert_eq!(pose2.len(), pose.len());
        // skin the body with both poses and plot side views (x/z of UE space) for a look
        let skin = |ps: &[(Quat, Vec3)]| -> Vec<Vec3> {
            m.lod.positions.iter().enumerate().map(|(vi, v)| {
                let v = Vec3::from(*v);
                let (bi, wt) = m.influences[vi];
                let mut o = Vec3::ZERO;
                for k in 0..4 {
                    if wt[k] == 0 { continue; }
                    let b = bi[k] as usize;
                    let (rr, rp) = pose[b];
                    let (cr, cp) = ps[b];
                    o += (cr * (rr.inverse() * (v - rp)) + cp) * (wt[k] as f32 / 255.0);
                }
                Vec3::new(0.0, 0.0, 100.0) + comp(o)
            }).collect()
        };
        for (name, ps) in [("rag_before", &pose), ("rag_after", &pose2)] {
            let pts = skin(ps);
            let (w, h) = (400usize, 300usize);
            let mut img = vec![255u8; w * h * 3];
            for p in &pts {
                for (ox, a, b) in [(0usize, p.x, p.z), (200usize, p.y, p.z)] {
                    let x = (ox as f32 + 100.0 + a * 0.8) as i32;
                    let y = (h as f32 - 20.0 - (b - 90.0) * 0.8) as i32;
                    if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                        let i = (y as usize * w + x as usize) * 3;
                        img[i] = 0; img[i + 1] = 0; img[i + 2] = 0;
                    }
                }
            }
            let mut out = format!("P6 {w} {h} 255\n").into_bytes();
            out.extend(img);
            let _ = std::fs::write(format!("/tmp/claude-0/-home-claude/026f9b4d-8882-5f13-9237-52fde54ee789/scratchpad/{name}.ppm"), out);
        }
        assert!(hi2 - lo2 < (hi - lo) * 0.5, "body should lie down");
    }
}
