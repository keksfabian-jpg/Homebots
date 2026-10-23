//! HUD minimap: a scene capture of the minimap geometry into "MinimapRenderTexture", which the
//! HUD's M_MiniMapTerrain material samples (trace t3 frame 2700).
//!
//! - Target 256x256, cleared to black; orthographic top-down view, 2 / 0.0004 = 5000 uu across
//!   (projection x/y scale 0.0004), centred on the player, rotated so the view direction is up
//!   (the player arrow in the user's video always points up).
//! - Drawn: StaticMeshComponents with a Homefront MinimapRenderMode. MRM_Terrain (the *_minimap
//!   sublevel's HiddenGame plane, Minimap.Statics.<Map>_minimap) with its material's emissive
//!   (MI_MiniMap_<Map>, PS 0x617e4f60 = the material graph); MRM_Impassable with PS 0x2f8f9680:
//!   colour = 0.45 * normalize(world normal).z + UniformVector_0 (0,0,0); MRM_Road with
//!   PS 0x2f8fa580: tex(vertex colour) * (0.694, 0.8, 0.652) + UniformVector_0 - the lookup
//!   texture is not identified, taken as 1 (NOT from data).
//! - The capture's render target is Rgba8UnormSrgb: the UI samples back exactly the values the
//!   shaders wrote, as with the game's plain RGBA8 target (Bevy's blit needs the sRGB format).
use crate::menu::{MatchFlow, Phase};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey, NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::camera::{RenderTarget, ScalingMode};
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::RenderLayers;
use std::path::PathBuf;

/// Render layer of the minimap geometry (the main camera renders layer 0 only).
pub const LAYER: usize = 1;
/// MinimapRenderTexture size (trace t3: SetViewport 256x256 on the capture target).
pub const RT_SIZE: u32 = 256;
/// Width of the captured square in UE units (trace t3 frame 2700 projection scale 0.0004).
pub const VIEW_UU: f32 = 5000.0;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        let mut img = Image::new_fill(
            Extent3d { width: RT_SIZE, height: RT_SIZE, depth_or_array_layers: 1 },
            TextureDimension::D2,
            &[0, 0, 0, 255],
            // sRGB storage: written and sampled values are the shader's (D3D9: plain RGBA8)
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
        let h = app.world_mut().resource_mut::<Assets<Image>>().add(img);
        app.insert_resource(MinimapTarget(h))
            .init_resource::<Pending>()
            .add_plugins(MaterialPlugin::<MinimapMat> { prepass_enabled: false, shadows_enabled: false, ..default() })
            .add_plugins(bevy::render::extract_component::ExtractComponentPlugin::<MinimapCam>::default())
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (build, follow).chain());
    }
}

/// The capture target (menu.rs binds it for Textures.MinimapRenderTexture).
#[derive(Resource, Clone)]
pub struct MinimapTarget(pub Handle<Image>);

/// Marks the capture camera (extracted: the scene-copy nodes skip its view).
#[derive(Component, Clone, bevy::render::extract_component::ExtractComponent)]
pub struct MinimapCam;

/// Filled by the level loader: world placements with a MinimapRenderMode (meshes already
/// built) and the map folder (for the *_minimap sublevel).
#[derive(Resource, Default)]
pub struct Pending {
    pub objects: Vec<(Vec<Handle<Mesh>>, Transform, String)>,
    pub dir: Option<PathBuf>,
    pub ready: bool,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(MinimapMatKey)]
