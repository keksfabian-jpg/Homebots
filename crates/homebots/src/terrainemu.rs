//! Homefront terrain rendered with the game's own compiled shaders.
//!
//! Measured in the SP-Devil D3D9 capture (frame 2, see MODLOG): the game draws terrain in two
//! passes per component — the "lighting pass" (FTerrainLightingPassVertexFactory,
//! TBasePassVertexShader/TBasePassPixelShader FDirectionalLightLightMapPolicy[SkyLight] of the
//! palette's `TerrainMaterialResource:<layers> (MatType:1)`) writes lighting rgb + specular a,
//! then the "colour pass" (FTerrainColorPixelShader of `(MatType:0)`) is blended with
//! SRCBLEND=DESTALPHA, DESTBLEND=SRCCOLOR, i.e. final = colour.rgb * (light.rgb + light.a).
//! Both passes use the same vertex shader outputs (identical code apart from LightDirection).
//! Here the shader bytecode from RefShaderCache-PC-D3D-SM3.upk is translated to WGSL
//! (`upk::sm3`) and the two pixel shaders run in one fragment with exactly that blend.
//!
//! Vertex shader constants (FTerrainVertexFactoryShaderParameters::SetMesh, HOMEFRONT.exe
//! FUN_00e185b0, values checked against the capture):
//! * LocalToWorld = diag(DrawScale3D*DrawScale), translation Location + SectionBase*scale
//! * InvMaxTesselationLevel_ZScale = (32 / MaxTessellationLevel, 1/128, -, -)
//! * InvTerrainSize_SectionBase = (1/NumVerticesX, 1/NumVerticesY, SectionBaseX, SectionBaseY)
//! * WeightMapDistort = (WeightMapMinX, WeightMapMinY, 1/(MaxX-MinX), 1/(MaxY-MinY))
//! * TerrainHeight = 0 for terrain draws (capture)
//! Vertex stream: one UBYTE4 per vertex (x, y, height low byte, height high byte), with x/y in
//! units of 32/MaxTessellationLevel patches inside the component.

use bevy::{
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::{
        mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef, VertexFormat},
        render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderRef, SpecializedMeshPipelineError},
        storage::ShaderStorageBuffer,
    },
};

pub const MAX_TEX: usize = 24;
pub const MAX_SMP: usize = 6;

/// Raw game vertex (x, y, hlo, hhi) as floats.
pub const ATTR_TERRAIN_VERT: MeshVertexAttribute = MeshVertexAttribute::new("UE_TerrainVert", 0x4f1a_0010, VertexFormat::Float32x4);

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TerrainEmuKey {
    pub shader: Handle<Shader>,
    /// shadow depth pass variant of `shader` (None = does not cast)
    pub prepass_shader: Option<Handle<Shader>>,
    pub cull: Option<bevy::render::render_resource::Face>,
    /// Vertex layout: 0 = terrain stream, 1 = static/skeletal mesh layout (gameshader.rs),
    /// 2 + n = n raw D3D vertex elements (`ATTR_RAW`, gameshader::RawLayout).
    pub mesh_layout: u8,
}

