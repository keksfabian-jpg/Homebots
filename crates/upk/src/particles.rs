//! UE3 Cascade particle systems as cooked by Homefront (ParticleSystem -> ParticleSpriteEmitter
//! -> ParticleLODLevel -> modules). Distributions are cooked into FRawDistribution lookup tables
//! (Op 1 = constant/curve, 2 = random between two values, 3 = extreme), table = [range min,
//! range max, entries ...], entry k at 2 + k * ChunkSize, index = (t - StartTime) * TimeScale.
//! Properties equal to the class default are not serialised: values (also single fields of a
//! RawDistribution struct) come from Default__<Class> in Engine.u / GCGame.u.

use crate::props::{Props, Value};
use crate::Package;

#[derive(Debug, Clone, Default)]
pub struct RawDist {
    pub op: u8,
    pub n: usize,
    pub chunk: usize,
    pub lut: Vec<f32>,
    pub time_scale: f32,
    pub start_time: f32,
    /// Distribution*ParticleParameter (no lookup table): instance parameter name and its
    /// Constant (DPM_Direct), e.g. the tracers' TracerSize / TracerColor / TracerSpeed
    pub param: Option<(String, [f32; 3])>,
}

impl RawDist {
    fn entries(&self, t: f32) -> (usize, usize, f32) {
        let x = ((t - self.start_time) * self.time_scale).max(0.0);
        let i = x as usize;
        let a = x - i as f32;
        let n = self.n.max(1);
        let i1 = i.min(n - 1);
        let i2 = (i + 1).min(n - 1);
        (2 + i1 * self.chunk, 2 + i2 * self.chunk, a)
    }
    fn at(&self, k: usize) -> f32 {
        self.lut.get(k).copied().unwrap_or(0.0)
    }
    pub fn float(&self, t: f32, rand: &mut dyn FnMut() -> f32) -> f32 {
        if self.lut.len() < 3 {
            return 0.0;
        }
        let (e1, e2, a) = self.entries(t);
        let l = |k: usize| self.at(e1 + k) + (self.at(e2 + k) - self.at(e1 + k)) * a;
        match self.op {
            2 => {
                let r = rand();
                l(0) + (l(1) - l(0)) * r
            }
            3 => {
                let k = if rand() > 0.5 { 1 } else { 0 };
                l(k)
            }
            _ => l(0),
        }
    }
    /// `vec3` with component instance parameters for parameter distributions.
    pub fn vec3_p(&self, t: f32, rand: &mut dyn FnMut() -> f32, params: &[(String, [f32; 3])]) -> [f32; 3] {
        if let (true, Some((name, c))) = (self.lut.len() < 5, &self.param) {
            return params.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|x| x.1).unwrap_or(*c);
        }
        self.vec3(t, rand)
    }
    pub fn vec3(&self, t: f32, rand: &mut dyn FnMut() -> f32) -> [f32; 3] {
        if self.lut.len() < 5 {
            return self.param.as_ref().map(|p| p.1).unwrap_or([0.0; 3]);
        }
        let (e1, e2, a) = self.entries(t);
        let l = |k: usize| self.at(e1 + k) + (self.at(e2 + k) - self.at(e1 + k)) * a;
        match self.op {
            2 => {
                let r = [rand(), rand(), rand()];
                [0, 1, 2].map(|c| l(c) + (l(3 + c) - l(c)) * r[c])
            }
            3 => {
                let k = if rand() > 0.5 { 3 } else { 0 };
                [l(k), l(k + 1), l(k + 2)]
            }
            _ => [l(0), l(1), l(2)],
        }
    }
}