pub struct MinimapMat {
    /// UniformVector_0 (added to the colour)
    #[uniform(0)]
    pub color: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub t0: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub t1: Option<Handle<Image>>,
    #[texture(5)]
    #[sampler(6)]
    pub t2: Option<Handle<Image>>,
    #[texture(7)]
    #[sampler(8)]
    pub t3: Option<Handle<Image>>,
    pub shader: Handle<Shader>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MinimapMatKey {
    shader: Handle<Shader>,
}

impl From<&MinimapMat> for MinimapMatKey {
    fn from(m: &MinimapMat) -> Self {
        MinimapMatKey { shader: m.shader.clone() }
    }
}

impl Material for MinimapMat {
    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vb = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            crate::gamemat::ATTR_TANGENT_Z.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vb];
        descriptor.vertex.shader = key.bind_group_data.shader.clone();
        descriptor.vertex.entry_point = "vertex".into();
        if let Some(f) = descriptor.fragment.as_mut() {
            f.shader = key.bind_group_data.shader.clone();
            f.entry_point = "fragment".into();
        }
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

const PRELUDE: &str = r#"
#import bevy_pbr::mesh_view_bindings::{view, globals}
#import bevy_pbr::mesh_functions::get_world_from_local

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

@group(2) @binding(0) var<uniform> mm_color: vec4<f32>;
@group(2) @binding(1) var t0: texture_2d<f32>;
@group(2) @binding(2) var s0: sampler;
@group(2) @binding(3) var t1: texture_2d<f32>;
@group(2) @binding(4) var s1: sampler;
@group(2) @binding(5) var t2: texture_2d<f32>;
@group(2) @binding(6) var s2: sampler;
@group(2) @binding(7) var t3: texture_2d<f32>;
@group(2) @binding(8) var s3: sampler;

fn hf_sky_light(n: vec3<f32>) -> vec3<f32> { return vec3<f32>(1.0); }
fn hf_material_tint() -> vec4<f32> { return vec4<f32>(1.0); }
fn hf_env_cube(c: vec4<f32>) -> vec4<f32> { return c; }
fn hf_scene_color(uv: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

fn inverse3(m: mat3x3<f32>) -> mat3x3<f32> {
    let a = m[0]; let b = m[1]; let c = m[2];
    let r0 = cross(b, c); let r1 = cross(c, a); let r2 = cross(a, b);
    let det = dot(a, r0);
    return transpose(mat3x3<f32>(r0, r1, r2)) * (1.0 / det);
}

struct VIn {
    @builtin(instance_index) instance: u32,
    @location(0) pos: vec3<f32>,
    @location(1) tz: vec4<f32>,
    @location(2) uv0: vec2<f32>,
    @location(3) uv1: vec2<f32>,
}

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) uv1: vec2<f32>,
    @location(2) normal: vec3<f32>,
}

@vertex
fn vertex(v: VIn) -> VOut {
    var o: VOut;
    let w = get_world_from_local(v.instance);
    o.clip = view.clip_from_world * (w * vec4<f32>(v.pos, 1.0));
    // UE TangentZ (UBYTE4, x * 2/255 - 1) in UE local axes -> Bevy local (x, z, y) -> world
    let n = (v.tz.xyz * 255.0) * 0.00784314 - 1.0;
    let m3 = mat3x3<f32>(w[0].xyz, w[1].xyz, w[2].xyz);
    let nw = transpose(inverse3(m3)) * vec3<f32>(n.x, n.z, n.y);
    // back to UE world axes (z up)
    o.normal = vec3<f32>(nw.x, nw.z, nw.y);
    o.uv0 = v.uv0;
    o.uv1 = v.uv1;
    return o;
}
"#;

/// MRM_Impassable: PS 0x2f8f9680.
const IMPASSABLE: &str = r#"
@fragment
fn fragment(i: VOut) -> @location(0) vec4<f32> {
    let nz = normalize(i.normal).z;
    return vec4<f32>(vec3<f32>(nz * 0.45) + mm_color.xyz, 1.0);
}
"#;

/// MRM_Road: PS 0x2f8fa580 with the lookup texture taken as 1.
const ROAD: &str = r#"
@fragment
fn fragment(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(vec3<f32>(0.694, 0.8, 0.652) + mm_color.xyz, 1.0);
}
"#;

/// MRM_Terrain: the component material's emissive (compiled graph).
const TERRAIN: &str = r#"
@fragment
fn fragment(i: VOut) -> @location(0) vec4<f32> {
    var m: MatIn;
    m.uv = array<vec2<f32>, 4>(i.uv0, i.uv1, i.uv1, i.uv1);
    m.vcolor = vec4<f32>(1.0);
    m.cam = vec3<f32>(0.0, 0.0, 1.0);
    m.refl = vec3<f32>(0.0, 0.0, 1.0);
    m.refl_world = vec3<f32>(0.0, 0.0, 1.0);
    m.light = vec3<f32>(0.0, 0.0, 1.0);
    m.tangent_to_world = mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    m.screen = i.clip.xy / view.viewport.zw;
    m.time = globals.time;
    m.scene_w = 1000000.0;
    let o = mat_eval(m);
    return vec4<f32>(o.emissive + mm_color.xyz, 1.0);
}
"#;

