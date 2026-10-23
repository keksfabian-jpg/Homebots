//! Cascade sprite emitters (placed Emitter actors, e.g. the Overpass fires), simulated on the CPU
//! like UE3's FParticleSpriteEmitterInstance and drawn as camera-facing quads with the emitter's
//! material (graph-compiled GameMat, particle colour in the vertex colour).
//!
//! Sprite orientation from the game's FParticleSubUVVertexFactory base-pass VS (shader cache):
//! offset = (u - 0.5) * Size.x * R + (v - 0.5) * Size.y * U with, for PSA_Square,
//! R = sin(rot) * CameraRight - cos(rot) * CameraUp, U = cos(rot) * CameraRight + sin(rot) * CameraUp
//! (Size.y = Size.x; the engine sets CameraRight = -view up, CameraUp = -view right, so rotation 0
//! is upright with v = 0 at the top), and for PSA_Velocity R = normalize(cross(dir to camera, velocity dir)),
//! U = -velocity dir. Module semantics: UE3 Cascade (not yet checked against a trace).
//! Not drawn: mesh emitters (TypeDataMesh, the "FakeLight" glow spheres).

use bevy::{asset::RenderAssetUsages, prelude::*, render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues}};
use upk::particles::{Emitter, Module};

struct Particle {
    pos: Vec3,
    old: Vec3,
    vel: Vec3,
    base_vel: Vec3,
    size: Vec3,
    base_size: Vec3,
    color: Vec4,
    base_color: Vec4,
    rot: f32,
    rot_rate: f32,
    base_rot_rate: f32,
    rt: f32,
    inv_life: f32,
    sub_start: f32,
    sub_image: f32,
    /// spawn order (trail ribbons join the particles in this order)
    seq: u32,
}

pub struct EmitterState {
    def: Emitter,
    time: f32,
    loops_done: i32,
    duration: f32,
    spawn_frac: f32,
    particles: Vec<Particle>,
    done: bool,
    pub mesh: Handle<Mesh>,
    /// the mesh holds no particle (written empty): no rewrite until particles exist again
    empty: bool,
    /// trail emitters: ribbon mesh (TrailMaterial), last spawn point, spawn counter
    pub trail_mesh: Option<Handle<Mesh>>,
    trail_empty: bool,
    last_spawn: Option<Vec3>,
    seq: u32,
    /// released trail emitter: one more step spawns the last stretch up to the end point
    finish: bool,
}

pub struct SystemInstance {
    /// UE world location / rotation of the emitter actor
    pub location: Vec3,
    pub rotation: Quat,
    /// DrawScale (sprite size)
    pub scale: f32,
    /// DrawScale * DrawScale3D (local-to-world scale of local offsets)
    pub scale3: Vec3,
    /// component InstanceParameters (name, RGBA)
    pub colors: Vec<(String, [u8; 4])>,
    /// vector instance parameters set by the code (tracers: TracerSize, TracerColor, ...)
    pub vparams: Vec<(String, [f32; 3])>,
    pub emitters: Vec<EmitterState>,
    /// mesh vertices are relative to this point (placed: the actor; pooled: the world origin)
    pub anchor: Vec3,
    /// pooled one-shot effect currently not playing
    pub dormant: bool,
    pub pooled: bool,
    pub fired_at: f64,
    /// component distance cull (UE units, 0 = none; upk EmitterPlacement::max_draw_distance):
    /// hidden while the particles' bounds centre is farther from the camera (UE3 culls a
    /// primitive by the distance of its bounds origin to the view origin)
    pub max_draw: f32,
    /// emitter speed (UE units / s, from the location change between frames) and the location
    /// it was measured from: input of GCParticleModuleLifetimePerUnit
    pub speed: f32,
    last_loc: Option<Vec3>,
}

impl SystemInstance {
    pub fn placed(location: Vec3, rotation: Quat, scale: f32, scale3: Vec3, colors: Vec<(String, [u8; 4])>, emitters: Vec<EmitterState>) -> SystemInstance {
        SystemInstance { location, rotation, scale, scale3, colors, vparams: Vec::new(), emitters, anchor: location, dormant: false, pooled: false, fired_at: 0.0, max_draw: 0.0, speed: 0.0, last_loc: None }
    }
}

#[derive(Resource, Default)]
pub struct Particles {
    pub systems: Vec<SystemInstance>,
    seed: u32,
    /// one-shot effect pools: template path -> system indices
    pub pools: std::collections::HashMap<String, Vec<usize>>,
    now: f64,
    /// effects ended after a time (tracers at their impact distance)
    timed: Vec<((usize, f64), f32)>,
}

impl Particles {
    /// Play a pooled effect (UE location / rotation); the least recently used instance restarts.
    pub fn fire(&mut self, template: &str, location: Vec3, rotation: Quat) -> bool {
        let Some(list) = self.pools.get(template) else { return false };
        let Some(&k) = list.iter().min_by(|&&a, &&b| {
            let (sa, sb) = (&self.systems[a], &self.systems[b]);
            (!sa.dormant, sa.fired_at).partial_cmp(&(!sb.dormant, sb.fired_at)).unwrap()
        }) else {
            return false;
        };
        let now = self.now;
        let s = &mut self.systems[k];
        s.location = location;
        s.rotation = rotation;
        s.dormant = false;
        s.fired_at = now;
        s.vparams.clear();
        s.scale = 1.0;
        s.scale3 = Vec3::ONE;
        s.speed = 0.0;
        s.last_loc = None;
        for e in s.emitters.iter_mut() {
            e.time = 0.0;
            e.loops_done = 0;
            e.duration = e.def.duration;
            e.spawn_frac = 0.0;
            e.particles.clear();
            e.done = false;
            e.last_spawn = None;
            e.finish = false;
        }
        true
    }
}

