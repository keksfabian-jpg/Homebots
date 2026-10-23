//! Homefront materials on static meshes, rendered with the game's own base-pass math.
//!
//! The material graph is compiled to WGSL by `upk::matcompile`; this module wraps it in
//! the vertex/pixel code of Homefront's base pass for FLocalVertexFactory with the
//! directional-light + sky-light policy, transcribed from the game's compiled shaders
//! (RefShaderCache-PC-D3D-SM3.upk, e.g. the M_Opaque permutation used by the Overpass
//! gas station: TBasePassVertexShaderFDirectionalLightLightMapPolicyFNoDensityPolicy /
//! TBasePassPixelShaderFDirectionalLightLightMapPolicySkyLight):
//!
//! * tangent-space camera, light and world-up vectors are formed per vertex through
//!   WorldToLocal and the packed tangent basis (TangentY = cross(Z, X) * Z.w)
//! * sky light: lower colour is lerp(LowerSkyColor, GrassColorTexture * LowerSky.w,
//!   1 - sat((z - TerrainHeight.x) * TerrainHeight.y)), sampled where the light ray
//!   through the pixel hits the terrain; diffuse sky = Upper * ((1+N.U)/2)^2 * |N.V|^P
//!   + Lower * ((1-N.U)/2)^2, plus the "bounce" term with the reflection vector
//!   weighted by (1-|N.V|)^Pb (AmbSpec* material inputs)
//! * directional light: LightColor * shadow * (D * max(sat(N.L), 1e-6) + S * max(sat(R.L), 1e-6)^SpecPower)
//! * colours: FColor -> linear with pow(x, 2.2) (checked against the captured constants)

use bevy::{
    asset::RenderAssetUsages,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::{
        mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef, VertexFormat},
        render_resource::{AsBindGroup, Extent3d, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureDimension, TextureFormat},
        storage::ShaderStorageBuffer,
    },
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageFilterMode},
};

pub const MAX_TEX: usize = 8;

/// Light-map UV (the StaticMesh's LightMapCoordinateIndex channel; world.rs static_mesh).
pub const ATTR_LM_UV: MeshVertexAttribute = MeshVertexAttribute::new("UE_LightMapUV", 0x4f1a_0003, VertexFormat::Float32x2);

/// Packed UE3 TangentX / TangentZ, as Unorm8x4 (the shader rescales to the game's x*2/255-1).
pub const ATTR_TANGENT_X: MeshVertexAttribute = MeshVertexAttribute::new("UE_TangentX", 0x4f1a_0001, VertexFormat::Unorm8x4);
pub const ATTR_TANGENT_Z: MeshVertexAttribute = MeshVertexAttribute::new("UE_TangentZ", 0x4f1a_0002, VertexFormat::Unorm8x4);

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GameMatKey {
    pub shader: Handle<Shader>,
    pub prepass_shader: Option<Handle<Shader>>,
    pub cull: Option<Face>,
    /// light-mapped variant (shader_source_lightmap): the mesh's ATTR_LM_UV is a vertex input
    pub lightmap: bool,
}

impl From<&GameMat> for GameMatKey {
    fn from(m: &GameMat) -> Self {
        GameMatKey { shader: m.shader.clone(), prepass_shader: m.prepass_shader.clone(), cull: m.cull, lightmap: m.lightmap }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(GameMatKey)]
pub struct GameMat {
    #[storage(0, read_only)]
    pub globals: Handle<ShaderStorageBuffer>,
    #[texture(1)]
    #[sampler(2)]
    pub grass: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub t0: Option<Handle<Image>>,
    #[texture(5)]
    #[sampler(6)]
    pub t1: Option<Handle<Image>>,
    #[texture(7)]
    #[sampler(8)]
    pub t2: Option<Handle<Image>>,
    #[texture(9)]
    #[sampler(10)]
    pub t3: Option<Handle<Image>>,
    #[texture(11)]
    #[sampler(12)]
    pub t4: Option<Handle<Image>>,
    #[texture(13)]
    #[sampler(14)]
    pub t5: Option<Handle<Image>>,
    #[texture(15)]
    #[sampler(16)]
    pub t6: Option<Handle<Image>>,
    #[texture(17)]
    #[sampler(18)]
    pub t7: Option<Handle<Image>>,
    #[texture(19, dimension = "cube")]
    #[sampler(20)]
    pub cube: Option<Handle<Image>>,
    /// scenecopy.rs: scene colour + depth (alpha) after the opaque pass (DepthBiasedAlpha)
    #[texture(21)]
    pub scene: Option<Handle<Image>>,
    /// Light map (world.rs material_lightmapped): LightMapTexture2D of coefficient 0 (HQ) or
    /// 2 (LQ), and coefficient 1 (HQ only), one sampler (the game's LightAndShadowMap group)
    #[texture(22)]
    #[sampler(23)]
    pub lm0: Option<Handle<Image>>,
    #[texture(24)]
    pub lm1: Option<Handle<Image>>,
    pub lightmap: bool,
    pub shader: Handle<Shader>,
    pub prepass_shader: Option<Handle<Shader>>,
    pub alpha: AlphaMode,
    pub cull: Option<Face>,
}

impl GameMat {
    pub fn set_tex(&mut self, i: usize, h: Handle<Image>) {
        let slot = match i {
            0 => &mut self.t0,
            1 => &mut self.t1,
            2 => &mut self.t2,
            3 => &mut self.t3,
            4 => &mut self.t4,
            5 => &mut self.t5,
            6 => &mut self.t6,
            _ => &mut self.t7,
        };
        *slot = Some(h);
    }
}

impl Material for GameMat {
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vb = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            ATTR_TANGENT_X.at_shader_location(1),
            ATTR_TANGENT_Z.at_shader_location(2),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(3),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(4),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(5),
        ])?;
        // light-mapped variant: the light-map UV channel (StaticMesh LightMapCoordinateIndex)
        // (the depth pass with its own prepass shader has no light-map input)
        let own_prepass = key.mesh_key.contains(bevy::pbr::MeshPipelineKey::DEPTH_PREPASS) && key.bind_group_data.prepass_shader.is_some();
        let vb = if key.bind_group_data.lightmap && !own_prepass {
            layout.0.get_layout(&[
                Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
                ATTR_TANGENT_X.at_shader_location(1),
                ATTR_TANGENT_Z.at_shader_location(2),
                Mesh::ATTRIBUTE_UV_0.at_shader_location(3),
                Mesh::ATTRIBUTE_UV_1.at_shader_location(4),
                Mesh::ATTRIBUTE_COLOR.at_shader_location(5),
                ATTR_LM_UV.at_shader_location(6),
            ])?
        } else {
            vb
        };
        descriptor.vertex.buffers = vec![vb];
        descriptor.primitive.cull_mode = key.bind_group_data.cull;
        if key.mesh_key.contains(bevy::pbr::MeshPipelineKey::DEPTH_PREPASS) {
            let Some(ps) = key.bind_group_data.prepass_shader.clone() else {
                // not a caster (entities get NotShadowCaster); depth only with the main vertex code
                descriptor.vertex.shader = key.bind_group_data.shader.clone();
                descriptor.vertex.entry_point = "vertex".into();
                descriptor.fragment = None;
                crate::gamemat::canonical_defs(descriptor);
                return Ok(());
            };
            descriptor.vertex.shader = ps.clone();
            descriptor.vertex.entry_point = "vertex".into();
            descriptor.fragment = Some(bevy::render::render_resource::FragmentState {
                shader: ps,
                shader_defs: descriptor.vertex.shader_defs.clone(),
                entry_point: "fragment".into(),
                targets: vec![],
            });
            crate::gamemat::canonical_defs(descriptor);
            return Ok(());
        }
        descriptor.vertex.shader = key.bind_group_data.shader.clone();
        descriptor.vertex.entry_point = "vertex".into();
        if let Some(f) = descriptor.fragment.as_mut() {
            f.shader = key.bind_group_data.shader.clone();
            f.entry_point = "fragment".into();
        }
        crate::gamemat::canonical_defs(descriptor);
        Ok(())
    }
}

