//! Front-end menus: the game's own UIScenes (CookedPC/UI/*.upk) laid out and drawn by
//! `upk::ui`, rendered by a second camera on top of the 3D view like the game's canvas.
//!
//! Navigation is data-driven: menu labels (GCUILabel_MenuAction) and command buttons carry
//! GCCommand objects; the ones the offline game supports are executed here, the others
//! (single player, online services) are shown locked (GCUIInfo.m_LockedItemColor).
use bevy::core_pipeline::core_2d::graph::{Core2d, Node2d};
use bevy::core_pipeline::fullscreen_vertex_shader::fullscreen_shader_vertex_state;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_graph::{NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner};
use bevy::render::render_resource::binding_types::texture_2d;
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice};
use bevy::render::view::ViewTarget;
use bevy::render::RenderApp;
use bevy::sprite::{AlphaMode2d, Material2d, Material2dPlugin};
use bevy::window::PrimaryWindow;
use std::collections::HashMap;
use std::path::PathBuf;
use upk::ui::{GRef, Quad, Scene, Ui, UiState, UiWorld, WidgetRt};

pub struct MenuPlugin {
    pub game: PathBuf,
    /// start in the main menu (no map given on the command line)
    pub enabled: bool,
    /// main-menu background level (map file stem), see main.rs menu_background
    pub background: String,
}

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "ui.wgsl");
        bevy::asset::embedded_asset!(app, "uigamma.wgsl");
        app.add_plugins(Material2dPlugin::<UiMat>::default())
            .add_plugins(Material2dPlugin::<UiMatGraph>::default())
            .add_plugins(ExtractComponentPlugin::<UiGammaOut>::default())
            .insert_non_send_resource(MenuInit { game: self.game.clone(), enabled: self.enabled, background: self.background.clone() })
            .insert_resource(InMenu(self.enabled))
            .init_resource::<MatchFlow>()
            .init_resource::<crate::scoreboard::ScoreRows>()
            .add_systems(Startup, setup)
            .add_systems(Update, (crate::scoreboard::collect.before(update), update, menu_fov, loading_cameras.after(update)))
            .add_systems(Last, refresh_ui_mats);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_render_graph_node::<ViewNodeRunner<UiGammaNode>>(Core2d, UiGammaLabel)
            .add_render_graph_edges(Core2d, (Node2d::Tonemapping, UiGammaLabel, Node2d::EndMainPassPostProcessing));
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<UiGammaPipeline>();
    }
}

/// A resized image (the minimap capture switches between 256 and 1024 for the full map, M) gets
/// a new GPU texture, but Bevy keeps a material's bind group until the material itself changes:
/// the HUD went on showing the old texture - the minimap froze after the full map had been
/// open. Materials that use a modified image are marked changed so their bind group is rebuilt.
fn refresh_ui_mats(mut events: EventReader<AssetEvent<Image>>, mut ui: ResMut<Assets<UiMat>>, mut graph: ResMut<Assets<UiMatGraph>>) {
    let ids: Vec<AssetId<Image>> = events.read().filter_map(|e| if let AssetEvent::Modified { id } = e { Some(*id) } else { None }).collect();
    if ids.is_empty() {
        return;
    }
    let uses = |h: &Option<Handle<Image>>| h.as_ref().is_some_and(|h| ids.contains(&h.id()));
    let hit_ui: Vec<AssetId<UiMat>> = ui.iter().filter(|(_, m)| ids.contains(&m.tex.id())).map(|(id, _)| id).collect();
    for id in hit_ui {
        let _ = ui.get_mut(id);
    }
    let hit_g: Vec<AssetId<UiMatGraph>> = graph.iter().filter(|(_, m)| ids.contains(&m.scene.id()) || uses(&m.t0) || uses(&m.t1) || uses(&m.t2) || uses(&m.t3)).map(|(id, _)| id).collect();
    for id in hit_g {
        let _ = graph.get_mut(id);
    }
}

struct MenuInit {
    game: PathBuf,
    enabled: bool,
    background: String,
}

/// Settings of the running match (from "Spiel erstellen").
#[derive(Resource, Clone, Debug)]
pub struct MatchConfig(pub crate::creategame::MatchSettings);

/// The front end owns input and the camera.
#[derive(Resource)]
pub struct InMenu(pub bool);

/// Front-end camera projection: trace frames 400-1200 (1366x768) ViewProjection x scale
/// 3.405349, y scale 6.05691 (= x scale * aspect), i.e. a fixed horizontal field of view.
#[derive(Component)]
pub struct MenuFov;

const MENU_PROJ_X: f32 = 3.405349;

fn menu_fov(windows: Query<&Window, With<PrimaryWindow>>, mut cams: Query<&mut Projection, With<MenuFov>>, flow: Res<MatchFlow>) {
    if flow.phase == Phase::Playing {
        return;
    }
    let Ok(w) = windows.single() else { return };
    let aspect = w.physical_width() as f32 / w.physical_height().max(1) as f32;
    for mut p in &mut cams {
        if let Projection::Perspective(pp) = &mut *p {
            let fov = 2.0 * (1.0 / (MENU_PROJ_X * aspect)).atan();
            if (pp.fov - fov).abs() > 1e-6 {
                pp.fov = fov;
            }
        }
    }
}

/// Request to the level loader (main.rs): load these maps instead of the current ones.
#[derive(Resource, Clone, Debug)]
pub struct LevelRequest {
    pub maps: Vec<PathBuf>,
    /// persistent level the maps are streamed into (its WorldInfo provides the default
    /// post-process settings), e.g. HFGameEntry.map for the front end
    pub persistent: Option<PathBuf>,
    /// keep loaded after switching away (front-end levels)
    pub cache: bool,
    /// make it the visible level (false: preload in the background)
    pub show: bool,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct UiMat {
    #[texture(0)]
    #[sampler(1)]
    pub tex: Handle<Image>,
}

impl Material2d for UiMat {
    fn fragment_shader() -> bevy::render::render_resource::ShaderRef {
        "embedded://homebots/ui.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Canvas tile drawn with a UE3 material (MaterialInstanceConstant in an image component),
/// compiled by upk::matcompile; SceneTexture samples the scene colour before post-processing.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(UiMatKey)]
pub struct UiMatGraph {
    #[texture(0)]
    pub scene: Handle<Image>,
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
    /// run-time scalar parameters (mat_param(i), e.g. M_roundedMask Progress)
    #[uniform(9)]
    pub params: Vec4,
    pub shader: Handle<Shader>,
    /// BLEND_Additive (1) / BLEND_Modulate (2): frame buffer blend of the material
    pub blend: u8,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct UiMatKey {
    shader: Handle<Shader>,
    blend: u8,
}

impl From<&UiMatGraph> for UiMatKey {
    fn from(m: &UiMatGraph) -> Self {
        UiMatKey { shader: m.shader.clone(), blend: m.blend }
    }
}

impl Material2d for UiMatGraph {
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::render::mesh::MeshVertexBufferLayoutRef,
        key: bevy::sprite::Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(f) = descriptor.fragment.as_mut() {
            f.shader = key.bind_group_data.shader.clone();
            f.entry_point = "fragment".into();
            // UE3 material blend modes: additive = ONE/ONE, modulate = DESTCOLOR/ZERO
            let comp = match key.bind_group_data.blend {
                1 => Some(BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add }),
                2 => Some(BlendComponent { src_factor: BlendFactor::Dst, dst_factor: BlendFactor::Zero, operation: BlendOperation::Add }),
                _ => None,
            };
            if let (Some(c), Some(Some(t))) = (comp, f.targets.first_mut()) {
                t.blend = Some(BlendState { color: c, alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add } });
            }
        }
        Ok(())
    }
}

const UI_MAT_PRELUDE: &str = r#"
#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bevy_sprite::mesh2d_view_bindings::{view, globals}

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

@group(2) @binding(0) var ui_scene: texture_2d<f32>;
@group(2) @binding(1) var t0: texture_2d<f32>;
@group(2) @binding(2) var s0: sampler;
@group(2) @binding(3) var t1: texture_2d<f32>;
@group(2) @binding(4) var s1: sampler;
@group(2) @binding(5) var t2: texture_2d<f32>;
@group(2) @binding(6) var s2: sampler;
@group(2) @binding(7) var t3: texture_2d<f32>;
@group(2) @binding(8) var s3: sampler;
@group(2) @binding(9) var<uniform> ui_params: vec4<f32>;

fn mat_param(i: i32) -> f32 {
    return ui_params[i];
}

// SceneColorTexture: point-sampled (trace: MIN/MAG/MIP POINT, clamp), linear HDR scene before post
fn hf_sky_light(n: vec3<f32>) -> vec3<f32> { return vec3<f32>(1.0); }
fn hf_material_tint() -> vec4<f32> { return vec4<f32>(1.0); }
fn hf_env_cube(c: vec4<f32>) -> vec4<f32> { return c; }
fn hf_scene_color(uv: vec2<f32>) -> vec4<f32> {
    let p =vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(0.99999)) * view.viewport.zw);
    return vec4<f32>(textureLoad(ui_scene, p, 0).rgb, 1.0);
}
"#;

const UI_MAT_FRAGMENT: &str = r#"
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var m: MatIn;
    m.uv = array<vec2<f32>, 4>(in.uv, in.uv, in.uv, in.uv);
#ifdef VERTEX_COLORS
    m.vcolor = in.color;
#else
    m.vcolor = vec4<f32>(1.0);
#endif
    m.cam = vec3<f32>(0.0, 0.0, 1.0);
    m.refl = vec3<f32>(0.0, 0.0, 1.0);
    m.refl_world = vec3<f32>(0.0, 0.0, 1.0);
    m.light = vec3<f32>(0.0, 0.0, 1.0);
    m.tangent_to_world = mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    m.screen = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    m.time = globals.time;
    m.scene_w = 1000000.0;
    let o = mat_eval(m);
    // canvas material tile (PS 2e5b4200): rgb = emissive, written as is (gamma-space target)
    return vec4<f32>(o.emissive, ALPHA);
}
"#;

/// Marks the UI camera: its view is converted gamma -> linear after the UI pass.
#[derive(Component, Clone, Copy, ExtractComponent)]
pub struct UiGammaOut;

#[derive(Component)]
struct UiBatch(usize);

enum Batch {
    Tex(Handle<UiMat>),
    Graph(Handle<UiMatGraph>),
}

/// What a menu entry or button does.
#[derive(Debug, Clone)]
enum Action {
    Open(GRef, Option<String>),
    Back,
    Exit,
    AddToQueue,
    RemoveFromQueue,
    Launch,
    /// spawn selection: "Entstehen"
    Spawn,
    /// pause menu "SPIEL VERLASSEN": confirmation popup
    LeaveConfirm,
    /// confirmed: end the match, back to the front end
    LeaveMatch,
    LeaveDone,
    /// options: "Übernehmen" (window mode / resolution) and "Standard"
    ApplyOptions,
    ResetOptions,
    /// pause menu "KLASSE WECHSELN" (GCCommand_OpenSpawnSelection): the spawn selection while
    /// alive; "Fertig" keeps the choice for the next spawn, "Selbstmord" respawns now
    ChangeClass,
    ClassDone,
    ClassSuicide,
    /// spawn selection "TEAM WECHSELN" (GCCommand_UISwitchTeam): dead = the new team's spawn
    /// selection at once, alive (pause menu "AUSRÜSTUNG WECHSELN") = die, respawn on the new team
    SwitchTeam,
}