impl From<&TerrainEmu> for TerrainEmuKey {
    fn from(m: &TerrainEmu) -> Self {
        TerrainEmuKey { shader: m.shader.clone(), prepass_shader: m.prepass_shader.clone(), cull: m.cull, mesh_layout: m.mesh_layout }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(TerrainEmuKey)]
pub struct TerrainEmu {
    #[storage(0, read_only)]
    pub globals: Handle<ShaderStorageBuffer>,
    /// Per-component vertex shader constants (see `DrawConsts`).
    #[storage(1, read_only)]
    pub draw: Handle<ShaderStorageBuffer>,
    #[texture(2)]
    pub t0: Option<Handle<Image>>,
    #[texture(3)]
    pub t1: Option<Handle<Image>>,
    #[texture(4)]
    pub t2: Option<Handle<Image>>,
    #[texture(5)]
    pub t3: Option<Handle<Image>>,
    #[texture(6)]
    pub t4: Option<Handle<Image>>,
    #[texture(7)]
    pub t5: Option<Handle<Image>>,
    #[texture(8)]
    pub t6: Option<Handle<Image>>,
    #[texture(9)]
    pub t7: Option<Handle<Image>>,
    #[texture(10)]
    pub t8: Option<Handle<Image>>,
    #[texture(11)]
    pub t9: Option<Handle<Image>>,
    #[texture(12)]
    pub t10: Option<Handle<Image>>,
    #[texture(13)]
    pub t11: Option<Handle<Image>>,
    #[texture(14)]
    pub t12: Option<Handle<Image>>,
    #[texture(15)]
    pub t13: Option<Handle<Image>>,
    #[texture(16)]
    pub t14: Option<Handle<Image>>,
    #[texture(17)]
    pub t15: Option<Handle<Image>>,
    #[texture(18)]
    pub t16: Option<Handle<Image>>,
    #[texture(19)]
    pub t17: Option<Handle<Image>>,
    #[texture(20)]
    pub t18: Option<Handle<Image>>,
    #[texture(21)]
    pub t19: Option<Handle<Image>>,
    #[texture(22)]
    pub t20: Option<Handle<Image>>,
    #[texture(23)]
    pub t21: Option<Handle<Image>>,
    #[texture(24)]
    pub t22: Option<Handle<Image>>,
    #[texture(25)]
    pub t23: Option<Handle<Image>>,
    #[texture(26)]
    #[sampler(27)]
    pub s0: Option<Handle<Image>>,
    #[texture(28)]
    #[sampler(29)]
    pub s1: Option<Handle<Image>>,
    #[texture(30)]
    #[sampler(31)]
    pub s2: Option<Handle<Image>>,
    #[texture(32)]
    #[sampler(33)]
    pub s3: Option<Handle<Image>>,
    #[texture(34)]
    #[sampler(35)]
    pub s4: Option<Handle<Image>>,
    #[texture(36)]
    #[sampler(37)]
    pub s5: Option<Handle<Image>>,
    #[texture(38, dimension = "cube")]
    pub c0: Option<Handle<Image>>,
    #[texture(39, dimension = "cube")]
    pub c1: Option<Handle<Image>>,
    pub shader: Handle<Shader>,
    pub prepass_shader: Option<Handle<Shader>>,
    pub alpha: AlphaMode,
    pub cull: Option<bevy::render::render_resource::Face>,
    pub mesh_layout: u8,
}

impl TerrainEmu {
    pub fn new(globals: Handle<ShaderStorageBuffer>, draw: Handle<ShaderStorageBuffer>, shader: Handle<Shader>) -> Self {
        let m = TerrainEmu {
            globals,
            draw,
            t0: None,
            t1: None,
            t2: None,
            t3: None,
            t4: None,
            t5: None,
            t6: None,
            t7: None,
            t8: None,
            t9: None,
            t10: None,
            t11: None,
            t12: None,
            t13: None,
            t14: None,
            t15: None,
            t16: None,
            t17: None,
            t18: None,
            t19: None,
            t20: None,
            t21: None,
            t22: None,
            t23: None,
            s0: None,
            s1: None,
            s2: None,
            s3: None,
            s4: None,
            s5: None,
            c0: None,
            c1: None,
            shader,
            prepass_shader: None,
            alpha: AlphaMode::Opaque,
            cull: None,
            mesh_layout: 0,
        };
        m
    }
    pub fn set_tex(&mut self, i: usize, h: Handle<Image>) {
        match i {
            0 => self.t0 = Some(h),
            1 => self.t1 = Some(h),
            2 => self.t2 = Some(h),
            3 => self.t3 = Some(h),
            4 => self.t4 = Some(h),
            5 => self.t5 = Some(h),
            6 => self.t6 = Some(h),
            7 => self.t7 = Some(h),
            8 => self.t8 = Some(h),
            9 => self.t9 = Some(h),
            10 => self.t10 = Some(h),
            11 => self.t11 = Some(h),
            12 => self.t12 = Some(h),
            13 => self.t13 = Some(h),
            14 => self.t14 = Some(h),
            15 => self.t15 = Some(h),
            16 => self.t16 = Some(h),
            17 => self.t17 = Some(h),
            18 => self.t18 = Some(h),
            19 => self.t19 = Some(h),
            20 => self.t20 = Some(h),
            21 => self.t21 = Some(h),
            22 => self.t22 = Some(h),
            23 => self.t23 = Some(h),
            _ => {}
        }
    }
    /// Sampler slots take the sampler of a (1x1) carrier image.
    pub fn set_smp(&mut self, i: usize, h: Handle<Image>) {
        match i {
            0 => self.s0 = Some(h),
            1 => self.s1 = Some(h),
            2 => self.s2 = Some(h),
            3 => self.s3 = Some(h),
            4 => self.s4 = Some(h),
            5 => self.s5 = Some(h),
            _ => {}
        }
    }
}

impl Material for TerrainEmu {
    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let ml = key.bind_group_data.mesh_layout;
        let vb = if ml >= 2 {
            let mut attrs = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
            for k in 0..(ml - 2) as usize {
                attrs.push(crate::gameshader::ATTR_RAW[k].at_shader_location(1 + k as u32));
            }
            layout.0.get_layout(&attrs)?
        } else if ml == 1 {
            layout.0.get_layout(&[
                Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
                crate::gameshader::ATTR_UE_POS.at_shader_location(1),
                crate::gamemat::ATTR_TANGENT_X.at_shader_location(2),
                crate::gamemat::ATTR_TANGENT_Z.at_shader_location(3),
                Mesh::ATTRIBUTE_UV_0.at_shader_location(4),
                Mesh::ATTRIBUTE_UV_1.at_shader_location(5),
            ])?
        } else {
            layout.0.get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0), ATTR_TERRAIN_VERT.at_shader_location(1)])?
        };
        descriptor.vertex.buffers = vec![vb];
        descriptor.primitive.cull_mode = key.bind_group_data.cull;
        if key.mesh_key.contains(bevy::pbr::MeshPipelineKey::DEPTH_PREPASS) {
            // shadow depth pass (sunshadow.rs): same vertex code, fragment only for texkill
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
    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Default
    }
}

