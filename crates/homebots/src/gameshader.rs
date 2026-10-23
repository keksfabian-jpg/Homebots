//! Static, skeletal and spline meshes rendered with the game's own compiled base-pass
//! shaders (RefShaderCache-PC-D3D-SM3.upk), translated by `upk::sm3`.
//!
//! The material's cooked FMaterialResource gives the shader-cache record (vertex factory ->
//! shaders) and the uniform expressions behind `UniformVector_N` / `UniformScalar(s)_N` /
//! `Texture2D_N` / `TextureCube_N`; parameter values come from the instance chain.
//! Global constants are bound by name; the values were measured in the SP-Devil capture:
//! * PrimaryLightColor = LightColor (sun colour^2.2 * brightness), w = 0
//! * PrimaryLightDirection = -LightDirection.xyz (direction the light travels), w = 0
//! * MaterialUpperSkyColor / MaterialLowerSkyColor = the bUseAsMaterialSkyLight sky light's
//!   upper / lower colour, w = 0
//! * StippleValue = 0 (no fade), NvStereoEnabled = 0
//! Per-object constants (LocalToWorld etc.) live in a storage buffer indexed by MeshTag.

use crate::terrainemu::{TexSrc, BINDINGS, MAX_SMP, MAX_TEX};
use bevy::prelude::*;
use bevy::render::mesh::{MeshVertexAttribute, VertexFormat};
use upk::matcompile::Address;
use upk::matres::{MaterialParams, UniformSet};
use upk::sm3::{translate, Bindings, Decl, Shader, Translated, Usage};

/// Raw UE local-space position (the game's Position input).
pub const ATTR_UE_POS: MeshVertexAttribute = MeshVertexAttribute::new("UE_Position", 0x4f1a_0020, VertexFormat::Float32x3);

/// Generic raw vertex elements (one D3D vertex element each, expanded to float4 with the
/// D3D defaults), for vertex factories whose streams are taken over verbatim (SpeedTree).
pub const ATTR_RAW: [MeshVertexAttribute; 12] = [
    MeshVertexAttribute::new("UE_Raw0", 0x4f1a_0100, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw1", 0x4f1a_0101, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw2", 0x4f1a_0102, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw3", 0x4f1a_0103, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw4", 0x4f1a_0104, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw5", 0x4f1a_0105, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw6", 0x4f1a_0106, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw7", 0x4f1a_0107, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw8", 0x4f1a_0108, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw9", 0x4f1a_0109, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw10", 0x4f1a_010a, VertexFormat::Float32x4),
    MeshVertexAttribute::new("UE_Raw11", 0x4f1a_010b, VertexFormat::Float32x4),
];

/// Raw vertex layout: attribute k carries the D3D element with these usages/indices.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawLayout(pub Vec<Vec<(Usage, u32)>>);

/// vec4 slots per object in the objects buffer.
pub const OBJ_STRIDE: usize = 27;
const OBJ_SLOTS: [&str; 27] = [
    "LocalToWorld[0]",
    "LocalToWorld[1]",
    "LocalToWorld[2]",
    "LocalToWorld[3]",
    "WorldToLocal[0]",
    "WorldToLocal[1]",
    "WorldToLocal[2]",
    "TerrainHeight",
    "SplineEndPos",
    "SplineStartTangent",
    "SplineEndTangent",
    "SplineScales",
    "SplineInfo",
    "UpperSkyColor",
    "LowerSkyColor",
    "GridSize",
    // SpeedTree (trace 2026-10-15): rows = columns of the actor's rotation matrix
    "RotationOnlyMatrix[0]",
    "RotationOnlyMatrix[1]",
    "RotationOnlyMatrix[2]",
    "LODData",
    // SpeedTree wind (CWind shader table, updated every frame by treewind.rs)
    "WindDir",
    "WindTimes",
    "WindDistances",
    "WindLeaves",
    "WindFrondRipple",
    "WindGust",
    "WindGustHints",
];
/// First wind slot in the object block.
pub const OBJ_WIND: usize = 20;