#[derive(Debug, Clone)]
pub enum Module {
    Lifetime(RawDist),
    /// GCParticleModuleLifetimePerUnit (Homefront, native): LifetimePerUnit distribution, e.g.
    /// KFX_VHC FX_WreckSmoke_Medium 1.5 s at 0 falling to 0.15 over 0..~1800 (TimeScale 0.033),
    /// Fire_Damage1 1.2 -> 0.25 over 0..1200. The vehicle damage / wreck smoke emitters have it
    /// instead of ParticleModuleLifetime (Tank_DamageSmoke, OA_*_RearSmoke, *_Chassis_WreckFire).
    LifetimePerUnit(RawDist),
    Location(RawDist),
    Velocity { start: RawDist, radial: RawDist, world_space: bool },
    Size(RawDist),
    Color { color: RawDist, alpha: RawDist, clamp_alpha: bool },
    ColorOverLife { color: RawDist, alpha: RawDist, clamp_alpha: bool },
    ColorScaleOverLife { color: RawDist, alpha: RawDist, emitter_time: bool },
    SizeMultiplyLife { mult: RawDist, axes: [bool; 3] },
    Rotation(RawDist),
    RotationRate(RawDist),
    VelocityOverLifetime { vel: RawDist, absolute: bool },
    Acceleration(RawDist),
    KillHeight { height: RawDist, floor: bool, absolute: bool },
    SubUV(RawDist),
    /// ParticleModuleColorByParameter: component instance parameter `param` (colour), else DefaultColor (RGBA)
    ColorByParameter { param: String, default: [u8; 4] },
    Other(String),
}

/// A placed Emitter actor.
#[derive(Debug, Clone)]
pub struct EmitterPlacement {
    pub template: i32,
    pub location: [f32; 3],
    pub rotation: [i32; 3],
    /// DrawScale and DrawScale3D
    pub scale: f32,
    pub scale3d: [f32; 3],
    /// component InstanceParameters: (name, Color RGBA)
    pub colors: Vec<(String, [u8; 4])>,
    /// distance cull (UE units, 0 = none): the smaller non-zero value of the component's
    /// MaxDrawDistance (set by the LD, e.g. 800 on every Angel Island GRay_Beams_B god ray) and
    /// CachedMaxDrawDistance (cooked, 1000000 on all Angel_FX emitters)
    pub max_draw_distance: f32,
}

/// GCParticleModuleTypeDataTrail (Homefront): the emitter's particles joined into a ribbon drawn
/// with TrailMaterial; bRenderSprites also draws them as sprites with the emitter material.
#[derive(Debug, Clone)]
pub struct TrailDef {
    pub material: i32,
    pub render_sprites: bool,
    pub u_tiling: f32,
    pub sheets: i32,
    pub sprite_frequency: i32,
    pub v_size: f32,
    pub alpha_tile: f32,
}

#[derive(Debug, Clone)]
pub struct Emitter {
    pub name: String,
    /// mesh emitters (TypeDataMesh) are not drawn as sprites
    pub mesh: bool,
    /// trail emitters (GCParticleModuleTypeDataTrail)
    pub trail: Option<TrailDef>,
    pub material: i32,
    pub screen_alignment: String,
    pub sub_h: u32,
    pub sub_v: u32,
    pub interpolation: String,
    pub local_space: bool,
    pub duration: f32,
    pub duration_low: f32,
    pub duration_use_range: bool,
    pub loops: i32,
    pub delay: f32,
    pub rate: RawDist,
    pub rate_scale: RawDist,
    /// (count, count low (-1 = none), time as fraction of the duration)
    pub bursts: Vec<(i32, i32, f32)>,
    pub modules: Vec<Module>,
}

/// Default__<Class> objects of the script packages.
pub struct ClassDefaults {
    pkgs: Vec<Package>,
    index: std::collections::HashMap<String, (usize, usize)>,
}

impl ClassDefaults {
    pub fn new(lib: &crate::library::Library) -> Self {
        let pkgs: Vec<Package> = ["Engine", "GCGame"].iter().filter_map(|n| lib.open(n)).collect();
        // class default objects by name, first package wins (a linear search over GCGame's 64k
        // exports per particle module made the particle template reads take seconds)
        let mut index = std::collections::HashMap::new();
        for (pi, p) in pkgs.iter().enumerate() {
            for (i, e) in p.exports.iter().enumerate() {
                let n = p.object_name_of(e);
                if n.starts_with("Default__") {
                    index.entry(n).or_insert((pi, i));
                }
            }
        }
        ClassDefaults { pkgs, index }
    }
    fn get(&self, class: &str) -> Option<(&Package, Props)> {
        let &(pi, i) = self.index.get(&format!("Default__{class}"))?;
        let p = &self.pkgs[pi];
        p.export_props(i).map(|pr| (p, pr))
    }
}