impl Particles {
    /// Play a pooled effect that follows something (rocket smoke trail): (system, start stamp).
    pub fn fire_tracked(&mut self, template: &str, location: Vec3, rotation: Quat) -> Option<(usize, f64)> {
        if !self.fire(template, location, rotation) {
            return None;
        }
        let now = self.now;
        let list = self.pools.get(template)?;
        list.iter().copied().find(|&k| self.systems[k].fired_at == now && self.systems[k].location == location).map(|k| (k, now))
    }

    /// A tracked pooled effect drawn larger (DrawScale: spawn offsets and sprite sizes).
    pub fn fire_scaled(&mut self, template: &str, location: Vec3, rotation: Quat, scale: f32) -> Option<(usize, f64)> {
        let id = self.fire_tracked(template, location, rotation)?;
        let s = &mut self.systems[id.0];
        s.scale = scale;
        s.scale3 = Vec3::splat(scale);
        Some(id)
    }

    /// A tracked pooled effect with vector instance parameters (tracers).
    pub fn fire_params(&mut self, template: &str, location: Vec3, rotation: Quat, params: &[(&str, [f32; 3])]) -> Option<(usize, f64)> {
        let id = self.fire_tracked(template, location, rotation)?;
        self.systems[id.0].vparams = params.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        Some(id)
    }

    /// `fire_params`, ended (particles included) after `life` seconds.
    pub fn fire_timed(&mut self, template: &str, location: Vec3, rotation: Quat, params: &[(&str, [f32; 3])], life: f32) {
        if let Some(id) = self.fire_params(template, location, rotation, params) {
            self.timed.push((id, life));
        }
    }

    /// End an effect now, live particles included (a tracer at its impact point).
    pub fn kill(&mut self, id: (usize, f64)) {
        if let Some(s) = self.systems.get_mut(id.0).filter(|s| s.fired_at == id.1) {
            for e in s.emitters.iter_mut() {
                e.done = true;
                e.particles.clear();
            }
        }
    }

    /// Move a tracked effect (ignored when its instance was restarted for another use).
    pub fn track(&mut self, id: (usize, f64), location: Vec3, rotation: Quat) {
        if let Some(s) = self.systems.get_mut(id.0).filter(|s| s.fired_at == id.1 && !s.dormant) {
            s.location = location;
            s.rotation = rotation;
        }
    }

    /// Stop spawning; live particles finish their lifetime.
    pub fn release(&mut self, id: (usize, f64)) {
        if let Some(s) = self.systems.get_mut(id.0).filter(|s| s.fired_at == id.1) {
            for e in s.emitters.iter_mut() {
                if e.def.trail.is_some() && e.last_spawn.is_some() && !e.done {
                    e.finish = true;
                    continue;
                }
                e.done = true;
                // particles without a lifetime (Lifetime 0 = live as long as the emitter, e.g. the
                // rocket trail's "Glow" M_LightFlash burst) end with it; before, they stayed at the
                // point of impact
                e.particles.retain(|p| p.inv_life > 0.0);
                // local-space emitters (bUseLocalSpace) draw relative to the component of the
                // destroyed projectile: they go with it (ST_ATW_Rocket "Glow", Lifetime 100 s,
                // stayed lit at the impact point); world-space smoke keeps fading out
                if e.def.local_space {
                    e.particles.clear();
                }
            }
        }
    }
}

impl Particles {
    fn rand(&mut self) -> f32 {
        // xorshift32
        let mut x = if self.seed == 0 { 0x9e37_79b9 } else { self.seed };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / 16_777_216.0
    }
}

/// Night class of a sprite emitter (weather.rs dims the scene at night; the game itself has no
/// night on these maps, so an unlit sprite's colour is its daylight look). true = the sprite
/// shows daylit matter or scattered daylight and is darkened with the scene brightness
/// (gamemat.rs objects[].z flag 3), false = it is a light source or already lit (flag 4).
/// Rule (from the data, survey examples/partmats over all KFX packages, 2026-10-21):
/// * lit materials (lighting model not MLM_Unlit, drawn through the lit path) and unlit materials
///   that do their own lighting (SkyLightContribution / light colour nodes, e.g.
///   KFX_Weapons M_PuffyLitSmoke1) already follow the weather lighting: not dimmed
/// * BLEND_Modulate multiplies the scene: not dimmed
/// * particle colour above 1 (Color / ColorOverLife maximum x ColorScaleOverLife maximum) is
///   HDR emission: fire M_fire4a_1 1.5-22, M_firetest_alpha 2-10, sparks 50, Emberz 1.5-10,
///   IMP_Sparks 1.2-15, Thunderball 10-155; smoke / dust / debris mostly stay at or below 1
///   (MF_Smoke_32FRM <= 0.4, IMP_Soot_Dirt mostly <= 1, Dust <= 0.91, GRay_Test <= 1)
/// * BLEND_Additive is light (muzzle flash MF_Flare_AM 0.07-0.8, sparks, fire), except LDR
///   additive sprites of the ambient package KFX_Atmosphere (FX_Godrays GRay_*, FX_Matter
///   Dust_swarm): sunbeams and sunlit dust motes
/// * alpha-blended unlit sprites (smoke, dust, dirt, debris) are dimmed unless HDR (fire); in
///   KFX_Atmosphere always (leaves at colour 2, birds, moths)
/// NOT from data: that these classes map to "dim" / "keep" (the game has no night here).
pub fn night_dimmed(def: &Emitter, template_path: &str, c: Option<&upk::matcompile::Compiled>) -> bool {
    use upk::level::Blend;
    let Some(c) = c else { return false };
    if c.lighting != "MLM_Unlit" && !matches!(c.info.blend, Blend::Additive | Blend::Modulate) {
        return false;
    }
    if matches!(c.info.blend, Blend::Modulate) || c.code.contains("hf_sky_light") || c.code.contains("m.light_color") {
        return false;
    }
    let atmosphere = template_path.to_ascii_lowercase().starts_with("kfx_atmosphere.");
    let hdr = color_max(def) > 1.0;
    if matches!(c.info.blend, Blend::Additive) {
        return atmosphere && !hdr;
    }
    // alpha-blended: HDR colour is fire (M_firetest_alpha), except in the ambient package, whose
    // leaves are drawn at colour 2 (Leaves1_Dir_Straight_Loop_Far Leaves_big)
    atmosphere || !hdr
}