/// Per-object constant block for a UE LocalToWorld (rows = images of the local axes).
pub fn object_block(l2w: [[f32; 3]; 4], terrain_height: [f32; 4], upper: [f32; 4], lower: [f32; 4]) -> [[f32; 4]; OBJ_STRIDE] {
    let m = Mat3::from_cols(Vec3::from(l2w[0]), Vec3::from(l2w[1]), Vec3::from(l2w[2]));
    // WorldToLocal rows: images of the world axes in local space = columns of M^-1 as rows
    let inv = m.inverse();
    let mut b = [[0.0f32; 4]; OBJ_STRIDE];
    for k in 0..3 {
        b[k] = [l2w[k][0], l2w[k][1], l2w[k][2], 0.0];
        let r = inv.col(k);
        b[4 + k] = [r.x, r.y, r.z, 0.0];
    }
    b[3] = [l2w[3][0], l2w[3][1], l2w[3][2], 1.0];
    b[7] = terrain_height;
    b[13] = upper;
    b[14] = lower;
    b
}

/// UE LocalToWorld rows from location, rotator and scale.
pub fn ue_l2w(location: [f32; 3], rot: [i32; 3], scale: [f32; 3]) -> [[f32; 3]; 4] {
    let k = std::f32::consts::TAU / 65536.0;
    let (sp, cp) = (rot[0] as f32 * k).sin_cos();
    let (sy, cy) = (rot[1] as f32 * k).sin_cos();
    let (sr, cr) = (rot[2] as f32 * k).sin_cos();
    let ax = [cp * cy, cp * sy, sp];
    let ay = [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp];
    let az = [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp];
    let s = |a: [f32; 3], f: f32| [a[0] * f, a[1] * f, a[2] * f];
    [s(ax, scale[0]), s(ay, scale[1]), s(az, scale[2]), location]
}

/// a then b (row-vector convention: v * a * b).
pub fn l2w_mul(a: [[f32; 3]; 4], b: [[f32; 3]; 4]) -> [[f32; 3]; 4] {
    let tr = |v: [f32; 3], w: f32| -> [f32; 3] {
        [0, 1, 2].map(|j| v[0] * b[0][j] + v[1] * b[1][j] + v[2] * b[2][j] + w * b[3][j])
    };
    [tr(a[0], 0.0), tr(a[1], 0.0), tr(a[2], 0.0), tr(a[3], 1.0)]
}

pub struct Generated {
    pub wgsl: String,
    /// Same shader for the shadow depth pass (LightAttenuation = 1, fragment without output).
    pub prepass_wgsl: String,
    pub textures: Vec<TexSrc>,
    pub cubes: Vec<TexSrc>,
    pub samplers: Vec<[Address; 2]>,
    pub problems: Vec<String>,
}

struct Binder<'a> {
    stage: &'static str,
    uniforms: &'a UniformSet,
    params: &'a MaterialParams,
    address: &'a dyn Fn(i32) -> [Address; 2],
    textures: std::cell::RefCell<Vec<TexSrc>>,
    cubes: std::cell::RefCell<Vec<TexSrc>>,
    samplers: std::cell::RefCell<Vec<[Address; 2]>>,
    tex_smp: std::cell::RefCell<Vec<usize>>,
    problems: std::cell::RefCell<Vec<String>>,
    /// expression giving the object block base index in this stage
    obj: &'static str,
    /// value used for LightAttenuationTexture lookups ("@expr")
    att: &'static str,
}