/// A RawDistribution struct property. If the instance carries no lookup table the whole struct
/// comes from the class default object. Entry count = (table length - 2) / chunk size, chunk
/// size 1 (float) / 3 (vector) unless serialised: consistent with every cooked table here (e.g.
/// AlphaOverLife: 23 floats, chunk 1 -> 21 entries over TimeScale 20), while the serialised
/// LookupTableNumElements is often left at the default 1.
fn raw_dist(pkg: &Package, own: Option<&Value>, def: Option<(&Package, &Value)>, vector: bool) -> RawDist {
    let own = own.and_then(|v| pkg.struct_props_pub(v));
    let def = def.and_then(|(p, v)| p.struct_props_pub(v));
    let src = match &own {
        Some(o) if o.get("LookupTable").is_some() => o.clone(),
        _ => match def {
            Some(d) => d,
            None => return RawDist::default(),
        },
    };
    let byte = |n: &str| match src.get(n) {
        Some(Value::Byte(b)) => Some(*b),
        Some(Value::Int(i)) => Some(*i as u8),
        _ => None,
    };
    let lut: Vec<f32> = match src.get("LookupTable") {
        Some(Value::Raw { data, .. }) if data.len() >= 4 => {
            let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
            (0..n).filter_map(|k| data.get(4 + 4 * k..8 + 4 * k).map(|b| f32::from_le_bytes(b.try_into().unwrap()))).collect()
        }
        _ => Vec::new(),
    };
    let op = byte("Op").unwrap_or(1);
    let chunk = byte("LookupTableChunkSize").map(|c| c as usize).filter(|&c| c > 0).unwrap_or(if vector { 3 } else { 1 });
    let n = (lut.len().saturating_sub(2) / chunk).max(1);
    // parameter distributions are not baked into the table: name and Constant from the object
    let param = if lut.len() < 3 {
        src.object("Distribution").filter(|&o| o > 0).and_then(|o| {
            let dp = pkg.export_props(o as usize - 1)?;
            let name = match dp.get("ParameterName") {
                Some(Value::Name(n)) => n.clone(),
                _ => return None,
            };
            let c = dp.vector("Constant").or_else(|| dp.float("Constant").map(|f| [f, f, f])).unwrap_or([0.0; 3]);
            Some((name, c))
        })
    } else {
        None
    };
    // not baked (bCanBeBaked false, e.g. ST_ATW_Rocket's smoke ColorOverLife: an empty table,
    // the colour read as black): evaluate the Distribution object itself
    if lut.len() < 3 && param.is_none() {
        if let Some(b) = src.object("Distribution").filter(|&o| o > 0).and_then(|o| bake_distribution(pkg, o, vector)) {
            return b;
        }
    }
    RawDist { op, n, chunk, lut, time_scale: src.float("LookupTableTimeScale").unwrap_or(0.0), start_time: src.float("LookupTableStartTime").unwrap_or(0.0), param }
}

