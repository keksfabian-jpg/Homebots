//! SoundCue graphs and SoundNodeWave audio (PC: Ogg Vorbis in the CompressedPCData bulk).
//!
//! Node semantics follow UE3 (SoundNode*.uc): Mixer plays all children with InputVolume,
//! Random picks one by Weights, Modulator scales volume / pitch by a uniform value of its
//! distributions, Delay waits DelayDuration, Attenuation fades out to RadiusMax. Game nodes
//! (GCSoundNodeStereoToMono / MonoToSpeaker / SoundMode / Smoothing / Repeater) pass their first
//! child; GCSoundNodeSwitch picks a child by an instance parameter (PlayFireRelease, Inside, ...).
//! RawDistributionFloat LookupTable: [min, max, ...] (constants: min = max).
use crate::props::{Props, Rd, Value};
use crate::Package;

#[derive(Debug, Clone)]
pub enum SoundNode {
    /// object path of the SoundNodeWave
    Wave(String),
    Random(Vec<(f32, SoundNode)>),
    Mixer(Vec<(f32, SoundNode)>),
    Modulator { volume: (f32, f32), pitch: (f32, f32), child: Box<SoundNode> },
    Delay { delay: (f32, f32), child: Box<SoundNode> },
    /// MinRadius / MaxRadius (RawDistributionFloat, class defaults 400 / 5000), DistanceModel
    /// (0 Linear, 1 Logarithmic, 2 Inverse, 3 LogReverse, 4 NaturalSound), dBAttenuationAtMax
    Attenuation { radius_min: f32, radius_max: f32, model: u8, db_at_max: f32, child: Box<SoundNode> },
    /// SoundNodeLooping: bLoopIndefinitely (default) or LoopCount repeats
    Looping { forever: bool, count: f32, child: Box<SoundNode> },
    /// GCSoundNodeSwitch: child chosen by the named instance parameter (set by native code)
    Switch { param: String, children: Vec<SoundNode> },
    /// SoundNodeDistanceCrossFade: per child (FadeInDistance min / max, FadeOutDistance min / max,
    /// Volume): silent closer than the fade-in, rising over it, full until the fade-out, falling
    /// over it (UE units from the listener)
    CrossFade(Vec<((f32, f32), (f32, f32), f32, SoundNode)>),
    /// SoundNodeConcatenator: the children one after another (InputVolume each)
    Concat(Vec<(f32, SoundNode)>),
    /// SoundNodeWaveParam: the wave the playing instance passes under WaveParameterName
    /// (GCSoundCueInstance DefaultParameters), else its child
    WaveParam { name: String, child: Box<SoundNode> },
    Empty,
}

#[derive(Debug, Clone)]
pub struct SoundCueData {
    pub volume: f32,
    pub pitch: f32,
    pub root: SoundNode,
    /// MaxConcurrentPlayCount (0 = unlimited)
    pub max_concurrent: i32,
    /// SoundGroup (sound class, volumes in [Engine.AudioDevice] SoundGroups)
    pub group: String,
}

fn floats(v: Option<&Value>) -> Vec<f32> {
    match v {
        Some(Value::Raw { data, .. }) if data.len() >= 4 => {
            let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
            data[4..].chunks_exact(4).take(n).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
        }
        _ => Vec::new(),
    }
}

fn objs(v: Option<&Value>) -> Vec<i32> {
    match v {
        Some(Value::Raw { data, .. }) if data.len() >= 4 => {
            let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
            data[4..].chunks_exact(4).take(n).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()
        }
        _ => Vec::new(),
    }
}

impl Package {
    fn raw_dist(&self, pr: &Props, name: &str, default: f32) -> (f32, f32) {
        let Some(s) = self.struct_prop(pr, name) else { return (default, default) };
        let t = floats(s.get("LookupTable"));
        match t.len() {
            0 => (default, default),
            1 => (t[0], t[0]),
            _ => (t[0].min(t[1]), t[0].max(t[1])),
        }
    }