impl Binder<'_> {
    fn sampler_for(&self, addr: [Address; 2]) -> usize {
        let mut s = self.samplers.borrow_mut();
        match s.iter().position(|&a| a == addr) {
            Some(i) => i,
            None => {
                s.push(addr);
                s.len() - 1
            }
        }
    }
    fn slot(&self, src: TexSrc) -> (usize, usize) {
        let addr = match src {
            TexSrc::Object(o) => (self.address)(o),
            TexSrc::Grass | TexSrc::Scene => [Address::Clamp; 2],
            TexSrc::White | TexSrc::Black => [Address::Wrap; 2],
        };
        let mut t = self.textures.borrow_mut();
        if let Some(i) = t.iter().position(|&x| x == src) {
            return (i, self.tex_smp.borrow()[i]);
        }
        let si = self.sampler_for(addr);
        t.push(src);
        self.tex_smp.borrow_mut().push(si);
        (t.len() - 1, si)
    }
    fn tex_src(&self, e: Option<&upk::matres::UExpr>) -> TexSrc {
        match e.and_then(|e| e.texture()) {
            // FluidSurface detail simulation normal (G16R16F render target, see MODLOG): at rest = 0
            Some((Some("FluidDetailNormal"), _)) => TexSrc::Black,
            Some((Some(name), o)) => TexSrc::Object(self.params.texture(name).unwrap_or(o)),
            Some((None, o)) => TexSrc::Object(o),
            None => {
                self.problems.borrow_mut().push(format!("{}: texture missing", self.stage));
                TexSrc::White
            }
        }
    }
}