pub const BINDINGS: &str = "@group(2) @binding(2) var t0: texture_2d<f32>;\n@group(2) @binding(3) var t1: texture_2d<f32>;\n@group(2) @binding(4) var t2: texture_2d<f32>;\n@group(2) @binding(5) var t3: texture_2d<f32>;\n@group(2) @binding(6) var t4: texture_2d<f32>;\n@group(2) @binding(7) var t5: texture_2d<f32>;\n@group(2) @binding(8) var t6: texture_2d<f32>;\n@group(2) @binding(9) var t7: texture_2d<f32>;\n@group(2) @binding(10) var t8: texture_2d<f32>;\n@group(2) @binding(11) var t9: texture_2d<f32>;\n@group(2) @binding(12) var t10: texture_2d<f32>;\n@group(2) @binding(13) var t11: texture_2d<f32>;\n@group(2) @binding(14) var t12: texture_2d<f32>;\n@group(2) @binding(15) var t13: texture_2d<f32>;\n@group(2) @binding(16) var t14: texture_2d<f32>;\n@group(2) @binding(17) var t15: texture_2d<f32>;\n@group(2) @binding(18) var t16: texture_2d<f32>;\n@group(2) @binding(19) var t17: texture_2d<f32>;\n@group(2) @binding(20) var t18: texture_2d<f32>;\n@group(2) @binding(21) var t19: texture_2d<f32>;\n@group(2) @binding(22) var t20: texture_2d<f32>;\n@group(2) @binding(23) var t21: texture_2d<f32>;\n@group(2) @binding(24) var t22: texture_2d<f32>;\n@group(2) @binding(25) var t23: texture_2d<f32>;\n@group(2) @binding(27) var s0: sampler;\n@group(2) @binding(29) var s1: sampler;\n@group(2) @binding(31) var s2: sampler;\n@group(2) @binding(33) var s3: sampler;\n@group(2) @binding(35) var s4: sampler;\n@group(2) @binding(37) var s5: sampler;\n@group(2) @binding(38) var tcube0: texture_cube<f32>;\n@group(2) @binding(39) var tcube1: texture_cube<f32>;\n";