/// Largest particle colour channel an emitter can reach (initial colour x scale over life).
pub fn color_max(def: &Emitter) -> f32 {
    let mx = |r: &upk::particles::RawDist| {
        let t = r.lut.get(2..).map(|v| v.iter().copied().fold(0.0f32, f32::max)).unwrap_or(0.0);
        t.max(r.param.as_ref().map(|q| q.1.iter().copied().fold(0.0f32, f32::max)).unwrap_or(0.0))
    };
    let (mut base, mut scale) = (1.0f32, 1.0f32);
    for m in &def.modules {
        match m {
            Module::Color { color, .. } | Module::ColorOverLife { color, .. } => base = mx(color),
            Module::ColorScaleOverLife { color, .. } => scale *= mx(color),
            _ => {}
        }
    }
    base * scale
}

pub fn emitter_state(def: Emitter, mesh: Handle<Mesh>) -> EmitterState {
    let duration = def.duration;
    EmitterState { def, time: 0.0, loops_done: 0, duration, spawn_frac: 0.0, particles: Vec::new(), done: false, mesh, empty: true, trail_mesh: None, trail_empty: true, last_spawn: None, seq: 0, finish: false }
}

impl EmitterState {
    /// Attach the ribbon mesh of a trail emitter.
    pub fn with_trail(mut self, mesh: Handle<Mesh>) -> Self {
        self.trail_mesh = Some(mesh);
        self
    }
}

/// Trail emitters also spawn along the emitter's path (the GC trail source /
/// type data code is native, not decoded): one particle per this many UE units moved. NOT from
/// data.
const TRAIL_SPAWN_DIST: f32 = 24.0;

/// Empty dynamic mesh with the GameMat vertex layout.
pub fn empty_mesh() -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    fill_mesh(&mut m, &[], &[], &[]);
    m
}

/// Hide / show the entities drawing an emitter mesh (Inherited keeps a hidden parent's say).
fn set_shown(me: &crate::MeshEntities, vis: &mut Query<&mut Visibility>, mesh: &Handle<Mesh>, on: bool) {
    for &e in me.0.get(&mesh.id()).into_iter().flatten() {
        if let Ok(mut v) = vis.get_mut(e) {
            let want = if on { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
    }
}

fn fill_mesh(m: &mut Mesh, pos: &[[f32; 3]], uv: &[[f32; 2]], col: &[[f32; 4]]) {
    fill_mesh_owned(m, pos.to_vec(), uv.to_vec(), col.to_vec());
}

/// `fill_mesh` taking the vertex arrays (no copies). With the same vertex count as the mesh holds
/// (most frames of a running effect) the index buffer and the constant tangents are the same and
/// are kept, the rest is written into the existing buffers. Same mesh data as before.
fn fill_mesh_owned(m: &mut Mesh, pos: Vec<[f32; 3]>, uv: Vec<[f32; 2]>, col: Vec<[f32; 4]>) {
    // degenerate triangle when empty (keeps the pipeline valid)
    let (pos, uv, col): (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 4]>) = if pos.is_empty() { (vec![[0.0; 3]; 3], vec![[0.0; 2]; 3], vec![[0.0; 4]; 3]) } else { (pos, uv, col) };
    let n = pos.len();
    let want_idx = if n >= 4 { n / 4 * 6 } else { 3 };
    let same = m.attribute(Mesh::ATTRIBUTE_POSITION).is_some_and(|a| a.len() == n)
        && m.attribute(crate::gamemat::ATTR_TANGENT_X).is_some_and(|a| a.len() == n)
        && m.attribute(crate::gamemat::ATTR_TANGENT_Z).is_some_and(|a| a.len() == n)
        && m.indices().is_some_and(|i| i.len() == want_idx);
    if !same {
        let mut idx = Vec::with_capacity(want_idx);
        if n >= 4 {
            for q in 0..n / 4 {
                let b = (q * 4) as u32;
                idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            }
        } else {
            idx.extend_from_slice(&[0, 1, 2]);
        }
        m.insert_attribute(crate::gamemat::ATTR_TANGENT_X, VertexAttributeValues::Unorm8x4(vec![[255, 128, 128, 255]; n]));
        m.insert_attribute(crate::gamemat::ATTR_TANGENT_Z, VertexAttributeValues::Unorm8x4(vec![[128, 128, 255, 255]; n]));
        m.insert_indices(Indices::U32(idx));
    }
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    match m.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(v)) => {
            v.clear();
            v.extend_from_slice(&uv);
        }
        _ => m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv.clone()),
    }
    m.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
}

fn v3(a: [f32; 3]) -> Vec3 {
    Vec3::from(a)
}