/// Shader defs of a GameMat / TerrainEmu pipeline reduced to the ones its WGSL can see.
///
/// Bevy's ShaderCache composes (naga_oil) one shader module per (shader, shader defs); the defs
/// come from the pipeline key (vertex colours present, MAY_DISCARD, blend mode, DEPTH_PREPASS,
/// DEPTH_CLAMP_ORTHO of the directional shadow pass, tonemapping, ...), so one generated shader
/// was composed again for every pass and mesh layout it is drawn with. Composing is the serial
/// part of pipeline creation (naga_oil ~4.6 ms per module on the container CPU for our ~12 KB
/// shaders, under the ShaderCache mutex; naga validation + SPIR-V ~0.5 ms). Our generated WGSL
/// has no #ifdef of its own (gamemat.rs resolves HF_MASKED / HF_UNLIT when generating) and
/// imports only `view`, `globals`, `get_world_from_local`, `get_tag`, which depend only on the
/// defs kept here; with the rest dropped every pass shares one module per shader.
/// NOT from data (renderer detail).
pub fn canonical_defs(d: &mut RenderPipelineDescriptor) {
    use bevy::render::render_resource::ShaderDefVal;
    let keep = |v: &ShaderDefVal| {
        let name = match v {
            ShaderDefVal::Bool(n, _) | ShaderDefVal::Int(n, _) | ShaderDefVal::UInt(n, _) => n.as_str(),
        };
        matches!(name, "PER_OBJECT_BUFFER_BATCH_SIZE" | "SIXTEEN_BYTE_ALIGNMENT" | "MESHLET_MESH_MATERIAL_PASS" | "NO_ARRAY_TEXTURES_SUPPORT" | "NO_CUBE_ARRAY_TEXTURES_SUPPORT" | "WEBGL2")
    };
    if std::env::var_os("HB_KEEP_SHADER_DEFS").is_some() {
        return;
    }
    d.vertex.shader_defs.retain(keep);
    if let Some(f) = d.fragment.as_mut() {
        f.shader_defs.retain(keep);
    }
}

/// Per-level shading constants shared by all materials (storage buffer).
#[derive(Clone, Copy, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Globals {
    pub upper_sky: [f32; 4],
    pub lower_sky: [f32; 4],
    pub light_color: [f32; 4],
    /// UE world direction towards the light; w = 1 / z (as the game uploads it).
    pub light_dir: [f32; 4],
    pub ambient_sky: [f32; 4],
    /// TerrainBounds: (MinX, 1/(MaxX-MinX), MinY, 1/(MaxY-MinY)); zero = no terrain.
    pub terrain_bounds: [f32; 4],
    /// x = 1 / TerrainLightingFalloffDistance
    pub misc: [f32; 4],
    /// weather.rs (day/night cycle and weather, NOT in the game; all zero = the level's own
    /// look): x sky darkening, y stars, z overcast desaturation, w lightning flash, applied to
    /// the sky dome / fog dome only (objects[].z = 1 / 2, main.rs)
    pub sky_fx: [f32; 4],
    /// weather.rs: rgb = 1 - sky colour multiplier, w = 1 - brightness of unlit backdrop cards
    /// (objects[].z = 3) and of MaterialTintColor (zero = unchanged)
    pub sky_tint: [f32; 4],
    /// weather.rs fog for translucent surfaces (drawn after the full-screen fog pass):
    /// rgb fog colour, w density (1/m; 0 = off); sun direction (Bevy), w height falloff;
    /// rgb sun in-scatter, w max opacity
    pub fog: [[f32; 4]; 3],
}

/// Per-object constants, indexed by MeshTag: x = TerrainHeight.x (terrain height below
/// the object's origin), y = TerrainHeight.y.
pub fn globals_buffer(g: &Globals, objects: &[[f32; 4]]) -> ShaderStorageBuffer {
    let mut bytes: Vec<u8> = bytemuck::bytes_of(g).to_vec();
    if objects.is_empty() {
        bytes.extend_from_slice(&[0u8; 16]);
    }
    for o in objects {
        bytes.extend_from_slice(bytemuck::cast_slice(o));
    }
    let mut b = ShaderStorageBuffer::new(&bytes, RenderAssetUsages::default());
    // crater.rs writes misc.yzw at runtime (queue.write_buffer needs COPY_DST)
    b.buffer_description.usage |= bevy::render::render_resource::BufferUsages::COPY_DST;
    b
}

pub fn ue_color(c: [u8; 4], brightness: f32) -> [f32; 3] {
    let g = |x: u8| (x as f32 / 255.0).powf(2.2) * brightness;
    [g(c[0]), g(c[1]), g(c[2])]
}

/// sunshadow::wgsl(), set once at startup (main.rs).
pub static SHADOW_WGSL: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

const PRELUDE: &str = r#"
#import bevy_pbr::mesh_view_bindings::{view, globals}
#import bevy_pbr::mesh_functions::{get_world_from_local, get_tag}

struct HfGlobals {
    upper_sky: vec4<f32>,
    lower_sky: vec4<f32>,
    light_color: vec4<f32>,
    light_dir: vec4<f32>,
    ambient_sky: vec4<f32>,
    terrain_bounds: vec4<f32>,
    misc: vec4<f32>,
    sky_fx: vec4<f32>,
    sky_tint: vec4<f32>,
    fog_color: vec4<f32>,
    fog_sun_dir: vec4<f32>,
    fog_sun: vec4<f32>,
    objects: array<vec4<f32>>,
}

