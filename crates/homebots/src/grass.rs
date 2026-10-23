//! Homefront dynamic terrain grass (UTerrain grass billboards).
//!
//! The game builds the billboards on the CPU every frame (job FUN_004e6180, view-space quads,
//! 6 vertices x 48 bytes) and draws them with the global shaders
//! FDynamicGrassBillboardVertexShader / PixelShader. Here the per-instance part that does not
//! depend on the camera (jitter, slope height, size variation, mirroring) is precomputed and
//! the camera-dependent part runs in the vertex shader with the same formulas.
//! Verified against the Overpass trace (see upk/src/grass.rs and MODLOG "Dynamic grass").
//!
//! Wind (EXE): the job sways the two top vertices in view x by sum_i f_i * amp_i * sin(a + phase_i)
//! over 4 sources (x, y, r^2, phase, amp) picked from the wind manager at 0x1b92f20 (FUN_00a5c490,
//! pointer stored by FUN_00896060). On level load (FUN_00512420) it holds one permanent source:
//! (0, 0), r^2 = 1e20, amp 3, phase += pi * 2 * (1/62) per update; unused slots r^2 = 1, amp 0.
//! f = max(r^2 - d^2, 0) / r^2 (d = jittered xy - source), a = ((iy ^ ix) & 7) * (1/7) * pi * 2
//! with ix, iy = trunc(jittered x, y). The game advances the phase per frame; here per second
//! (equal at 62 updates/s). The vertex's other wind output (sat(sum f*sin) * 0.7, VS TexCoord1.z ->
//! cubed into TexCoord1.x) is not read by the pixel shader.
//! The light term uses the sun shadow (sunshadow.rs) for both LightAttenuation and the own test.

use bevy::{
    asset::RenderAssetUsages,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::{
        mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
        render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
        view::NoFrustumCulling,
    },
};

/// Per-set jitter offsets (x, y) in UE units, indexed by (trunc(x) & 7) + billboard index.
/// The game reads them from a runtime-filled table (DAT_017e8f50 + set * 0xe0, entry
/// NumBillboardsPerCell + index); values measured from the Overpass trace (all 645 billboards):
/// set 0 all zero, set 1 diagonals of radius 64, set 2 axis offsets of 64 with period 4
/// (entries 9..11 continue the observed period).
const D: f32 = 45.254_833; // 64 / sqrt(2)
const JITTER: [&[(f32, f32)]; 3] = [
    &[],
    &[(D, D), (-D, -D), (-D, D), (D, -D), (D, D), (-D, -D), (D, D), (-D, -D), (-D, D)],
    &[
        (-64.0, 0.0),
        (0.0, 64.0),
        (0.0, -64.0),
        (64.0, 0.0),
        (-64.0, 0.0),
        (0.0, 64.0),
        (0.0, -64.0),
        (64.0, 0.0),
        (-64.0, 0.0),
        (0.0, 64.0),
        (0.0, -64.0),
        (64.0, 0.0),
    ],
];

fn jitter(set: usize, e: usize) -> (f32, f32) {
    JITTER.get(set).and_then(|t| t.get(e)).copied().unwrap_or((0.0, 0.0))
}

#[derive(Clone, Copy, Default, Debug, ShaderType)]
pub struct GrassParams {
    /// NearWidth, FarWidth, NearWidthDist, FarWidthDist
    pub width: Vec4,
    /// Height, NearHeightDist, FarHeightDist, CameraAlignedY
    pub height: Vec4,
    /// MinDrawDist, MaxDrawDist
    pub draw: Vec4,
    /// TerrainBounds: MinX, 1/(MaxX-MinX), MinY, 1/(MaxY-MinY)
    pub bounds: Vec4,
    /// nuclear crater (crater.rs, NOT from data): UE x, y, radius without grass (0 = none)
    pub crater: Vec4,
    /// weather.rs (day/night, weather; NOT in the game): rgb = 1 - light multiplier (zero = unchanged)
    pub weather: Vec4,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GrassKey {
    pub shader: Handle<Shader>,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(GrassKey)]
pub struct GrassMat {
    #[uniform(0)]
    pub params: GrassParams,
    #[texture(1)]
    #[sampler(2)]
    pub billboard: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub overlay: Option<Handle<Image>>,
    #[texture(5)]
    pub scene: Option<Handle<Image>>,
    pub shader: Handle<Shader>,
}

impl From<&GrassMat> for GrassKey {
    fn from(m: &GrassMat) -> Self {
        GrassKey { shader: m.shader.clone() }
    }
}

impl Material for GrassMat {
    // Drawn after the scene copy (the pixel shader reads SceneColorTexture), but opaque:
    // blending is switched off and depth written in specialize(), as in the game.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vb = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vb];
        descriptor.vertex.shader = key.bind_group_data.shader.clone();
        descriptor.vertex.entry_point = "vertex".into();
        if let Some(f) = descriptor.fragment.as_mut() {
            f.shader = key.bind_group_data.shader.clone();
            f.entry_point = "fragment".into();
            for t in f.targets.iter_mut().flatten() {
                t.blend = None;
            }
        }
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = true;
        }
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// FDynamicGrassBillboardVertexShader + the CPU job's per-vertex math (view space, UE units),
/// FDynamicGrassBillboardPixelShader (blob_call21233) with light attenuation = 1.
pub const SHADER: &str = r#"
#import bevy_pbr::mesh_view_bindings::{view, globals}

struct GrassParams { width: vec4<f32>, height: vec4<f32>, draw: vec4<f32>, bounds: vec4<f32>, crater: vec4<f32>, weather: vec4<f32> }
@group(2) @binding(0) var<uniform> gp: GrassParams;
@group(2) @binding(1) var bb_tex: texture_2d<f32>;
@group(2) @binding(2) var bb_smp: sampler;
@group(2) @binding(3) var ov_tex: texture_2d<f32>;
@group(2) @binding(4) var ov_smp: sampler;
@group(2) @binding(5) var scene_tex: texture_2d<f32>;

struct VIn {
    // jittered position (UE world)
    @location(0) pos: vec3<f32>,
    // corner: x = -1 left / +1 right, y = 1 top
    @location(1) corner: vec2<f32>,
    // instance x, y (UE world), size scale, U mirror sign
    @location(2) inst: vec4<f32>,
}
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec4<f32>,
    @location(1) w: f32,
}