fn spawn_camera(mut commands: Commands, target: Res<MinimapTarget>) {
    commands.spawn((
        Camera3d::default(),
        Camera { order: -5, target: RenderTarget::Image(target.0.clone().into()), is_active: false, clear_color: ClearColorConfig::Custom(Color::BLACK), hdr: false, ..default() },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed { width: VIEW_UU * 0.01, height: VIEW_UU * 0.01 },
            near: 0.0,
            far: 4000.0,
            ..OrthographicProjection::default_3d()
        }),
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(LAYER),
        Transform::default(),
        MinimapCam,
    ));
}

#[derive(Default)]
struct Shaders {
    impassable: Option<Handle<Shader>>,
    road: Option<Handle<Shader>>,
    lib: Option<upk::library::Library>,
}

fn flat_material(st: &mut Shaders, mode: &str, shaders: &mut Assets<Shader>, mats: &mut Assets<MinimapMat>) -> Handle<MinimapMat> {
    let road = mode == "MRM_Road";
    let slot = if road { &mut st.road } else { &mut st.impassable };
    let sh = slot.get_or_insert_with(|| shaders.add(Shader::from_wgsl(format!("{PRELUDE}\n{}", if road { ROAD } else { IMPASSABLE }), if road { "minimap_road.wgsl" } else { "minimap_impassable.wgsl" }))).clone();
    mats.add(MinimapMat { color: Vec4::ZERO, t0: None, t1: None, t2: None, t3: None, shader: sh })
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    mut pending: ResMut<Pending>,
    root: Option<Res<crate::sfx::GameRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut mats: ResMut<Assets<MinimapMat>>,
    mut st: Local<Shaders>,
) {
    if !pending.ready {
        return;
    }
    pending.ready = false;
    let layer = RenderLayers::layer(LAYER);
    // one material per mode for the world objects
    let mut by_mode: std::collections::HashMap<String, Handle<MinimapMat>> = Default::default();
    let mut n = 0;
    for (parts, tf, mode) in std::mem::take(&mut pending.objects) {
        if mode != "MRM_Impassable" && mode != "MRM_Road" {
            continue;
        }
        let m = by_mode.entry(mode.clone()).or_insert_with(|| flat_material(&mut st, &mode, &mut shaders, &mut mats)).clone();
        for h in parts {
            commands.spawn((Mesh3d(h), MeshMaterial3d(m.clone()), tf, layer.clone(), NotShadowCaster, NotShadowReceiver, crate::LevelEntity));
            n += 1;
        }
    }
    // the *_minimap sublevel: terrain plane (HiddenGame) and impassable markers
    let Some(dir) = pending.dir.take() else { return };
    if st.lib.is_none() {
        let Some(root) = root else { return };
        st.lib = upk::library::Library::scan(&root.0).ok();
    }
    let file = std::fs::read_dir(&dir).ok().and_then(|rd| {
        rd.filter_map(|e| e.ok().map(|e| e.path())).find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().to_ascii_lowercase().ends_with("_minimap")) && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("map")))
    });
    let Some(file) = file else {
        info!("minimap: {} world objects, no minimap sublevel in {}", n, dir.display());
        return;
    };
    let Ok(pkg) = upk::Package::open(&file) else {
        warn!("minimap: {} not readable", file.display());
        return;
    };
    let Some(lib) = st.lib.take() else { return };
    let mut terrain = 0;
    for (pl, mode) in pkg.minimap_placements() {
        if pl.mesh <= 0 {
            continue;
        }
        let Some(sm) = pkg.read_static_mesh_full(pl.mesh as usize - 1, &lib) else { continue };
        let Some(lod) = sm.lods.first() else { continue };
        let tf = Transform {
            translation: crate::ue_to_bevy(pl.location) * 0.01,
            rotation: crate::rotator_to_quat(pl.rotation),
            scale: Vec3::new(pl.scale[0], pl.scale[2], pl.scale[1]) * 0.01,
        };
        for (si, s) in lod.sections.iter().enumerate() {
            let Some(mesh) = crate::build_game_mesh(lod, s) else { continue };
            let mat = if mode == "MRM_Terrain" {
                let mref = pl.materials.get(si).copied().filter(|&m| m != 0).unwrap_or(s.material);
                match terrain_material(&pkg, &lib, mref, &mut images, &mut shaders, &mut mats) {
                    Some(m) => m,
                    None => continue,
                }
            } else {
                by_mode.entry(mode.clone()).or_insert_with(|| flat_material(&mut st, &mode, &mut shaders, &mut mats)).clone()
            };
            commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), tf, layer.clone(), NotShadowCaster, NotShadowReceiver, crate::LevelEntity));
            if mode == "MRM_Terrain" {
                terrain += 1;
            }
            n += 1;
        }
    }
    st.lib = Some(lib);
    info!("minimap: {n} objects ({terrain} terrain) incl. {}", file.file_name().unwrap_or_default().to_string_lossy());
}