@group(2) @binding(0) var<storage, read> hfg: HfGlobals;
@group(2) @binding(1) var grass_tex: texture_2d<f32>;
@group(2) @binding(2) var grass_smp: sampler;
@group(2) @binding(3) var t0: texture_2d<f32>;
@group(2) @binding(4) var s0: sampler;
@group(2) @binding(5) var t1: texture_2d<f32>;
@group(2) @binding(6) var s1: sampler;
@group(2) @binding(7) var t2: texture_2d<f32>;
@group(2) @binding(8) var s2: sampler;
@group(2) @binding(9) var t3: texture_2d<f32>;
@group(2) @binding(10) var s3: sampler;
@group(2) @binding(11) var t4: texture_2d<f32>;
@group(2) @binding(12) var s4: sampler;
@group(2) @binding(13) var t5: texture_2d<f32>;
@group(2) @binding(14) var s5: sampler;
@group(2) @binding(15) var t6: texture_2d<f32>;
@group(2) @binding(16) var s6: sampler;
@group(2) @binding(17) var t7: texture_2d<f32>;
@group(2) @binding(18) var s7: sampler;
@group(2) @binding(19) var tc0: texture_cube<f32>;
@group(2) @binding(20) var sc0: sampler;
@group(2) @binding(21) var scene_tex: texture_2d<f32>;

// MaterialExpressionSkyLightContribution (matcompile.rs): the ambient sky hemisphere of the
// lit path below for a UE world normal
fn hf_sky_light(n: vec3<f32>) -> vec3<f32> {
    let u = clamp(normalize(n).z, -1.0, 1.0);
    let up = (0.5 + 0.5 * u) * (0.5 + 0.5 * u);
    let dn = (0.5 - 0.5 * u) * (0.5 - 0.5 * u);
    return (hfg.upper_sky.rgb * up + hfg.lower_sky.rgb * dn) * hfg.ambient_sky.w;
}

// MaterialExpressionSceneTexture: opaque scene colour (scene copy rgb; alpha holds depth),
// clamped to [0, 4] as the game's base-pass shaders do with every SceneColorTexture sample
// (RefShaderCache M_Fire_Distortion PS: max(s, 0), min(s, 4))
fn hf_scene_color(uv: vec2<f32>) -> vec4<f32> {
    let p = vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(0.99999)) * view.viewport.zw);
    return vec4<f32>(clamp(textureLoad(scene_tex, p, 0).rgb, vec3<f32>(0.0), vec3<f32>(4.0)), 1.0);
}

// Final output. `scene_blend` (shader_source: translucent materials that read the scene colour,
// e.g. the heat haze sprites KFX_VHC M_Fire_Distortion / M_Exhaust_Distortion): the game's PS
// blends itself, oc0.rgb = lerp(S, E, opacity) with S = SceneColorTexture at the pixel, alpha 0
// (RefShaderCache M_Fire_Distortion), and UE3 draws such primitives after the other
// translucency with SceneColor resolved again, so S holds the smoke / fire behind. The copy
// here holds only the opaque scene: drawn as alpha blend over S it replaced the smoke behind with
// square holes ("squares in the black smoke", 2026-10-21). Instead the difference to the copy is
// added (AlphaMode::Add, alpha 0): dst + opacity * (E - S) = lerp(S, E, opacity) where nothing
// translucent is behind, and keeps what is (refracted opaque scene only; NOT from data).
// NaN / Inf -> 0, then at most 60000 (the HDR target is 16-bit float: 65504 max). A single
// non-finite pixel spread through bloom / eye adaptation and turned the whole screen red / pink /
// green / black for a few frames (user 2026-10-22 20:48, video). Bit test: WGSL compilers may
// assume finite floats and drop `x != x`.
fn hf_fin1(x: f32) -> f32 {
    return select(x, 0.0, (bitcast<u32>(x) & 0x7f800000u) == 0x7f800000u);
}
fn hf_fin3(c: vec3<f32>) -> vec3<f32> {
    return clamp(vec3<f32>(hf_fin1(c.x), hf_fin1(c.y), hf_fin1(c.z)), vec3<f32>(-60000.0), vec3<f32>(60000.0));
}

fn hf_final(c0: vec3<f32>, a0: f32, clip: vec4<f32>, scene_blend: bool) -> vec4<f32> {
    let c = hf_fin3(c0);
    let a = hf_fin1(a0);
    if (!scene_blend) { return vec4<f32>(c, a); }
    let s = clamp(hf_fin3(textureLoad(scene_tex, vec2<i32>(clip.xy), 0).rgb), vec3<f32>(0.0), vec3<f32>(4.0));
    return vec4<f32>(a * (c - s), 0.0);
}

// weather.rs (NOT in the game): night / overcast / lightning on the sky dome (flag 1) and the
// fog dome (flag 2); flag 0 or all-zero sky_fx / sky_tint leave the colour unchanged
fn hf_weather_sky(c: vec3<f32>, flag: f32, world: vec3<f32>) -> vec3<f32> {
    if (flag < 0.5 || flag > 2.5) { return c; }
    let f = hfg.sky_fx;
    let l = dot(c, vec3<f32>(0.3, 0.59, 0.11));
    var o = mix(c, vec3<f32>(l), f.z) * (vec3<f32>(1.0) - hfg.sky_tint.rgb) * (1.0 - f.x);
    if (flag < 1.5 && f.y > 0.0) {
        // stars: one per lit cell of a direction grid (NOT from data)
        let d = normalize(world - swz(view.world_position) * 100.0);
        if (d.z > 0.02) {
            let p = d * 220.0;
            let cell = floor(p);
            let h = fract(sin(vec3<f32>(dot(cell, vec3<f32>(127.1, 311.7, 74.7)), dot(cell, vec3<f32>(269.5, 183.3, 246.1)), dot(cell, vec3<f32>(113.5, 271.9, 124.6)))) * 43758.547);
            let q = fract(p) - 0.5 - (h.yzx - 0.5) * 0.5;
            let tw = 0.75 + 0.25 * sin(globals.time * (1.5 + 3.0 * h.y) + h.z * 6.28);
            let s = step(0.992, h.x) * smoothstep(0.16, 0.0, length(q)) * tw * smoothstep(0.02, 0.2, d.z);
            o += vec3<f32>(0.9, 0.95, 1.0) * s * f.y * (0.5 + 1.5 * h.y);
        }
    }
    return o + vec3<f32>(0.75, 0.8, 1.0) * f.w * (0.3 + l);
}

// weather.rs (NOT in the game): the scene brightness factor (sky_tint.rgb tint, sky_tint.w =
// 1 - sky-light fraction); 1 with the weather off (all zero)
fn hf_weather_dim() -> vec3<f32> {
    return (vec3<f32>(1.0) - hfg.sky_tint.rgb) * (1.0 - hfg.sky_tint.w);
}

// weather.rs: unlit backdrop cards (flag 3, SKY_Elements meshes such as the San Francisco
// skyline) and daylit particle sprites (flag 3, particles::night_dimmed) darken at night / in
// overcast like lit surfaces; overcast desaturates them. Flag 4: light-source sprites (fire,
// sparks, muzzle flashes), unchanged.
fn hf_weather_backdrop(c: vec3<f32>, flag: f32) -> vec3<f32> {
    if (flag < 2.5 || flag > 3.5) { return c; }
    let l = dot(c, vec3<f32>(0.3, 0.59, 0.11));
    return mix(c, vec3<f32>(l), hfg.sky_fx.z) * hf_weather_dim();
}