impl Bindings for Binder<'_> {
    fn constant(&self, reg: u32, name: Option<&str>) -> String {
        let name = name.unwrap_or("");
        if let Some(k) = name.strip_prefix("ViewProjectionMatrix[").and_then(|x| x.strip_suffix(']')) {
            // clip = M * ue_world; M = clip_from_world * (ue -> bevy), scaled by 100 so that
            // clip.w is in UE units as in the game (homogeneous, rasterisation unchanged)
            return match k {
                "0" => "view.clip_from_world[0]".into(),
                "1" => "view.clip_from_world[2]".into(),
                "2" => "view.clip_from_world[1]".into(),
                _ => "(view.clip_from_world[3] * 100.0)".into(),
            };
        }
        // per-primitive sky light (UE3 FSkyLightPolicy constants): the object block holds the
        // level's load-time values (world.rs push_object = Globals.upper/lower_sky), which
        // weather.rs never updates - roads, SpeedTrees and water kept the full daylight sky
        // light at night (bluish-light road strips, bright trees, 2026-10-21). The globals hold
        // the same values with the weather applied (identical with the weather off).
        if name == "UpperSkyColor" {
            return "hfg.upper_sky".into();
        }
        if name == "LowerSkyColor" {
            return "hfg.lower_sky".into();
        }
        if let Some(k) = OBJ_SLOTS.iter().position(|&s| s == name) {
            return format!("hfo[{} + {k}u]", self.obj);
        }
        let time = "globals.time";
        if let Some(n) = name.strip_prefix("UniformVector_").and_then(|x| x.parse::<usize>().ok()) {
            return match self.uniforms.vectors.get(n) {
                Some(e) => e.to_wgsl(self.params, time),
                None => {
                    self.problems.borrow_mut().push(format!("{}: {name} missing", self.stage));
                    "vec4<f32>(0.0)".into()
                }
            };
        }
        if let Some(n) = name.strip_prefix("UniformScalar_").and_then(|x| x.parse::<usize>().ok()) {
            return match self.uniforms.scalars.get(n) {
                Some(e) => format!("vec4<f32>(({}).x)", e.to_wgsl(self.params, time)),
                None => {
                    self.problems.borrow_mut().push(format!("{}: {name} missing", self.stage));
                    "vec4<f32>(0.0)".into()
                }
            };
        }
        if let Some(n) = name.strip_prefix("UniformScalars_").and_then(|x| x.parse::<usize>().ok()) {
            let c: Vec<String> = (0..4)
                .map(|k| self.uniforms.scalars.get(n * 4 + k).map(|e| format!("({}).x", e.to_wgsl(self.params, time))).unwrap_or("0.0".into()))
                .collect();
            return format!("vec4<f32>({})", c.join(", "));
        }
        match name {
            "CameraPosition" => "vec4<f32>(swz(view.world_position) * 100.0, 1.0)".into(),
            "LightDirection" => "hfg.light_dir".into(),
            "PrimaryLightDirection" => "vec4<f32>(-hfg.light_dir.xyz, 0.0)".into(),
            "LightColor" | "PrimaryLightColor" => "vec4<f32>(hfg.light_color.rgb, 0.0)".into(),
            "MaterialUpperSkyColor" => "vec4<f32>(hfg.material_upper_sky.rgb, 0.0)".into(),
            "MaterialLowerSkyColor" => "vec4<f32>(hfg.material_lower_sky.rgb, 0.0)".into(),
            // WorldInfo.CheapAmbientColor as linear colour (trace: (c/255)^2.2, w = 1)
            "CheapAmbientColor" => "vec4<f32>(hfg.cheap_ambient.rgb, 1.0)".into(),
            // SpeedTree camera-facing basis (trace 2026-10-15): row2 = view direction F,
            // row0 = normalize(Z x F), row1 = F x row0 (UE world space)
            "CameraAlignMatrix[0]" => "vec4<f32>(normalize(vec3<f32>(-hf_cam_fwd().y, hf_cam_fwd().x, 0.0)), 0.0)".into(),
            "CameraAlignMatrix[1]" => "vec4<f32>(cross(hf_cam_fwd(), normalize(vec3<f32>(-hf_cam_fwd().y, hf_cam_fwd().x, 0.0))), 0.0)".into(),
            "CameraAlignMatrix[2]" => "vec4<f32>(hf_cam_fwd(), 0.0)".into(),
            "AmbientColorAndSkyFactor" => "hfg.ambient_sky".into(),
            "TerrainBounds" => "hfg.terrain_bounds".into(),
            // view -> scene colour buffer (scenecopy::SIZE), as UE3 with a larger buffer
            "ScreenPositionScaleBias" => format!(
                "(vec4<f32>(0.5, -0.5, 0.5, 0.5) * vec4<f32>(view.viewport.z / {w:?}, view.viewport.w / {h:?}, view.viewport.w / {h:?}, view.viewport.z / {w:?}))",
                w = crate::scenecopy::SIZE[0] as f32,
                h = crate::scenecopy::SIZE[1] as f32
            ),
            "ScreenResolutionSize" => "vec4<f32>(view.viewport.zw, 1.0 / view.viewport.zw)".into(),
            "StippleValue" | "NvStereoEnabled" | "StereoDepth" | "EnableNearFogNoise" | "DecalFarPlaneDistance" => "vec4<f32>(0.0)".into(),
            // scene depth in SceneColorTexture alpha, see scenecopy.rs
            "MinZ_MaxZRatio" => format!("vec4<f32>({:?}, 0.0, 0.0, 0.0)", 100.0 * crate::scenecopy::NEAR * crate::scenecopy::DEPTH_SCALE),
            // no fog yet: start beyond any view distance (fog factor 0)
            "FarFogParams" => "vec4<f32>(1e30, 2e30, 1.0, 1.0)".into(),
            "NearFogParams" => "vec4<f32>(0.0, 1.0, 1.0, 1.0)".into(),
            "TranslucentFogSampleOffsets" => "vec4<f32>(0.0)".into(),
            // no dynamic shadow map yet: shadow coordinates (0, 0, 0) against a depth of 1 = lit
            "ScreenToShadowMatrix[0]" | "ScreenToShadowMatrix[1]" | "ScreenToShadowMatrix[2]" => "vec4<f32>(0.0)".into(),
            "ScreenToShadowMatrix[3]" => "vec4<f32>(0.0, 0.0, 0.0, 1.0)".into(),
            "ShadowBufferSize" => "vec4<f32>(1.0, 1.0, 1.0, 1.0)".into(),
            "ShadowBufferValidRect" => "vec4<f32>(0.0, 1.0, 0.0, 1.0)".into(),
            "FullLevelShadowMethod" => "vec4<f32>(0.0)".into(),
            n if n.starts_with("ShadowSampleOffsets[") => "vec4<f32>(0.0)".into(),
            _ => {
                self.problems.borrow_mut().push(format!("{}: c{reg} {name} unbound", self.stage));
                "vec4<f32>(0.0)".into()
            }
        }
    }

    fn sampler(&self, reg: u32, name: Option<&str>) -> (String, String) {
        let name = name.unwrap_or("");
        if let Some(n) = name.strip_prefix("TextureCube_").and_then(|x| x.parse::<usize>().ok()) {
            let src = self.tex_src(self.uniforms.cubes.get(n));
            let mut c = self.cubes.borrow_mut();
            let i = match c.iter().position(|&x| x == src) {
                Some(i) => i,
                None => {
                    c.push(src);
                    c.len() - 1
                }
            };
            let si = self.sampler_for([Address::Clamp; 2]);
            return (format!("tcube{i}"), format!("s{si}"));
        }
        let src = if let Some(n) = name.strip_prefix("Texture2D_").and_then(|x| x.parse::<usize>().ok()) {
            self.tex_src(self.uniforms.tex2d.get(n))
        } else {
            match name {
                // dynamic sun shadow computed in place (sunshadow.rs)
                "LightAttenuationTexture" => return (self.att.to_string(), String::new()),
                // no stipple fading
                "NvStereoFixTexture" | "NoiseTexture" | "ShadowDepthTexture" | "TranslucentFogColorTexture" | "TranslucentFogNoiseTexture" => TexSrc::White,
                "SceneColorTexture" => TexSrc::Scene,
                "GrassColorTexture" => TexSrc::Grass,
                _ => {
                    self.problems.borrow_mut().push(format!("{}: s{reg} {name} unbound", self.stage));
                    TexSrc::White
                }
            }
        };
        let (k, si) = self.slot(src);
        (format!("t{k}"), format!("s{si}"))
    }
}

