//! AnimSet / AnimSequence decoding (UE3 compressed animation, key encoding ConstantKeyLerp).
//!
//! AnimSet: TrackBoneNames (FName per track); its AnimSequence exports are children.
//! AnimSequence: CompressedTrackOffsets (4 ints per track: translation offset, translation key
//! count, rotation offset, rotation key count) into the native CompressedByteStream (TArray<BYTE>
//! after the properties). Translation format ACF_None (float3 per key), rotation:
//! one key = Float96NoW, more keys = RotationCompressionFormat (IntervalFixed32NoW: 6 floats
//! mins/ranges then a u32 per key with x:11 y:11 z:10 bits). Keys are evenly spread over the
//! sequence (ConstantKeyLerp).
use crate::props::Value;
use crate::Package;

#[derive(Debug, Clone, Default)]
pub struct AnimTrack {
    pub pos: Vec<[f32; 3]>,
    /// quaternions x, y, z, w (as stored, see bone convention notes in the skinning code)
    pub rot: Vec<[f32; 4]>,
}

#[derive(Debug, Clone, Default)]
pub struct AnimSeq {
    pub name: String,
    pub length: f32,
    pub frames: i32,
    pub rate: f32,
    pub tracks: Vec<AnimTrack>,
    /// AnimNotify_Sound entries: (Time, SoundCue object path)
    pub sounds: Vec<(f32, String)>,
    /// AnimNotify_Footstep times (the 1P weapon sets' ADD_move_F / ADD_sprint_loop carry them)
    pub footsteps: Vec<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct AnimSetData {
    pub path: String,
    pub bones: Vec<String>,
    pub seqs: Vec<AnimSeq>,
}

fn ints(v: Option<&Value>) -> Vec<i32> {
    match v {
        Some(Value::Raw { data, .. }) if data.len() >= 4 => {
            let n = i32::from_le_bytes(data[0..4].try_into().unwrap()).max(0) as usize;
            data[4..].chunks_exact(4).take(n).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()
        }
        _ => Vec::new(),
    }
}

fn wfrom(x: f32, y: f32, z: f32) -> [f32; 4] {
    let w2 = 1.0 - x * x - y * y - z * z;
    [x, y, z, if w2 > 0.0 { w2.sqrt() } else { 0.0 }]
}

impl AnimSeq {
    /// Track key at time t (seconds): (translation, rotation) with linear / nlerp interpolation.
    pub fn sample(&self, track: usize, t: f32) -> Option<([f32; 3], [f32; 4])> {
        let tr = self.tracks.get(track)?;
        if tr.pos.is_empty() && tr.rot.is_empty() {
            // no track for this bone (sets merged by bone name)
            return None;
        }
        let f = if self.length > 0.0 { (t / self.length).clamp(0.0, 1.0) } else { 0.0 };
        let pick3 = |v: &Vec<[f32; 3]>| -> [f32; 3] {
            if v.len() <= 1 {
                return v.first().copied().unwrap_or([0.0; 3]);
            }
            let x = f * (v.len() - 1) as f32;
            let i = (x.floor() as usize).min(v.len() - 2);
            let a = x - i as f32;
            let (p, q) = (v[i], v[i + 1]);
            [p[0] + (q[0] - p[0]) * a, p[1] + (q[1] - p[1]) * a, p[2] + (q[2] - p[2]) * a]
        };
        let pick4 = |v: &Vec<[f32; 4]>| -> [f32; 4] {
            if v.len() <= 1 {
                return v.first().copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
            }
            let x = f * (v.len() - 1) as f32;
            let i = (x.floor() as usize).min(v.len() - 2);
            let a = x - i as f32;
            let (p, mut q) = (v[i], v[i + 1]);
            if p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3] < 0.0 {
                q = q.map(|c| -c);
            }
            let r = [p[0] + (q[0] - p[0]) * a, p[1] + (q[1] - p[1]) * a, p[2] + (q[2] - p[2]) * a, p[3] + (q[3] - p[3]) * a];
            let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3]).sqrt().max(1e-9);
            r.map(|c| c / l)
        };
        Some((pick3(&tr.pos), pick4(&tr.rot)))
    }
}