// weather.rs night class of unlit level surfaces (flag 0, not 1P; NOT from data): `daylit`
// (shader_source: MLM_Unlit opaque / masked / translucent material that does not light itself,
// e.g. the unlit translucent graffiti / burn decals) = its colour is a daylight look and darkens
// with the scene; additive glows (lamps) and modulate stay as they are
fn hf_daylit(c: vec3<f32>, flag: f32, fp: f32, daylit: bool) -> vec3<f32> {
    if (!daylit || flag > 0.5 || fp > 0.5) { return c; }
    return c * hf_weather_dim();
}

// weather.rs fog for translucent surfaces (drawn after the full-screen fog pass; NOT from
// data): mode 0 opaque / modulate (unchanged), 1 alpha blend, 2 additive; not on the sky / fog
// dome (flag 1 / 2)
fn hf_fog_out(c: vec3<f32>, flag: f32, world: vec3<f32>, mode: u32) -> vec3<f32> {
    if (mode == 0u || (flag > 0.5 && flag < 2.5)) { return c; }
    let f = hf_translucent_fog(world);
    if (mode == 1u) { return mix(c, f.rgb, f.a); }
    return c * (1.0 - f.a);
}

// MaterialExpressionMaterialTintColor (matcompile.rs): the engine's GlobalMaterialTintColor
// vector parameter (EXE name table; default (1,1,1,1) in the cached uniform expressions, e.g.
// ATW_JAV M_Scope_Lens01). NOT from data: weather.rs drives it with the scene brightness, so
// the scope lenses (dust / reflection terms scaled by it) darken at night.
fn hf_material_tint() -> vec4<f32> {
    return vec4<f32>(hf_weather_dim(), 1.0);
}

// cube map samples (matcompile.rs): the static environment reflections (ENV_Reflections
// ENV_LEVEL_CUBE_REFLECTION etc., captured in daylight). NOT from data: darkened with the
// weather's scene brightness (scope lenses, water, glass stayed bright at night)
fn hf_env_cube(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * hf_weather_dim(), c.a);
}

struct MatIn {
    uv: array<vec2<f32>, 4>,
    vcolor: vec4<f32>,
    cam: vec3<f32>,
    refl: vec3<f32>,
    refl_world: vec3<f32>,
    light: vec3<f32>,
    light_color: vec3<f32>,
    world_pos: vec3<f32>,
    camera_world: vec3<f32>,
    tangent_to_world: mat3x3<f32>,
    screen: vec2<f32>,
    depth: f32,
    time: f32,
    // view-space depth (UE units) of the pixel and of the opaque scene behind it
    pixel_w: f32,
    scene_w: f32,
}

struct MatOut {
    diffuse: vec3<f32>,
    specular: vec3<f32>,
    spec_power: f32,
    emissive: vec3<f32>,
    opacity: f32,
    opacity_mask: f32,
    custom: vec3<f32>,
    diffuse_power: f32,
    amb_spec_power: f32,
    amb_spec_bounce_power: f32,
    amb_spec_bounce_amt: f32,
    amb_spec_color_contrib: f32,
}

// UE world (Z up, centimetres) <-> Bevy world (Y up, metres): bevy = 0.01 * swap_yz(ue)
fn swz(v: vec3<f32>) -> vec3<f32> { return vec3<f32>(v.x, v.z, v.y); }

fn inverse3(m: mat3x3<f32>) -> mat3x3<f32> {
    let a = m[0]; let b = m[1]; let c = m[2];
    let r0 = cross(b, c); let r1 = cross(c, a); let r2 = cross(a, b);
    let det = dot(a, r0);
    return transpose(mat3x3<f32>(r0, r1, r2)) * (1.0 / det);
}

struct VIn {
    @builtin(instance_index) instance: u32,
    @location(0) pos: vec3<f32>,
    @location(1) tx: vec4<f32>,
    @location(2) tz: vec4<f32>,
    @location(3) uv0: vec2<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) color: vec4<f32>,
    // HF_LM_VIN
}

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) cam_t: vec3<f32>,
    @location(2) light_t: vec3<f32>,
    @location(3) up_t: vec4<f32>,
    @location(4) uv: vec4<f32>,
    @location(5) grass_uv: vec2<f32>,
    @location(6) color: vec4<f32>,
    @location(7) tw0: vec3<f32>,
    @location(8) tw1: vec3<f32>,
    @location(9) tw2: vec3<f32>,
    // weather.rs sky flag (objects[].z: 1 sky dome, 2 fog dome)
    @location(10) sky: f32,
    // 1P mesh (objects[].w = 1, viewmodel.rs)
    @location(11) fp: f32,
    // HF_LM_VOUT
}

@vertex
fn vertex(v: VIn) -> VOut {
    var o: VOut;
    let wfl = get_world_from_local(v.instance);
    let world_b = wfl * vec4<f32>(v.pos, 1.0);
    o.clip = view.clip_from_world * world_b;
    // UE-space LocalToWorld: L2W = C^-1 * wfl * W  (mesh positions are swz(ue_local))
    let m3 = mat3x3<f32>(wfl[0].xyz, wfl[1].xyz, wfl[2].xyz);
    let l2w = mat3x3<f32>(swz(m3 * vec3<f32>(1.0, 0.0, 0.0)), swz(m3 * vec3<f32>(0.0, 0.0, 1.0)), swz(m3 * vec3<f32>(0.0, 1.0, 0.0))) * 100.0;
    let w2l = inverse3(l2w);
    let world = swz(world_b.xyz) * 100.0;
    o.world = world;
    let cam = swz(view.world_position) * 100.0;

    // game: x * 2/255 - 1 on the raw UBYTE4 values
    let tx = (v.tx * 255.0) * 0.00784314 - 1.0;
    let tz = (v.tz * 255.0) * 0.00784314 - 1.0;
    let tzv = tz.xyz;
    let tyv = cross(tzv, tx.xyz) * tz.w;
    let cam_l = w2l * (cam - world);
    o.cam_t = vec3<f32>(dot(tx.xyz, cam_l), dot(tyv, cam_l), dot(tzv, cam_l));
    let ld = hfg.light_dir;
    let l_l = w2l * ld.xyz;
    let lt = vec3<f32>(dot(tx.xyz, l_l), dot(tyv, l_l), dot(tzv, l_l));
    o.light_t = lt;
    let up_l = vec3<f32>(w2l[0].z, w2l[1].z, w2l[2].z);
    let ut = vec3<f32>(dot(tx.xyz, up_l), dot(tyv, up_l), dot(tzv, up_l));
    // TerrainHeight per object (MeshTag index)
    let th = hfg.objects[get_tag(v.instance)];
    // 1P meshes (viewmodel.rs, objects[].w = 1) are SDPG_Foreground in UE3 (drawn after the world
    // with their own depth): here their reverse-Z depth is squeezed into the nearest tenth,
    // and their near plane moves 10x closer (aiming puts the sights 5-9 units ahead of the eye,
    // inside the 10-unit near plane: examples/posecmp M4 ADS_idle crosshair at z 7.06)
    if th.w > 0.5 {
        o.clip.z = o.clip.w * (0.9 + 0.01 * (o.clip.z / o.clip.w));
    }
    o.fp = th.w;
    let h = world.z - th.x;
    o.up_t = vec4<f32>(ut, h * th.y);
    o.sky = th.z;
    let tb = hfg.terrain_bounds;
    let refl_z = clamp(lt.z - 2.0 * dot(lt, ut) * ut.z, 0.0, 1.0);
    let off = (h * ld.w) * ld.xy * vec2<f32>(tb.y, tb.w) * refl_z;
    o.grass_uv = (world.xy - vec2<f32>(tb.x, tb.z)) * vec2<f32>(tb.y, tb.w) + off;
    o.uv = vec4<f32>(v.uv0, v.uv1);
    o.color = v.color;
    // HF_LM_VS
    // tangent basis in UE world space (for Transform nodes / cube reflections)
    o.tw0 = l2w * tx.xyz;
    o.tw1 = l2w * tyv;
    o.tw2 = l2w * tzv;
    return o;
}
"#;