/// A Distribution{Float,Vector}{Constant,Uniform,ConstantCurve} object as a lookup table
/// (curves sampled linearly between their points, 32 entries; tangents ignored).
fn bake_distribution(pkg: &Package, obj: i32, vector: bool) -> Option<RawDist> {
    let class = pkg.export_class(&pkg.exports[obj as usize - 1]);
    let pr = pkg.export_props(obj as usize - 1).unwrap_or_default();
    let w = if vector { 3 } else { 1 };
    let val = |v: Option<&Value>| -> Vec<f32> {
        match v {
            Some(Value::Vector(x)) => x.to_vec(),
            Some(Value::Float(f)) => vec![*f; w],
            _ => vec![0.0; w],
        }
    };
    if class.ends_with("ConstantCurve") {
        // InterpCurveFloat / InterpCurveVector serialise natively: Points (count, then per point
        // InVal, OutVal, ArriveTangent, LeaveTangent, InterpMode as FName), InterpMethod
        let Some(Value::Raw { data, .. }) = pr.get("ConstantCurve") else { return None };
        let rd = |o: usize| data.get(o..o + 4).map(|b| f32::from_le_bytes(b.try_into().unwrap()));
        let count = data.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)?;
        let stride = 4 + 3 * 4 * w + 8;
        let mut pts: Vec<(f32, Vec<f32>)> = Vec::new();
        for k in 0..count.min(64) {
            let o = 4 + k * stride;
            let Some(t) = rd(o) else { break };
            let v: Option<Vec<f32>> = (0..w).map(|c| rd(o + 4 + 4 * c)).collect();
            let Some(v) = v else { break };
            pts.push((t, v));
        }
        if pts.is_empty() {
            return None;
        }
        let (t0, t1) = (pts[0].0, pts[pts.len() - 1].0);
        let n = if t1 > t0 { 32 } else { 1 };
        let mut lut = vec![0.0, 0.0];
        for k in 0..n {
            let t = if n > 1 { t0 + (t1 - t0) * k as f32 / (n - 1) as f32 } else { t0 };
            let j = pts.iter().rposition(|p| p.0 <= t).unwrap_or(0);
            let (a, b) = (&pts[j], &pts[(j + 1).min(pts.len() - 1)]);
            let f = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
            for c in 0..w {
                lut.push(a.1[c] + (b.1[c] - a.1[c]) * f);
            }
        }
        let ts = if n > 1 { (n - 1) as f32 / (t1 - t0) } else { 0.0 };
        return Some(RawDist { op: 1, n, chunk: w, lut, time_scale: ts, start_time: t0, param: None });
    }
    if class.ends_with("Uniform") {
        let (lo, hi) = (val(pr.get("Min")), val(pr.get("Max")));
        let mut lut = vec![0.0, 0.0];
        lut.extend(lo);
        lut.extend(hi);
        return Some(RawDist { op: 2, n: 1, chunk: 2 * w, lut, time_scale: 0.0, start_time: 0.0, param: None });
    }
    if class.ends_with("Constant") {
        let mut lut = vec![0.0, 0.0];
        lut.extend(val(pr.get("Constant")));
        if !vector {
            lut.push(lut[2]);
        }
        return Some(RawDist { op: 1, n: 1, chunk: w, lut, time_scale: 0.0, start_time: 0.0, param: None });
    }
    None
}