/// Where the match flow is (front end -> "Spiel wird erstellt" -> movie + loading -> spawn
/// selection -> playing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Front,
    Creating,
    Movie,
    Spawn,
    Playing,
    /// "Spielsitzung geschlossen." before returning to the front end
    Leaving,
    /// black loading screen while the front end level loads
    Loading,
}

/// Match flow state other systems read (player, HUD).
#[derive(Resource, Default, Debug, Clone)]
pub struct MatchFlow {
    pub phase: Phase,
    /// the player pressed "Entstehen" (or the auto spawn ran out): spawn once
    pub spawn_requested: bool,
    /// selected loadout index (DefaultLoadouts) and vehicle class
    pub loadout: usize,
    pub vehicle: Option<String>,
    /// spawn selection vehicle entry chosen last (kept for the next spawn menu)
    pub vehicle_sel: usize,
    /// pause menu "Selbstmord": the player dies (combat.rs) and gets the spawn selection
    pub suicide: bool,
    /// a menu scene is open over the game (pause menu): no player input
    pub paused: bool,
    /// the debug menu (F1, debug.rs) is open: game input off, cursor free
    pub debug: bool,
    /// values on the HUD (timer, scores, KP, ammo)
    pub hud: crate::hud::HudValues,
    /// counts the spawns (combat.rs resets the weapon on a change)
    pub spawn_serial: u32,
    /// the player died: back to the spawn selection (combat.rs sets it)
    pub respawn: bool,
    /// the match is over (objectives.rs): back to the main menu
    pub leave: bool,
}

/// Movie frame at which automated tests (HOMEBOTS_MENU_LAUNCH) skip the movie: trace t3 shows the
/// GroundControl movie at frame 288 (of 642) when the game had loaded. Interactive: Enter (shown
/// once the packages are read) or the movie's end.
const MOVIE_LOAD_FRAME: usize = 288;

/// "Spiel wird erstellt …" popup time before the movie (user's video, see Phase::Creating).
const CREATING_POPUP_SECS: f64 = 3.0;

struct MenuW {
    labels: Vec<usize>,
}

struct Active {
    root: GRef,
    scene: Scene,
    size: (f32, f32),
    menus: Vec<MenuW>,
    buttons: Vec<usize>,
    actions: HashMap<usize, Option<Action>>,
    sel: Option<(usize, usize)>,
    hover: Option<usize>,
    /// 3D level behind this scene (opening command's m_LevelName, else the scene's)
    level: Option<String>,
    /// PC_U_CreateMPGameScreen state
    create: Option<crate::creategame::CreateGame>,
    /// drawn over the scene below (message boxes)
    popup: bool,
    /// label texts set at runtime (widget index)
    texts: HashMap<usize, String>,
    /// widgets shown/hidden at runtime
    shown: HashMap<usize, bool>,
    /// PC_U_SpawnSelectionMenu state
    spawn: Option<crate::spawnsel::SpawnSel>,
    /// options screens (options.rs)
    options: Option<crate::options::Options>,
}

struct Menu {
    ui: Ui,
    lib: upk::library::Library,
    stack: Vec<Active>,
    textures: HashMap<GRef, Option<Handle<Image>>>,
    mats: HashMap<GRef, Handle<UiMat>>,
    graphs: HashMap<GRef, Option<Handle<UiMatGraph>>>,
    /// batch entities: mesh and whether they carry a UiMatGraph (else UiMat)
    batches: Vec<(Entity, Handle<Mesh>, bool)>,
    last: Vec<Quad>,
    colors: ([f32; 4], [f32; 4], [f32; 4]),
    version: Option<String>,
    /// level currently requested from the loader
    loaded: Option<String>,
    /// levels still to load hidden
    preload: Vec<String>,
    game: PathBuf,
    background: String,
    /// a match is running: no menu drawn, Esc returns to the main menu
    in_game: bool,
    last_click: Option<(f64, Vec2)>,
    phase: Phase,
    now: f64,
    creating_since: f64,
    movie_file: Option<PathBuf>,
    /// maps of the match being started
    match_maps: Vec<PathBuf>,
    load_requested: Option<u32>,
    /// the Loading phase builds the match level (not the front end)
    match_loading: bool,
    /// the match level was built during the "creating" popup: the movie follows, then the
    /// spawn selection (no loading screen after the movie)
    match_built: bool,
    /// the movie is over (or skipped) but the match preload (warmup.rs) is not: the loading
    /// screen stays until it is, then the spawn selection
    await_preload: bool,
    /// PC_UI_Fonts.TradGothicBoldCondensed (movie subtitles)
    sub_font: Option<GRef>,
    /// PC_U_PlayerHudScreen while playing
    hud: Option<(Active, crate::hud::Hud)>,
    /// loading screen: DefaultLoadingSequence SpinnerTextureName (Radial2, rotating) and
    /// SpinnerTextureName2 (Radial1), frames drawn so far
    spinner: (Option<GRef>, Option<GRef>),
    loading_frames: u32,
    /// GameObjectivesMP ProgressIconTemplate (UI_PlayerHUD.Materials.M_roundedMask)
    ring: Option<GRef>,
    /// the minimap capture (minimap.rs), bound for Textures.MinimapRenderTexture
    minimap_rt: Option<Handle<Image>>,
    /// materials of the player HUD, drawn as empty quads outside the match (pipeline warm-up)
    warm: Vec<GRef>,
    /// PC_U_InGameScoreboard while EKBC_OpenScoreboard (Tab) is held (scoreboard.rs)
    scoreboard: Option<Active>,
}

fn setup(world: &mut World) {
    let init = world.remove_non_send_resource::<MenuInit>().unwrap();
    let no_post = std::env::var_os("HOMEBOTS_NO_POST").is_some();
    let mut cam = world.spawn((
        Camera2d,
        Camera { order: 1, hdr: true, clear_color: ClearColorConfig::None, ..default() },
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
    ));
    // with the raw scene (no post) the target is linear already
    if !no_post {
        cam.insert(UiGammaOut);
    }
    if !init.enabled {
        return;
    }
    let cooked = init.game.join("GCGame").join("CookedPC");
    let lib = match upk::library::Library::scan(&init.game) {
        Ok(l) => l,
        Err(e) => {
            error!("menu: {e}");
            return;
        }
    };
    // all UI packages plus the script packages their classes and defaults live in
    let mut list = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(cooked.join("UI"))
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("upk"))).collect())
        .unwrap_or_default();
    files.sort();
    for p in files.iter().chain([cooked.join("Game").join("GCLoadoutItemInfos.upk"), cooked.join("Engine.u"), cooked.join("GCGame.u"), cooked.join("Core.u")].iter()) {
        match upk::Package::open(p) {
            Ok(pk) => list.push((p.file_stem().unwrap().to_string_lossy().to_string(), pk)),
            Err(e) => warn!("menu: {}: {e}", p.display()),
        }
    }
    let lang = language(&init.game);
    let Some(loc) = upk::ui::Loc::load(&cooked.join(format!("Coalesced_{lang}.bin"))).or_else(|| upk::ui::Loc::load(&cooked.join("Coalesced_int.bin"))) else {
        error!("menu: Coalesced_{lang}.bin not readable");
        return;
    };
    info!("menu: {} UI packages, language {lang}", list.len());
    let ui = Ui::new(UiWorld::new(list), loc);
    let Some(root) = ui.world.find("PC_UI_FrontEndMenus.UIScenes.U_MainMenuScreen") else {
        error!("menu: U_MainMenuScreen not found");
        return;
    };
    let mut menu = Menu {
        ui,
        lib,
        stack: Vec::new(),
        textures: HashMap::new(),
        mats: HashMap::new(),
        graphs: HashMap::new(),
        batches: Vec::new(),
        last: Vec::new(),
        colors: ([1.0; 4], [1.0; 4], [1.0; 4]),
        version: None,
        loaded: None,
        preload: Vec::new(),
        game: init.game.clone(),
        background: init.background.clone(),
        in_game: false,
        last_click: None,
        phase: Phase::Front,
        now: 0.0,
        creating_since: 0.0,
        movie_file: None,
        match_maps: Vec::new(),
        load_requested: None,
        match_loading: false,
        match_built: false,
        await_preload: false,
        sub_font: None,
        hud: None,
        spinner: (None, None),
        loading_frames: 0,
        ring: None,
        minimap_rt: world.get_resource::<crate::minimap::MinimapTarget>().map(|t| t.0.clone()),
        warm: Vec::new(),
        scoreboard: None,
    };
    menu.sub_font = menu.ui.world.find("PC_UI_Fonts.TradGothicBoldCondensed");
    menu.ring = menu.ui.world.find("UI_PlayerHUD.Materials.M_roundedMask");
    // the HUD's materials (and the progress ring) for the pipeline warm-up
    if let Some(s) = menu.find_scene("PC_U_PlayerHudScreen") {
        let mut a = menu.activate(s);
        a.scene.layout(1280.0, 720.0);
        let h = crate::hud::Hud::new(&menu.ui, &a.scene);
        let rt = h.runtime(&menu.ui, &crate::hud::HudValues::default());
        let mut q = Vec::new();
        menu.ui.draw_scene(&a.scene, 1280.0, 720.0, &rt, &mut q);
        let mut warm: Vec<GRef> = menu.ring.into_iter().collect();
        for t in q.iter().filter_map(|x| x.tex) {
            if menu.ui.world.class(t).contains("Material") && !warm.contains(&t) {
                warm.push(t);
            }
        }
        menu.warm = warm;
    }
    menu.spinner = (menu.ui.world.find("UI_FrontEndTextures.Textures.Radial2"), menu.ui.world.find("UI_FrontEndTextures.Textures.Radial1"));
    // GCUIInfo palette of the scene (UIInfoClass = GCUIInfos.GCUIInfo archetype)
    if let Some(info) = menu.ui.world.props(root).obj("UIInfoClass") {
        let p = menu.ui.world.props(info);
        menu.colors = (
            p.lcolor("m_UnSelectedItemColor").unwrap_or([1.0; 4]),
            p.lcolor("m_SelectedItemColor").unwrap_or([1.0; 4]),
            p.lcolor("m_LockedItemColor").unwrap_or([1.0; 4]),
        );
    }
    // GCPublicVersion, as the game prints it ("%s: %d.%d.%d", trace frame 400)
    menu.version = Some("VERSION: 1.5.500001".into());
    let mut a = menu.activate(root);
    a.level = Some(init.background.clone());
    menu.stack.push(a);
    menu.loaded = Some(init.background.clone());
    // the other front-end level is streamed in the background so switching is instant
    menu.preload = vec!["HFstreamOnlineMenu".into()];
    // HOMEBOTS_MENU_OPEN=U_OnlineMenuScreen,PC_U_CreateMPGameScreen: open scenes on start (tests)
    if let Ok(v) = std::env::var("HOMEBOTS_MENU_OPEN") {
        for n in v.split(',') {
            if let Some(s) = menu.find_scene(n.trim()) {
                let a = menu.activate(s);
                menu.stack.push(a);
            }
        }
        let want = menu.stack.iter().rev().find_map(|a| a.level.clone());
        if want.is_some() && want != menu.loaded {
            if let Some(p) = menu.lib.path_of(want.as_ref().unwrap()).cloned() {
                world.insert_resource(LevelRequest { maps: vec![p], persistent: menu.lib.path_of("HFGameEntry").cloned(), cache: true, show: true });
                menu.loaded = want;
            }
        }
    }
    world.insert_non_send_resource(menu);
}