const FRAGMENT: &str = r#"
@fragment
fn fragment(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var m: MatIn;
    m.uv = array<vec2<f32>, 4>(i.uv.xy, i.uv.zw, i.uv.xy, i.uv.xy);
    m.vcolor = i.color;
    let V = normalize(i.cam_t);
    let L = normalize(i.light_t);
    let U = normalize(i.up_t.xyz);
    m.cam = V;
    m.light = L;
    m.light_color = hfg.light_color.rgb;
    m.world_pos = i.world;
    m.camera_world = swz(view.world_position) * 100.0;
    m.tangent_to_world = mat3x3<f32>(normalize(i.tw0), normalize(i.tw1), normalize(i.tw2));
    m.screen = i.clip.xy / view.viewport.zw;
    m.time = globals.time;
    m.pixel_w = -(view.view_from_world * vec4<f32>(swz(i.world) * 0.01, 1.0)).z * 100.0;
    // PixelDepth = ScreenPosition.w (view depth), e.g. GRay_Test PS 11d676de… (w / 300)
    m.depth = m.pixel_w;
    // as the game decodes SceneColorTexture alpha with MinZ_MaxZRatio (x, 0): z = x / a,
    // 1e6 for a < 1e-6 or z <= 2
    let scene_a = textureLoad(scene_tex, vec2<i32>(i.clip.xy), 0).a;
    let scene_inv = select(1000000.0, 1.0 / scene_a, scene_a - 0.000001 >= 0.0);
    let scene_z = scene_inv * HF_MINZ;
    m.scene_w = select(scene_z, 1000000.0, 2.0 - scene_z >= 0.0);
    m.refl = vec3<f32>(-V.x, -V.y, V.z);
    m.refl_world = m.tangent_to_world * m.refl;

    var n = mat_normal(m);
    if (dot(n, n) <= 0.0) { n = vec3<f32>(0.0, 0.0, 1.0); }
    let N = normalize(n);
    let NV = dot(N, V);
    let R = 2.0 * NV * N - V;
    m.refl = R;
    m.refl_world = m.tangent_to_world * R;
    var o = mat_eval(m);
    // levelcars.rs burnt car (objects[].z = 5; NOT from data: the jiggle cars have no burnt
    // material, SP swaps in *_Wreck / *_DEST MICs): sooty, nearly black, little shine
    if (HF_FLAG > 4.5 && HF_FLAG < 5.5) {
        let bl = dot(o.diffuse, vec3<f32>(0.3, 0.59, 0.11));
        o.diffuse = mix(o.diffuse, vec3<f32>(bl), 0.85) * 0.12;
        o.specular = o.specular * 0.08;
        o.emissive = o.emissive * 0.1;
    }

#ifdef HF_MASKED
    if (o.opacity_mask < HF_MASK_CLIP) { discard; }
#endif

#ifdef HF_UNLIT
    return hf_final(hf_fog_out(hf_weather_sky(hf_weather_backdrop(hf_daylit(o.emissive * HF_ADD_OPACITY, HF_FLAG, i.fp, HF_DAYLIT), HF_FLAG), HF_FLAG, i.world), HF_FLAG, i.world, HF_FOG_MODE), HF_ALPHA, i.clip, HF_SCENE_BLEND);
#else
    // lower sky colour from the terrain grass colour
    var lower = hfg.lower_sky.rgb;
    if (i.grass_uv.x > 0.0) {
        let g = textureSample(grass_tex, grass_smp, i.grass_uv).rgb;
        lower = mix(hfg.lower_sky.rgb, g * hfg.lower_sky.w, 1.0 - clamp(i.up_t.w, 0.0, 1.0));
    }
    let upper = hfg.upper_sky.rgb;
    let NU = dot(U, N);
    let RU = dot(U, R);
    let nup = (0.5 + 0.5 * NU) * (0.5 + 0.5 * NU);
    let ndn = (0.5 - 0.5 * NU) * (0.5 - 0.5 * NU);
    let rup = (0.5 + 0.5 * RU) * (0.5 + 0.5 * RU);
    let rdn = (0.5 - 0.5 * RU) * (0.5 - 0.5 * RU);
    let anv = abs(NV);
    let fa = pow(max(anv, 0.000001), o.amb_spec_power);
    let fb = pow(max(abs(1.0 - anv), 0.000001), o.amb_spec_bounce_power);
    let D = o.diffuse;
    let Dsky = D * (1.0 + o.specular * o.amb_spec_color_contrib);
    let sky = Dsky * (upper * nup * fa + lower * ndn) + o.amb_spec_bounce_amt * fb * Dsky * (upper * rup + lower * rdn);
    var c = o.emissive + sky * hfg.ambient_sky.w;
    // 1P meshes: their squeezed depth does not give their world position; the shadow is looked
    // up at the real position, so the gun / arms are in shadow indoors (NOT from data: UE3 lit
    // them through a light environment; here they were moon-/sun-lit everywhere: shiny at night)
    let shadow = select(hf_sun_shadow(i.clip), hf_sun_shadow_at(swz(i.world) * 0.01), i.fp > 0.5);
    let NL = max(clamp(dot(N, L), 0.0, 1.0), 0.000001);
    let RL = max(clamp(dot(R, L), 0.0, 1.0), 0.000001);
    c += (D * NL + o.specular * pow(RL, o.spec_power)) * shadow * hfg.light_color.rgb;
    c += D * hfg.ambient_sky.rgb;
    // HF_LM_PS
    // dynamic point lights (explosions, C4 blink: dynlights.rs)
    c += D * hf_dyn_lights(i.clip, i.world, normalize(m.tangent_to_world * N));
    return hf_final(hf_fog_out(hf_weather_sky(c, HF_FLAG, i.world), HF_FLAG, i.world, HF_FOG_MODE), HF_ALPHA, i.clip, HF_SCENE_BLEND);
#endif
}
"#;

/// MinZ_MaxZRatio.x for the scene copy's depth encoding (scenecopy.rs)
fn minz() -> String {
    format!("{:?}", 100.0 * crate::scenecopy::NEAR * crate::scenecopy::DEPTH_SCALE)
}