    fn sound_node(&self, pkg: &str, obj: i32, depth: usize) -> SoundNode {
        if obj == 0 || depth > 32 {
            return SoundNode::Empty;
        }
        if obj < 0 {
            // a wave in another package (or a node we cannot read)
            return SoundNode::Wave(self.object_path(obj));
        }
        let i = obj as usize - 1;
        let class = self.export_class(&self.exports[i]);
        if class == "SoundNodeWave" {
            // cooked into a map (pkg ""): the path already starts with the package
            return SoundNode::Wave(if pkg.is_empty() { self.object_path(obj) } else { format!("{pkg}.{}", self.object_path(obj)) });
        }
        let Some(pr) = self.export_props(i) else { return SoundNode::Empty };
        let kids: Vec<i32> = objs(pr.get("ChildNodes"));
        let child = |k: usize| kids.get(k).map(|&c| self.sound_node(pkg, c, depth + 1)).unwrap_or(SoundNode::Empty);
        match class.as_str() {
            "SoundNodeMixer" => {
                let v = floats(pr.get("InputVolume"));
                SoundNode::Mixer(kids.iter().enumerate().map(|(k, &c)| (v.get(k).copied().unwrap_or(1.0), self.sound_node(pkg, c, depth + 1))).collect())
            }
            "SoundNodeRandom" => {
                let w = floats(pr.get("Weights"));
                SoundNode::Random(kids.iter().enumerate().map(|(k, &c)| (w.get(k).copied().unwrap_or(1.0), self.sound_node(pkg, c, depth + 1))).collect())
            }
            "SoundNodeDistanceCrossFade" => {
                let inputs = self.struct_array(&pr, "CrossFadeInput");
                SoundNode::CrossFade(
                    kids.iter()
                        .enumerate()
                        .map(|(k, &c)| {
                            let inp = inputs.get(k);
                            let fi = inp.map(|i| self.raw_dist(i, "FadeInDistance", 0.0)).unwrap_or((0.0, 0.0));
                            let fo = inp.map(|i| self.raw_dist(i, "FadeOutDistance", 1.0e9)).unwrap_or((1.0e9, 1.0e9));
                            let v = inp.and_then(|i| i.float("Volume")).unwrap_or(1.0);
                            (fi, fo, v, self.sound_node(pkg, c, depth + 1))
                        })
                        .collect(),
                )
            }
            "SoundNodeConcatenator" => {
                let v = floats(pr.get("InputVolume"));
                SoundNode::Concat(kids.iter().enumerate().map(|(k, &c)| (v.get(k).copied().unwrap_or(1.0), self.sound_node(pkg, c, depth + 1))).collect())
            }
            "SoundNodeWaveParam" => SoundNode::WaveParam { name: pr.enum_name("WaveParameterName").unwrap_or("").to_string(), child: Box::new(child(0)) },
            "SoundNodeModulator" => SoundNode::Modulator { volume: self.raw_dist(&pr, "VolumeModulation", 1.0), pitch: self.raw_dist(&pr, "PitchModulation", 1.0), child: Box::new(child(0)) },
            "SoundNodeDelay" => SoundNode::Delay { delay: self.raw_dist(&pr, "DelayDuration", 0.0), child: Box::new(child(0)) },
            "SoundNodeAttenuation" | "SoundNodeAttenuationAndGain" => {
                let model = match pr.get("DistanceModel") {
                    Some(Value::Enum(e)) => match e.as_str() {
                        "ATTENUATION_Logarithmic" => 1,
                        "ATTENUATION_Inverse" => 2,
                        "ATTENUATION_LogReverse" => 3,
                        "ATTENUATION_NaturalSound" => 4,
                        _ => 0,
                    },
                    _ => 0,
                };
                let bt = pr.get("bAttenuate").map(|v| !matches!(v, Value::Bool(false))).unwrap_or(true);
                let (rmin, rmax) = if bt { (self.raw_dist(&pr, "MinRadius", 400.0).1, self.raw_dist(&pr, "MaxRadius", 5000.0).1) } else { (1.0e9, 2.0e9) };
                SoundNode::Attenuation { radius_min: rmin, radius_max: rmax, model, db_at_max: pr.float("dBAttenuationAtMax").unwrap_or(-60.0), child: Box::new(child(0)) }
            }
            "SoundNodeLooping" => {
                let forever = !matches!(pr.get("bLoopIndefinitely"), Some(Value::Bool(false)));
                SoundNode::Looping { forever, count: self.raw_dist(&pr, "LoopCount", 1_000_000.0).1, child: Box::new(child(0)) }
            }
            // ParameterName missing = the class default (GCGame.u Default__GCSoundNodeSwitch: "BehindView")
            "GCSoundNodeSwitch" => SoundNode::Switch { param: pr.enum_name("ParameterName").unwrap_or("BehindView").to_string(), children: kids.iter().map(|&c| self.sound_node(pkg, c, depth + 1)).collect() },
            _ => child(0),
        }
    }