fn terrain_material(pkg: &upk::Package, lib: &upk::library::Library, mref: i32, images: &mut Assets<Image>, shaders: &mut Assets<Shader>, mats: &mut Assets<MinimapMat>) -> Option<Handle<MinimapMat>> {
    if mref <= 0 {
        warn!("minimap: terrain material is an import");
        return None;
    }
    let c = pkg.compile_material(mref)?;
    let mut tex: Vec<Option<Handle<Image>>> = Vec::new();
    for slot in c.textures.iter().take(4) {
        let img = if slot.object > 0 { crate::gamemat::load_texture(pkg, lib, slot.object, slot.address, 4096) } else { None };
        tex.push(img.map(|i| images.add(i)));
    }
    let src = format!("{PRELUDE}\n{}\n{TERRAIN}", c.code);
    if let Err(e) = crate::gamemat::validate_wgsl(&src) {
        warn!("minimap: terrain material {}: {e}", pkg.object_path(mref));
        return None;
    }
    let shader = shaders.add(Shader::from_wgsl(src, format!("minimap_terrain_{mref}.wgsl")));
    let get = |i: usize| tex.get(i).cloned().flatten();
    Some(mats.add(MinimapMat { color: Vec4::ZERO, t0: get(0), t1: get(1), t2: get(2), t3: get(3), shader }))
}

/// Minimap view for the HUD (hud.rs): centre and heading in UE units / radians, icons.
#[derive(Clone, Debug, Default)]
pub struct MinimapHud {
    pub center: [f32; 2],
    pub yaw: f32,
    /// objective location, letter, owner
    pub objectives: Vec<([f32; 2], usize, Option<u8>)>,
    /// friendly soldiers
    pub friends: Vec<[f32; 2]>,
    /// enemy soldiers while a UAV sweep runs
    pub enemies: Vec<[f32; 2]>,
    /// full map (M held, GCInput EKBC_OpenZoneMap StartFullMap / StopFullMap): the captured
    /// square (min x, min y, side, UE units, X right / Y down) and the MinimapVolume bounds
    pub full: Option<([f32; 3], [f32; 4])>,
}

/// Size of the capture while the full map shows (NOT from data: the minimap's 256 would blur
/// a whole level).
const FULL_RT: u32 = 1024;