/// Shadow depth pass variant: same vertex code; fragment only evaluates the opacity mask.
pub fn prepass_source(c: &upk::matcompile::Compiled) -> String {
    use upk::level::Blend;
    let masked = matches!(c.info.blend, Blend::Masked);
    let mut frag = String::from("@fragment\nfn fragment(i: VOut, @builtin(front_facing) front: bool) {\n");
    if masked {
        let start = FRAGMENT.find("    var m: MatIn;").unwrap();
        let end = FRAGMENT.find("#ifdef HF_MASKED").unwrap();
        // HF_FLAG (burnt-car darkening, levelcars.rs) only changes colours: 0 in the depth pass.
        // It was left unreplaced: 33 masked materials (vines, fences, wires, ...) failed to compile
        // their shadow / depth pipeline ("no definition in scope for identifier: HF_FLAG").
        frag.push_str(&FRAGMENT[start..end].replace("HF_MINZ", &minz()).replace("HF_FLAG", "0.0"));
        frag.push_str(&format!("    if (o.opacity_mask < {:?}) {{ discard; }}\n", c.info.mask_clip));
    }
    frag.push_str("}\n");
    format!("{}\n{}\n{}\n{frag}", prepass_imports(PRELUDE), crate::weather::TRANSLUCENT_FOG_WGSL, c.code)
}

/// The prepass view layout has `globals` at binding 1 (main pass: 11).
pub fn prepass_imports(src: &str) -> String {
    src.replace(
        "#import bevy_pbr::mesh_view_bindings::{view, globals}",
        "#import bevy_pbr::mesh_view_bindings::view\n#import bevy_render::globals::Globals\n@group(0) @binding(1) var<uniform> globals: Globals;",
    )
}

/// Full WGSL module for one compiled material.
pub fn shader_source(c: &upk::matcompile::Compiled) -> String {
    use upk::level::Blend;
    let mut defs = String::new();
    let unlit = c.lighting == "MLM_Unlit" || matches!(c.info.blend, Blend::Additive | Blend::Modulate);
    // AlphaMode::Add is Bevy's premultiplied blend (One, OneMinusSrcAlpha): alpha 0 gives
    // UE3's BLEND_Additive (One, One)
    let alpha = match c.info.blend {
        Blend::Translucent => "clamp(o.opacity, 0.0, 1.0)",
        Blend::Additive => "0.0",
        _ => "1.0",
    };
    // scene-colour translucency (hf_final): its colour comes from the scene copy, which already
    // has the fog and the weather's dimming: no fog / night class on top (NOT from data)
    let scene_blend = scene_blend(c);
    let mut frag = FRAGMENT.replace("HF_MINZ", &minz()).replace("HF_ADD_OPACITY", if matches!(c.info.blend, Blend::Additive) { "o.opacity" } else { "1.0" }).replace("HF_ALPHA", alpha).replace("HF_FOG_MODE", match c.info.blend {
        _ if scene_blend => "0u",
        Blend::Translucent => "1u",
        Blend::Additive => "2u",
        _ => "0u",
    }).replace("HF_MASK_CLIP", &format!("{:?}", c.info.mask_clip)).replace("HF_DAYLIT", if daylit_unlit(c) && !scene_blend { "true" } else { "false" })
    .replace("HF_FLAG", if scene_blend { "0.0" } else { "i.sky" })
    .replace("HF_SCENE_BLEND", if scene_blend { "true" } else { "false" });
    // poor man's preprocessor for the two switches
    let keep = |frag: &mut String, name: &str, on: bool| {
        let start_tag = format!("#ifdef {name}\n");
        while let Some(s) = frag.find(&start_tag) {
            let rest = &frag[s + start_tag.len()..];
            let end = rest.find("#endif\n").unwrap();
            let body = &rest[..end];
            let (yes, no) = match body.find("#else\n") {
                Some(e) => (&body[..e], &body[e + 6..]),
                None => (body, ""),
            };
            let repl = if on { yes.to_string() } else { no.to_string() };
            let tail = rest[end + 7..].to_string();
            frag.truncate(s);
            frag.push_str(&repl);
            frag.push_str(&tail);
        }
    };
    keep(&mut frag, "HF_MASKED", matches!(c.info.blend, Blend::Masked));
    keep(&mut frag, "HF_UNLIT", unlit);
    defs.push_str(PRELUDE);
    defs.push_str(crate::weather::TRANSLUCENT_FOG_WGSL);
    defs.push_str(&SHADOW_WGSL.read().map(|g| g.clone()).unwrap_or_default());
    defs.push_str(crate::dynlights::WGSL);
    defs.push_str("\n");
    defs.push_str(&c.code);
    defs.push_str("\n");
    defs.push_str(&frag);
    defs
}

/// Translucent material that reads the scene colour (SceneColor node: heat haze / distortion
/// sprites, refracting glass or water): drawn with `hf_final`'s scene blend and AlphaMode::Add.
pub fn scene_blend(c: &upk::matcompile::Compiled) -> bool {
    matches!(c.info.blend, upk::level::Blend::Translucent) && c.code.contains("hf_scene_color(")
}

/// Bevy blend mode of a graph-compiled material (world.rs build_material).
pub fn alpha_mode(c: &upk::matcompile::Compiled) -> AlphaMode {
    use upk::level::Blend;
    match c.info.blend {
        Blend::Opaque => AlphaMode::Opaque,
        Blend::Masked => AlphaMode::Opaque, // discard in the shader
        // premultiplied add of hf_final's difference term (alpha 0 = dst + src)
        Blend::Translucent if scene_blend(c) => AlphaMode::Add,
        Blend::Translucent => AlphaMode::Blend,
        Blend::Additive => AlphaMode::Add,
        Blend::Modulate => AlphaMode::Multiply,
    }
}

/// Night class of an unlit level material (hf_daylit; weather.rs only, NOT from data): an
/// MLM_Unlit opaque / masked / translucent material whose colour is not produced by the lighting
/// (no SkyLightContribution / light colour, no env cube or MaterialTintColor - those are dimmed
/// where they are sampled - and no scene colour, which is already dark). Additive (lamp glows,
/// light cards: light sources) and modulate materials are not.
pub fn daylit_unlit(c: &upk::matcompile::Compiled) -> bool {
    use upk::level::Blend;
    c.lighting == "MLM_Unlit"
        && matches!(c.info.blend, Blend::Opaque | Blend::Masked | Blend::Translucent)
        && !["hf_sky_light", "m.light_color", "hf_env_cube", "hf_material_tint", "hf_scene_color"].iter().any(|k| c.code.contains(k))
}

