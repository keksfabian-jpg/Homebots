//! Nuclear crater (nuke.rs; the game has none, everything here is NOT from data). Swapped in while
//! the view is white after the detonation, so the world comes back from the white-out blasted:
//! - everything placed in the level within R_GONE of ground zero is gone (static meshes, wrecks,
//!   roads, SpeedTrees, placed particle emitters), grass within R_GRASS
//! - the terrain gets a bowl with a raised rim (render meshes and collision)
//! - the ground and the surviving buildings are charred towards the centre, burnt out to R_SCORCH
//!   (game-shader globals misc.yzw = ground zero x, y and R_SCORCH in UE units; the terrain and
//!   static-mesh fragment shaders darken by distance, terrainemu.rs / gameshader.rs hf_crater)
//!
//! A new match reloads the level, which brings everything back; the globals are cleared with the
//! crater (Crater::reset in nuke.rs).
use bevy::{
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        renderer::RenderQueue,
        storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
        Render, RenderApp, RenderSet,
    },
};

/// crater radius at the rim, depth at the centre, rim height (m)
pub const CRATER_R: f32 = 70.0;
pub const CRATER_DEPTH: f32 = 16.0;
pub const RIM_H: f32 = 4.0;
/// nothing placed in the level survives inside this radius (m)
pub const R_GONE: f32 = 380.0;
/// grass burnt away (m)
pub const R_GRASS: f32 = 600.0;
/// ground and buildings charred out to here (m)
pub const R_SCORCH: f32 = 720.0;

/// WGSL shared by the terrain and static-mesh fragment shaders (needs `hfg`): colour factor of
/// the charred zone at a UE world position.
pub const WGSL: &str = r#"
fn hf_crater_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn hf_crater_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hf_crater_hash(i), hf_crater_hash(i + vec2<f32>(1.0, 0.0)), u.x), mix(hf_crater_hash(i + vec2<f32>(0.0, 1.0)), hf_crater_hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}
fn hf_crater(w: vec3<f32>) -> vec3<f32> {
    let rs = hfg.misc.w;
    if (rs <= 0.0) {
        return vec3<f32>(1.0);
    }
    let d = length(w.xy - hfg.misc.yz) / rs;
    if (d >= 1.0) {
        return vec3<f32>(1.0);
    }
    // ragged edge, streaks pointing away from ground zero
    let n = hf_crater_noise(w.xy / 1800.0) * 0.6 + hf_crater_noise(w.xy / 450.0) * 0.4;
    let a = atan2(w.y - hfg.misc.z, w.x - hfg.misc.y);
    let streak = hf_crater_noise(vec2<f32>(a * 18.0, 0.5)) * 0.25;
    let burn = 1.0 - smoothstep(0.35 + 0.3 * n - streak, 1.0, d);
    // glassy black at the centre, soot, then scorched brown towards the edge
    let soot = mix(vec3<f32>(0.42, 0.36, 0.3), vec3<f32>(0.07, 0.065, 0.06), smoothstep(0.0, 0.75, burn));
    return mix(vec3<f32>(1.0), soot, burn);
}
"#;

#[derive(Resource, Default)]
pub struct Crater {
    /// ground zero (UE units), set once the view is white
    pub at: Option<Vec3>,
    applied: bool,
}

impl Crater {
    pub fn trigger(&mut self, gz_ue: Vec3) {
        if self.at.is_none() {
            self.at = Some(gz_ue);
        }
    }
    pub fn reset(&mut self) {
        *self = Crater::default();
    }
}

/// Where a level object without its own transform stands (SpeedTrees: Transform::IDENTITY, the
/// location lives in the object block), Bevy metres.
#[derive(Component)]
pub struct PlacedAt(pub Vec3);

#[derive(Resource, Default, Clone, ExtractResource)]
struct Upload {
    buffers: Vec<Handle<ShaderStorageBuffer>>,
    misc: [f32; 3],
}

pub struct CraterPlugin;

impl Plugin for CraterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Crater>().init_resource::<Upload>().add_plugins(ExtractResourcePlugin::<Upload>::default()).add_systems(Update, apply.after(crate::combat::CombatSet));
        if let Some(r) = app.get_sub_app_mut(RenderApp) {
            r.add_systems(Render, upload.in_set(RenderSet::PrepareResources));
        }
    }
}

/// Height change of the ground (m) at `r` metres from ground zero: bowl and rim.
pub fn ground_offset(r: f32) -> f32 {
    let x = r / CRATER_R;
    let bowl = if x < 1.0 { -CRATER_DEPTH * (1.0 - x * x) } else { 0.0 };
    let rim = RIM_H * (-((r - CRATER_R) / (0.4 * CRATER_R)).powi(2)).exp();
    bowl + rim
}