/// Slots of the per-component constant buffer.
const DRAW_SLOTS: [&str; 8] = [
    "LocalToWorld[0]",
    "LocalToWorld[1]",
    "LocalToWorld[2]",
    "LocalToWorld[3]",
    "TerrainHeight",
    "WeightMapDistort",
    "InvMaxTesselationLevel_ZScale",
    "InvTerrainSize_SectionBase",
];

/// Vertex shader constants of one component, in `DRAW_SLOTS` order.
pub fn draw_consts(t: &upk::terrain::TerrainRender, c: &upk::terrain::TerrainComp) -> [[f32; 4]; 8] {
    let s = t.scale;
    let wm = t.weight_map_bounds;
    [
        [s[0], 0.0, 0.0, 0.0],
        [0.0, s[1], 0.0, 0.0],
        [0.0, 0.0, s[2], 0.0],
        [t.location[0] + c.base[0] as f32 * s[0], t.location[1] + c.base[1] as f32 * s[1], t.location[2], 1.0],
        [0.0; 4],
        [wm[0], wm[2], 1.0 / (wm[1] - wm[0]), 1.0 / (wm[3] - wm[2])],
        [(32 / c.tess) as f32, 1.0 / 128.0, 0.0, 0.0],
        [1.0 / t.num_vertices[0] as f32, 1.0 / t.num_vertices[1] as f32, c.base[0] as f32, c.base[1] as f32],
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TexSrc {
    Object(i32),
    White,
    /// Zero (a cleared render target, e.g. the fluid detail normal at rest)
    Black,
    /// Scene colour with depth in alpha (scenecopy.rs)
    Scene,
    /// The terrain's GrassColorTexture
    Grass,
}

pub struct Generated {
    pub wgsl: String,
    /// Texture slot -> source
    pub textures: Vec<TexSrc>,
    /// Sampler slot -> address modes
    pub samplers: Vec<[upk::matcompile::Address; 2]>,
    pub problems: Vec<String>,
}

struct Binder<'a> {
    stage: &'static str,
    res: &'a upk::matres::UniformSet,
    weight_maps: &'a [i32],
    address: &'a dyn Fn(i32) -> [upk::matcompile::Address; 2],
    textures: std::cell::RefCell<Vec<TexSrc>>,
    samplers: std::cell::RefCell<Vec<[upk::matcompile::Address; 2]>>,
    /// texture slot -> sampler slot
    tex_smp: std::cell::RefCell<Vec<usize>>,
    problems: std::cell::RefCell<Vec<String>>,
}

fn f(x: f32) -> String {
    let s = format!("{x:?}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") { s.replace("inf", "3.4e38").replace("NaN", "0.0") } else { format!("{s}.0") }
}

fn v4(v: [f32; 4]) -> String {
    format!("vec4<f32>({}, {}, {}, {})", f(v[0]), f(v[1]), f(v[2]), f(v[3]))
}

impl Binder<'_> {
    fn slot(&self, src: TexSrc) -> usize {
        let mut t = self.textures.borrow_mut();
        if let Some(i) = t.iter().position(|&x| x == src) {
            return i;
        }
        let addr = match src {
            TexSrc::Object(o) => (self.address)(o),
            TexSrc::Grass | TexSrc::Scene => [upk::matcompile::Address::Clamp; 2],
            TexSrc::White | TexSrc::Black => [upk::matcompile::Address::Wrap; 2],
        };
        let mut s = self.samplers.borrow_mut();
        let si = match s.iter().position(|&a| a == addr) {
            Some(i) => i,
            None => {
                s.push(addr);
                s.len() - 1
            }
        };
        t.push(src);
        self.tex_smp.borrow_mut().push(si);
        t.len() - 1
    }
}