    /// `pkg`: this package's name (waves inside it are returned as "pkg.Group.Name"); "" for
    /// objects cooked into a map, whose paths carry their package already.
    pub fn read_sound_cue(&self, export_index: usize, pkg: &str) -> Option<SoundCueData> {
        let pr = self.export_props(export_index)?;
        let root = self.sound_node(pkg, pr.object("FirstNode").unwrap_or(0), 0);
        Some(SoundCueData { volume: pr.float("VolumeMultiplier").unwrap_or(1.0), pitch: pr.float("PitchMultiplier").unwrap_or(1.0), root, max_concurrent: pr.int("MaxConcurrentPlayCount").unwrap_or(16), group: pr.enum_name("SoundGroup").unwrap_or("").to_string() })
    }

    /// GCSoundCueInstance (the voice lines of KAOSAIChatter utterances): its master cue (object
    /// index), the wave parameters (name, wave object path), VolumeMultiplier and SoundGroup.
    pub fn read_cue_instance(&self, export_index: usize) -> Option<(i32, Vec<(String, String)>, f32, String)> {
        let pr = self.export_props(export_index)?;
        let cue = pr.object("Cue").filter(|&o| o != 0)?;
        let params = self
            .struct_array(&pr, "DefaultParameters")
            .into_iter()
            .filter_map(|st| {
                let name = st.enum_name("ParamName")?.to_string();
                let w = st.object("WaveParam").filter(|&o| o != 0)?;
                Some((name, self.object_path(w)))
            })
            .collect();
        Some((cue, params, pr.float("VolumeMultiplier").unwrap_or(1.0), pr.enum_name("SoundGroup").unwrap_or("").to_string()))
    }

    /// Duration of a SoundNodeWave.
    pub fn wave_duration(&self, export_index: usize) -> Option<f32> {
        self.export_props(export_index)?.float("Duration")
    }

    /// Ogg Vorbis stream of a SoundNodeWave (CompressedPCData: the bulk header's size precedes
    /// the "OggS" magic by 8 bytes) and its Duration.
    pub fn read_wave_ogg(&self, export_index: usize) -> Option<(Vec<u8>, f32)> {
        let e = &self.exports[export_index];
        let mut r = self.export_reader(export_index);
        let pr = self.read_props(&mut r)?;
        let b = self.export_bytes(e);
        let start = r.pos;
        let rel = b[start..].windows(4).position(|w| w == b"OggS")?;
        let pos = start + rel;
        let size = u32::from_le_bytes(b.get(pos - 8..pos - 4)?.try_into().ok()?) as usize;
        let data = b.get(pos..pos + size)?.to_vec();
        let _ = Rd::new(&[]);
        Some((data, pr.float("Duration").unwrap_or(0.0)))
    }
}