impl Package {
    /// Sprite emitters (LOD 0) of a ParticleSystem export.
    pub fn read_particle_system(&self, idx: usize, defs: &ClassDefaults) -> Option<Vec<Emitter>> {
        let sys = self.export_props(idx)?;
        let mut out = Vec::new();
        for e in sys.get("Emitters").map(|v| self.array_objects(v)).unwrap_or_default().into_iter().filter(|&e| e > 0) {
            let ep = self.export_props(e as usize - 1)?;
            if matches!(ep.get("bIsSoloing"), Some(Value::Bool(true))) {
                continue;
            }
            let Some(lod) = ep.get("LODLevels").map(|v| self.array_objects(v)).and_then(|v| v.into_iter().next()).filter(|&l| l > 0) else { continue };
            let lp = self.export_props(lod as usize - 1)?;
            if matches!(lp.get("bEnabled"), Some(Value::Bool(false))) {
                continue;
            }
            // module properties with class defaults
            let module = |o: i32| -> Option<(String, Props, Option<(&Package, Props)>)> {
                if o <= 0 {
                    return None;
                }
                let class = self.export_class(&self.exports[o as usize - 1]);
                let p = self.export_props(o as usize - 1)?;
                let d = defs.get(&class);
                Some((class, p, d))
            };
            let dist = |p: &Props, d: &Option<(&Package, Props)>, n: &str| {
                let vector = matches!(p.get(n).or_else(|| d.as_ref().and_then(|(_, dp)| dp.get(n))), Some(Value::Raw { struct_name: Some(s), .. }) if s.contains("Vector"));
                raw_dist(self, p.get(n), d.as_ref().and_then(|(dp, dpr)| dpr.get(n).map(|v| (*dp, v))), vector)
            };
            let boolp = |p: &Props, d: &Option<(&Package, Props)>, n: &str, dflt: bool| match p.get(n).or_else(|| d.as_ref().and_then(|(_, dp)| dp.get(n))) {
                Some(Value::Bool(b)) => *b,
                _ => dflt,
            };
            let (_, rq, rqd) = module(lp.object("RequiredModule")?)?;
            let gf = |n: &str, dflt: f32| rq.float(n).or_else(|| rqd.as_ref().and_then(|(_, d)| d.float(n))).unwrap_or(dflt);
            let gi = |n: &str, dflt: i32| rq.int(n).or_else(|| rqd.as_ref().and_then(|(_, d)| d.int(n))).unwrap_or(dflt);
            let ge = |n: &str, dflt: &str| rq.enum_name(n).or_else(|| rqd.as_ref().and_then(|(_, d)| d.enum_name(n))).unwrap_or(dflt).to_string();
            let (rate, rate_scale, bursts) = match lp.object("SpawnModule").and_then(module) {
                Some((_, sp, sd)) => {
                    let bursts = sp
                        .get("BurstList")
                        .map(|v| self.array_structs(v))
                        .unwrap_or_default()
                        .iter()
                        .map(|b| (b.int("Count").unwrap_or(0), b.int("CountLow").unwrap_or(-1), b.float("Time").unwrap_or(0.0)))
                        .collect();
                    (dist(&sp, &sd, "Rate"), dist(&sp, &sd, "RateScale"), bursts)
                }
                None => (dist(&rq, &rqd, "SpawnRate"), RawDist { op: 1, n: 1, chunk: 1, lut: vec![1.0, 1.0, 1.0, 1.0], ..Default::default() }, Vec::new()),
            };
            let mut modules = Vec::new();
            for m in lp.get("Modules").map(|v| self.array_objects(v)).unwrap_or_default() {
                let Some((class, p, d)) = module(m) else { continue };
                if !boolp(&p, &d, "bEnabled", true) {
                    continue;
                }
                modules.push(match class.as_str() {
                    "ParticleModuleLifetime" => Module::Lifetime(dist(&p, &d, "Lifetime")),
                    "GCParticleModuleLifetimePerUnit" => Module::LifetimePerUnit(dist(&p, &d, "LifetimePerUnit")),
                    "ParticleModuleLocation" => Module::Location(dist(&p, &d, "StartLocation")),
                    "ParticleModuleVelocity" => Module::Velocity { start: dist(&p, &d, "StartVelocity"), radial: dist(&p, &d, "StartVelocityRadial"), world_space: boolp(&p, &d, "bInWorldSpace", false) },
                    "ParticleModuleSize" => Module::Size(dist(&p, &d, "StartSize")),
                    "ParticleModuleColor" => Module::Color { color: dist(&p, &d, "StartColor"), alpha: dist(&p, &d, "StartAlpha"), clamp_alpha: boolp(&p, &d, "bClampAlpha", true) },
                    "ParticleModuleColorOverLife" => Module::ColorOverLife { color: dist(&p, &d, "ColorOverLife"), alpha: dist(&p, &d, "AlphaOverLife"), clamp_alpha: boolp(&p, &d, "bClampAlpha", true) },
                    "ParticleModuleColorScaleOverLife" => Module::ColorScaleOverLife {
                        color: dist(&p, &d, "ColorScaleOverLife"),
                        alpha: dist(&p, &d, "AlphaScaleOverLife"),
                        emitter_time: boolp(&p, &d, "bEmitterTime", false),
                    },
                    "ParticleModuleSizeMultiplyLife" => Module::SizeMultiplyLife {
                        mult: dist(&p, &d, "LifeMultiplier"),
                        axes: [boolp(&p, &d, "MultiplyX", true), boolp(&p, &d, "MultiplyY", true), boolp(&p, &d, "MultiplyZ", true)],
                    },
                    "ParticleModuleRotation" => Module::Rotation(dist(&p, &d, "StartRotation")),
                    "ParticleModuleRotationRate" => Module::RotationRate(dist(&p, &d, "StartRotationRate")),
                    "ParticleModuleVelocityOverLifetime" => Module::VelocityOverLifetime { vel: dist(&p, &d, "VelOverLife"), absolute: boolp(&p, &d, "Absolute", false) },
                    "ParticleModuleAcceleration" => Module::Acceleration(dist(&p, &d, "Acceleration")),
                    "ParticleModuleKillHeight" => Module::KillHeight { height: dist(&p, &d, "Height"), floor: boolp(&p, &d, "bFloor", false), absolute: boolp(&p, &d, "bAbsolute", false) },
                    "ParticleModuleSubUV" => Module::SubUV(dist(&p, &d, "SubImageIndex")),
                    "ParticleModuleColorByParameter" => Module::ColorByParameter {
                        param: p.enum_name("ColorParam").or_else(|| d.as_ref().and_then(|(_, dp)| dp.enum_name("ColorParam"))).unwrap_or("").to_string(),
                        default: p.color("DefaultColor").or_else(|| d.as_ref().and_then(|(_, dp)| dp.color("DefaultColor"))).unwrap_or([0; 4]),
                    },
                    other => Module::Other(other.to_string()),
                });
            }
            let td = lp.object("TypeDataModule").filter(|&t| t > 0).and_then(module);
            let trail = td.as_ref().filter(|(c, _, _)| c == "GCParticleModuleTypeDataTrail").map(|(_, p, d)| {
                let f = |n: &str, dflt: f32| p.float(n).or_else(|| d.as_ref().and_then(|(_, dp)| dp.float(n))).unwrap_or(dflt);
                let i = |n: &str, dflt: i32| p.int(n).or_else(|| d.as_ref().and_then(|(_, dp)| dp.int(n))).unwrap_or(dflt);
                TrailDef {
                    material: p.object("TrailMaterial").unwrap_or(0),
                    render_sprites: boolp(p, d, "bRenderSprites", true),
                    u_tiling: f("UTiling", 1.0),
                    sheets: i("Sheets", 4),
                    sprite_frequency: i("SpriteFrequency", 1),
                    v_size: f("VSize", 1.0),
                    alpha_tile: f("AlphaTile", 1.0),
                }
            });
            out.push(Emitter {
                name: ep.enum_name("EmitterName").unwrap_or("").to_string(),
                mesh: td.is_some() && trail.is_none(),
                trail,
                material: rq.object("Material").unwrap_or(0),
                screen_alignment: ge("ScreenAlignment", "PSA_Square"),
                sub_h: gi("SubImages_Horizontal", 1).max(1) as u32,
                sub_v: gi("SubImages_Vertical", 1).max(1) as u32,
                interpolation: ge("InterpolationMethod", "PSUVIM_None"),
                local_space: boolp(&rq, &rqd, "bUseLocalSpace", false),
                duration: gf("EmitterDuration", 1.0),
                duration_low: gf("EmitterDurationLow", 0.0),
                duration_use_range: boolp(&rq, &rqd, "EmitterDurationUseRange", false),
                loops: gi("EmitterLoops", 0),
                delay: gf("EmitterDelay", 0.0),
                rate,
                rate_scale,
                bursts,
                modules,
            });
        }
        Some(out)
    }