/// Light-mapped variant of `shader_source` (static meshes whose component LOD has a 2D light
/// map, upk level.rs LightMap2D). From the game's FLocalVertexFactory base-pass shaders
/// (RefShaderCache, M_Opaque, examples/lmps):
/// * VS: LightMapCoordinate = LightMapUV * ScaleBias.xy + ScaleBias.wz (UE3 packs
///   (CoordinateScale.xy, CoordinateBias.yx)), i.e. uv * CoordinateScale + CoordinateBias
/// * PS FDirectionalLightWithSimpleLightMapTexturePolicy (LQ, mode 1):
///   Diffuse * max(abs(LightMapTextures.rgb * LightMapScale.rgb), 1e-6)
/// * PS FDirectionalLightWithLightMapPolicy (HQ, mode 2):
///   Diffuse * (LightMapTextures[0].rgb * LightMapScale[0].rgb) * dot(LightMapTextures[1].rgb * LightMapScale[1].rgb, 1/3)
/// added to emissive + sky + sun (the sun stays dynamic: the light maps hold only the baked
/// point / spot lights, examples/lmguids). Per object: objects[tag + 1] = (CoordinateScale,
/// CoordinateBias), objects[tag + 2] = (LightMapScale[0 or LQ], mode), objects[tag + 3] =
/// (LightMapScale[1], 0) (world.rs push_object_lightmap).
/// NOT from data: the light map is scaled by the weather's scene brightness (hf_weather_dim,
/// as the daylight env cubes): the bake includes blue "daylight fill" point lights
/// (LightColor 174,190,202) next to the warm lamps, so at night it dims with the sky light.
/// None when the base source lacks the markers.
pub fn shader_source_lightmap(c: &upk::matcompile::Compiled) -> Option<String> {
    let src = shader_source(c);
    let marks = ["    // HF_LM_VIN\n", "    // HF_LM_VOUT\n", "    // HF_LM_VS\n", "    // HF_LM_PS\n"];
    if marks.iter().any(|m| src.matches(m).count() != 1) {
        return None;
    }
    let decl = "@group(2) @binding(22) var lm_t0: texture_2d<f32>;\n@group(2) @binding(23) var lm_s: sampler;\n@group(2) @binding(24) var lm_t1: texture_2d<f32>;\n";
    let src = src
        .replacen(marks[0], "    @location(6) lm_uv: vec2<f32>,\n", 1)
        .replacen(marks[1], "    @location(12) lm_uv: vec2<f32>,\n    @location(13) @interpolate(flat) lm_tag: u32,\n", 1)
        .replacen(marks[2], "    let lm_sb = hfg.objects[get_tag(v.instance) + 1u];\n    o.lm_uv = v.lm_uv * lm_sb.xy + lm_sb.zw;\n    o.lm_tag = get_tag(v.instance);\n", 1)
        .replacen(
            marks[3],
            "    {\n        let lm_a = hfg.objects[i.lm_tag + 2u];\n        let lm_b = hfg.objects[i.lm_tag + 3u];\n        let lm0 = textureSample(lm_t0, lm_s, i.lm_uv).rgb * lm_a.rgb;\n        let lm1 = textureSample(lm_t1, lm_s, i.lm_uv).rgb * lm_b.rgb;\n        let lm = select(max(abs(lm0), vec3<f32>(0.000001)), lm0 * dot(lm1, vec3<f32>(0.33333334)), lm_a.w > 1.5);\n        c += D * lm * hf_weather_dim();\n    }\n",
            1,
        );
    // bindings next to the material's own (after the scene texture)
    let anchor = "@group(2) @binding(21) var scene_tex: texture_2d<f32>;\n";
    if !src.contains(anchor) {
        return None;
    }
    Some(src.replacen(anchor, &format!("{anchor}{decl}"), 1))
}

/// Load a Texture2D (export) as an RGBA8 image with a CPU-built mip chain and the
/// texture's own address modes. sRGB textures use an sRGB format (hardware decode, as
/// the game's D3DSAMP_SRGBTEXTURE).
pub fn load_texture(
    pkg: &upk::Package,
    lib: &upk::library::Library,
    object: i32,
    address: [upk::matcompile::Address; 2],
    max_dim: u32,
) -> Option<Image> {
    let texture_classes = ["Texture2D", "TerrainWeightMapTexture", "TerrainNormalMapTexture", "LightMapTexture2D"];
    if object <= 0 || !texture_classes.contains(&pkg.export_class(&pkg.exports[object as usize - 1]).as_str()) {
        return None;
    }
    let idx = object as usize - 1;
    let mut tx = pkg.read_texture2d(idx)?;
    tx.load_full(lib, &pkg.source_package(idx), max_dim);
    let srgb = !matches!(tx.props.get("SRGB"), Some(upk::props::Value::Bool(false)));
    if let Some(img) = bc_image(&mut tx, lib, &pkg.source_package(idx), max_dim, srgb, address) {
        return Some(img);
    }
    let (w, h, px) = tx.decode_rgba()?;
    // no mip chain where the game has none (NumMips 1 / CompressionNoMipmaps: KFX_LensFX Bloom01 /
    // Bloom02, holoHair). The sight crosshairs sample them in screen space at ~60 texels per
    // pixel: built mips picked a 4x4 level whose blurred edge texels, clamped, drew a grey cross
    // through every sight.
    let one_mip = matches!(tx.props.get("CompressionNoMipmaps"), Some(upk::props::Value::Bool(true))) || tx.props.int("NumMips") == Some(1);
    let mut data = px.clone();
    let (mut cw, mut ch, mut cur) = (w as usize, h as usize, px);
    let mut levels = 1u32;
    while (cw > 1 || ch > 1) && !one_mip {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for k in 0..4 {
                    let mut s = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(cw - 1);
                        let sy = (y * 2 + dy).min(ch - 1);
                        s += cur[(sy * cw + sx) * 4 + k] as u32;
                    }
                    next[(y * nw + x) * 4 + k] = ((s + 2) / 4) as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        cur = next;
        cw = nw;
        ch = nh;
        levels += 1;
    }
    let mut img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::default(),
    );
    img.texture_descriptor.mip_level_count = levels;
    img.sampler = load_texture_sampler(address)?;
    Some(img)
}

/// The GPU samples BC1-3 (DXT1/3/5) natively (wgpu TEXTURE_COMPRESSION_BC); set at startup.
pub static BC_SUPPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn detect_bc(dev: Res<bevy::render::renderer::RenderDevice>) {
    let ok = dev.features().contains(bevy::render::settings::WgpuFeatures::TEXTURE_COMPRESSION_BC) && std::env::var_os("HB_NO_BC").is_none();
    BC_SUPPORTED.store(ok, std::sync::atomic::Ordering::Relaxed);
    info!("BC texture compression: {ok}");
}