fn bevy_pos(p: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(p.x * 0.01, p.z * 0.01, p.y * 0.01, 1.0);
}

// the job's clamp: t < 0 -> 0, t >= 1 -> 1
fn clamp01(t: f32) -> f32 {
    return select(select(t, 1.0, t >= 1.0), 0.0, t < 0.0);
}

@vertex
fn vertex(i: VIn) -> VOut {
    var o: VOut;
    let cam = vec2<f32>(view.world_position.x, view.world_position.z) * 100.0;
    let di = distance(i.inst.xy, cam);
    let burnt = gp.crater.z > 0.0 && distance(i.inst.xy, gp.crater.xy) < gp.crater.z;
    if (di > gp.draw.y || di < gp.draw.x || burnt) {
        o.clip = vec4<f32>(4.0, 4.0, 0.5, 1.0);
        o.uv = vec4<f32>(0.0);
        o.w = 0.0;
        return o;
    }
    let d = distance(i.pos.xy, cam);
    let s = i.inst.z;
    let tw = clamp01((d - gp.width.z) / (gp.width.w - gp.width.z)) * s;
    let hw = (gp.width.y - gp.width.x) * tw + gp.width.x;
    let th = clamp01((gp.height.z - d) / (gp.height.z - gp.height.y));
    let h = gp.height.x * s * th;
    let mirror = ((1.0 - tw) * 0.5 + 0.5) * i.inst.w;
    let r = hw / gp.width.x;
    let u = 0.5 + i.corner.x * mirror * 0.5 * r;
    let top = i.corner.y > 0.5;
    var vp = view.view_from_world * bevy_pos(i.pos);
    if (top) {
        if (gp.height.w > 0.5) {
            vp = view.view_from_world * bevy_pos(i.pos + vec3<f32>(0.0, 0.0, h));
        } else {
            vp.y += h * 0.01;
        }
    }
    vp.x += i.corner.x * hw * 0.01;
    if (top) {
        // permanent wind source (see module doc)
        let ix = i32(trunc(i.pos.x));
        let iy = i32(trunc(i.pos.y));
        let a = f32((iy ^ ix) & 7) * 0.14285715 * 3.1415927 * 2.0;
        let phase = globals.time * 3.1415927 * 2.0;
        let d2 = i.pos.x * i.pos.x + i.pos.y * i.pos.y;
        let f = max(1.0e20 - d2, 0.0) / 1.0e20;
        vp.x += f * 3.0 * sin(a + phase) * 0.01;
    }
    o.clip = view.clip_from_view * vp;
    o.uv = vec4<f32>(u, select(1.0, 0.1, top), (i.pos.x - gp.bounds.x) * gp.bounds.y, (i.pos.y - gp.bounds.z) * gp.bounds.w);
    o.w = -vp.z * 100.0;
    return o;
}