/// Steam passes the language on the command line; its choice is in the app manifest
/// (UserConfig.language). Override: HOMEBOTS_LANG=deu etc. Fallback "int".
pub(crate) fn language(game: &std::path::Path) -> String {
    if let Ok(l) = std::env::var("HOMEBOTS_LANG") {
        return l;
    }
    let steam_lang = |name: &str| -> Option<&'static str> {
        Some(match name {
            "german" => "deu",
            "french" => "fra",
            "italian" => "ita",
            "spanish" => "spa",
            "russian" => "rus",
            "polish" => "pol",
            "czech" => "cze",
            "dutch" => "dut",
            "japanese" => "jpn",
            "english" => "int",
            _ => return None,
        })
    };
    if let Some(steamapps) = game.parent().and_then(|p| p.parent()) {
        if let Ok(rd) = std::fs::read_dir(steamapps) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if !n.starts_with("appmanifest_") {
                    continue;
                }
                let Ok(s) = std::fs::read_to_string(e.path()) else { continue };
                if !s.contains("\"HOMEFRONT\"") && !s.to_ascii_lowercase().contains("\"installdir\"\t\t\"homefront\"") {
                    continue;
                }
                for l in s.lines() {
                    let t: Vec<&str> = l.split('"').filter(|x| !x.trim().is_empty()).collect();
                    if t.len() >= 2 && t[0] == "language" {
                        if let Some(code) = steam_lang(t[1]) {
                            return code.to_string();
                        }
                    }
                }
            }
        }
    }
    String::from("int")
}

impl Menu {
    fn activate(&mut self, root: GRef) -> Active {
        let scene = self.ui.world.load_scene(root);
        let w = &self.ui.world;
        let mut menus = Vec::new();
        let mut buttons = Vec::new();
        let mut actions = HashMap::new();
        let index: HashMap<GRef, usize> = scene.widgets.iter().enumerate().map(|(i, w)| (w.obj, i)).collect();
        for (i, wd) in scene.widgets.iter().enumerate() {
            if wd.class.contains("MenuWidget") {
                let labels: Vec<usize> = w.objects(wd.props.get("m_MenuLabels")).iter().filter_map(|g| index.get(g).copied()).collect();
                for &l in &labels {
                    let act = scene.widgets[l].props.obj("m_MenuAction").and_then(|c| self.command(c));
                    actions.insert(l, act);
                }
                menus.push(MenuW { labels });
            }
            if wd.class.contains("CommandButton") {
                let act = w.objects(wd.props.get("GCCommand")).into_iter().find_map(|c| self.command(c));
                actions.insert(i, act);
                buttons.push(i);
            }
            // GCUIObject_BackButton (scene script: close scene), create screen's launch button
            if wd.class.contains("BackButton") {
                actions.insert(i, Some(Action::Back));
                buttons.push(i);
            }
            if wd.tag == "LaunchButton" {
                actions.insert(i, Some(Action::Launch));
                buttons.push(i);
            }
        }
        let level = w.props(root).enum_i("m_LevelName", 0).filter(|l| *l != "None").map(String::from);
        let create = (w.name(root) == "PC_U_CreateMPGameScreen").then(|| crate::creategame::CreateGame::new(&self.ui, &scene, &self.game));
        let name = w.name(root);
        let popup = w.class(root).contains("MessageBox");
        let mut a = Active { root, scene, size: (0.0, 0.0), menus, buttons, actions, sel: None, hover: None, level, create, popup, texts: HashMap::new(), shown: HashMap::new(), spawn: None, options: None };
        if crate::options::SCENES.contains(&name.as_str()) {
            a.options = Some(crate::options::Options::new(&self.ui, &a.scene));
            // "Übernehmen" (UILabelButton) and "Standard" (GCCommand_ResetInput, not decoded)
            for (i, wd) in a.scene.widgets.iter().enumerate() {
                let act = match wd.tag.as_str() {
                    "ApplyChangesButton" => Some(Action::ApplyOptions),
                    "ResetToDefaults" | "ResetToDefaultsButton" => Some(Action::ResetOptions),
                    _ => None,
                };
                if let Some(act) = act {
                    a.actions.insert(i, Some(act));
                    if !a.buttons.contains(&i) {
                        a.buttons.push(i);
                    }
                }
            }
        }
        if name == "PC_U_SpawnSelectionMenu" {
            a.spawn = Some(crate::spawnsel::SpawnSel::new(&self.ui, &a.scene, self.now));
            for (i, wd) in a.scene.widgets.iter().enumerate() {
                match wd.tag.as_str() {
                    "SpawnMebutton" => {
                        a.actions.insert(i, Some(Action::Spawn));
                        if !a.buttons.contains(&i) {
                            a.buttons.push(i);
                        }
                    }
                    // the quick-spawn panel (Done/Suicide) is the alive-player variant
                    "QuickSpawnButtonPanel" | "Done" | "Suicide" => {
                        a.shown.insert(i, false);
                        a.buttons.retain(|&b| b != i);
                    }
                    _ => {}
                }
            }
        }
        if name == "PC_U_PauseMenu_MP" {
            for (i, wd) in a.scene.widgets.iter().enumerate() {
                if wd.tag == "Quit" {
                    a.actions.insert(i, Some(Action::LeaveConfirm));
                }
            }
        }
        a
    }

    /// GCCommand object -> action this offline build performs (None: locked).
    fn command(&self, c: GRef) -> Option<Action> {
        let w = &self.ui.world;
        let p = w.props(c);
        match w.class(c).as_str() {
            "GCCommand_UIMenuConfirmButton" => {
                let s = p.obj("m_ActionCommand")?;
                // pause menu "KLASSE WECHSELN" (ChangeLoadout -> PC_U_SpawnSelectionMenu)
                if self.in_game && w.name(s) == "PC_U_SpawnSelectionMenu" {
                    return Some(Action::ChangeClass);
                }
                self.allowed(s).then_some(Action::Open(s, None))
            }
            "GCCommand_UIOpenScene" | "GCCommand_UIOpenOnlineScene" => {
                let name = p.enum_i("m_SceneName", 0)?;
                let s = self.find_scene(name)?;
                let level = p.enum_i("m_LevelName", 0).filter(|l| *l != "None").map(String::from);
                self.allowed(s).then_some(Action::Open(s, level))
            }
            "GCCommand_GoBackScene" | "GCCommand_UICloseScene" => Some(Action::Back),
            "GCCommand_ExitGame" if self.in_game => Some(Action::LeaveConfirm),
            "GCCommand_ExitToWindows" | "GCCommand_ExitGame" => Some(Action::Exit),
            "GCCommand_OpenSpawnSelection" if self.in_game => Some(Action::ChangeClass),
            "GCCommand_UISwitchTeam" if self.in_game => Some(Action::SwitchTeam),
            "GCCommand_AddToQueue" => Some(Action::AddToQueue),
            "GCCommand_RemoveFromQueue" => Some(Action::RemoveFromQueue),
            _ => None,
        }
    }

    /// Scenes of the offline multiplayer front end: options and creating a match. Single
    /// player, matchmaking, server browser, stats/leaderboards, demos and the armory are locked.
    fn allowed(&self, scene: GRef) -> bool {
        let n = self.ui.world.name(scene);
        matches!(
            n.as_str(),
            "U_MainMenuScreen" | "U_OnlineMenuScreen" | "PC_U_CreateMPGameScreen" | "U_Options" | "PC_U_KeyBindings" | "U_HUDPrefs" | "PC_U_GraphicSettings" | "U_AudioSettings" | "PC_U_Controls" | "PC_U_SpawnSelectionMenu"
        )
    }

    fn find_scene(&self, name: &str) -> Option<GRef> {
        for (pi, p) in self.ui.world.pkgs.iter().enumerate() {
            for (i, e) in p.exports.iter().enumerate() {
                if p.object_name_of(e).eq_ignore_ascii_case(name) && p.export_class(e).contains("Scene") {
                    return Some(GRef { pkg: pi, idx: i as i32 + 1 });
                }
            }
        }
        None
    }

    fn texture(&mut self, t: GRef, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(h) = self.textures.get(&t) {
            return h.clone();
        }
        if self.ui.world.class(t) == "TextureRenderTarget2D" && self.ui.world.name(t).eq_ignore_ascii_case("MinimapRenderTexture") {
            self.textures.insert(t, self.minimap_rt.clone());
            return self.minimap_rt.clone();
        }
        let h = if t.idx > 0 {
            let pkg = &self.ui.world.pkgs[t.pkg];
            let p = self.ui.world.props(t);
            let addr = |k: &str| match p.enum_i(k, 0) {
                Some("TA_Clamp") => upk::matcompile::Address::Clamp,
                Some("TA_Mirror") => upk::matcompile::Address::Mirror,
                _ => upk::matcompile::Address::Wrap,
            };
            crate::gamemat::load_texture(pkg, &self.lib, t.idx, [addr("AddressX"), addr("AddressY")], 4096).map(|i| images.add(i))
        } else {
            None
        };
        if h.is_none() && !self.ui.world.class(t).contains("Material") {
            warn!("menu: texture {} not drawable ({})", self.ui.world.path(t), self.ui.world.class(t));
        }
        self.textures.insert(t, h.clone());
        h
    }

    /// Material/MaterialInstanceConstant -> canvas material (None: not drawable here).
    fn graph(&mut self, t: GRef, images: &mut Assets<Image>, shaders: &mut Assets<Shader>, gmats: &mut Assets<UiMatGraph>, scene: &Handle<Image>) -> Option<Handle<UiMatGraph>> {
        if let Some(h) = self.graphs.get(&t) {
            return h.clone();
        }
        let h = (|| {
            if t.idx <= 0 {
                return None;
            }
            // the objective progress ring's Progress is set every frame (see ring_progress)
            let ext = upk::matcompile::MicChain { dynamic: if Some(t) == self.ring { vec!["Progress".into()] } else { Vec::new() }, ..Default::default() };
            let c = self.ui.world.pkgs[t.pkg].compile_material_ext(t.idx, &[], &ext)?;
            if c.textures.len() > 4 || c.textures.iter().any(|x| x.cube) {
                warn!("menu: material {} needs {} textures", self.ui.world.path(t), c.textures.len());
                return None;
            }
            if !c.stubbed.is_empty() {
                warn!("menu: material {} stubbed {:?}", self.ui.world.path(t), c.stubbed);
            }
            let mut tex: Vec<Option<Handle<Image>>> = Vec::new();
            for slot in &c.textures {
                let g = self.ui.world.gref(t.pkg, slot.object)?;
                tex.push(self.texture(g, images));
            }
            use upk::level::Blend;
            let alpha = match c.info.blend {
                Blend::Opaque | Blend::Masked => "1.0".to_string(),
                _ => "clamp(o.opacity, 0.0, 1.0)".to_string(),
            };
            let mut frag = UI_MAT_FRAGMENT.replace("ALPHA", &alpha);
            if matches!(c.info.blend, Blend::Masked) {
                frag = frag.replace("    let o = mat_eval(m);\n", &format!("    let o = mat_eval(m);\n    if (o.opacity_mask < {:?}) {{ discard; }}\n", c.info.mask_clip));
            }
            let src = format!("{UI_MAT_PRELUDE}\n{}\n{frag}", c.code);
            if let Ok(dir) = std::env::var("HB_UIMAT_DUMP") {
                let _ = std::fs::write(format!("{dir}/ui_mat_{}.wgsl", self.ui.world.name(t)), &src);
                info!("menu: material {} textures {:?}", self.ui.world.path(t), c.textures.iter().map(|x| self.ui.world.pkgs[t.pkg].object_path(x.object)).collect::<Vec<_>>());
            }
            let path = format!("ui_mat_{}_{}.wgsl", t.pkg, t.idx);
            let shader = shaders.add(Shader::from_wgsl(src, path));
            let get = |i: usize| tex.get(i).cloned().flatten();
            let blend = match c.info.blend {
                Blend::Additive => 1,
                Blend::Modulate => 2,
                _ => 0,
            };
            Some(gmats.add(UiMatGraph { scene: scene.clone(), t0: get(0), t1: get(1), t2: get(2), t3: get(3), params: Vec4::ZERO, shader, blend }))
        })();
        if h.is_none() {
            warn!("menu: material {} not drawable", self.ui.world.path(t));
        }
        self.graphs.insert(t, h.clone());
        h
    }

