//! Own first-person arm animations made in Blender on the game's 1P arms rig (NOT from data; the
//! game has no such animations): `assets/nuke/*.hba`, written by the Blender exporter
//! (the Blender exporter, run through the Blender connection; source in MODLOG 2026-10-20). Used by the nuke: hooking the Fulton line to the harness
//! and the middle finger while towed away.
//!
//! Layout (little endian): "HBA1", f32 fps, u32 frames, u32 bones, per bone u8 name length + name;
//! u32 objects, per object u8 name length + name; f32 camera pitch per frame (radians, + = up);
//! per frame: per bone the component-space pose (quat xyzw, position; 1P mesh space as
//! viewmodel.rs composes it: X = -right, Y = -up, Z = forward, UE units, origin at the eye), then
//! per object its pose in the unscaled viewmodel space (quat xyzw, position, scale; Bevy axes X
//! right, Y up, -Z forward, metres, origin at the eye: props such as the carabiner); the game puts
//! them under a parent with the viewmodel's lateral scale (right and up times K, nuke.rs HookSpace),
//! as Blender's HB_VMSpace does.
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Clip {
    pub fps: f32,
    pub frames: usize,
    pub bones: Vec<String>,
    pub objects: Vec<String>,
    pub pitch: Vec<f32>,
    /// [frame][bone]
    pub pose: Vec<Vec<(Quat, Vec3)>>,
    /// [frame][object]
    pub obj: Vec<Vec<(Quat, Vec3, Vec3)>>,
}

impl Clip {
    pub fn length(&self) -> f32 {
        (self.frames.max(1) - 1) as f32 / self.fps
    }

    fn at(&self, t: f32) -> (usize, usize, f32) {
        let f = (t * self.fps).clamp(0.0, (self.frames.max(1) - 1) as f32);
        let a = f.floor() as usize;
        let b = (a + 1).min(self.frames.max(1) - 1);
        (a, b, f - a as f32)
    }

    /// Bone poses at t (lower-case bone name -> component pose).
    pub fn sample(&self, t: f32) -> HashMap<String, (Quat, Vec3)> {
        let (a, b, k) = self.at(t);
        self.bones.iter().enumerate().map(|(i, n)| (n.to_ascii_lowercase(), lerp(self.pose[a][i], self.pose[b][i], k))).collect()
    }

    pub fn pitch_at(&self, t: f32) -> f32 {
        let (a, b, k) = self.at(t);
        self.pitch[a] + (self.pitch[b] - self.pitch[a]) * k
    }

    /// An object's pose at t in the Bevy view space.
    pub fn object_at(&self, name: &str, t: f32) -> Option<(Quat, Vec3, Vec3)> {
        let i = self.objects.iter().position(|o| o.eq_ignore_ascii_case(name))?;
        let (a, b, k) = self.at(t);
        let (x, y) = (self.obj[a][i], self.obj[b][i]);
        Some((x.0.slerp(y.0, k), x.1.lerp(y.1, k), x.2.lerp(y.2, k)))
    }
}

fn lerp(a: (Quat, Vec3), b: (Quat, Vec3), k: f32) -> (Quat, Vec3) {
    (a.0.slerp(b.0, k), a.1.lerp(b.1, k))
}

pub fn load(path: &str) -> Option<Clip> {
    let b = std::fs::read(path).ok()?;
    let mut p = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = b.get(p..p + n)?;
        p += n;
        Some(s)
    };
    if take(4)? != b"HBA1" {
        return None;
    }
    let f32_ = |s: &[u8]| f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    let u32_ = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize;
    let fps = f32_(take(4)?);
    let frames = u32_(take(4)?);
    let nb = u32_(take(4)?);
    let mut bones = Vec::with_capacity(nb);
    for _ in 0..nb {
        let l = take(1)?[0] as usize;
        bones.push(String::from_utf8_lossy(take(l)?).into_owned());
    }
    let no = u32_(take(4)?);
    let mut objects = Vec::with_capacity(no);
    for _ in 0..no {
        let l = take(1)?[0] as usize;
        objects.push(String::from_utf8_lossy(take(l)?).into_owned());
    }
    let mut pitch = Vec::with_capacity(frames);
    for _ in 0..frames {
        pitch.push(f32_(take(4)?));
    }
    let mut pose = Vec::with_capacity(frames);
    let mut obj = Vec::with_capacity(frames);
    for _ in 0..frames {
        let mut fb = Vec::with_capacity(nb);
        for _ in 0..nb {
            let f: Vec<f32> = take(28)?.chunks_exact(4).map(f32_).collect();
            fb.push((Quat::from_xyzw(f[0], f[1], f[2], f[3]).normalize(), Vec3::new(f[4], f[5], f[6])));
        }
        let mut fo = Vec::with_capacity(no);
        for _ in 0..no {
            let f: Vec<f32> = take(40)?.chunks_exact(4).map(f32_).collect();
            fo.push((Quat::from_xyzw(f[0], f[1], f[2], f[3]).normalize(), Vec3::new(f[4], f[5], f[6]), Vec3::new(f[7], f[8], f[9])));
        }
        pose.push(fb);
        obj.push(fo);
    }
    Some(Clip { fps, frames, bones, objects, pitch, pose, obj })
}

/// The pose the viewmodel draws this frame instead of the weapon's (set by nuke.rs).
static CURRENT: Mutex<Option<HashMap<String, (Quat, Vec3)>>> = Mutex::new(None);

pub fn set(p: Option<HashMap<String, (Quat, Vec3)>>) {
    if let Ok(mut c) = CURRENT.lock() {
        *c = p;
    }
}

pub fn current() -> Option<HashMap<String, (Quat, Vec3)>> {
    CURRENT.lock().ok().and_then(|c| c.clone())
}

pub fn active() -> bool {
    CURRENT.lock().map(|c| c.is_some()).unwrap_or(false)
}

/// A view-space pose (Bevy view space, metres) in the world for the eye at `eye` (Bevy m) and the
/// view rotation `view`.
#[allow(dead_code)]
pub fn view_to_world(eye: Vec3, view: Quat, pose: (Quat, Vec3)) -> (Quat, Vec3) {
    (view * pose.0, eye + view * pose.1)
}