const PRELUDE: &str = r#"
#import bevy_pbr::mesh_view_bindings::{view, globals}
#import bevy_pbr::mesh_functions::get_tag

struct HfGlobals {
    upper_sky: vec4<f32>,
    lower_sky: vec4<f32>,
    light_color: vec4<f32>,
    light_dir: vec4<f32>,
    ambient_sky: vec4<f32>,
    terrain_bounds: vec4<f32>,
    misc: vec4<f32>,
    // weather.rs sky values (gamemat::Globals)
    sky_fx: vec4<f32>,
    sky_tint: vec4<f32>,
    fog_color: vec4<f32>,
    fog_sun_dir: vec4<f32>,
    fog_sun: vec4<f32>,
    material_upper_sky: vec4<f32>,
    material_lower_sky: vec4<f32>,
    cheap_ambient: vec4<f32>,
}

@group(2) @binding(0) var<storage, read> hfg: HfGlobals;
@group(2) @binding(1) var<storage, read> hfo: array<vec4<f32>>;

fn swz(v: vec3<f32>) -> vec3<f32> { return vec3<f32>(v.x, v.z, v.y); }
// weather.rs scene brightness (gamemat.rs hf_weather_dim): 1 with the weather off
fn hf_weather_dim() -> vec3<f32> {
    return (vec3<f32>(1.0) - hfg.sky_tint.rgb) * (1.0 - hfg.sky_tint.w);
}
fn hf_cam_fwd() -> vec3<f32> { return normalize(swz(-view.world_from_view[2].xyz)); }
"#;

fn key(d: &Decl) -> String {
    format!("{:?}{}", d.usage, d.index)
}