    fn runtime(&self, a: &Active) -> HashMap<usize, WidgetRt> {
        let mut rt: HashMap<usize, WidgetRt> = HashMap::new();
        let crumbs: Vec<String> = self.stack.iter().filter_map(|s| self.ui.world.props(s.root).str("CookieName").map(String::from)).filter(|c| c != "CookieName").collect();
        let version_obj = self.ui.world.props(a.root).obj("VersionLabel");
        for (i, w) in a.scene.widgets.iter().enumerate() {
            if w.class == "GCUIObject_CookieTrail" {
                rt.entry(i).or_default().crumbs = crumbs.clone();
            }
            if Some(w.obj) == version_obj {
                rt.entry(i).or_default().text = self.version.clone();
            }
        }
        let (unsel, sel, locked) = self.colors;
        for (mi, m) in a.menus.iter().enumerate() {
            for (li, &l) in m.labels.iter().enumerate() {
                let enabled = a.actions.get(&l).is_some_and(|x| x.is_some());
                let c = if !enabled { locked } else if a.sel == Some((mi, li)) { sel } else { unsel };
                rt.entry(l).or_default().text_color = Some(c);
            }
        }
        for &b in &a.buttons {
            if a.hover == Some(b) {
                rt.entry(b).or_default().state = Some(UiState::Focused);
            }
        }
        if let Some(cg) = &a.create {
            cg.runtime(&mut rt, [unsel, sel]);
        }
        if let Some(sp) = &a.spawn {
            sp.runtime(&self.ui, &mut rt, self.colors, self.now);
        }
        if let Some(op) = &a.options {
            op.runtime(&mut rt, crate::options::start_size());
        }
        for (&i, t) in &a.texts {
            rt.entry(i).or_default().text = Some(t.clone());
        }
        for (&i, &v) in &a.shown {
            rt.entry(i).or_default().hidden = Some(!v);
        }
        rt
    }
}

fn inside(r: [f32; 4], p: Vec2) -> bool {
    p.x >= r[0] && p.x < r[2] && p.y >= r[1] && p.y < r[3]
}