#[allow(clippy::type_complexity)]
fn follow(
    (_keys, config, mut images, target): (Res<ButtonInput<KeyCode>>, Option<Res<crate::menu::MatchConfig>>, ResMut<Assets<Image>>, Res<MinimapTarget>),
    (mut bounds, level, mut open): (Local<Option<(std::path::PathBuf, Option<[f32; 4]>)>>, Option<Res<crate::objectives::LevelData>>, Local<bool>),
    mut flow: ResMut<MatchFlow>,
    main: Query<&GlobalTransform, (With<Camera3d>, Without<MinimapCam>)>,
    pawns: Query<&crate::player::Pawn>,
    obj: Res<crate::objectives::Objectives>,
    (soldiers, marks): (Query<(Entity, &crate::combat::Soldier)>, Res<crate::drone::ReconMarks>),
    mut cam: Query<(&mut Camera, &mut Transform, &mut Projection), With<MinimapCam>>,
) {
    let _perf = crate::perf::scope("minimap");
    let Ok((mut c, mut t, mut proj)) = cam.single_mut() else { return };
    let playing = flow.phase == Phase::Playing;
    let view = main.iter().next();
    let (Some(view), true) = (view, playing) else {
        if c.is_active {
            c.is_active = false;
        }
        flow.hud.minimap = None;
        return;
    };
    if !c.is_active {
        c.is_active = true;
    }
    let pos = match pawns.single() {
        Ok(p) if !p.in_vehicle => p.feet,
        _ => view.translation(),
    };
    let f = view.forward();
    let fwd = Vec3::new(f.x, 0.0, f.z).try_normalize().unwrap_or(Vec3::X);
    // full-map bounds: the level's ground MinimapVolume (<map>_FrontlinesData, brush polygons)
    if let Some(cfg) = config.as_ref() {
        if bounds.as_ref().is_none_or(|b| b.0 != cfg.0.folder) {
            let maps = crate::level_files(&cfg.0.folder);
            let bb = match level.as_ref().filter(|l| l.maps.first().and_then(|m| m.parent()) == Some(cfg.0.folder.as_path())) {
                Some(l) => l.minimap_bounds,
                None => {
                    let pkgs: Vec<upk::Package> = maps.iter().filter_map(|m| upk::Package::open(m).ok()).collect();
                    crate::objectives::minimap_bounds(pkgs.iter())
                }
            };
            info!("full map bounds {bb:?}");
            *bounds = Some((cfg.0.folder.clone(), bb));
        }
    }
    // M opens, M again closes (user's wish; GCInput binds StartFullMap / StopFullMap to press /
    // release)
    if crate::input::hit(crate::input::Set::Global, "EKBC_OpenZoneMap") && !flow.paused {
        *open = !*open;
    }
    if flow.paused {
        *open = false;
    }
    let full = (*open || std::env::var_os("HB_FULLMAP").is_some()) && !flow.paused;
    let bb = bounds.as_ref().and_then(|b| b.1);
    let mut full_info = None;
    let want = if full && bb.is_some() { FULL_RT } else { RT_SIZE };
    if images.get(&target.0).is_some_and(|i| i.texture_descriptor.size.width != want) {
        if let Some(img) = images.get_mut(&target.0) {
            img.resize(Extent3d { width: want, height: want, depth_or_array_layers: 1 });
        }
    }
    if let (true, Some(b)) = (full, bb) {
        let side = (b[2] - b[0]).max(b[3] - b[1]) * 1.02;
        let (cx, cy) = ((b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5);
        if let Projection::Orthographic(o) = &mut *proj {
            o.scaling_mode = ScalingMode::Fixed { width: side * 0.01, height: side * 0.01 };
        }
        // looking down, screen up = UE -Y: UE X right, UE Y down
        *t = Transform::from_translation(Vec3::new(cx * 0.01, pos.y + 2000.0, cy * 0.01)).looking_to(Vec3::NEG_Y, Vec3::NEG_Z);
        full_info = Some(([cx - side * 0.5, cy - side * 0.5, side], b));
    } else {
        if let Projection::Orthographic(o) = &mut *proj {
            if !matches!(o.scaling_mode, ScalingMode::Fixed { width, .. } if (width - VIEW_UU * 0.01).abs() < 1e-3) {
                o.scaling_mode = ScalingMode::Fixed { width: VIEW_UU * 0.01, height: VIEW_UU * 0.01 };
            }
        }
        *t = Transform::from_translation(Vec3::new(pos.x, pos.y + 2000.0, pos.z)).looking_to(Vec3::NEG_Y, fwd);
    }
    let mut hud = MinimapHud { center: [pos.x * 100.0, pos.z * 100.0], yaw: fwd.z.atan2(fwd.x), full: full_info, ..default() };
    for o in obj.list.iter().filter(|o| o.line == obj.active_line) {
        hud.objectives.push(([o.location[0], o.location[1]], o.letter, o.owner));
    }
    for (e, s) in &soldiers {
        if s.alive && s.team == crate::combat::player_team() {
            let c = s.center_ue();
            hud.friends.push([c.x, c.y]);
        } else if s.alive && (flow.hud.uav_left > 0.0 || marks.marked.contains_key(&e)) {
            let c = s.center_ue();
            hud.enemies.push([c.x, c.y]);
        }
    }
    flow.hud.minimap = Some(hud);
}