#[allow(clippy::too_many_arguments)]
fn apply(
    mut cr: ResMut<Crater>,
    mut up: ResMut<Upload>,
    terrain_mats: Res<Assets<crate::terrainemu::TerrainEmu>>,
    mut meshes: ResMut<Assets<Mesh>>,
    terrain: Query<&Mesh3d, With<MeshMaterial3d<crate::terrainemu::TerrainEmu>>>,
    mut objs: Query<(&Transform, &mut Visibility, Option<&PlacedAt>), With<crate::LevelEntity>>,
    mut grass: ResMut<Assets<crate::grass::GrassMat>>,
    mut fx: ResMut<crate::particles::Particles>,
    mut col: ResMut<crate::player::CollisionWorld>,
) {
    // charred-zone globals: every frame (a reloaded level brings new buffers)
    let mut bufs: Vec<Handle<ShaderStorageBuffer>> = Vec::new();
    for (_, m) in terrain_mats.iter() {
        if !bufs.contains(&m.globals) {
            bufs.push(m.globals.clone());
        }
    }
    up.buffers = bufs;
    up.misc = match cr.at {
        Some(g) => [g.x, g.y, R_SCORCH * 100.0],
        None => [0.0; 3],
    };
    let Some(gz) = cr.at else { return };
    if cr.applied {
        return;
    }
    cr.applied = true;
    let t0 = std::time::Instant::now();
    let gzb = Vec3::new(gz.x, gz.z, gz.y) * 0.01;
    let flat = |p: Vec3| Vec2::new(p.x - gzb.x, p.z - gzb.z).length();
    // ---- level objects
    let mut hidden = 0;
    for (tf, mut v, at) in objs.iter_mut() {
        let p = match at {
            Some(a) => a.0,
            None if tf.translation != Vec3::ZERO => tf.translation,
            // terrain, fluid surfaces (Transform::IDENTITY)
            None => continue,
        };
        if flat(p) < R_GONE && *v != Visibility::Hidden {
            *v = Visibility::Hidden;
            hidden += 1;
        }
    }
    // ---- placed particle emitters (fires, smoke, steam of the level)
    let mut quiet = 0;
    for s in fx.systems.iter_mut() {
        if !s.pooled && Vec2::new(s.location.x - gz.x, s.location.y - gz.y).length() < R_GONE * 100.0 {
            s.dormant = true;
            quiet += 1;
        }
    }
    // ---- grass
    let ids: Vec<_> = grass.ids().collect();
    for id in ids {
        if let Some(g) = grass.get_mut(id) {
            g.params.crater = Vec4::new(gz.x, gz.y, R_GRASS * 100.0, 0.0);
        }
    }
    // ---- terrain render meshes: the heights in ATTR_TERRAIN_VERT (h = z, w = high byte) and the
    // bounds positions
    let mut done = std::collections::HashSet::new();
    let mut verts = 0;
    for m in terrain.iter() {
        if !done.insert(m.0.id()) {
            continue;
        }
        let Some(mesh) = meshes.get_mut(&m.0) else { continue };
        verts += deform_terrain_mesh(mesh, gzb);
    }
    // ---- collision: the ground follows, everything else in the zone goes
    let removed = col.crater(gzb, R_GONE, ground_offset, CRATER_R * 2.0);
    info!("crater: {hidden} objects hidden, {quiet} emitters stopped, {verts} terrain vertices moved, {removed} collision triangles off, {:.2} s", t0.elapsed().as_secs_f32());
}

/// Lowers the terrain vertices of one component mesh; returns how many moved.
fn deform_terrain_mesh(mesh: &mut Mesh, gzb: Vec3) -> usize {
    use bevy::render::mesh::VertexAttributeValues as V;
    let Some(V::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned() else { return 0 };
    let Some(V::Float32x4(tv)) = mesh.attribute(crate::terrainemu::ATTR_TERRAIN_VERT).cloned() else { return 0 };
    if pos.len() != tv.len() {
        return 0;
    }
    let reach = CRATER_R * 2.2;
    // metres per height step: from two vertices of different height (Z = loc.z + (h - 32768) /
    // 128 * scale.z, POSITION = UE * 0.01)
    let hv = |t: &[f32; 4]| t[2] + t[3] * 256.0;
    let mut per_step = None;
    'find: for i in 0..pos.len() {
        for j in (i + 1)..pos.len().min(i + 64) {
            let dh = hv(&tv[i]) - hv(&tv[j]);
            if dh.abs() > 8.0 {
                per_step = Some((pos[i][1] - pos[j][1]) / dh);
                break 'find;
            }
        }
    }
    let Some(per_step) = per_step.filter(|s| s.is_finite() && *s > 0.0) else { return 0 };
    let (mut pos, mut tv) = (pos, tv);
    let mut n = 0;
    for (p, t) in pos.iter_mut().zip(tv.iter_mut()) {
        let r = Vec2::new(p[0] - gzb.x, p[2] - gzb.z).length();
        if r > reach {
            continue;
        }
        let off = ground_offset(r);
        let h = (hv(t) + off / per_step).round().clamp(0.0, 65535.0);
        t[2] = (h as u32 & 0xff) as f32;
        t[3] = (h as u32 >> 8) as f32;
        p[1] += off;
        n += 1;
    }
    if n > 0 {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(crate::terrainemu::ATTR_TERRAIN_VERT, tv);
    }
    n
}

fn upload(up: Res<Upload>, buffers: Res<RenderAssets<GpuShaderStorageBuffer>>, queue: Res<RenderQueue>) {
    for h in &up.buffers {
        let Some(b) = buffers.get(h) else { continue };
        // Globals.misc.yzw (gamemat::Globals: 6 vec4 before misc)
        // (only buffers that allow writes: a missing COPY_DST is a fatal wgpu validation error)
        if b.buffer.size() >= 112 && b.buffer.usage().contains(bevy::render::render_resource::BufferUsages::COPY_DST) {
            queue.write_buffer(&b.buffer, 100, bytemuck::cast_slice(&up.misc));
        }
    }
}