/// Game-mode movie (DefaultEngine.ini [GCGame.GCUIDataStore_?] GameModeTypes:
/// (GameModePrefix="FL", GameModeName="GroundControl", GameModeMovie="GroundControl")).
fn mode_movie(game: &std::path::Path, prefix: &str) -> Option<PathBuf> {
    let ini = std::fs::read_to_string(game.join("GCGame").join("Config").join("DefaultEngine.ini")).ok()?;
    for l in ini.lines() {
        let Some(v) = l.trim().strip_prefix("+GameModeTypes=") else { continue };
        let p = crate::spawnsel::ue_field(v, "GameModePrefix").map(|x| x.trim_matches('"'));
        if p == Some(prefix) {
            let m = crate::spawnsel::ue_field(v, "GameModeMovie")?.trim_matches('"').to_string();
            return Some(game.join("GCGame").join("Movies").join(format!("{m}.bik")));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    menu: Option<NonSendMut<Menu>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<UiMat>>,
    mut gmats: ResMut<Assets<UiMatGraph>>,
    mut shaders: ResMut<Assets<Shader>>,
    scene_copy: Res<crate::scenecopy::SceneCopy>,
    mut exit: EventWriter<AppExit>,
    mut vis: Query<&mut Visibility, With<UiBatch>>,
    pending: Option<Res<LevelRequest>>,
    (time, mut in_menu, mut sfx, mut preload, mut batch_cache): (Res<Time<Real>>, ResMut<InMenu>, ResMut<crate::sfx::SfxQueue>, ResMut<crate::warmup::Preload>, Local<Vec<(Vec<Quad>, bevy::asset::UntypedAssetId)>>),
    (mut movie, levels, mut flow, config, mut capture, score_rows): (ResMut<crate::movie::MovieState>, Res<crate::Levels>, ResMut<MatchFlow>, Option<Res<MatchConfig>>, ResMut<crate::input::Capture>, Res<crate::scoreboard::ScoreRows>),
) {
    let _perf = crate::perf::scope("menu_ui");
    let Some(mut menu) = menu else { return };
    let now = time.elapsed_secs_f64();
    menu.now = now;
    // background preload, one level per free loader slot
    if pending.is_none() && !menu.in_game {
        if let Some(l) = menu.preload.pop() {
            if Some(&l) != menu.loaded.as_ref() {
                if let Some(p) = menu.lib.path_of(&l).cloned() {
                    commands.insert_resource(LevelRequest { maps: vec![p], persistent: menu.lib.path_of("HFGameEntry").cloned(), cache: true, show: false });
                }
            }
        }
    }
    let Ok(win) = windows.single() else { return };
    let (vw, vh) = (win.physical_width() as f32, win.physical_height() as f32);
    if vw < 1.0 || vh < 1.0 {
        return;
    }
    let scale = win.scale_factor();
    let cursor = win.physical_cursor_position();
    let mut action: Option<Action> = None;

    // ---- match flow
    let mut esc_consumed = false;
    match menu.phase {
        Phase::Creating => {
            // the popup is on screen for 3.0 s in the user's video (10 fps scan: 8.4 s - 11.4 s).
            // The match level is built while it shows (the build blocks the frame: the popup
            // stays on screen), then the movie plays with the level ready and can be skipped
            // at once, then the spawn selection follows. The game loads behind its movie (user:
            // the original's loading screen takes 21 s); before, the build came after the movie
            // behind a black screen (25 s on the user's PC).
            if now - menu.creating_since >= CREATING_POPUP_SECS && crate::preopen_ready(&menu.match_maps) {
                match menu.load_requested {
                    None => {
                        commands.insert_resource(LevelRequest { maps: menu.match_maps.clone(), persistent: None, cache: false, show: true });
                        menu.load_requested = Some(levels.loads);
                    }
                    Some(n) if levels.loads > n => {
                        menu.match_built = true;
                        menu.stack.clear();
                        // everything on the GPU and every pipeline compiled before the spawn
                        // selection (warmup.rs; user 2026-10-21 "Ich will, dass alles vorlädt")
                        preload.begin(now);
                        match menu.movie_file.take() {
                            Some(f) => {
                                // black (Loading) until the movie's audio is decoded and it starts
                                commands.insert_resource(crate::movie::MovieRequest { file: f });
                                menu.phase = Phase::Loading;
                                menu.match_loading = false;
                                menu.creating_since = now;
                                menu.loading_frames = 0;
                            }
                            None => {
                                // no movie: the loading screen until the preload is done
                                menu.match_built = false;
                                menu.phase = Phase::Loading;
                                menu.await_preload = true;
                                menu.loading_frames = 0;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Phase::Movie => {
            // the level is built: LoadingTips PCSkip "ZUM ÜBERSPRINGEN EINGABETASTE DRÜCKEN"
            // shows and Enter (or the movie's end) goes to the spawn selection
            let ended = !movie.active || (movie.frame_count > 0 && movie.frame + 1 >= movie.frame_count);
            let ready = crate::preopen_ready(&menu.match_maps);
            let auto = std::env::var_os("HOMEBOTS_MENU_LAUNCH").is_some() && movie.frame >= MOVIE_LOAD_FRAME;
            if ready && (ended || keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) || auto) {
                movie.stop = true;
                if menu.match_built && preload.pending() {
                    // the level is built but not everything is on the GPU yet: the loading
                    // screen until it is (warmup.rs), then the spawn selection
                    menu.match_built = false;
                    menu.match_loading = false;
                    menu.phase = Phase::Loading;
                    menu.await_preload = true;
                    menu.loading_frames = 0;
                    preload.held_since = Some(crate::warmup::wall());
                } else if menu.match_built {
                    menu.match_built = false;
                    menu.match_loading = false;
                    menu.phase = Phase::Spawn;
                    if let Some(s) = menu.find_scene("PC_U_SpawnSelectionMenu") {
                        let a = menu.activate(s);
                        menu.stack.push(a);
                    }
                    in_menu.0 = true;
                } else {
                    menu.phase = Phase::Loading;
                    menu.match_loading = true;
                    menu.load_requested = None;
                    menu.creating_since = now;
                    menu.loading_frames = 0;
                }
            }
        }
        Phase::Spawn => {
            let dt = time.delta_secs_f64();
            for a in menu.stack.iter_mut() {
                if let Some(s) = a.spawn.as_mut() {
                    s.tick(dt);
                }
            }
            let auto = menu.stack.iter().find_map(|a| a.spawn.as_ref()).is_some_and(|s| s.remaining(now) <= 0.0);
            if flow.leave {
                // match over while dead (end-of-match nuke, objectives.rs Endgame): out from the
                // spawn selection too, the same way as from the game
                flow.leave = false;
                menu.stack.clear();
                action = Some(Action::LeaveMatch);
            } else if auto {
                action = Some(Action::Spawn);
            }
        }
        Phase::Playing => {
            flow.hud.time_left = (flow.hud.time_left - time.delta_secs()).max(0.0);
            // the pause menu's spawn selection (no auto spawn): its input delay counts
            let dt = time.delta_secs_f64();
            for a in menu.stack.iter_mut() {
                if let Some(s) = a.spawn.as_mut() {
                    s.tick(dt);
                }
            }
            if flow.leave {
                // match over: the same way out as "leave match" in the pause menu - the
                // ClosingSession popup, the black loading screen, then the front end. Going
                // straight to LeaveDone loaded the front end inside the match's last frame: the
                // picture froze with the match's sound loops running for the whole load and the
                // HUD scene kept (user test 2026-10-18 "after we won the screen froze")
                flow.leave = false;
                action = Some(Action::LeaveMatch);
            } else if flow.respawn {
                // died: spawn selection again (PlayerSpawnManagerMP auto spawn timer restarts)
                flow.respawn = false;
                menu.stack.clear();
                menu.phase = Phase::Spawn;
                if let Some(s) = menu.find_scene("PC_U_SpawnSelectionMenu") {
                    let mut a = menu.activate(s);
                    if let Some(sp) = a.spawn.as_mut() {
                        sp.resume(flow.hud.kp, flow.loadout, flow.vehicle_sel);
                    }
                    menu.stack.push(a);
                }
                in_menu.0 = true;
            } else if menu.stack.is_empty() && keys.just_pressed(KeyCode::Escape) {
                if let Some(s) = menu.find_scene("PC_U_PauseMenu_MP") {
                    let a = menu.activate(s);
                    menu.stack.push(a);
                    in_menu.0 = true;
                    // the same Esc press must not reach the new top scene (it would go Back
                    // and close the pause menu in the frame it opened)
                    esc_consumed = true;
                }
            } else if menu.stack.is_empty() {
                in_menu.0 = false;
            }
        }
        Phase::Leaving => {
            if now - menu.creating_since >= 2.5 {
                // the game's black loading screen (trace t3 frame 3130) while the front end loads
                menu.phase = Phase::Loading;
                menu.creating_since = now;
                menu.loading_frames = 0;
                menu.stack.retain(|a| !a.popup);
            }
        }
        Phase::Loading if menu.await_preload => {
            menu.loading_frames += 1;
            if !preload.pending() {
                if let Some(t) = preload.held_since.take() {
                    info!("preload: loading screen held {:.2} s (wall clock) after the movie", crate::warmup::wall() - t);
                }
                menu.await_preload = false;
                menu.phase = Phase::Spawn;
                if let Some(s) = menu.find_scene("PC_U_SpawnSelectionMenu") {
                    let a = menu.activate(s);
                    menu.stack.push(a);
                }
                in_menu.0 = true;
            }
        }
        Phase::Loading if menu.match_built => {
            // built during the popup: black until the movie runs
            if movie.started {
                menu.phase = Phase::Movie;
            }
        }
        Phase::Loading => {
            // the level load blocks the frame: request it once the loading screen is on screen
            menu.loading_frames += 1;
            if menu.loading_frames == LOADING_SCREEN_FRAMES - 1 {
                if let Ok(p) = std::env::var("HB_SHOT_LOADING") {
                    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
                    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(PathBuf::from(p)));
                }
            }
            if menu.match_loading {
                match menu.load_requested {
                    None if menu.loading_frames >= LOADING_SCREEN_FRAMES => {
                        commands.insert_resource(LevelRequest { maps: menu.match_maps.clone(), persistent: None, cache: false, show: true });
                        menu.load_requested = Some(levels.loads);
                    }
                    Some(n) if levels.loads > n => {
                        // built after the movie: the loading screen stays for the preload
                        menu.match_loading = false;
                        menu.await_preload = true;
                        preload.begin(now);
                        preload.held_since = Some(crate::warmup::wall());
                    }
                    _ => {}
                }
            } else if menu.loading_frames >= LOADING_SCREEN_FRAMES {
                action = Some(Action::LeaveDone);
            }
        }
        Phase::Front => {}
    }

    // ---- input on the top scene
    // front-end music (HFGameEntry AddlObjReferences SC_MainMenu / SC_MP_Lobby; GCUIScene_Menu
    // PlayMainMenuMusic / PlayOnlineMenuMusic): the online scenes take the lobby cue
    if menu.phase == Phase::Front && !movie.active {
        let online = menu.stack.iter().any(|a| {
            let w = &menu.ui.world;
            w.class(a.root).contains("Online") || w.name(a.root).contains("Online")
        });
        sfx.looping("music", if online { "HFGameEntry.A2_Music_C.SC_MP_Lobby" } else { "HFGameEntry.A2_Music_C.SC_MainMenu" }, None);
    }
    let input_ok = !esc_consumed && !matches!(menu.phase, Phase::Creating | Phase::Movie | Phase::Leaving | Phase::Loading);
    if input_ok && action.is_none() && !menu.stack.is_empty() {
        let last_click = menu.last_click;
        let phase = menu.phase;
        let a = menu.stack.last_mut().unwrap();
        if a.size != (vw, vh) {
            a.scene.layout(vw, vh);
            a.size = (vw, vh);
        }
        // mouse hover selects menu entries / buttons
        if let Some(c) = cursor {
            let mut hit = None;
            for (mi, m) in a.menus.iter().enumerate() {
                for (li, &l) in m.labels.iter().enumerate() {
                    if inside(a.scene.widgets[l].rect, c) && a.actions.get(&l).is_some_and(|x| x.is_some()) {
                        hit = Some((mi, li));
                    }
                }
            }
            if hit.is_some() {
                a.sel = hit;
            }
            a.hover = a.buttons.iter().copied().find(|&b| inside(a.scene.widgets[b].rect, c) && a.shown.get(&b) != Some(&false));
            let pressed = mouse.just_pressed(MouseButton::Left) && a.spawn.as_ref().is_none_or(|s| s.accepts_input());
            // Windows default double-click time 500 ms
            let double = pressed && last_click.is_some_and(|(t, p)| now - t < 0.5 && p.distance(c) < 4.0);
            let mut consumed = match a.create.as_mut() {
                Some(cg) => cg.mouse(&a.scene, c, pressed, mouse.pressed(MouseButton::Left), double, vw, vh),
                None => false,
            };
            if let Some(sp) = a.spawn.as_mut() {
                consumed |= sp.mouse(&a.scene, c, pressed);
            }
            if let Some(op) = a.options.as_mut() {
                consumed |= op.mouse(&a.scene, c, pressed, mouse.pressed(MouseButton::Left), mouse.just_pressed(MouseButton::Right), vw, vh, &mut capture);
            }
            if pressed && !consumed {
                if let Some(b) = a.hover {
                    action = a.actions.get(&b).cloned().flatten();
                } else if let Some((mi, li)) = hit {
                    action = a.actions.get(&a.menus[mi].labels[li]).cloned().flatten();
                }
            }
        }
        // keyboard: up/down over enabled entries (m_bWrapIndex), enter, escape
        if let Some(m0) = a.menus.first() {
            let enabled: Vec<usize> = (0..m0.labels.len()).filter(|&li| a.actions.get(&m0.labels[li]).is_some_and(|x| x.is_some())).collect();
            if !enabled.is_empty() {
                let cur = a.sel.and_then(|(_, li)| enabled.iter().position(|&e| e == li));
                let n = enabled.len();
                if keys.just_pressed(KeyCode::ArrowDown) {
                    a.sel = Some((0, enabled[cur.map(|c| (c + 1) % n).unwrap_or(0)]));
                }
                if keys.just_pressed(KeyCode::ArrowUp) {
                    a.sel = Some((0, enabled[cur.map(|c| (c + n - 1) % n).unwrap_or(n - 1)]));
                }
                if keys.just_pressed(KeyCode::Enter) {
                    if let Some((mi, li)) = a.sel {
                        action = a.actions.get(&a.menus[mi].labels[li]).cloned().flatten();
                    }
                }
            }
        }
        // a key field waiting for its key takes Escape (cancel) and every key
        let capturing = a.options.as_ref().is_some_and(|o| o.capturing());
        if let Some(op) = a.options.as_mut() {
            op.poll(&mut capture);
        }
        if keys.just_pressed(KeyCode::Escape) && action.is_none() && !capturing {
            // spawn selection: Esc opens the pause menu over it
            action = if phase == Phase::Spawn && a.spawn.is_some() {
                menu.find_scene("PC_U_PauseMenu_MP").map(|s| Action::Open(s, None))
            } else {
                Some(Action::Back)
            };
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        menu.last_click = cursor.map(|c| (now, c));
    }
    // HOMEBOTS_MENU_LAUNCH=1: press "Starten" once (tests); HOMEBOTS_AUTOSPAWN=1: spawn at once
    if action.is_none() && std::env::var_os("HOMEBOTS_MENU_LAUNCH").is_some() && menu.phase == Phase::Front && menu.stack.last().is_some_and(|a| a.create.is_some()) && menu.loaded.is_some() {
        action = Some(Action::Launch);
    }
    if action.is_none() && std::env::var_os("HOMEBOTS_AUTOSPAWN").is_some() && menu.phase == Phase::Spawn {
        action = Some(Action::Spawn);
    }
    // HB_TEST_CHANGECLASS=<vehicle entry>: 8 s after the first spawn the pause menu's "KLASSE
    // WECHSELN", that entry, "Selbstmord" (tests)
    if let Some(vi) = std::env::var("HB_TEST_CHANGECLASS").ok().and_then(|v| v.parse::<usize>().ok()) {
        static STEP: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        use std::sync::atomic::Ordering::Relaxed;
        if action.is_none() && menu.phase == Phase::Playing {
            match STEP.load(Relaxed) {
                0 if menu.stack.is_empty() && now - menu.creating_since > 8.0 => {
                    action = Some(Action::ChangeClass);
                    STEP.store(1, Relaxed);
                }
                1 => {
                    if let Some(sp) = menu.stack.last_mut().and_then(|a| a.spawn.as_mut()).filter(|sp| sp.accepts_input()) {
                        info!("test: change class, vehicle entries {:?}", sp.vehicles.iter().map(|v| (v.label.clone(), v.enabled)).collect::<Vec<_>>());
                        if sp.vehicles.get(vi).is_some_and(|v| v.enabled) {
                            sp.sel_vehicle = vi;
                        }
                        action = Some(Action::ClassSuicide);
                        STEP.store(2, Relaxed);
                    }
                }
                _ => {}
            }
        }
    }
    // HB_TEST_SWITCHTEAM=<s>: that many seconds after a spawn the pause menu's "AUSRÜSTUNG
    // WECHSELN", then "TEAM WECHSELN" (alive: dies, respawns on the other team; tests)
    if let Some(t) = std::env::var("HB_TEST_SWITCHTEAM").ok().and_then(|v| v.parse::<f64>().ok()) {
        if action.is_none() && menu.phase == Phase::Playing && now - menu.creating_since > t {
            if menu.stack.is_empty() {
                action = Some(Action::ChangeClass);
            } else if menu.stack.last().and_then(|a| a.spawn.as_ref()).is_some_and(|s| s.alive && s.accepts_input()) {
                action = Some(Action::SwitchTeam);
            }
        }
    }
    // HOMEBOTS_TEST_LEAVE=<s>: leave the match that many seconds after spawning (tests)
    if action.is_none() && menu.phase == Phase::Playing {
        if let Some(t) = std::env::var("HOMEBOTS_TEST_LEAVE").ok().and_then(|v| v.parse::<f64>().ok()) {
            if now - menu.creating_since >= t {
                action = Some(Action::LeaveMatch);
            }
        }
    }
    let mut changed = false;
    match action {
        Some(Action::Open(s, level)) => {
            info!("menu: open {} (level {:?})", menu.ui.world.path(s), level);
            let mut a = menu.activate(s);
            if level.is_some() {
                a.level = level;
            }
            menu.stack.push(a);
            changed = true;
        }
        Some(Action::Back) if menu.stack.len() > 1 || (menu.phase == Phase::Playing && !menu.stack.is_empty()) => {
            if let Some(a) = menu.stack.pop() {
                if a.options.is_some() {
                    crate::settings::save();
                }
            }
            changed = true;
        }
        Some(Action::ApplyOptions) => {
            crate::settings::apply_window();
            crate::settings::save();
        }
        Some(Action::ResetOptions) => {
            if let Some(op) = menu.stack.last_mut().and_then(|a| a.options.as_mut()) {
                op.reset();
            }
        }
        Some(Action::Exit) => {
            exit.write(AppExit::Success);
        }
        Some(Action::AddToQueue) => {
            if let Some(cg) = menu.stack.last_mut().and_then(|a| a.create.as_mut()) {
                cg.add_selected();
            }
        }
        Some(Action::RemoveFromQueue) => {
            if let Some(cg) = menu.stack.last_mut().and_then(|a| a.create.as_mut()) {
                cg.remove_selected();
            }
        }
        Some(Action::Launch) => {
            let settings = menu.stack.last().and_then(|a| a.create.as_ref().and_then(|cg| cg.settings(&a.scene)));
            match settings {
                Some(st) => {
                    info!("menu: start {:?}", st);
                    // the mode's sublevels (mapmode.rs): Bodenkontrolle everything but TDM_<map>_c,
                    // TDM everything but <map>_FrontlinesData
                    menu.match_maps = crate::mapmode::match_maps(&st.folder, &st.mode);
                    // read the level packages while the popup and the movie run
                    crate::preopen(menu.match_maps.clone());
                    // "Spiel wird erstellt …" (GCUIScene_OnlineCreate CreatingMatchString, trace t3
                    // frames 1026-1211: U_MessagePopup over the create screen, label iMessageLabel)
                    if let Some(s) = menu.find_scene("U_MessagePopup") {
                        let mut a = menu.activate(s);
                        let text = menu.ui.loc.get("ui_frontend.GCUIScene_OnlineCreate.CreatingMatchString").unwrap_or("").to_string();
                        if let Some(i) = a.scene.widgets.iter().position(|w| w.tag == "iMessageLabel") {
                            a.texts.insert(i, text);
                        }
                        a.popup = true;
                        menu.stack.push(a);
                    }
                    menu.movie_file = mode_movie(&menu.game, &st.mode).filter(|f| f.exists());
                    if menu.movie_file.is_none() {
                        warn!("menu: no game-mode movie for {}", st.mode);
                    }
                    menu.creating_since = now;
                    commands.insert_resource(MatchConfig(st));
                    menu.phase = Phase::Creating;
                    menu.load_requested = None;
                    menu.match_built = false;
                    menu.await_preload = false;
                    menu.in_game = true;
                    menu.loaded = None;
                    menu.preload.clear();
                }
                None => warn!("menu: nothing playable in the map queue"),
            }
        }
        Some(Action::Spawn) => {
            if menu.hud.is_none() {
                if let Some(s) = menu.find_scene("PC_U_PlayerHudScreen") {
                    let a = menu.activate(s);
                    let h = crate::hud::Hud::new(&menu.ui, &a.scene);
                    menu.hud = Some((a, h));
                }
                // match start: round time (RoundTimeSlider, minutes), GCBattlePoints InitialBattlePoints
                let minutes = config.as_ref().and_then(|c| c.0.values.get("RoundTimeSlider").copied()).unwrap_or(20.0);
                flow.hud.time_left = minutes * 60.0;
                flow.hud.score_limit = config.as_ref().and_then(|c| c.0.values.get("ScoreLimitSlider").copied()).unwrap_or(1000.0) as i32;
                flow.hud.kp = menu.ui.loc.ini.get("gcbattlepoints.default.initialbattlepoints").and_then(|v| v.first()).and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(k) = std::env::var("HB_KP").ok().and_then(|v| v.parse().ok()) {
                    flow.hud.kp = k; // test hook
                }
                flow.hud.kp_shown = flow.hud.kp as f32;
                flow.hud.ammo = 30;
                flow.hud.grenades = [2, 2];
            }
            if let Some(sp) = menu.stack.iter().find_map(|a| a.spawn.as_ref()) {
                flow.loadout = std::env::var("HB_LOADOUT").ok().and_then(|v| v.parse().ok()).unwrap_or(sp.sel_loadout);
                flow.vehicle = sp.vehicles.get(sp.sel_vehicle).and_then(|v| v.class.clone());
                flow.vehicle_sel = sp.sel_vehicle;
            }
            flow.spawn_requested = true;
            flow.spawn_serial += 1;
            flow.respawn = false;
            menu.stack.clear();
            menu.phase = Phase::Playing;
            menu.creating_since = now;
            in_menu.0 = false;
        }
        Some(Action::ChangeClass) => {
            if let Some(s) = menu.find_scene("PC_U_SpawnSelectionMenu") {
                let mut a = menu.activate(s);
                if let Some(sp) = a.spawn.as_mut() {
                    sp.alive = true;
                    sp.resume(flow.hud.kp, flow.loadout, flow.vehicle_sel);
                }
                // the alive variant: the quick-spawn panel (Fertig / Selbstmord) instead of
                // "Entstehen"
                for (i, wd) in a.scene.widgets.iter().enumerate() {
                    let act = match wd.tag.as_str() {
                        "SpawnMebutton" | "SpawnSelectionButtonPanel" => {
                            a.shown.insert(i, false);
                            a.buttons.retain(|&b| b != i);
                            continue;
                        }
                        "QuickSpawnButtonPanel" => {
                            a.shown.insert(i, true);
                            continue;
                        }
                        "Done" => Action::ClassDone,
                        "Suicide" => Action::ClassSuicide,
                        _ => continue,
                    };
                    a.shown.insert(i, true);
                    a.actions.insert(i, Some(act));
                    if !a.buttons.contains(&i) {
                        a.buttons.push(i);
                    }
                }
                info!("menu: change class, buttons {:?}", a.buttons.iter().map(|&b| (a.scene.widgets[b].tag.clone(), a.scene.widgets[b].rect)).collect::<Vec<_>>());
                menu.stack.push(a);
                changed = true;
            }
        }
        Some(Action::ClassDone) | Some(Action::ClassSuicide) => {
            if let Some(sp) = menu.stack.iter().rev().find_map(|a| a.spawn.as_ref()) {
                flow.loadout = sp.sel_loadout;
                flow.vehicle_sel = sp.sel_vehicle;
                info!("menu: class {} vehicle entry {} for the next spawn", sp.sel_loadout, sp.sel_vehicle);
            }
            if matches!(action, Some(Action::ClassSuicide)) {
                flow.suicide = true;
            }
            menu.stack.clear();
            in_menu.0 = false;
            changed = true;
        }
        Some(Action::SwitchTeam) => {
            switch_team(&mut menu, &mut flow, config.as_deref(), &mut in_menu);
            changed = true;
        }
        Some(Action::LeaveConfirm) => {
            // UI_MenuText ExitGamePopUp ConfirmExit with Ja/Nein (user's video)
            if let Some(s) = menu.find_scene("U_MessagePopup") {
                let mut a = menu.activate(s);
                let text = menu.ui.loc.get("UI_MenuText.ExitGamePopUp.ConfirmExit").unwrap_or("").to_string();
                let yes = menu.ui.loc.get("Engine.Menu.YesString").unwrap_or("").to_string();
                let no = menu.ui.loc.get("Engine.Menu.NoString").unwrap_or("").to_string();
                for (i, w) in a.scene.widgets.iter().enumerate() {
                    match w.tag.as_str() {
                        "iMessageLabel" => {
                            a.texts.insert(i, text.clone());
                        }
                        "CommandButton_Yes" => {
                            a.texts.insert(i, yes.clone());
                            a.shown.insert(i, true);
                            a.actions.insert(i, Some(Action::LeaveMatch));
                            a.buttons.push(i);
                        }
                        "CommandButton_No" => {
                            a.texts.insert(i, no.clone());
                            a.shown.insert(i, true);
                            a.actions.insert(i, Some(Action::Back));
                            a.buttons.push(i);
                        }
                        _ => {}
                    }
                }
                a.popup = true;
                menu.stack.push(a);
            }
        }
        Some(Action::LeaveMatch) => {
            // UI_PopupMessages GCUIEvent_MessagePopup ClosingSession, on screen 2.5 s in the
            // user's video (97.7 s - 100.2 s), then the front end loads
            menu.hud = None;
            menu.stack.retain(|a| !a.popup);
            if let Some(s) = menu.find_scene("U_MessagePopup") {
                let mut a = menu.activate(s);
                let text = menu.ui.loc.get("UI_PopupMessages.GCUIEvent_MessagePopup.ClosingSession").unwrap_or("").to_string();
                if let Some(i) = a.scene.widgets.iter().position(|w| w.tag == "iMessageLabel") {
                    a.texts.insert(i, text);
                }
                a.popup = true;
                menu.stack.push(a);
            }
            menu.phase = Phase::Leaving;
            menu.creating_since = now;
        }
        Some(Action::LeaveDone) => {
            menu.phase = Phase::Front;
            menu.in_game = false;
            in_menu.0 = true;
            // back on the multiplayer screen (user's video 101.5 s: HAUPTMENÜ > MEHRSPIELER over the
            // online menu level); the main menu level is streamed in the background
            menu.stack.clear();
            let bg = menu.background.clone();
            let online = "HFstreamOnlineMenu".to_string();
            if let Some(root) = menu.ui.world.find("PC_UI_FrontEndMenus.UIScenes.U_MainMenuScreen") {
                let mut a = menu.activate(root);
                a.level = Some(bg.clone());
                menu.stack.push(a);
            }
            if let Some(s) = menu.find_scene("U_OnlineMenuScreen") {
                let mut a = menu.activate(s);
                a.level = Some(online.clone());
                menu.stack.push(a);
            }
            if let Some(p) = menu.lib.path_of(&online).cloned() {
                commands.insert_resource(LevelRequest { maps: vec![p], persistent: menu.lib.path_of("HFGameEntry").cloned(), cache: true, show: true });
            }
            menu.loaded = Some(online);
            menu.preload = vec![bg];
            *flow = MatchFlow::default();
            commands.remove_resource::<MatchConfig>();
        }
        _ => {}
    }
    // pause menu server info (UIScenes.PC_U_PauseMenu_MP.IntermissionListBackground): server name
    // GameReplicationInfo.ServerName (ini), game type and map (ui_frontend.mapselect.<map>_name)
    // of the match, friendly fire ServerInfo.FriendlyFireOn/Off by the create-game check box
    if let (Some(cfg), Some(top)) = (config.as_ref(), menu.stack.last()) {
        if menu.ui.world.name(top.root) == "PC_U_PauseMenu_MP" && top.texts.is_empty() {
            let w = &menu.ui.world;
            let loc = &menu.ui.loc;
            let map = cfg.0.folder.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let ff = cfg.0.flags.get("FriendlyFireCheckBox").copied().unwrap_or(false);
            let mut texts = Vec::new();
            for (i, wd) in top.scene.widgets.iter().enumerate() {
                let t = match w.name(wd.obj).as_str() {
                    "ServerNameInfoLabel" => loc.ini.get("gcgame.engine.gamereplicationinfo.servername").and_then(|v| v.first()).cloned(),
                    "GameTypeInfoLabel" => Some(cfg.0.game_type.clone()),
                    "FriendltFireInfoLabel" => loc.get(if ff { "UI_MenuText.ServerInfo.FriendlyFireOn" } else { "UI_MenuText.ServerInfo.FriendlyFireOff" }).map(String::from),
                    "LevelName_Field" => loc.get(&format!("UI_FrontEnd.MapSelect.{map}_Name")).map(String::from),
                    _ => None,
                };
                if let Some(t) = t {
                    texts.push((i, t));
                }
            }
            if let Some(top) = menu.stack.last_mut() {
                top.texts.extend(texts);
            }
        }
    }
    if flow.phase != menu.phase {
        info!("match flow: {:?} -> {:?} (stack {:?})", flow.phase, menu.phase, menu.stack.iter().map(|a| menu.ui.world.name(a.root)).collect::<Vec<_>>());
    }
    flow.phase = menu.phase;
    flow.paused = menu.phase == Phase::Playing && (!menu.stack.is_empty() || flow.debug);
    if changed && !menu.in_game {
        let want = menu.stack.iter().rev().find_map(|a| a.level.clone());
        if want.is_some() && want != menu.loaded {
            let l = want.clone().unwrap();
            match menu.lib.path_of(&l).cloned() {
                Some(p) => {
                    commands.insert_resource(LevelRequest { maps: vec![p], persistent: menu.lib.path_of("HFGameEntry").cloned(), cache: true, show: true });
                    menu.loaded = want;
                }
                None => warn!("menu: level {l} not found"),
            }
        }
    }
    // objective progress ring: M_roundedMask Progress (0..1)
    if let Some(Some(h)) = menu.ring.and_then(|r| menu.graphs.get(&r)) {
        let p = flow.hud.active_obj.map(|a| a.2).unwrap_or(0.0);
        if gmats.get(h).is_some_and(|m| m.params.x != p) {
            if let Some(m) = gmats.get_mut(h) {
                m.params.x = p;
            }
        }
    }
    // ---- draw list: scenes from the last non-popup one up, then overlays (movie subtitles)
    let mut quads = Vec::new();
    {
        let base = menu.stack.iter().rposition(|a| !a.popup).unwrap_or(0);
        for i in base..menu.stack.len() {
            if menu.stack[i].size != (vw, vh) {
                menu.stack[i].scene.layout(vw, vh);
                menu.stack[i].size = (vw, vh);
            }
        }
        if menu.phase == Phase::Playing {
            if let Some((a, _)) = menu.hud.as_mut() {
                if a.size != (vw, vh) {
                    a.scene.layout(vw, vh);
                    a.size = (vw, vh);
                }
            }
        }
        if menu.phase == Phase::Playing && menu.scoreboard.is_none() {
            if let Some(s) = menu.find_scene("PC_U_InGameScoreboard") {
                let a = menu.activate(s);
                menu.scoreboard = Some(a);
            }
        }
        if let Some(a) = menu.scoreboard.as_mut() {
            if a.size != (vw, vh) {
                a.scene.layout(vw, vh);
                a.size = (vw, vh);
            }
        }
        let menu_ref = &*menu;
        if menu_ref.phase == Phase::Playing {
            if let Some((a, h)) = menu_ref.hud.as_ref() {
                let rt = h.runtime(&menu_ref.ui, &flow.hud);
                menu_ref.ui.draw_scene(&a.scene, vw, vh, &rt, &mut quads);
            }
        }
        // scoreboard over the HUD while Tab (EKBC_OpenScoreboard) is held and no menu is open
        if menu_ref.phase == Phase::Playing && menu_ref.stack.is_empty() && !flow.debug && crate::input::down(crate::input::Set::Global, "EKBC_OpenScoreboard") {
            if let Some(a) = menu_ref.scoreboard.as_ref() {
                let rt = crate::scoreboard::runtime(&menu_ref.ui, &a.scene, &score_rows, vw, vh);
                menu_ref.ui.draw_scene(&a.scene, vw, vh, &rt, &mut quads);
            }
        }
        if menu_ref.phase == Phase::Movie && crate::preopen_ready(&menu_ref.match_maps) {
            if let Some(f) = menu_ref.sub_font {
                let text = menu_ref.ui.loc.get("LoadingTips.Loading.PCSkip").unwrap_or("").to_string();
                centered_text_quads(&menu_ref.ui, f, &text, vw * 0.5, 610.0, vh, &mut quads);
            }
        }
        if menu_ref.phase == Phase::Loading {
            if let Some(f) = menu_ref.sub_font {
                let text = menu_ref.ui.loc.get("LoadingTips.Loading.LoadingText").unwrap_or("").to_string();
                loading_quads(&menu_ref.ui, f, menu_ref.spinner, &text, vw, vh, &mut quads);
            }
        } else {
            for a in menu_ref.stack.iter().skip(base) {
                let rt = menu_ref.runtime(a);
                menu_ref.ui.draw_scene(&a.scene, vw, vh, &rt, &mut quads);
                // an open combo list over the scene
                if let Some(op) = &a.options {
                    let white = menu_ref.ui.world.find("UI_Common.Textures.T_DefaultWhite");
                    op.overlay(&menu_ref.ui, &a.scene, menu_ref.sub_font, white, [menu_ref.colors.0, menu_ref.colors.1], vw, vh, &mut quads);
                }
            }
        }
        // pipeline warm-up: the HUD's materials (with the progress ring) are drawn as empty quads
        // outside the match, so their pipelines exist when the HUD appears (a pipeline first
        // needed in the match was not compiled for minutes on the software renderer)
        if menu_ref.phase != Phase::Playing {
            for &r in &menu_ref.warm {
                quads.push(Quad { rot: 0.0, rect: [1.0, 1.0, 1.0, 1.0], uv: [0.0, 0.0, 1.0, 1.0], tex: Some(r), color: [1.0; 4] });
            }
        }
        // subtitles: PlayerDetails:Subtitles (GCEngine.ini bSubtitlesEnabled)
        if menu_ref.phase == Phase::Movie && movie.started && crate::settings::flag("PlayerDetails:Subtitles") {
            if let Some(f) = menu_ref.sub_font {
                for (t0, t1, key) in &movie.cues {
                    if movie.time >= *t0 && movie.time < *t1 {
                        if let Some(text) = menu_ref.ui.loc.get(&format!("Subtitles.Subtitles.{key}")) {
                            crate::movie::subtitle_quads(&menu_ref.ui, f, text, vw, vh, &mut quads);
                        }
                    }
                }
            }
        }
    }
    if same(&quads, &menu.last) && menu.phase != Phase::Loading {
        return;
    }
    // Radial2 turns clockwise; about one turn per second in the user's video (100.3 s - 101.3 s,
    // red arc centroid 356 deg) - measured, not a game value
    let spin_angle = ((now - menu.creating_since) * std::f64::consts::TAU) as f32;
    let spin_tex = if menu.phase == Phase::Loading { menu.spinner.0 } else { None };
    if std::env::var_os("HB_UIDBG").is_some() {
        info!("ui: {} quads, phase {:?}", quads.len(), menu.phase);
    }
    // batches of consecutive quads with the same texture/material, in draw order
    let mut batches: Vec<(Batch, Vec<&Quad>)> = Vec::new();
    let mut cur: Option<GRef> = None;
    for q in &quads {
        let Some(t) = q.tex else { continue };
        let b = if menu.ui.world.class(t).contains("Material") {
            let Some(g) = menu.graph(t, &mut images, &mut shaders, &mut gmats, &scene_copy.final_image) else { continue };
            Batch::Graph(g)
        } else {
            let Some(img) = menu.texture(t, &mut images) else { continue };
            Batch::Tex(menu.mats.entry(t).or_insert_with(|| mats.add(UiMat { tex: img })).clone())
        };
        if cur != Some(t) || batches.is_empty() {
            batches.push((b, Vec::new()));
            cur = Some(t);
        }
        batches.last_mut().unwrap().1.push(q);
    }
    let (hw, hh) = (vw * 0.5, vh * 0.5);
    batch_cache.truncate(batches.len());
    for (bi, (mat, qs)) in batches.iter().enumerate() {
        // a batch with the same quads and material as last frame keeps its mesh (most HUD
        // batches between two changes of the HUD: each one was a new mesh + upload + a mesh2d
        // re-specialisation per frame). Same picture. NOT while the loading spinner turns.
        let mat_id = match mat {
            Batch::Tex(m) => m.id().untyped(),
            Batch::Graph(m) => m.id().untyped(),
        };
        let graph = matches!(mat, Batch::Graph(_));
        if spin_tex.is_none() {
            let kept = batch_cache.get(bi).is_some_and(|(pq, pm)| *pm == mat_id && pq.len() == qs.len() && pq.iter().zip(qs.iter()).all(|(a, b)| a.rot == b.rot && a.rect == b.rect && a.uv == b.uv && a.color == b.color && a.tex == b.tex));
            if let Some((e, _, was_graph)) = menu.batches.get(bi).cloned().filter(|b| kept && b.2 == graph) {
                if let Ok(mut v) = vis.get_mut(e) {
                    if *v != Visibility::Visible {
                        *v = Visibility::Visible;
                    }
                }
                let _ = was_graph;
                continue;
            }
        }
        let entry = (qs.iter().map(|q| **q).collect::<Vec<Quad>>(), mat_id);
        if bi < batch_cache.len() {
            batch_cache[bi] = entry;
        } else {
            batch_cache.push(entry);
        }
        let mut pos = Vec::with_capacity(qs.len() * 4);
        let mut uv = Vec::with_capacity(qs.len() * 4);
        let mut col = Vec::with_capacity(qs.len() * 4);
        let mut idx = Vec::with_capacity(qs.len() * 6);
        for q in qs {
            let b = pos.len() as u32;
            // UE pixels (y down, physical) -> Camera2d world (y up, logical, origin at centre)
            let x0 = (q.rect[0] - hw) / scale;
            let x1 = (q.rect[2] - hw) / scale;
            let y0 = (hh - q.rect[1]) / scale;
            let y1 = (hh - q.rect[3]) / scale;
            let spin = spin_tex.is_some() && q.tex == spin_tex;
            if spin || q.rot != 0.0 {
                // rotated about the centre (y up here: clockwise = negative angle)
                let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
                let (s, c) = (-(if spin { spin_angle } else { q.rot })).sin_cos();
                let r = |x: f32, y: f32| [cx + (x - cx) * c - (y - cy) * s, cy + (x - cx) * s + (y - cy) * c, 0.0];
                pos.extend([r(x0, y0), r(x1, y0), r(x1, y1), r(x0, y1)]);
            } else {
                pos.extend([[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]]);
            }
            uv.extend([[q.uv[0], q.uv[1]], [q.uv[2], q.uv[1]], [q.uv[2], q.uv[3]], [q.uv[0], q.uv[3]]]);
            col.extend([q.color; 4]);
            idx.extend([b, b + 2, b + 1, b, b + 3, b + 2]);
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        mesh.insert_indices(Indices::U32(idx));
        let h = meshes.add(mesh);
        let z = bi as f32 * 0.001;
        // an entity keeps one material type: swapping MeshMaterial2d<UiMat> for <UiMatGraph> on
        // the same entity left the new material unspecialised (never drawn) in Bevy 0.16
        let e = match menu.batches.get(bi).cloned() {
            Some((e, old, was_graph)) if was_graph == graph => {
                meshes.remove(&old);
                menu.batches[bi].1 = h.clone();
                e
            }
            Some((e, old, _)) => {
                meshes.remove(&old);
                commands.entity(e).despawn();
                let n = commands.spawn(UiBatch(bi)).id();
                menu.batches[bi] = (n, h.clone(), graph);
                n
            }
            None => {
                let e = commands.spawn(UiBatch(bi)).id();
                menu.batches.push((e, h.clone(), graph));
                e
            }
        };
        let mut ec = commands.entity(e);
        ec.insert((Mesh2d(h), Transform::from_xyz(0.0, 0.0, z), Visibility::Visible));
        match mat {
            Batch::Tex(m) => {
                ec.remove::<MeshMaterial2d<UiMatGraph>>().insert(MeshMaterial2d(m.clone()));
            }
            Batch::Graph(m) => {
                ec.remove::<MeshMaterial2d<UiMat>>().insert(MeshMaterial2d(m.clone()));
            }
        }
    }
    for (e, _, _) in menu.batches.iter().skip(batches.len()) {
        if let Ok(mut v) = vis.get_mut(*e) {
            *v = Visibility::Hidden;
        }
    }
    menu.last = quads;
}

/// Spawn selection "TEAM WECHSELN" (GCCommand_UISwitchTeam -> GCPlayerController.SwitchTeam ->
/// FL2GameMP.ChangeTeam). The game refuses a switch with UI_MenuText Popups CannotSwap
/// ("ZUGEHÖRIGKEIT KANN IM MOMENT NICHT GEWECHSELT WERDEN") when the teams would be uneven; here
/// the bots even the teams out (bots.rs manage_bots moves one bot), so it is refused only while
/// no bot can move (create-game "Auto. Team-Ausgleich" on and no bots) or outside a running round.
/// Dead: the new team's spawn selection at once (the loadout stays, the vehicle entry goes back
/// to infantry - the other team's vehicles). Alive (pause menu "AUSRÜSTUNG WECHSELN"): the player
/// dies (like "Selbstmord") and spawns on the new team. NOT from data: the native ChangeTeam rule
/// (team size check with GCOnlineServerOptions bAutoBalanceTeams, PendingTeamSwap pairing) is not
/// decoded; a switch while alive counts as a death here.
/// A team switch waits for its banner ("TEAM GEWECHSELT / DU BIST JETZT KVA", objectives.rs).
pub static TEAM_BANNER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn switch_team(menu: &mut Menu, flow: &mut MatchFlow, config: Option<&MatchConfig>, in_menu: &mut InMenu) {
    let bots = config.and_then(|c| c.0.values.get("MaxPlayersSlider").copied()).unwrap_or(16.0) as i32 - 1;
    let balance = config.and_then(|c| c.0.flags.get("AutoBalanceTeamsCheckBox").copied()).unwrap_or(true);
    let running = matches!(menu.phase, Phase::Spawn | Phase::Playing) && !flow.leave;
    if !running || (balance && bots < 1) {
        info!("menu: team switch refused (running {running}, auto balance {balance}, bots {bots})");
        if let Some(s) = menu.find_scene("U_MessagePopup") {
            let mut a = menu.activate(s);
            let text = menu.ui.loc.get("UI_MenuText.Popups.CannotSwap").unwrap_or("").to_string();
            let ok = menu.ui.loc.get("UI_MenuText.PopUps.OkButton").unwrap_or("OK").to_string();
            for (i, w) in a.scene.widgets.iter().enumerate() {
                match w.tag.as_str() {
                    "iMessageLabel" => {
                        a.texts.insert(i, text.clone());
                    }
                    "CommandButton_Yes" => {
                        a.texts.insert(i, ok.clone());
                        a.shown.insert(i, true);
                        a.actions.insert(i, Some(Action::Back));
                        a.buttons.push(i);
                    }
                    _ => {}
                }
            }
            a.popup = true;
            menu.stack.push(a);
        }
        return;
    }
    let old = crate::combat::player_team();
    let new = old ^ 1;
    crate::combat::set_player_team(new);
    info!("menu: team switch {old} -> {new} (phase {:?})", menu.phase);
    // banner at the next spawn (objectives.rs)
    TEAM_BANNER.store(true, std::sync::atomic::Ordering::Relaxed);
    // the other team's vehicle list: back to infantry
    flow.vehicle_sel = 0;
    flow.vehicle = None;
    let alive = menu.phase == Phase::Playing;
    if let Some(sp) = menu.stack.iter().rev().find_map(|a| a.spawn.as_ref()) {
        flow.loadout = sp.sel_loadout;
    }
    if alive {
        flow.suicide = true;
        menu.stack.clear();
        in_menu.0 = false;
        return;
    }
    // dead: the spawn selection again, built for the new team (flag, vehicles); the auto spawn
    // countdown keeps running
    let Some(pos) = menu.stack.iter().rposition(|a| a.spawn.is_some()) else { return };
    let elapsed = menu.stack[pos].spawn.as_ref().map(|s| s.elapsed).unwrap_or(0.0);
    let Some(s) = menu.find_scene("PC_U_SpawnSelectionMenu") else { return };
    let mut a = menu.activate(s);
    if let Some(sp) = a.spawn.as_mut() {
        sp.resume(flow.hud.kp, flow.loadout, 0);
        sp.elapsed = elapsed;
    }
    menu.stack.truncate(pos);
    menu.stack.push(a);
}

/// Frames the loading screen is drawn before the (blocking) front end load is requested.
const LOADING_SCREEN_FRAMES: u32 = 3;

/// The engine's loading screen (DefaultEngine.ini DefaultLoadingSequence: BackdropTexture
/// EngineResources.Black, FontName PC_UI_Fonts.TradGothicBoldCondensed, HorizontalTextSpacing 2,
/// spinner Radial2 under Radial1). Placement from trace t3 frame 3130 (1366x768):
/// - text "LADEVORGANG …" (LoadingTips [Loading] LoadingText) scaled by vh/720, line top 425,
///   left 429.75 (in 720-line units), glyph advance = width + 2, glyph top = line top + VerticalOffset;
/// - spinner square 0.08 vw centred at (0.88 vw, 0.85 vh), Radial2 rotated, Radial1 on top.
/// The black backdrop is the cleared target (see loading_cameras).
fn loading_quads(ui: &upk::ui::Ui, font_ref: GRef, spinner: (Option<GRef>, Option<GRef>), text: &str, vw: f32, vh: f32, out: &mut Vec<Quad>) {
    let s = vh / 720.0;
    if let Some(font) = ui.font(font_ref) {
        let mut x = 429.75 * s;
        let top = 425.0 * s;
        for ch in text.chars() {
            let Some(g) = font.glyph(ch) else { continue };
            if g.w > 0 && g.h > 0 {
                let tex = font.textures.get(g.tex as usize).and_then(|&t| ui.world.gref(font_ref.pkg, t));
                let (tw, th) = tex.and_then(|t| ui.tex_size(t)).unwrap_or((1024.0, 256.0));
                let y = top + g.voff as f32 * s;
                out.push(Quad {
                    rot: 0.0,
                    rect: [x, y, x + g.w as f32 * s, y + g.h as f32 * s],
                    uv: [g.u as f32 / tw, g.v as f32 / th, (g.u + g.w) as f32 / tw, (g.v + g.h) as f32 / th],
                    tex,
                    color: [1.0; 4],
                });
            }
            x += (g.w as f32 + 2.0) * s;
        }
    }
    let size = 0.08 * vw;
    let (cx, cy) = (0.88 * vw, 0.85 * vh);
    for t in [spinner.0, spinner.1].into_iter().flatten() {
        out.push(Quad { rot: 0.0, rect: [cx - size * 0.5, cy - size * 0.5, cx + size * 0.5, cy + size * 0.5], uv: [0.0, 0.0, 1.0, 1.0], tex: Some(t), color: [1.0; 4] });
    }
}

/// One line of text centred on `cx` with its top at `top` (720-line units), as the loading
/// text is drawn (glyph advance w + 2). The skip prompt's place is NOT from data.
fn centered_text_quads(ui: &upk::ui::Ui, font_ref: GRef, text: &str, cx: f32, top: f32, vh: f32, out: &mut Vec<Quad>) {
    let s = vh / 720.0;
    let Some(font) = ui.font(font_ref) else { return };
    let width: f32 = text.chars().filter_map(|c| font.glyph(c)).map(|g| (g.w as f32 + 2.0) * s).sum();
    let mut x = cx - width * 0.5;
    let top = top * s;
    for ch in text.chars() {
        let Some(g) = font.glyph(ch) else { continue };
        if g.w > 0 && g.h > 0 {
            let tex = font.textures.get(g.tex as usize).and_then(|&t| ui.world.gref(font_ref.pkg, t));
            let (tw, th) = tex.and_then(|t| ui.tex_size(t)).unwrap_or((1024.0, 256.0));
            let y = top + g.voff as f32 * s;
            out.push(Quad { rot: 0.0, rect: [x, y, x + g.w as f32 * s, y + g.h as f32 * s], uv: [g.u as f32 / tw, g.v as f32 / th, (g.u + g.w) as f32 / tw, (g.v + g.h) as f32 / th], tex, color: [1.0; 4] });
        }
        x += (g.w as f32 + 2.0) * s;
    }
}

/// Loading screen: the scene camera is switched off and the UI camera clears to black.
/// During the match preload (warmup.rs) the scene camera renders behind the loading screen /
/// movie (pipelines, shadow maps, post passes get built) without writing to the window.
fn loading_cameras(flow: Res<MatchFlow>, preload: Res<crate::warmup::Preload>, mut cams: Query<(&mut Camera, Has<Camera3d>, Has<Camera2d>), Without<crate::minimap::MinimapCam>>) {
    // the movie covers the screen: the match level built behind it is not drawn meanwhile
    let loading = matches!(flow.phase, Phase::Loading | Phase::Movie);
    let draw3d = !loading || preload.pending();
    for (mut c, is3d, is2d) in &mut cams {
        if is3d {
            if c.is_active != draw3d {
                c.is_active = draw3d;
            }
            // rendered but not shown: the UI camera stays the only one writing the window
            let skip = loading && draw3d;
            if skip != matches!(c.output_mode, bevy::render::camera::CameraOutputMode::Skip) {
                c.output_mode = if skip { bevy::render::camera::CameraOutputMode::Skip } else { default() };
            }
        }
        if is2d && c.order == 1 {
            let want = if loading { ClearColorConfig::Custom(Color::BLACK) } else { ClearColorConfig::None };
            if !matches!((&c.clear_color, &want), (ClearColorConfig::None, ClearColorConfig::None) | (ClearColorConfig::Custom(_), ClearColorConfig::Custom(_))) {
                c.clear_color = want;
            }
        }
    }
}

fn same(a: &[Quad], b: &[Quad]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.rect == y.rect && x.uv == y.uv && x.color == y.color && x.tex == y.tex)
}

// ---------------------------------------------------------------- gamma -> linear pass

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct UiGammaLabel;

#[derive(Resource)]
struct UiGammaPipeline {
    layout: BindGroupLayout,
    id: CachedRenderPipelineId,
}

impl FromWorld for UiGammaPipeline {
    fn from_world(world: &mut World) -> Self {
        let dev = world.resource::<RenderDevice>();
        let layout = dev.create_bind_group_layout(
            "ui_gamma_layout",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_2d(TextureSampleType::Float { filterable: false }),)),
        );
        let shader = world.load_asset("embedded://homebots/uigamma.wgsl");
        let id = world.resource_mut::<PipelineCache>().queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("ui_gamma".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader_vertex_state(),
            fragment: Some(FragmentState {
                shader,
                shader_defs: vec![],
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState { format: ViewTarget::TEXTURE_FORMAT_HDR, blend: None, write_mask: ColorWrites::ALL })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });
        UiGammaPipeline { layout, id }
    }
}

#[derive(Default)]
struct UiGammaNode;

impl ViewNode for UiGammaNode {
    type ViewQuery = (&'static ViewTarget, &'static UiGammaOut);

    fn run(&self, _graph: &mut RenderGraphContext, ctx: &mut RenderContext, (target, _): QueryItem<Self::ViewQuery>, world: &World) -> Result<(), NodeRunError> {
        let p = world.resource::<UiGammaPipeline>();
        let Some(pipe) = world.resource::<PipelineCache>().get_render_pipeline(p.id) else { return Ok(()) };
        let post = target.post_process_write();
        let bg = ctx.render_device().create_bind_group("ui_gamma", &p.layout, &BindGroupEntries::sequential((post.source,)));
        let mut rp = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("ui_gamma"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post.destination,
                resolve_target: None,
                ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_render_pipeline(pipe);
        rp.set_bind_group(0, &bg, &[]);
        rp.draw(0..3, 0..1);
        Ok(())
    }
}