fn simulate(rng: &mut Particles, sys: &SystemInstance, e: &mut EmitterState, dt: f32) {
    let (sys_loc, sys_rot, scale3) = (sys.location, sys.rotation, sys.scale3);
    let def = &e.def;
    // update existing particles: reset, modules, integrate
    let mut i = 0;
    while i < e.particles.len() {
        let p = &mut e.particles[i];
        p.vel = p.base_vel;
        p.size = p.base_size;
        p.rot_rate = p.base_rot_rate;
        p.color = p.base_color;
        p.rt += dt * p.inv_life;
        if p.rt >= 1.0 {
            e.particles.swap_remove(i);
            continue;
        }
        i += 1;
    }
    let et = e.time;
    for k in 0..e.particles.len() {
        let mut kill = false;
        for m in &def.modules {
            let p = &mut e.particles[k];
            let rt = p.rt;
            match m {
                Module::ColorOverLife { color, alpha, clamp_alpha } => {
                    let c = color.vec3(rt, &mut || rng.rand());
                    let mut a = alpha.float(rt, &mut || rng.rand());
                    if *clamp_alpha {
                        a = a.clamp(0.0, 1.0);
                    }
                    p.color = Vec4::new(c[0], c[1], c[2], a);
                }
                Module::ColorScaleOverLife { color, alpha, emitter_time } => {
                    let t = if *emitter_time { et } else { rt };
                    let c = color.vec3(t, &mut || rng.rand());
                    let a = alpha.float(t, &mut || rng.rand());
                    p.color *= Vec4::new(c[0], c[1], c[2], a);
                }
                Module::SizeMultiplyLife { mult, axes } => {
                    let s = mult.vec3(rt, &mut || rng.rand());
                    for c in 0..3 {
                        if axes[c] {
                            p.size[c] *= s[c];
                        }
                    }
                }
                Module::VelocityOverLifetime { vel, absolute } => {
                    let v = v3(vel.vec3(rt, &mut || rng.rand()));
                    if *absolute {
                        p.vel = if def.local_space { v } else { sys_rot * v };
                    } else {
                        p.vel *= v;
                    }
                }
                Module::Acceleration(a) => {
                    let a = v3(a.vec3(rt, &mut || rng.rand())) * dt;
                    p.vel += a;
                    p.base_vel += a;
                }
                Module::KillHeight { height, floor, absolute } => {
                    let h = height.float(et, &mut || rng.rand());
                    let z = if *absolute || def.local_space { p.pos.z } else { p.pos.z - sys_loc.z };
                    if (*floor && z < h) || (!*floor && z > h) {
                        kill = true;
                    }
                }
                Module::SubUV(idx) => {
                    p.sub_image = idx.float(rt, &mut || rng.rand());
                }
                _ => {}
            }
        }
        if kill {
            e.particles[k].rt = 2.0;
        }
    }
    e.particles.retain(|p| p.rt < 1.0);
    for p in e.particles.iter_mut() {
        p.old = p.pos;
        p.pos += p.vel * dt;
        p.rot += p.rot_rate * dt;
    }
    // emitter time, loops
    if e.done {
        return;
    }
    let prev = e.time;
    e.time += dt;
    if e.time < def.delay {
        return;
    }
    let mut spawn = 0usize;
    let rate = def.rate.float(e.time, &mut || rng.rand()) * def.rate_scale.float(e.time, &mut || rng.rand());
    if rate > 0.0 {
        let f = e.spawn_frac + rate * dt;
        spawn += f.floor() as usize;
        e.spawn_frac = f - f.floor();
    }
    // trail emitters without a rate: along the path since the last spawn
    let mut path: Option<(Vec3, Vec3)> = None;
    if def.trail.is_some() {
        let from = e.last_spawn.unwrap_or(sys_loc);
        let d = from.distance(sys_loc);
        let n = (d / TRAIL_SPAWN_DIST).floor() as usize;
        if std::env::var_os("HB_TRAILDBG").is_some() {
            info!("trail spawn {} from {from:?} to {sys_loc:?} d {d:.0} n {n} first {}", def.name, e.last_spawn.is_none());
        }
        if e.last_spawn.is_none() {
            e.last_spawn = Some(sys_loc);
            spawn += 1;
            path = Some((sys_loc, sys_loc));
        } else if n > 0 {
            let to = from + (sys_loc - from) * (n as f32 * TRAIL_SPAWN_DIST / d);
            e.last_spawn = Some(to);
            spawn += n.min(64);
            path = Some((from, to));
        }
    }
    if e.finish {
        e.done = true;
    }
    for &(count, low, t) in &def.bursts {
        let bt = t * e.duration;
        if (prev <= bt && bt < e.time) || (prev == 0.0 && bt == 0.0) {
            let n = if low >= 0 && low < count { low + ((count - low + 1) as f32 * rng.rand()) as i32 } else { count };
            spawn += n.max(0) as usize;
        }
    }
    for s in 0..spawn {
        // spawn time inside the frame (UE3 interpolated spawning)
        let st = dt * (s as f32 + 1.0) / (spawn as f32 + 1.0);
        let mut p = Particle {
            pos: Vec3::ZERO,
            old: Vec3::ZERO,
            vel: Vec3::ZERO,
            base_vel: Vec3::ZERO,
            size: Vec3::ZERO,
            base_size: Vec3::ZERO,
            color: Vec4::ONE,
            base_color: Vec4::ONE,
            rot: 0.0,
            rot_rate: 0.0,
            base_rot_rate: 0.0,
            rt: 0.0,
            inv_life: 0.0,
            sub_start: 0.0,
            sub_image: 0.0,
            seq: e.seq,
        };
        e.seq = e.seq.wrapping_add(1);
        let et = e.time;
        for m in &def.modules {
            match m {
                Module::Lifetime(d) => {
                    let l = d.float(et, &mut || rng.rand());
                    p.inv_life = if l > 0.0 { 1.0 / l } else { 0.0 };
                }
                // GCParticleModuleLifetimePerUnit (native Homefront module, code not decoded):
                // the curve (data) read at the emitter's speed in UE units / s, added to a
                // lifetime an earlier module set (UE3 ParticleModuleLifetime convention). NOT
                // from data: the speed as its input (the curves' ranges 0..1200 / 0..1800 fit
                // vehicle speeds; a parked wreck reads the start value, e.g. 1.5 s for
                // FX_WreckSmoke_Medium) and the addition. Without it these particles never aged:
                // alpha stayed at its first key (0 for all vehicle damage / wreck smoke, so the
                // smoke column was invisible) and they piled up for the whole wreck life.
                Module::LifetimePerUnit(d) => {
                    let l = d.float(sys.speed, &mut || rng.rand());
                    if l > 0.0 {
                        p.inv_life = if p.inv_life > 0.0 { 1.0 / (l + 1.0 / p.inv_life) } else { 1.0 / l };
                    }
                }
                Module::Location(d) => {
                    let l = v3(d.vec3_p(et, &mut || rng.rand(), &sys.vparams));
                    p.pos += if def.local_space { l } else { sys_rot * (scale3 * l) };
                }
                Module::Velocity { start, radial, world_space } => {
                    let mut v = v3(start.vec3_p(et, &mut || rng.rand(), &sys.vparams));
                    if !*world_space && !def.local_space {
                        v = sys_rot * v;
                    }
                    let r = radial.float(et, &mut || rng.rand());
                    v += p.pos.normalize_or_zero() * r;
                    p.vel += v;
                    p.base_vel += v;
                }
                Module::Size(d) => {
                    let s = v3(d.vec3_p(et, &mut || rng.rand(), &sys.vparams));
                    p.size += s;
                    p.base_size += s;
                }
                Module::Color { color, alpha, clamp_alpha } => {
                    let c = color.vec3_p(et, &mut || rng.rand(), &sys.vparams);
                    let mut a = alpha.float(et, &mut || rng.rand());
                    if *clamp_alpha {
                        a = a.clamp(0.0, 1.0);
                    }
                    p.color = Vec4::new(c[0], c[1], c[2], a);
                    p.base_color = p.color;
                }
                Module::ColorOverLife { color, alpha, clamp_alpha } => {
                    let c = color.vec3(0.0, &mut || rng.rand());
                    let mut a = alpha.float(0.0, &mut || rng.rand());
                    if *clamp_alpha {
                        a = a.clamp(0.0, 1.0);
                    }
                    p.color = Vec4::new(c[0], c[1], c[2], a);
                    p.base_color = p.color;
                }
                // EXE 0x41f460 (ColorByParameter Spawn): instance parameter matched by name only,
                // channels * 0.0039077764 clamped to [0, 1]; else DefaultColor via FLinearColor(FColor)
                // (0xd50440: rgb = (c / 255)^2.2, a = c / 255). Sets Color and BaseColor.
                Module::ColorByParameter { param, default } => {
                    p.color = match sys.colors.iter().find(|(n, _)| n.eq_ignore_ascii_case(param)) {
                        Some((_, c)) => Vec4::from_array(c.map(|x| (x as f32 * 0.003_907_776_4).clamp(0.0, 1.0))),
                        None => {
                            let g = |x: u8| (x as f32 * (1.0 / 255.0)).powf(2.2);
                            Vec4::new(g(default[0]), g(default[1]), g(default[2]), default[3] as f32 * (1.0 / 255.0))
                        }
                    };
                    p.base_color = p.color;
                }
                Module::Rotation(d) => p.rot += std::f32::consts::TAU * d.float(et, &mut || rng.rand()),
                Module::RotationRate(d) => {
                    let r = std::f32::consts::TAU * d.float(et, &mut || rng.rand());
                    p.rot_rate += r;
                    p.base_rot_rate += r;
                }
                Module::SubUV(_) => {
                    if def.interpolation.starts_with("PSUVIM_RandomStart") {
                        p.sub_start = (rng.rand() * (def.sub_h * def.sub_v) as f32).floor();
                    }
                }
                _ => {}
            }
        }
        if !def.local_space {
            p.pos += match path {
                Some((a, b)) => a + (b - a) * ((s + 1) as f32 / spawn as f32),
                None => sys_loc,
            };
        }
        p.old = p.pos;
        p.pos += p.vel * st;
        p.rt += st * p.inv_life;
        e.particles.push(p);
    }
    if e.time >= e.duration {
        e.loops_done += 1;
        if def.loops > 0 && e.loops_done >= def.loops {
            e.done = true;
        } else {
            e.time -= e.duration;
            if def.duration_use_range {
                e.duration = def.duration_low + (def.duration - def.duration_low) * rng.rand();
            }
        }
    }
}