/// DXT textures uploaded as stored (the game's own mip chain, like D3D9 does), instead of
/// decoding to RGBA8 and rebuilding mips on the CPU: 4-8x less memory and upload.
fn bc_image(tx: &mut upk::texture::Texture2D, lib: &upk::library::Library, src: &str, max_dim: u32, srgb: bool, address: [upk::matcompile::Address; 2]) -> Option<Image> {
    if !BC_SUPPORTED.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let (fmt, block) = match tx.format.as_str() {
        "PF_DXT1" => (if srgb { TextureFormat::Bc1RgbaUnormSrgb } else { TextureFormat::Bc1RgbaUnorm }, 8usize),
        "PF_DXT3" => (if srgb { TextureFormat::Bc2RgbaUnormSrgb } else { TextureFormat::Bc2RgbaUnorm }, 16),
        "PF_DXT5" => (if srgb { TextureFormat::Bc3RgbaUnormSrgb } else { TextureFormat::Bc3RgbaUnorm }, 16),
        _ => return None,
    };
    let top = tx.mips.iter().position(|m| m.width <= max_dim && m.height <= max_dim && !m.bulk.data.is_empty())?;
    let (w, h) = (tx.mips[top].width, tx.mips[top].height);
    if w % 4 != 0 || h % 4 != 0 || w == 0 || h == 0 {
        return None;
    }
    let mut data = Vec::new();
    let mut levels = 0u32;
    let (mut ew, mut eh) = (w, h);
    for k in top..tx.mips.len() {
        tx.load_mip(lib, src, k);
        let m = &tx.mips[k];
        if m.width != ew || m.height != eh {
            break;
        }
        let need = (ew as usize).div_ceil(4) * (eh as usize).div_ceil(4) * block;
        if m.bulk.data.len() < need {
            break;
        }
        data.extend_from_slice(&m.bulk.data[..need]);
        levels += 1;
        if ew == 1 && eh == 1 {
            break;
        }
        ew = (ew / 2).max(1);
        eh = (eh / 2).max(1);
    }
    if levels == 0 {
        return None;
    }
    let mut img = Image::new_uninit(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, fmt, RenderAssetUsages::default());
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels;
    img.sampler = load_texture_sampler(address)?;
    Some(img)
}

/// Sampler for a texture with the given address modes (trilinear, anisotropic 8).
pub fn load_texture_sampler(address: [upk::matcompile::Address; 2]) -> Option<ImageSampler> {
    let am = |a: upk::matcompile::Address| match a {
        upk::matcompile::Address::Wrap => ImageAddressMode::Repeat,
        upk::matcompile::Address::Clamp => ImageAddressMode::ClampToEdge,
        upk::matcompile::Address::Mirror => ImageAddressMode::MirrorRepeat,
    };
    Some(ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: am(address[0]),
        address_mode_v: am(address[1]),
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        // the options' anisotropy (settings.rs; the user's MaxAnisotropy 8)
        anisotropy_clamp: crate::settings::anisotropy(),
        ..default()
    }))
}

/// Validate a generated module before handing it to Bevy (an invalid shader would take the
/// whole pipeline cache down). Bevy's imports are replaced by stubs with the same types.
pub fn validate_wgsl(src: &str) -> Result<(), String> {
    let _perf = crate::perf::scope("ld_validate_wgsl");
    // a source validated before (this run or an earlier one, homebots_wgsl_ok.txt in the temp
    // folder) is not parsed again: validation was a third of the level build (927 calls for
    // Crossroads, many with the same source)
    let h = fnv64(src);
    if VALID_OK.lock().is_ok_and(|mut m| m.get_or_insert_with(load_valid_ok).contains(&h)) {
        return Ok(());
    }
    let r = validate_wgsl_uncached(src);
    if r.is_ok() {
        if let Ok(mut m) = VALID_OK.lock() {
            if m.get_or_insert_with(load_valid_ok).insert(h) {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(valid_ok_path()) {
                    let _ = writeln!(f, "{h:016x}");
                }
            }
        }
    }
    r
}

static VALID_OK: std::sync::Mutex<Option<std::collections::HashSet<u64>>> = std::sync::Mutex::new(None);

/// Results are kept per program version: a new build validates everything once again.
fn valid_ok_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().ok().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    std::env::temp_dir().join(format!("homebots_wgsl_ok_{exe}.txt"))
}

fn load_valid_ok() -> std::collections::HashSet<u64> {
    std::fs::read_to_string(valid_ok_path()).map(|t| t.lines().filter_map(|l| u64::from_str_radix(l.trim(), 16).ok()).collect()).unwrap_or_default()
}

fn fnv64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn validate_wgsl_uncached(src: &str) -> Result<(), String> {
    const STUB: &str = r#"
struct View { clip_from_world: mat4x4<f32>, world_from_clip: mat4x4<f32>, world_from_view: mat4x4<f32>, view_from_world: mat4x4<f32>, clip_from_view: mat4x4<f32>, world_position: vec3<f32>, viewport: vec4<f32> }
struct Globals { time: f32 }
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> globals: Globals;
fn get_world_from_local(i: u32) -> mat4x4<f32> { return mat4x4<f32>(); }
fn get_tag(i: u32) -> u32 { return 0u; }
"#;
    let stub = format!("{STUB}{}{}", crate::sunshadow::STUBS, crate::dynlights::STUBS);
    let body: String = src.lines().filter(|l| !l.trim_start().starts_with("#import")).collect::<Vec<_>>().join("\n");
    let full = format!("{stub}\n{body}");
    let m = naga::front::wgsl::parse_str(&full).map_err(|e| e.emit_to_string(&full))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&m)
        .map(|_| ())
        .map_err(|e| e.emit_to_string(&full))
}

/// Decoded RGBA8 of a Texture2D export (for cube faces).
pub fn decode_texture(pkg: &upk::Package, lib: &upk::library::Library, idx: usize, max_dim: u32) -> Option<(u32, u32, Vec<u8>, bool)> {
    let mut tx = pkg.read_texture2d(idx)?;
    tx.load_full(lib, &pkg.source_package(idx), max_dim);
    let (w, h, px) = tx.decode_rgba()?;
    let srgb = !matches!(tx.props.get("SRGB"), Some(upk::props::Value::Bool(false)));
    Some((w, h, px, srgb))
}

/// A TextureCube from six decoded faces (+X, -X, +Y, -Y, +Z, -Z) with a CPU mip chain.
/// D3D and wgpu share the cube face convention, and the game's shaders sample it with UE
/// world-space directions.
pub fn cube_image(faces: Vec<(u32, u32, Vec<u8>, bool)>) -> Option<Image> {
    use bevy::render::render_resource::{TextureViewDescriptor, TextureViewDimension};
    let size = faces.first()?.0;
    if faces.len() != 6 || faces.iter().any(|f| f.0 != size || f.1 != size) {
        return None;
    }
    let srgb = faces[0].3;
    let mut data = Vec::new();
    let mut levels = 0;
    for (_, _, px, _) in faces {
        let (mut cw, mut cur) = (size as usize, px);
        data.extend_from_slice(&cur);
        levels = 1;
        while cw > 1 {
            let nw = cw / 2;
            let mut next = vec![0u8; nw * nw * 4];
            for y in 0..nw {
                for x in 0..nw {
                    for k in 0..4 {
                        let s: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|(dx, dy)| cur[((y * 2 + dy) * cw + x * 2 + dx) * 4 + k] as u32).sum();
                        next[(y * nw + x) * 4 + k] = ((s + 2) / 4) as u8;
                    }
                }
            }
            data.extend_from_slice(&next);
            cur = next;
            cw = nw;
            levels += 1;
        }
    }
    let mut img = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::default(),
    );
    img.texture_descriptor.mip_level_count = levels;
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    img.sampler = load_texture_sampler([upk::matcompile::Address::Clamp; 2])?;
    Some(img)
}