/// Mesh vertex input for a game VS input declaration.
fn vs_input(d: &Decl, vf: &str, raw: Option<&RawLayout>) -> String {
    if let Some(r) = raw {
        return match r.0.iter().position(|us| us.iter().any(|(u, i)| *u == d.usage && *i == d.index)) {
            Some(k) => format!("vin.a{k}"),
            None => "vec4<f32>(0.0, 0.0, 0.0, 1.0)".into(),
        };
    }
    if vf == "FFluidVertexFactory" {
        // flat surface (simulation off): height 0, grid coordinate in uv0, zero gradient
        return match (&d.usage, d.index) {
            (Usage::TexCoord, 0) => "vec4<f32>(vin.uv0, 0.0, 1.0)".into(),
            _ => "vec4<f32>(0.0, 0.0, 0.0, 1.0)".into(),
        };
    }
    match (&d.usage, d.index) {
        (Usage::Position, 0) => "vec4<f32>(vin.ue_pos, 1.0)".into(),
        // UBYTE4: raw byte values, the shader rescales with x * 2/255 - 1
        (Usage::Tangent, 0) => "(vin.tx * 255.0)".into(),
        (Usage::Normal, 0) => "(vin.tz * 255.0)".into(),
        (Usage::TexCoord, 0) => "vec4<f32>(vin.uv0, 0.0, 1.0)".into(),
        (Usage::TexCoord, 1) => "vec4<f32>(vin.uv1, 0.0, 1.0)".into(),
        // UE3's null colour stream is white
        (Usage::Color, _) => "vec4<f32>(1.0)".into(),
        _ => "vec4<f32>(0.0, 0.0, 0.0, 1.0)".into(),
    }
}

/// WGSL module: the game's vertex + pixel shader of one material on one vertex factory.
#[allow(clippy::too_many_arguments)]
pub fn generate(vs_code: &[u8], ps_code: &[u8], uniforms: &UniformSet, params: &MaterialParams, address: &dyn Fn(i32) -> [Address; 2], blend: upk::level::Blend, vf: &str, raw: Option<&RawLayout>, shadow_wgsl: &str) -> Option<Generated> {
    let mut g = generate_mode(vs_code, ps_code, uniforms, params, address, blend, vf, raw, Some(shadow_wgsl))?;
    g.prepass_wgsl = generate_mode(vs_code, ps_code, uniforms, params, address, blend, vf, raw, None)?.wgsl;
    Some(g)
}