/// Simulate all emitters and rebuild their sprite meshes (UE units relative to the actor).
pub fn update(
    time: Res<Time>,
    mut ps: ResMut<Particles>,
    cams: Query<&GlobalTransform, (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    me: Res<crate::MeshEntities>,
    mut vis: Query<&mut Visibility>,
    mut frame_no: Local<u32>,
) {
    let _perf = crate::perf::scope("particles");
    *frame_no = frame_no.wrapping_add(1);
    let dt = time.delta_secs().min(0.1);
    if dt <= 0.0 {
        return;
    }
    let Some(cam) = cams.iter().next() else { return };
    // camera in UE axes (x, z, y) and units
    let ue = |v: Vec3| Vec3::new(v.x, v.z, v.y);
    let cam_pos = ue(cam.translation()) * 100.0;
    // the engine's CameraRight / CameraUp parameters (trace frame 3600, particle VS c12 / c13 next
    // to the view rows): CameraRight = -view up, CameraUp = -view right
    let cam_right = -ue(cam.up().as_vec3());
    let cam_up = -ue(cam.right().as_vec3());
    let cam_fwd = ue(cam.forward().as_vec3());
    ps.now = time.elapsed_secs_f64();
    let mut timed = std::mem::take(&mut ps.timed);
    for t in timed.iter_mut() {
        t.1 -= dt;
        if t.1 <= 0.0 {
            ps.kill(t.0);
        }
    }
    timed.retain(|t| t.1 > 0.0);
    ps.timed = timed;
    let ribbon = std::env::var_os("HB_TRAIL_RIBBON").is_some();
    // read once per frame, not per emitter (env lookups take the process environment lock)
    let particle_debug = std::env::var_os("HOMEBOTS_PARTICLE_DEBUG").is_some();
    let mut systems = std::mem::take(&mut ps.systems);
    for sys in systems.iter_mut() {
        if sys.dormant {
            continue;
        }
        if sys.pooled && sys.emitters.iter().all(|e| (e.done || e.def.loops == 0 && e.time > 3.0) && e.particles.is_empty()) && ps.now - sys.fired_at > 0.05 {
            // finished: clear the meshes and sleep
            sys.dormant = true;
            for e in sys.emitters.iter_mut() {
                e.done = true;
                if !e.empty {
                    if let Some(m) = meshes.get_mut(&e.mesh) {
                        fill_mesh(m, &[], &[], &[]);
                    }
                    e.empty = true;
                    set_shown(&me, &mut vis, &e.mesh, false);
                }
                clear_trail(e, &mut meshes, &me, &mut vis);
            }
            continue;
        }
        sys.speed = sys.last_loc.map_or(0.0, |l| l.distance(sys.location) / dt);
        sys.last_loc = Some(sys.location);
        let mut ems = std::mem::take(&mut sys.emitters);
        for e in ems.iter_mut() {
            simulate(&mut ps, sys, e, dt);
            // an emitter without particles: written empty once and hidden, then left alone (the
            // empty meshes were rewritten every frame: 400 mesh uploads per frame in the user's
            // profile 2026-10-18, and each one stayed a draw in the transparent pass)
            if e.particles.is_empty() {
                if !e.empty {
                    if let Some(m) = meshes.get_mut(&e.mesh) {
                        fill_mesh(m, &[], &[], &[]);
                    }
                    e.empty = true;
                    set_shown(&me, &mut vis, &e.mesh, false);
                }
                clear_trail(e, &mut meshes, &me, &mut vis);
                continue;
            }
            // HB_TRAIL_RIBBON=1: the TrailMaterial ribbon (unfinished: the GC trail renderer is
            // native; without it the trail particles draw as sprites)
            if e.trail_mesh.is_some() && ribbon {
                write_trail(e, sys, cam_pos, &mut meshes, &me, &mut vis);
            }
            // outside the view cone (bounding sphere of the particles against a 75 deg half
            // angle, wider than the view's diagonal): hidden and not rewritten. 125 particle
            // meshes were rewritten per frame in the user's profile 2026-10-19 (allocate_and_
            // free_meshes 3.7 ms), most of them level effects behind the player. NOT from data.
            {
                let (mut lo, mut hi, mut big) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN), 0.0f32);
                let k = sys.scale3.abs().max_element();
                for p in &e.particles {
                    let wp = if e.def.local_space { sys.location + sys.rotation * (sys.scale3 * p.pos) } else { p.pos };
                    lo = lo.min(wp);
                    hi = hi.max(wp);
                    big = big.max(p.size.x.abs().max(p.size.y.abs()) * k);
                }
                let r = (hi - lo).length() * 0.5 + big;
                let d = (lo + hi) * 0.5 - cam_pos;
                let dist = d.length();
                if (dist > r && d.dot(cam_fwd) + r < 0.25 * dist) || (sys.max_draw > 0.0 && dist > sys.max_draw) {
                    if !e.empty {
                        e.empty = true;
                        set_shown(&me, &mut vis, &e.mesh, false);
                    }
                    continue;
                }
                // distant effects are rewritten every 2nd (beyond 40 m) / 3rd (beyond 80 m)
                // frame: 82 particle meshes per frame were still rewritten in the user's
                // profile 2026-10-19 22:44 (allocate_and_free_meshes 2.9 ms). NOT from data.
                let every = if dist - r < 4000.0 { 1 } else if dist - r < 8000.0 { 2 } else { 3 };
                if every > 1 && !e.empty && (*frame_no + (e.def.name.len() as u32) * 7 + e.particles.len() as u32 / 64) % every != 0 {
                    continue;
                }
            }
            if e.empty {
                e.empty = false;
                set_shown(&me, &mut vis, &e.mesh, true);
            }
            crate::perf::count("particle_meshes_written", 1);
            let Some(mesh) = meshes.get_mut(&e.mesh) else { continue };
            let (h, v) = (e.def.sub_h as f32, e.def.sub_v as f32);
            let frames = (e.def.sub_h * e.def.sub_v).max(1) as f32;
            let mut pos = Vec::with_capacity(e.particles.len() * 4);
            let mut uv = Vec::with_capacity(e.particles.len() * 4);
            let mut col = Vec::with_capacity(e.particles.len() * 4);
            // trail emitters (GCParticleModuleTypeDataTrail): without the TrailMaterial ribbon the
            // particles are joined into a band drawn with the emitter's own material (each segment
            // a camera-facing quad from the previous particle to this one, width = particle size),
            // plus the sprites RenderSprites asks for (every SpriteFrequency-th). NOT from data:
            // the GC trail renderer is native; before, every trail particle was a separate sprite
            // (thin, gappy smoke; EMP / flash bang trails looked like a burst)
            let band = !ribbon && e.def.trail.is_some();
            if band {
                let mut pts: Vec<(u32, Vec3, f32, Vec4)> = e
                    .particles
                    .iter()
                    .map(|p| (p.seq, if e.def.local_space { sys.location + sys.rotation * (sys.scale3 * p.pos) } else { p.pos }, p.size.x * sys.scale3.x, p.color))
                    .collect();
                pts.sort_by_key(|p| p.0);
                for w in pts.windows(2) {
                    let (a, b) = (&w[0], &w[1]);
                    if b.0.wrapping_sub(a.0) > 4 {
                        continue;
                    }
                    let dir = b.1 - a.1;
                    if dir.length_squared() < 1e-4 {
                        continue;
                    }
                    let mid = (a.1 + b.1) * 0.5;
                    let side = (cam_pos - mid).cross(dir).normalize_or_zero();
                    let (wa, wb) = (side * a.2 * 0.5, side * b.2 * 0.5);
                    for (q, cu, cv, c) in [(a.1 - wa, 0.0f32, 0.0f32, a.3), (b.1 - wb, 0.0, 1.0, b.3), (b.1 + wb, 1.0, 1.0, b.3), (a.1 + wa, 1.0, 0.0, a.3)] {
                        let q = q - sys.anchor;
                        pos.push([q.x, q.z, q.y]);
                        uv.push([cu / h, cv / v]);
                        col.push(c.to_array());
                    }
                }
            }
            let every = if ribbon || band { e.def.trail.as_ref().map(|t| if t.render_sprites { t.sprite_frequency.max(1) as u32 } else { 0 }).unwrap_or(1) } else { 1 };
            for p in e.particles.iter().filter(|p| every > 0 && p.seq % every == 0) {
                let (wp, wold) = if e.def.local_space { (sys.location + sys.rotation * (sys.scale3 * p.pos), sys.location + sys.rotation * (sys.scale3 * p.old)) } else { (p.pos, p.old) };
                let (r, u, sx, sy) = if e.def.screen_alignment == "PSA_Velocity" {
                    let vd = (wp - wold).normalize_or_zero();
                    let dc = (cam_pos - wp).normalize_or_zero();
                    (dc.cross(vd).normalize_or_zero(), -vd, p.size.x * sys.scale3.x, p.size.y * sys.scale3.y)
                } else {
                    let (s, c) = p.rot.sin_cos();
                    (s * cam_right - c * cam_up, c * cam_right + s * cam_up, p.size.x * sys.scale3.x, p.size.x * sys.scale3.x)
                };
                let frame = ((p.sub_start + p.sub_image).floor() % frames).max(0.0);
                let (fx, fy) = (frame % h, (frame / h).floor());
                for (cu, cv) in [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                    // vertex Size = particle Size * DrawScale * DrawScale3D, square: Size.y = Size.x (god-ray
                    // trace 2026-10-16: Size 110, DrawScale3D (5,1,1) -> vertex Size (550, 550))
                    let q = wp + (cu - 0.5) * sx * r + (cv - 0.5) * sy * u - sys.anchor;
                    pos.push([q.x, q.z, q.y]);
                    uv.push([(fx + cu) / h, (fy + cv) / v]);
                    col.push(p.color.to_array());
                }
            }
            if particle_debug && !e.particles.is_empty() {
                let p = &e.particles[0];
                info!("emitter {} n={} t={:.2} p0 pos={:?} size={:?} color={:?} rt={:.2} img={}", e.def.name, e.particles.len(), e.time, p.pos, p.size, p.color, p.rt, p.sub_start + p.sub_image);
            }
            fill_mesh_owned(mesh, pos, uv, col);
        }
        sys.emitters = ems;
    }
    ps.systems = systems;
}