impl upk::sm3::Bindings for Binder<'_> {
    fn constant(&self, reg: u32, name: Option<&str>) -> String {
        let name = name.unwrap_or("");
        if let Some(k) = name.strip_prefix("ViewProjectionMatrix[").and_then(|x| x.strip_suffix(']')) {
            // clip = M * ue_pos; M = clip_from_world * (ue -> bevy), scaled by 100 so that
            // clip.w is in UE units like the game's (homogeneous, rasterisation unchanged)
            return match k {
                "0" => "view.clip_from_world[0]".into(),
                "1" => "view.clip_from_world[2]".into(),
                "2" => "view.clip_from_world[1]".into(),
                _ => "(view.clip_from_world[3] * 100.0)".into(),
            };
        }
        if let Some(k) = DRAW_SLOTS.iter().position(|&s| s == name) {
            return format!("td[{k}]");
        }
        if let Some(n) = name.strip_prefix("UniformVector_").and_then(|x| x.parse::<usize>().ok()) {
            return match self.res.vectors.get(n) {
                Some(e) => v4(e.eval(0.0)),
                None => {
                    self.problems.borrow_mut().push(format!("{}: {name} missing", self.stage));
                    v4([0.0; 4])
                }
            };
        }
        if let Some(n) = name.strip_prefix("UniformScalars_").and_then(|x| x.parse::<usize>().ok()) {
            let mut v = [0.0; 4];
            for k in 0..4 {
                if let Some(e) = self.res.scalars.get(n * 4 + k) {
                    v[k] = e.eval(0.0)[0];
                }
            }
            return v4(v);
        }
        match name {
            "CameraPosition" => "vec4<f32>(swz(view.world_position) * 100.0, 1.0)".into(),
            "LightDirection" => "hfg.light_dir".into(),
            "LightColor" => "hfg.light_color".into(),
            "UpperSkyColor" => "hfg.upper_sky".into(),
            "LowerSkyColor" => "hfg.lower_sky".into(),
            "AmbientColorAndSkyFactor" => "hfg.ambient_sky".into(),
            // LightAttenuationTexture is white here, the bias/scale only places the lookup
            "ScreenPositionScaleBias" => "vec4<f32>(0.5, -0.5, 0.5, 0.5)".into(),
            "NvStereoEnabled" => "vec4<f32>(0.0)".into(),
            // only feeds the colour pass alpha (scene depth), unused by the blend
            "MinZ_MaxZRatio" => "vec4<f32>(0.0)".into(),
            _ => {
                self.problems.borrow_mut().push(format!("{}: c{reg} {name} unbound", self.stage));
                "vec4<f32>(0.0)".into()
            }
        }
    }

    fn sampler(&self, reg: u32, name: Option<&str>) -> (String, String) {
        let name = name.unwrap_or("");
        let src = if let Some(n) = name.strip_prefix("Texture2D_").and_then(|x| x.parse::<usize>().ok()) {
            match self.res.tex2d.get(n).and_then(|e| e.texture()) {
                Some((Some(pn), _)) if pn.starts_with("TWeightMap") => {
                    let k: usize = pn["TWeightMap".len()..].parse().unwrap_or(0);
                    match self.weight_maps.get(k) {
                        Some(&o) => TexSrc::Object(o),
                        None => {
                            self.problems.borrow_mut().push(format!("{}: {pn} has no weight map", self.stage));
                            TexSrc::White
                        }
                    }
                }
                Some((_, o)) => TexSrc::Object(o),
                None => {
                    self.problems.borrow_mut().push(format!("{}: {name} missing", self.stage));
                    TexSrc::White
                }
            }
        } else {
            match name {
                // dynamic sun shadow computed per pixel (sunshadow.rs), set in fragment()
                "LightAttenuationTexture" => return ("@hf_att".into(), String::new()),
                "NvStereoFixTexture" => TexSrc::White,
                _ => {
                    self.problems.borrow_mut().push(format!("{}: s{reg} {name} unbound", self.stage));
                    TexSrc::White
                }
            }
        };
        let k = self.slot(src);
        let si = self.tex_smp.borrow()[k];
        (format!("t{k}"), format!("s{si}"))
    }
}