#[allow(clippy::too_many_arguments)]
fn generate_mode(vs_code: &[u8], ps_code: &[u8], uniforms: &UniformSet, params: &MaterialParams, address: &dyn Fn(i32) -> [Address; 2], blend: upk::level::Blend, vf: &str, raw: Option<&RawLayout>, shadow: Option<&str>) -> Option<Generated> {
    let vs = Shader::parse(vs_code)?;
    let ps = Shader::parse(ps_code)?;
    let mut b = Binder {
        stage: "vs",
        uniforms,
        params,
        address,
        textures: Default::default(),
        cubes: Default::default(),
        samplers: Default::default(),
        tex_smp: Default::default(),
        problems: Default::default(),
        obj: "obj",
        att: if shadow.is_some() { "@hf_att" } else { "@vec4<f32>(1.0)" },
    };
    let tvs: Translated = translate(&vs, &b);
    b.stage = "ps";
    b.obj = "i.obj";
    let tps: Translated = translate(&ps, &b);
    let mut problems = b.problems.borrow().clone();

    let mut w = if shadow.is_some() { String::from(PRELUDE) } else { crate::gamemat::prepass_imports(PRELUDE) };
    w.push_str(crate::crater::WGSL);
    w.push_str(crate::weather::TRANSLUCENT_FOG_WGSL);
    w.push_str(shadow.unwrap_or(""));
    if shadow.is_some() {
        w.push_str(crate::dynlights::WGSL);
    }
    w.push_str(BINDINGS);
    if vs.relative_const || ps.relative_const {
        w.push_str("var<private> hf_c: array<vec4<f32>, 256>;\n");
    }
    let pos_reg = vs.outputs.iter().find(|d| matches!(d.usage, Usage::Position) && d.index == 0)?.reg;
    let varyings: Vec<&Decl> = vs.outputs.iter().filter(|d| !(matches!(d.usage, Usage::Position) && d.index == 0)).collect();
    w.push_str("struct VOut {\n    @builtin(position) clip: vec4<f32>,\n    @location(0) @interpolate(flat) obj: u32,\n");
    for (k, _) in varyings.iter().enumerate() {
        w.push_str(&format!("    @location({}) l{k}: vec4<f32>,\n", k + 1));
    }
    match raw {
        Some(r) => {
            w.push_str("}\n\nstruct VIn {\n    @builtin(instance_index) instance: u32,\n    @location(0) pos: vec3<f32>,\n");
            for k in 0..r.0.len() {
                w.push_str(&format!("    @location({}) a{k}: vec4<f32>,\n", k + 1));
            }
            w.push_str("}\n\n");
        }
        None => w.push_str(
            "}\n\nstruct VIn {\n    @builtin(instance_index) instance: u32,\n    @location(0) pos: vec3<f32>,\n    @location(1) ue_pos: vec3<f32>,\n    @location(2) tx: vec4<f32>,\n    @location(3) tz: vec4<f32>,\n    @location(4) uv0: vec2<f32>,\n    @location(5) uv1: vec2<f32>,\n}\n\n",
        ),
    }
    w.push_str(&format!("@vertex\nfn vertex(vin: VIn) -> VOut {{\n    let obj = get_tag(vin.instance) * {OBJ_STRIDE}u;\n"));
    for d in &vs.inputs {
        w.push_str(&format!("    let v{} = {};\n", d.reg, vs_input(d, vf, raw)));
    }
    let max_out = vs.outputs.iter().map(|d| d.reg).max().unwrap_or(0);
    for r in 0..=max_out.max(11) {
        w.push_str(&format!("    var o{r}: vec4<f32> = vec4<f32>(0.0);\n"));
    }
    w.push_str("    var o_pos: vec4<f32> = vec4<f32>(0.0);\n");
    w.push_str(&tvs.declarations);
    w.push_str(&tvs.body);
    w.push_str(&format!("    var out: VOut;\n    out.clip = o{pos_reg};\n    out.obj = obj;\n"));
    for (k, d) in varyings.iter().enumerate() {
        w.push_str(&format!("    out.l{k} = o{};\n", d.reg));
    }
    w.push_str("    return out;\n}\n\n");
    // pixel stage
    if shadow.is_some() {
        w.push_str("@fragment\nfn fragment(i: VOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {\n");
        w.push_str("    let hf_att = vec4<f32>(hf_sun_shadow(i.clip));\n");
        // dynamic point lights (dynlights.rs): world position from the depth, face normal
        w.push_str("    let hf_ndc = vec2<f32>((i.clip.x - view.viewport.x) / view.viewport.z * 2.0 - 1.0, 1.0 - (i.clip.y - view.viewport.y) / view.viewport.w * 2.0);\n");
        w.push_str("    let hf_wh = view.world_from_clip * vec4<f32>(hf_ndc, i.clip.z, 1.0);\n");
        w.push_str("    let hf_wue = vec3<f32>(hf_wh.x, hf_wh.z, hf_wh.y) * (100.0 / hf_wh.w);\n");
        w.push_str("    var hf_n = normalize(cross(dpdx(hf_wue), dpdy(hf_wue)));\n");
        w.push_str("    if (dot(hf_n, swz(view.world_position) * 100.0 - hf_wue) < 0.0) { hf_n = -hf_n; }\n");
    } else {
        w.push_str("@fragment\nfn fragment(i: VOut, @builtin(front_facing) ff: bool) {\n");
    }
    w.push_str("    let vpos = vec4<f32>(i.clip.xy - 0.5, 0.0, 0.0);\n    let vface = vec4<f32>(select(-1.0, 1.0, ff));\n");
    for d in &ps.inputs {
        let src = match varyings.iter().position(|o| key(o) == key(d)) {
            Some(k) => format!("i.l{k}"),
            None => {
                problems.push(format!("ps: input {} not written by the VS", key(d)));
                "vec4<f32>(0.0)".into()
            }
        };
        w.push_str(&format!("    let v{} = {src};\n", d.reg));
    }
    w.push_str("    var oc0: vec4<f32> = vec4<f32>(0.0);\n    var oc1: vec4<f32> = vec4<f32>(0.0);\n    var o_depth: vec4<f32> = vec4<f32>(0.0);\n");
    w.push_str(&tps.declarations);
    w.push_str(&tps.body);
    if shadow.is_none() {
        // shadow depth pass: only the texkills matter
        w.push_str("}\n");
        return finish(w, b, problems);
    }
    if let Ok(e) = std::env::var("HOMEBOTS_GS_DEBUG") {
        // debugging aid: replace the output with an expression of the shader registers
        if std::env::var("HOMEBOTS_GS_DEBUG_VF").map(|v| v == vf).unwrap_or(true) {
            w.push_str(&format!("    return {e};\n}}\n"));
            return finish(w, b, problems);
        }
    }
    use upk::level::Blend;
    // weather.rs night class (NOT from data, as gamemat.rs hf_daylit): an opaque / masked /
    // translucent PS that reads no light or sky constant, no env cube and no scene colour has a
    // daylight look baked into its colour (unlit materials, e.g. the road edge strips Roads.upk
    // M_Road_Translucent, MLM_Unlit): it darkens with the scene (hf_weather_dim; 1 with the
    // weather off). Additive (lamp glows) and modulate stay.
    if daylit_ps(&ps, blend) {
        w.push_str("    oc0 = vec4<f32>(oc0.rgb * hf_weather_dim(), oc0.a);\n");
    }
    if matches!(blend, Blend::Translucent | Blend::Additive | Blend::Modulate) {
        // weather fog (weather.rs, NOT from data): translucent surfaces are drawn after the
        // full-screen fog pass, so they are fogged here (alpha blend: towards the fog colour,
        // additive: fades out; modulate unchanged)
        match blend {
            Blend::Translucent => w.push_str("    let hf_fog = hf_translucent_fog(hf_wue);\n    return vec4<f32>(mix(oc0.rgb, hf_fog.rgb, hf_fog.a), oc0.a);\n}\n"),
            Blend::Additive => w.push_str("    let hf_fog = hf_translucent_fog(hf_wue);\n    return vec4<f32>(oc0.rgb * (1.0 - hf_fog.a), oc0.a);\n}\n"),
            _ => w.push_str("    return oc0;\n}\n"),
        }
    } else {
        // NOT from data: the base pass output stands in for the diffuse colour (the game lights
        // dynamic point lights in a separate per-light pass that is not translated)
        w.push_str("    return vec4<f32>((oc0.rgb + oc0.rgb * hf_dyn_lights(i.clip, hf_wue, hf_n)) * hf_crater(hf_wue), 1.0);\n}\n");
    }
    finish(w, b, problems)
}