fn clear_trail(e: &mut EmitterState, meshes: &mut Assets<Mesh>, me: &crate::MeshEntities, vis: &mut Query<&mut Visibility>) {
    if let Some(tm) = e.trail_mesh.clone() {
        if !e.trail_empty {
            if let Some(m) = meshes.get_mut(&tm) {
                fill_mesh(m, &[], &[], &[]);
            }
            e.trail_empty = true;
            set_shown(me, vis, &tm, false);
        }
    }
}

/// Ribbon of a trail emitter (GCParticleModuleTypeDataTrail): the particles in spawn order joined
/// by camera-facing quads, width = particle size, colour = particle colour, U along the trail
/// (UTiling per particle), V across. NOT from data: the GC trail renderer itself is native; the
/// layout follows UE3's beam / trail vertex layout.
fn write_trail(e: &mut EmitterState, sys: &SystemInstance, cam_pos: Vec3, meshes: &mut Assets<Mesh>, me: &crate::MeshEntities, vis: &mut Query<&mut Visibility>) {
    let Some(tm) = e.trail_mesh.clone() else { return };
    let Some(td) = e.def.trail.clone() else { return };
    let mut pts: Vec<(u32, Vec3, f32, Vec4)> = e
        .particles
        .iter()
        .map(|p| {
            let wp = if e.def.local_space { sys.location + sys.rotation * (sys.scale3 * p.pos) } else { p.pos };
            (p.seq, wp, p.size.x * sys.scale3.x * td.v_size, p.color)
        })
        .collect();
    pts.sort_by_key(|p| p.0);
    if std::env::var_os("HB_TRAILDBG").is_some() && !pts.is_empty() {
        let (a, b) = (&pts[0], &pts[pts.len() - 1]);
        info!("trail {} n={} oldest {:?} w {:.0} c {:?} newest {:?} w {:.0} c {:?}", e.def.name, pts.len(), a.1, a.2, a.3, b.1, b.2, b.3);
    }
    if pts.len() < 2 {
        clear_trail(e, meshes, me, vis);
        return;
    }
    let (mut pos, mut uv, mut col) = (Vec::with_capacity(pts.len() * 4), Vec::with_capacity(pts.len() * 4), Vec::with_capacity(pts.len() * 4));
    let side = |i: usize| -> Vec3 {
        let a = pts[i.saturating_sub(1)].1;
        let b = pts[(i + 1).min(pts.len() - 1)].1;
        let dir = (b - a).normalize_or_zero();
        let dc = (cam_pos - pts[i].1).normalize_or_zero();
        dc.cross(dir).normalize_or_zero()
    };
    for i in 0..pts.len() - 1 {
        // a gap (particles of an earlier rocket in the same pool slot): no quad
        if pts[i + 1].0.wrapping_sub(pts[i].0) > 4 {
            continue;
        }
        let (s0, s1) = (side(i), side(i + 1));
        let (p0, p1) = (pts[i].1 - sys.anchor, pts[i + 1].1 - sys.anchor);
        let (w0, w1) = (pts[i].2 * 0.5, pts[i + 1].2 * 0.5);
        let (u0, u1) = (pts[i].0 as f32 * td.u_tiling, pts[i + 1].0 as f32 * td.u_tiling);
        for (q, u, v, c) in [(p0 - s0 * w0, u0, 0.0f32, pts[i].3), (p1 - s1 * w1, u1, 0.0, pts[i + 1].3), (p1 + s1 * w1, u1, 1.0, pts[i + 1].3), (p0 + s0 * w0, u0, 1.0, pts[i].3)] {
            pos.push([q.x, q.z, q.y]);
            uv.push([u, v]);
            col.push(c.to_array());
        }
    }
    if let Some(m) = meshes.get_mut(&tm) {
        fill_mesh_owned(m, pos, uv, col);
    }
    if e.trail_empty {
        e.trail_empty = false;
        set_shown(me, vis, &tm, true);
    }
}