@fragment
fn fragment(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(bb_tex, bb_smp, i.uv.xy);
    if (t.a - 0.33 < 0.0) {
        discard;
    }
    // SceneColorTexture: rgb + scene depth in alpha, z = MinZ.x / (alpha - MinZ.y)
    let sc = textureLoad(scene_tex, vec2<i32>(i.clip.xy), 0);
    let a = sc.a;
    let inv = select(1.0e6, 1.0 / a, a - 1.0e-6 >= 0.0);
    let dz = abs(MINZ * inv - i.w) - 50.0;
    let r2 = select(vec2<f32>(0.0), clamp(dz * vec2<f32>(0.01, 0.002), vec2<f32>(0.0), vec2<f32>(1.0)), dz >= 0.0);
    let fade = clamp(i.w * 0.000333333 - r2.y, 0.0, 1.0);
    // light = lerp(LightAttenuation, own shadow test, r2.x): both the sun shadow here
    let light = hf_sun_shadow(i.clip);
    let l = light * vec3<f32>(0.864, 0.7385, 0.5668) + vec3<f32>(0.136, 0.2615, 0.4332);
    let ov = textureSample(ov_tex, ov_smp, i.uv.zw).rgb;
    let r4 = l * ov * t.rgb;
    let r5 = clamp(t.rgb - vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));
    let g = t.g + 0.5;
    var c = ov * l + r5 - 2.0 * r4;
    c = (g - fract(g)) * c + 2.0 * r4;
    c = c * (vec3<f32>(1.0) - gp.weather.rgb);
    return vec4<f32>(mix(c, sc.rgb, fade), 1.0);
}
"#;

/// Static vertex data of one billboard set of one grass layer.
pub fn build_mesh(layer: &upk::grass::GrassLayer, set: usize) -> Option<Mesh> {
    let bb = layer.billboards.get(set)?;
    let n = bb.num_per_cell as usize;
    if n == 0 || layer.instances.is_empty() {
        return None;
    }
    let cap = layer.instances.len() * n * 4;
    let (mut pos, mut corner, mut inst, mut idx) = (Vec::with_capacity(cap), Vec::with_capacity(cap), Vec::with_capacity(cap), Vec::with_capacity(cap / 4 * 6));
    for gi in &layer.instances {
        let [x, y, z] = gi.pos;
        let [s0, s1, s2, s3] = gi.slope;
        let k = (x as i32 & 7) as usize;
        for b in 0..n {
            let (jx, jy) = jitter(set, k + b);
            let dz = (if jx < 0.0 { -(jx * s3) } else { jx * s2 }) + (if jy < 0.0 { -(jy * s1) } else { jy * s0 });
            let p = [x + jx, y + jy, z + dz];
            let iy = p[1] as i32;
            let scale = (iy & 7) as f32 * (1.0f32 / 7.0) + 0.5;
            let sign = (((iy >> 4) & 1) * 2 - 1) as f32;
            let base = pos.len() as u32;
            for c in [[-1.0f32, 0.0], [1.0, 0.0], [-1.0, 1.0], [1.0, 1.0]] {
                pos.push(p);
                corner.push(c);
                inst.push([x, y, scale, sign]);
            }
            // game order: TL BL BR, TL BR TR
            idx.extend_from_slice(&[base + 2, base, base + 1, base + 2, base + 1, base + 3]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, corner);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, inst);
    m.insert_indices(Indices::U32(idx));
    Some(m)
}

pub fn params(bb: &upk::grass::GrassBillboard, bounds: [f32; 4]) -> GrassParams {
    GrassParams {
        width: Vec4::new(bb.near_width, bb.far_width, bb.near_width_dist, bb.far_width_dist),
        height: Vec4::new(bb.height, bb.near_height_dist, bb.far_height_dist, if bb.camera_aligned_y { 1.0 } else { 0.0 }),
        // options FoliageDetail = SystemSettings FoliageDrawRadiusMultiplier (UE3 scales the
        // foliage draw radius with it)
        draw: Vec4::new(bb.min_draw_dist, bb.max_draw_dist * crate::settings::get("GraphicsSettings:FoliageDetail").clamp(0.1, 1.0), 0.0, 0.0),
        bounds: Vec4::new(bounds[0], 1.0 / (bounds[1] - bounds[0]), bounds[2], 1.0 / (bounds[3] - bounds[2])),
        crater: Vec4::ZERO,
        weather: Vec4::ZERO,
    }
}

/// Keep only mip 0 (textures cooked with NumMips = 1, e.g. GrassColorTexture: the game creates
/// them with one level).
pub fn single_mip(mut img: Image) -> Image {
    let f = img.texture_descriptor.format;
    let (w, h) = (img.texture_descriptor.size.width as usize, img.texture_descriptor.size.height as usize);
    let bpp = f.block_copy_size(None).unwrap_or(4) as usize;
    if let Some(d) = img.data.as_mut() {
        d.truncate(w * h * bpp);
    }
    img.texture_descriptor.mip_level_count = 1;
    img
}

#[derive(Component)]
pub struct Grass;

pub fn spawn(commands: &mut Commands, mesh: Handle<Mesh>, mat: Handle<GrassMat>) {
    commands.spawn((crate::LevelEntity, Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY, NoFrustumCulling, bevy::pbr::NotShadowCaster, Grass));
}