/// Night class of a game pixel shader (see generate_mode): no lighting / sky / ambient constant,
/// no cube map, no scene colour in its constant table, blend opaque / masked / translucent.
fn daylit_ps(ps: &Shader, blend: upk::level::Blend) -> bool {
    use upk::level::Blend;
    const LIT: [&str; 11] = [
        "UpperSkyColor",
        "LowerSkyColor",
        "LightColor",
        "PrimaryLightColor",
        "MaterialUpperSkyColor",
        "MaterialLowerSkyColor",
        "AmbientColorAndSkyFactor",
        "CheapAmbientColor",
        "SceneColorTexture",
        "LightAttenuationTexture",
        "LightMapScale",
    ];
    matches!(blend, Blend::Opaque | Blend::Masked | Blend::Translucent)
        && !ps.ctab.values().any(|n| {
            let n = n.split('[').next().unwrap_or(n);
            LIT.contains(&n) || n.starts_with("TextureCube") || n.starts_with("LightMapTextures")
        })
}

fn finish(w: String, b: Binder, mut problems: Vec<String>) -> Option<Generated> {
    let textures = b.textures.borrow().clone();
    let cubes = b.cubes.borrow().clone();
    let samplers = b.samplers.borrow().clone();
    if textures.len() > MAX_TEX || samplers.len() > MAX_SMP || cubes.len() > 2 {
        problems.push(format!("{} textures / {} cubes / {} samplers exceed the binding slots", textures.len(), cubes.len(), samplers.len()));
        return None;
    }
    Some(Generated { wgsl: w, prepass_wgsl: String::new(), textures, cubes, samplers, problems })
}