/// HB_FXNIGHT=<map package path>[,...]: headless report (no window) of the night class of every
/// placed emitter (particles::night_dimmed) and of the unlit static-mesh materials
/// (gamemat::daylit_unlit), each GameMat shader validated with naga. Dev tool, 2026-10-21.
pub fn night_report(game: &std::path::Path, maps: &str) {
    let lib = upk::library::Library::scan(game).expect("game folder");
    let defs = upk::particles::ClassDefaults::new(&lib);
    if let Ok(mut g) = crate::gamemat::SHADOW_WGSL.write() {
        if g.is_empty() {
            *g = crate::sunshadow::wgsl(&Default::default(), 1.0, 1.0);
        }
    }
    let check = |c: &upk::matcompile::Compiled| match crate::gamemat::validate_wgsl(&crate::gamemat::shader_source(c)) {
        Ok(_) => "ok",
        Err(e) => {
            if std::env::var_os("HB_FXNIGHT_ERR").is_some() {
                println!("{e}");
            }
            "INVALID"
        }
    };
    for f in maps.split(',') {
        let Ok(p) = upk::Package::open(f) else { println!("{f}: cannot open"); continue };
        for pl in p.emitter_placements() {
            let tp = p.object_path(pl.template);
            let Some(ems) = p.read_particle_system(pl.template as usize - 1, &defs).or_else(|| {
                let (pk, rest) = tp.split_once('.')?;
                let q = lib.open(pk)?;
                let i = q.exports.iter().enumerate().position(|(i, e)| q.export_class(e) == "ParticleSystem" && q.object_path(i as i32 + 1).eq_ignore_ascii_case(rest))?;
                q.read_particle_system(i, &defs)
            }) else { continue };
            for em in ems.iter().filter(|e| !e.mesh && e.material != 0) {
                let c = p.compile_material(em.material);
                let v = c.as_ref().map(check).unwrap_or("-");
                println!("emitter {tp} {:?} maxdraw {} | {} | {} | col {:.2} | dim {} | wgsl {v}", pl.location, pl.max_draw_distance, em.name, p.object_path(em.material), color_max(em), night_dimmed(em, &tp, c.as_ref()));
            }
        }
        // every Material of the package (effect packages such as KFX_VHC.upk have no
        // placements): WGSL validity, scene-colour translucency (gamemat::scene_blend)
        let (mut n, mut bad) = (0, 0);
        for (i, e) in p.exports.iter().enumerate().filter(|(_, e)| p.export_class(e) == "Material") {
            let Some(c) = p.compile_material(i as i32 + 1) else { continue };
            n += 1;
            let v = check(&c);
            if v != "ok" {
                bad += 1;
            }
            if crate::gamemat::scene_blend(&c) || v != "ok" {
                println!("material {} {:?} {} | scene blend {} | wgsl {v}", p.object_path(i as i32 + 1), c.info.blend, c.lighting, crate::gamemat::scene_blend(&c));
            }
            let _ = e;
        }
        println!("{f}: {n} materials compiled, {bad} INVALID");
        let mut seen = std::collections::HashSet::new();
        for pl in p.static_mesh_placements() {
            if pl.mesh <= 0 { continue; }
            let Some(m) = p.read_static_mesh(pl.mesh as usize - 1) else { continue };
            let Some(l) = m.lods.first() else { continue };
            for (k, s) in l.sections.iter().enumerate() {
                let mat = pl.materials.get(k).copied().filter(|&o| o != 0).unwrap_or(s.material);
                if mat == 0 || !seen.insert(mat) { continue; }
                let Some(c) = p.compile_material(mat) else { continue };
                if c.lighting != "MLM_Unlit" { continue; }
                println!("unlit {} {:?} {} | daylit {} | wgsl {}", p.object_path(mat), c.info.blend, if pl.sky { "sky" } else { "" }, crate::gamemat::daylit_unlit(&c), check(&c));
            }
        }
    }
}