impl Package {
    pub fn read_anim_seq(&self, export_index: usize) -> Option<AnimSeq> {
        let mut r = self.export_reader(export_index);
        let props = self.read_props(&mut r)?;
        let n = r.i32()?;
        let stream = r.bytes(n.max(0) as usize)?;
        let offsets = ints(props.get("CompressedTrackOffsets"));
        let rot_fmt = match props.get("RotationCompressionFormat") {
            Some(Value::Enum(s)) => s.clone(),
            _ => "ACF_Float96NoW".into(),
        };
        let trans_fmt = match props.get("TranslationCompressionFormat") {
            Some(Value::Enum(s)) => s.clone(),
            _ => "ACF_None".into(),
        };
        let key_fmt = match props.get("KeyEncodingFormat") {
            Some(Value::Enum(s)) => s.clone(),
            _ => "AKF_ConstantKeyLerp".into(),
        };
        if key_fmt != "AKF_ConstantKeyLerp" || trans_fmt != "ACF_None" {
            return None;
        }
        let f = |o: usize| stream.get(o..o + 4).map(|x| f32::from_le_bytes(x.try_into().unwrap()));
        let u = |o: usize| stream.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()));
        let h = |o: usize| stream.get(o..o + 2).map(|x| u16::from_le_bytes(x.try_into().unwrap()));
        let mut tracks = Vec::new();
        for t in offsets.chunks_exact(4) {
            let (to, tn, ro, rn) = (t[0].max(0) as usize, t[1].max(0) as usize, t[2].max(0) as usize, t[3].max(0) as usize);
            let mut tr = AnimTrack::default();
            for k in 0..tn {
                let o = to + k * 12;
                tr.pos.push([f(o)?, f(o + 4)?, f(o + 8)?]);
            }
            if rn == 1 {
                tr.rot.push(wfrom(f(ro)?, f(ro + 4)?, f(ro + 8)?));
            } else if rn > 1 {
                match rot_fmt.as_str() {
                    "ACF_IntervalFixed32NoW" => {
                        let mins = [f(ro)?, f(ro + 4)?, f(ro + 8)?];
                        let rng = [f(ro + 12)?, f(ro + 16)?, f(ro + 20)?];
                        for k in 0..rn {
                            let p = u(ro + 24 + k * 4)?;
                            let (ux, uy, uz) = (p >> 21, (p >> 10) & 0x7ff, p & 0x3ff);
                            let x = (ux as f32 - 1023.0) / 1023.0 * rng[0] + mins[0];
                            let y = (uy as f32 - 1023.0) / 1023.0 * rng[1] + mins[1];
                            let z = (uz as f32 - 511.0) / 511.0 * rng[2] + mins[2];
                            tr.rot.push(wfrom(x, y, z));
                        }
                    }
                    // Homefront writes the 24-byte mins / ranges header before the keys of every
                    // multi-key rotation track, whatever the format (CHC_MPAnimations stand_idle:
                    // Float96NoW / Fixed48NoW tracks are 24 + n * key size bytes and their keys lie
                    // inside the header's intervals); Fixed48 / Float96 keys are absolute
                    "ACF_Fixed48NoW" => {
                        for k in 0..rn {
                            let o = ro + 24 + k * 6;
                            let c = |v: u16| (v as f32 - 32767.0) / 32767.0;
                            tr.rot.push(wfrom(c(h(o)?), c(h(o + 2)?), c(h(o + 4)?)));
                        }
                    }
                    _ => {
                        for k in 0..rn {
                            let o = ro + 24 + k * 12;
                            tr.rot.push(wfrom(f(o)?, f(o + 4)?, f(o + 8)?));
                        }
                    }
                }
            }
            tracks.push(tr);
        }
        let name = match props.get("SequenceName") {
            Some(Value::Name(s)) => s.clone(),
            _ => String::new(),
        };
        let length = match props.get("SequenceLength") {
            Some(Value::Float(v)) => *v,
            _ => 0.0,
        };
        let frames = match props.get("NumFrames") {
            Some(Value::Int(v)) => *v,
            _ => 0,
        };
        let rate = match props.get("RateScale") {
            Some(Value::Float(v)) => *v,
            _ => 1.0,
        };
        let mut sounds = Vec::new();
        let mut footsteps = Vec::new();
        for n in self.struct_array(&props, "Notifies") {
            let (Some(t), Some(o)) = (n.float("Time"), n.object("Notify")) else { continue };
            if o > 0 && self.export_class(&self.exports[o as usize - 1]) == "AnimNotify_Footstep" {
                footsteps.push(t);
                continue;
            }
            if o <= 0 || self.export_class(&self.exports[o as usize - 1]) != "AnimNotify_Sound" {
                continue;
            }
            if let Some(c) = self.export_props(o as usize - 1).and_then(|p| p.object("SoundCue")).filter(|&c| c != 0) {
                sounds.push((t, self.object_path(c)));
            }
        }
        Some(AnimSeq { name, length, frames, rate, tracks, sounds, footsteps })
    }

    /// AnimSet with all its sequences (children exports).
    pub fn read_anim_set(&self, export_index: usize) -> Option<AnimSetData> {
        let mut r = self.export_reader(export_index);
        let props = self.read_props(&mut r)?;
        let mut bones = Vec::new();
        if let Some(Value::Raw { data, .. }) = props.get("TrackBoneNames") {
            if data.len() >= 4 {
                let n = i32::from_le_bytes(data[0..4].try_into().unwrap()).max(0) as usize;
                for c in data[4..].chunks_exact(8).take(n) {
                    bones.push(self.name(crate::FName { index: i32::from_le_bytes(c[0..4].try_into().unwrap()), number: i32::from_le_bytes(c[4..8].try_into().unwrap()) }));
                }
            }
        }
        let me = export_index as i32 + 1;
        let seqs = self
            .exports
            .iter()
            .enumerate()
            .filter(|(_, e)| e.outer_index == me && self.export_class(e) == "AnimSequence")
            .filter_map(|(i, _)| self.read_anim_seq(i))
            .collect();
        Some(AnimSetData { path: self.object_path(me), bones, seqs })
    }
}