const PRELUDE: &str = r#"
#import bevy_pbr::mesh_view_bindings::view

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
    objects: array<vec4<f32>>,
}

@group(2) @binding(0) var<storage, read> hfg: HfGlobals;
@group(2) @binding(1) var<storage, read> td: array<vec4<f32>>;

fn swz(v: vec3<f32>) -> vec3<f32> { return vec3<f32>(v.x, v.z, v.y); }
"#;

fn usage_key(d: &upk::sm3::Decl) -> String {
    format!("{:?}{}", d.usage, d.index)
}

/// WGSL module for one palette: the game's lighting-pass VS, lighting-pass PS and colour PS.
pub fn generate(
    vs_code: &[u8],
    psl_code: &[u8],
    psc_code: &[u8],
    res_light: &upk::matres::UniformSet,
    res_color: &upk::matres::UniformSet,
    weight_maps: &[i32],
    address: &dyn Fn(i32) -> [upk::matcompile::Address; 2],
) -> Option<Generated> {
    use upk::sm3::{translate, Shader, Usage};
    let vs = Shader::parse(vs_code)?;
    let psl = Shader::parse(psl_code)?;
    let psc = Shader::parse(psc_code)?;
    let shared = |stage, res| Binder {
        stage,
        res,
        weight_maps,
        address,
        textures: Default::default(),
        samplers: Default::default(),
        tex_smp: Default::default(),
        problems: Default::default(),
    };
    // one binder for all stages so texture/sampler slots are shared
    let mut b = shared("vs", res_light);
    let tvs = translate(&vs, &b);
    b.stage = "ps_light";
    let tpl = translate(&psl, &b);
    b.stage = "ps_color";
    b.res = res_color;
    let tpc = translate(&psc, &b);

    let mut w = String::from(PRELUDE);
    w.push_str(crate::crater::WGSL);
    w.push_str(&crate::gamemat::SHADOW_WGSL.read().map(|g| g.clone()).unwrap_or_default());
    w.push_str(crate::dynlights::WGSL);
    w.push_str("var<private> hf_att: vec4<f32>;\n");
    w.push_str(BINDINGS);
    if vs.relative_const || psl.relative_const || psc.relative_const {
        w.push_str("var<private> hf_c: array<vec4<f32>, 256>;\n");
    }
    // vertex stage
    let pos_reg = vs.outputs.iter().find(|d| matches!(d.usage, Usage::Position) && d.index == 0)?.reg;
    let varyings: Vec<&upk::sm3::Decl> = vs.outputs.iter().filter(|d| !(matches!(d.usage, Usage::Position) && d.index == 0)).collect();
    w.push_str("struct VOut {\n    @builtin(position) clip: vec4<f32>,\n");
    for (k, _) in varyings.iter().enumerate() {
        w.push_str(&format!("    @location({k}) l{k}: vec4<f32>,\n"));
    }
    w.push_str("}\n\nstruct VIn {\n    @location(0) pos: vec3<f32>,\n    @location(1) gv: vec4<f32>,\n}\n\n");
    let max_out = vs.outputs.iter().map(|d| d.reg).max().unwrap_or(0);
    w.push_str("@vertex\nfn vertex(vin: VIn) -> VOut {\n    let v0 = vin.gv;\n");
    for r in 0..=max_out.max(11) {
        w.push_str(&format!("    var o{r}: vec4<f32> = vec4<f32>(0.0);\n"));
    }
    w.push_str("    var o_pos: vec4<f32> = vec4<f32>(0.0);\n");
    w.push_str(&tvs.declarations);
    w.push_str(&tvs.body);
    w.push_str(&format!("    var out: VOut;\n    out.clip = o{pos_reg};\n"));
    for (k, d) in varyings.iter().enumerate() {
        w.push_str(&format!("    out.l{k} = o{};\n", d.reg));
    }
    w.push_str("    return out;\n}\n\n");
    // pixel stages as functions of their input registers
    let ps_fn = |w: &mut String, fname: &str, s: &Shader, t: &upk::sm3::Translated| {
        let params: Vec<String> = s.inputs.iter().map(|d| format!("v{}: vec4<f32>", d.reg)).collect();
        w.push_str(&format!("fn {fname}(vpos: vec4<f32>, vface: vec4<f32>{}{}) -> vec4<f32> {{\n", if params.is_empty() { "" } else { ", " }, params.join(", ")));
        w.push_str("    var oc0: vec4<f32> = vec4<f32>(0.0);\n    var oc1: vec4<f32> = vec4<f32>(0.0);\n    var o_depth: vec4<f32> = vec4<f32>(0.0);\n");
        w.push_str(&t.declarations);
        w.push_str(&t.body);
        w.push_str("    return oc0;\n}\n\n");
    };
    ps_fn(&mut w, "ps_light", &psl, &tpl);
    ps_fn(&mut w, "ps_color", &psc, &tpc);
    let args = |s: &Shader, problems: &mut Vec<String>, stage: &str| -> String {
        s.inputs
            .iter()
            .map(|d| match varyings.iter().position(|o| usage_key(o) == usage_key(d)) {
                Some(k) => format!("i.l{k}"),
                None => {
                    problems.push(format!("{stage}: input {} not written by the VS", usage_key(d)));
                    "vec4<f32>(0.0)".into()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut problems = b.problems.borrow().clone();
    let al = args(&psl, &mut problems, "ps_light");
    let ac = args(&psc, &mut problems, "ps_color");
    let sep = |a: &str| if a.is_empty() { String::new() } else { format!(", {a}") };
    w.push_str(&format!(
        "@fragment\nfn fragment(i: VOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {{\n    \
         // D3D9 vPos is the pixel's top-left corner, vFace > 0 for front faces\n    \
         let vp = vec4<f32>(i.clip.xy - 0.5, 0.0, 0.0);\n    \
         let vf = vec4<f32>(select(-1.0, 1.0, ff));\n    \
         hf_att = vec4<f32>(hf_sun_shadow(i.clip));\n    \
         // dynamic point lights (dynlights.rs): world position from the depth, face normal\n    \
         let hf_ndc = vec2<f32>((i.clip.x - view.viewport.x) / view.viewport.z * 2.0 - 1.0, 1.0 - (i.clip.y - view.viewport.y) / view.viewport.w * 2.0);\n    \
         let hf_wh = view.world_from_clip * vec4<f32>(hf_ndc, i.clip.z, 1.0);\n    \
         let hf_wue = vec3<f32>(hf_wh.x, hf_wh.z, hf_wh.y) * (100.0 / hf_wh.w);\n    \
         var hf_n = normalize(cross(dpdx(hf_wue), dpdy(hf_wue)));\n    \
         if (hf_n.z < 0.0) {{ hf_n = -hf_n; }}\n    \
         let l = ps_light(vp, vf{});\n    \
         let c = ps_color(vp, vf{});\n    \
         // colour pass blend: SRCBLEND=DESTALPHA, DESTBLEND=SRCCOLOR\n    \
         let o = c.rgb * l.a + l.rgb * c.rgb + c.rgb * hf_dyn_lights(i.clip, hf_wue, hf_n);\n    \
         if (HF_NAN_DEBUG && !all(abs(o) < vec3<f32>(1e30))) {{ return vec4<f32>(50.0 * vec3<f32>(f32(!all(abs(l) < vec4<f32>(1e30))), 0.0, f32(!all(abs(c) < vec4<f32>(1e30)))), 1.0); }}\n    \
         if (HF_NAN_DEBUG) {{ return vec4<f32>(0.0, 0.2, 0.0, 1.0); }}\n    \
         return vec4<f32>(o * hf_crater(hf_wue), 1.0);\n}}\n",
        sep(&al),
        sep(&ac)
    ));
    let w = w.replace("HF_NAN_DEBUG", if std::env::var_os("HOMEBOTS_NAN_DEBUG").is_some() { "true" } else { "false" });
    let textures = b.textures.borrow().clone();
    let samplers = b.samplers.borrow().clone();
    if textures.len() > MAX_TEX || samplers.len() > MAX_SMP {
        problems.push(format!("{} textures / {} samplers exceed the binding slots", textures.len(), samplers.len()));
        return None;
    }
    Some(Generated { wgsl: w, textures, samplers, problems })
}

/// Meshes of one component, one per palette used by its sections. POSITION is the Bevy-space
/// position (bounds/culling only); the game's vertex shader works from ATTR_TERRAIN_VERT.
///
/// Quad diagonal: the shorter one in height (93 % agreement with the game's dynamic index
/// buffer for SP-Devil TerrainComponent_94 in the capture; exact rule not found yet).
pub fn component_meshes(t: &upk::terrain::TerrainRender, c: &upk::terrain::TerrainComp) -> Vec<(u8, Mesh)> {
    use bevy::render::mesh::{Indices, PrimitiveTopology};
    let [nx, ny] = c.verts();
    let step = 32.0 / c.tess as f32;
    let s = t.scale;
    let mut pos = Vec::with_capacity(nx * ny);
    let mut gv = Vec::with_capacity(nx * ny);
    for j in 0..ny {
        for i in 0..nx {
            let h = c.heights[j * nx + i];
            let ue = [
                t.location[0] + (c.base[0] as f32 + i as f32 * step) * s[0],
                t.location[1] + (c.base[1] as f32 + j as f32 * step) * s[1],
                t.location[2] + (h as f32 - 32768.0) / 128.0 * s[2],
            ];
            pos.push([ue[0] * 0.01, ue[2] * 0.01, ue[1] * 0.01]);
            gv.push([i as f32, j as f32, (h & 0xff) as f32, (h >> 8) as f32]);
        }
    }
    let mut per_pal: std::collections::BTreeMap<u8, Vec<u32>> = Default::default();
    let tess = c.tess as usize;
    for qj in 0..ny - 1 {
        for qi in 0..nx - 1 {
            let sx = c.base[0] as usize / 32 + qi / tess;
            let sy = c.base[1] as usize / 32 + qj / tess;
            let pal = t.section_palette.get(sy * t.sections[0] + sx).copied().unwrap_or(0);
            let v = |i: usize, j: usize| (j * nx + i) as u32;
            let h = |i: usize, j: usize| c.heights[j * nx + i] as i32;
            let (v00, v10, v01, v11) = (v(qi, qj), v(qi + 1, qj), v(qi, qj + 1), v(qi + 1, qj + 1));
            let d_a = (h(qi, qj) - h(qi + 1, qj + 1)).abs();
            let d_b = (h(qi + 1, qj) - h(qi, qj + 1)).abs();
            let idx = per_pal.entry(pal).or_default();
            if d_a < d_b {
                idx.extend_from_slice(&[v00, v01, v11, v00, v11, v10]);
            } else {
                idx.extend_from_slice(&[v00, v01, v10, v10, v01, v11]);
            }
        }
    }
    per_pal
        .into_iter()
        .map(|(pal, idx)| {
            let m = Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos.clone())
                .with_inserted_attribute(ATTR_TERRAIN_VERT, gv.clone())
                .with_inserted_indices(Indices::U32(idx));
            (pal, m)
        })
        .collect()
}