    /// Placed Emitter actors.
    pub fn emitter_placements(&self) -> Vec<EmitterPlacement> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            if !self.export_class(e).contains("Emitter") || !self.object_path(i as i32 + 1).starts_with("TheWorld.") {
                continue;
            }
            let Some(ap) = self.export_props(i) else { continue };
            if matches!(ap.get("bHidden"), Some(Value::Bool(true))) {
                continue;
            }
            let Some(pc) = ap.object("ParticleSystemComponent").filter(|&x| x > 0) else { continue };
            let Some(cp) = self.export_props(pc as usize - 1) else { continue };
            if matches!(cp.get("bAutoActivate"), Some(Value::Bool(false))) {
                continue;
            }
            let Some(t) = cp.object("Template").filter(|&t| t != 0) else { continue };
            let colors = cp
                .get("InstanceParameters")
                .map(|v| self.array_structs(v))
                .unwrap_or_default()
                .iter()
                .filter_map(|ip| Some((ip.enum_name("Name")?.to_string(), ip.color("Color").unwrap_or([0; 4]))))
                .collect();
            out.push(EmitterPlacement {
                template: t,
                location: ap.vector("Location").unwrap_or([0.0; 3]),
                rotation: ap.rotator("Rotation").unwrap_or([0; 3]),
                scale: ap.float("DrawScale").unwrap_or(1.0),
                scale3d: ap.vector("DrawScale3D").unwrap_or([1.0; 3]),
                colors,
                max_draw_distance: [cp.float("MaxDrawDistance").unwrap_or(0.0), cp.float("CachedMaxDrawDistance").unwrap_or(0.0)].into_iter().filter(|&d| d > 0.0).fold(0.0, |m: f32, d| if m == 0.0 { d } else { m.min(d) }),
            });
        }
        out
    }
}
