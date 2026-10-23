#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! HomeBots map viewer: loads static meshes + textures of Homefront maps straight from
//! the local game install and renders them with a free-fly camera.
//!
//! Usage: homebots [--game <HOMEFRONT dir>] [--max-tex N] [--no-cull] [map files or folders...]
//! Default: all Overpass sublevels.
//! Controls: hold right mouse to look, WASD/QE to move, Shift = fast, Ctrl = slow.
// fields and helpers read from the game data but not used (yet), and the ShaderType derives'
// generated `check` functions: no dead-code warnings in the build output (user 2026-10-19)
#![allow(dead_code)]

mod hfpost;
mod gamemat;
mod terrainemu;
mod world;
mod gameshader;
mod scenecopy;
mod grass;
mod speedwind;
mod treewind;
mod sunshadow;
mod particles;
mod menu;
mod creategame;
mod mapmode;
mod mapcheck;
mod tdm;
mod movie;
mod spawnsel;
mod scoreboard;
mod player;
mod hud;
mod objectives;
mod viewmodel;
mod weaponcfg;
mod combat;
mod bots;
mod botextras;
mod botvehicles;
mod botdrones;
mod nav;
mod mixer;
mod sfx;
mod vehicles;
mod vehload;
mod slopetest;
mod warmup;
mod minimap;
mod airstrike;
mod drone;
mod perf;
mod rigdebug;
mod ragdoll;
mod impacts;
mod debug;
mod videofx;
mod settings;
mod input;
mod options;
mod dynlights;
mod pa;
mod nuke;
mod nukecloud;
mod extmodel;
mod record;
mod crater;
mod weather;
mod vmanim;
mod destruction;
mod dmgfeedback;
mod levelcars;
mod piecephys;
mod screenrain;
mod coldebug;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, NotShadowCaster, NotShadowReceiver};
use bevy::render::render_resource::{AsBindGroup, ShaderRef, ShaderType};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::render::camera::Exposure;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::{CursorGrabMode, PrimaryWindow};
use std::collections::HashMap;
use std::path::PathBuf;
use upk::library::Library;
use upk::Package;

const DEFAULT_GAME: &str = r"C:\Program Files (x86)\Steam\steamapps\common\HOMEFRONT";
const UU_TO_M: f32 = 0.01; // Unreal units are centimetres

struct Args {
    game: PathBuf,
    max_tex: u32,
    /// --max-tex given: it wins over the options' texture quality
    max_tex_cli: bool,
    no_cull: bool,
    maps: Vec<PathBuf>,
    post: Option<upk::level::PostProcess>,
    /// --shot <png> --cam x,y,z,yaw,pitch : render one frame from there, save, exit.
    shot: Option<(PathBuf, Option<[f32; 5]>)>,
    /// no map given: start in the front end (main menu over its 3D level)
    menu: bool,
}

fn parse_args() -> Args {
    // the Homefront install: --game, else HOMEBOTS_GAME, else the first line of gamepath.txt
    // (older name: spielpfad.txt; next to the exe / project), else the default Steam path
    let mut game = std::env::var("HOMEBOTS_GAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| ["gamepath.txt", "spielpfad.txt"].iter().filter_map(|f| std::fs::read_to_string(f).ok()).find_map(|t| t.lines().map(|l| l.trim().trim_start_matches('\u{feff}').trim().trim_matches('"').to_string()).find(|l| !l.is_empty() && !l.starts_with('#'))))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_GAME));
    let mut max_tex = 1024;
    let mut max_tex_cli = false;
    let mut no_cull = false;
    let mut shot_path: Option<PathBuf> = None;
    let mut cam = [0.0f32; 5];
    let mut cam_given = false;
    let mut inputs = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--game" => game = it.next().map(PathBuf::from).unwrap_or(game),
            "--no-cull" => no_cull = true,
            "--shot" => shot_path = it.next().map(PathBuf::from),
            "--cam" => {
                cam_given = true;
                if let Some(v) = it.next() {
                    for (i, x) in v.split(',').enumerate().take(5) {
                        cam[i] = x.trim().parse().unwrap_or(0.0);
                    }
                }
            }
            "--max-tex" => {
                max_tex = it.next().and_then(|s| s.parse().ok()).unwrap_or(max_tex);
                max_tex_cli = true;
            }
            _ => inputs.push(PathBuf::from(a)),
        }
    }
    let menu = inputs.is_empty();
    if menu {
        inputs.push(game.join("GCGame").join("CookedPC").join("Maps").join(menu_background(&game)));
    }
    let mut maps = Vec::new();
    for p in inputs {
        if p.is_dir() {
            maps.extend(level_files(&p));
        } else {
            maps.push(p);
        }
    }
    Args { game, max_tex, max_tex_cli, no_cull, maps, post: None, shot: shot_path.map(|p| (p, cam_given.then_some(cam))), menu }
}

/// The log also goes to logs/homebots_latest.log (the previous run's to homebots_prev.log),
/// written unbuffered so it survives a crash; panics are written there with a backtrace.
fn file_log_layer(_app: &mut App) -> Option<bevy::log::BoxedLayer> {
    let _ = std::fs::create_dir_all("logs");
    let _ = std::fs::rename("logs/homebots_latest.log", "logs/homebots_prev.log");
    let f = std::fs::File::create("logs/homebots_latest.log").ok()?;
    let f2 = f.try_clone().ok();
    let prev = std::panic::take_hook();
    let f2 = std::sync::Mutex::new(f2);
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        if let Ok(mut g) = f2.lock() {
            if let Some(f) = g.as_mut() {
                let thread = std::thread::current().name().unwrap_or("?").to_string();
                let _ = writeln!(f, "PANIC in thread '{thread}': {info}\n{}", std::backtrace::Backtrace::force_capture());
            }
        }
        prev(info);
    }));
    use bevy::log::tracing_subscriber::Layer;
    // the frame profiler (perf.rs) times Bevy's system / schedule / render spans
    Some(Box::new(bevy::log::tracing_subscriber::fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(f)).and_then(perf::PerfLayer)))
}

/// Task pools with more threads for Bevy's pipeline compilation (NOT from data).
///
/// Bevy's PipelineCache creates every render pipeline as a task on the AsyncComputeTaskPool,
/// which by default gets 25 % of the logical cores (2 on the user's 8-thread Ryzen Z2 A). The
/// driver's vkCreateGraphicsPipelines (and naga's SPIR-V output) runs there in parallel, so with
/// ~500 pipelines in the match preload two threads were the bottleneck. Async compute now gets
/// all logical cores but two (at least 2; HB_COMPILE_THREADS=<n> overrides); the threads sleep
/// when nothing compiles. The IO and compute pools keep Bevy's default sizes.
fn task_pools() -> bevy::app::TaskPoolPlugin {
    use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
    let total = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let quarter = (((total as f32) * 0.25).round() as usize).clamp(1, 4);
    // Bevy's defaults: IO 25 %, async compute 25 %, compute the rest (at least 1)
    let compute = total.saturating_sub(2 * quarter).max(1);
    let async_threads = std::env::var("HB_COMPILE_THREADS").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(total.saturating_sub(2)).max(2);
    let fixed = |n: usize| TaskPoolThreadAssignmentPolicy { min_threads: n, max_threads: n, percent: 0.0, on_thread_spawn: None, on_thread_destroy: None };
    let mut o = TaskPoolOptions::default();
    o.io = fixed(quarter);
    o.async_compute = fixed(async_threads);
    o.compute = fixed(compute);
    TaskPoolPlugin { task_pool_options: o }
}

/// Release package (tools/make_release.cmd): HomeBots.exe sits next to its `assets` folder and
/// writes logs / settings there; started from Explorer the working directory may be elsewhere.
/// Diagnosis switch: the environment variable is set to something other than "0" (read once).
pub fn diag(name: &'static str) -> bool {
    static CACHE: std::sync::Mutex<Vec<(&'static str, bool)>> = std::sync::Mutex::new(Vec::new());
    let Ok(mut c) = CACHE.lock() else { return false };
    if let Some(v) = c.iter().find(|x| x.0 == name) {
        return v.1;
    }
    let v = std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0");
    c.push((name, v));
    v
}

fn use_exe_dir() {
    if std::path::Path::new("assets").is_dir() {
        return;
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        if dir.join("assets").is_dir() {
            let _ = std::env::set_current_dir(&dir);
        }
    }
}

fn main() {
    use_exe_dir();
    // a failed entity command (e.g. bevy_audio inserting a sink into an entity despawned the
    // same frame) is logged instead of ending the match (user's crash.log 2026-10-18)
    let _ = bevy::ecs::error::GLOBAL_ERROR_HANDLER.set(bevy::ecs::error::warn);
    let args = parse_args();
    let _ = GAME_ROOT.set(args.game.clone());
    // HB_SLOPETEST=1: vehicle slope bench without a window (slopetest.rs)
    if std::env::var_os("HB_SLOPETEST").is_some() {
        slopetest::run(&args.game);
        return;
    }
    // HB_FXNIGHT=<map packages>: night class of placed emitters / unlit materials (particles.rs)
    if let Ok(m) = std::env::var("HB_FXNIGHT") {
        particles::night_report(&args.game, &m);
        return;
    }
    // HB_MAPCHECK=<map folder>[:<mode>]: a map's match data without a window (mapcheck.rs)
    if let Ok(m) = std::env::var("HB_MAPCHECK") {
        mapcheck::run(&args.game, &m);
        return;
    }
    // options (settings.rs) and key bindings (input.rs)
    settings::load();
    input::init(&args.game, &menu::language(&args.game));
    MAX_TEX.store(if args.max_tex_cli { args.max_tex } else { settings::max_tex() }, std::sync::atomic::Ordering::Relaxed);
    let first_level = menu::LevelRequest {
        maps: args.maps.clone(),
        persistent: args.menu.then(|| args.game.join("GCGame").join("CookedPC").join("Maps").join("HFGameEntry.map")),
        cache: args.menu,
        show: true,
    };
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(bevy::log::LogPlugin { custom_layer: file_log_layer, ..default() }).set(task_pools()).set(WindowPlugin {
            primary_window: Some(Window {
                title: "HomeBots".into(),
                // HOMEBOTS_SIZE=WxH (physical pixels), e.g. 1366x768 to compare with the traces
                resolution: std::env::var("HOMEBOTS_SIZE")
                    .ok()
                    .and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))))
                    .map(|(w, h)| bevy::window::WindowResolution::new(w, h).with_scale_factor_override(1.0))
                    .unwrap_or_default(),
                // HB_VSYNC=0: no vsync (profiling the real frame time; default Fifo = vsync)
                present_mode: if std::env::var("HB_VSYNC").is_ok_and(|v| v == "0") { bevy::window::PresentMode::AutoNoVsync } else { bevy::window::PresentMode::Fifo },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(MaterialPlugin::<HfMat>::default())
        .add_plugins(MaterialPlugin::<gamemat::GameMat> { prepass_enabled: false, shadows_enabled: true, ..default() })
        .add_plugins(MaterialPlugin::<terrainemu::TerrainEmu> { prepass_enabled: false, shadows_enabled: true, ..default() })
        .add_plugins(sunshadow::SunShadowPlugin)
        .add_plugins(warmup::WarmUpPlugin)
        .add_plugins(dynlights::DynLightsPlugin)
        .add_plugins(pa::PaPlugin)
        .add_plugins(hfpost::HfPostPlugin)
        .add_plugins(minimap::MinimapPlugin)
        .add_plugins(menu::MenuPlugin {
            game: args.game.clone(),
            enabled: args.menu,
            background: args.maps.first().and_then(|m| m.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        })
        .add_plugins(scenecopy::SceneCopyPlugin)
        .add_plugins(movie::MoviePlugin)
        .add_plugins(player::PlayerPlugin)
        .add_plugins(objectives::ObjectivesPlugin)
        .add_plugins(viewmodel::ViewModelPlugin)
        .add_plugins(combat::CombatPlugin)
        .add_plugins(airstrike::AirstrikePlugin)
        .add_plugins(videofx::VideoFxPlugin)
        .add_plugins(nuke::NukePlugin)
        .add_plugins(nukecloud::NukeCloudPlugin)
        .add_plugins(record::RecordPlugin)
        .add_plugins(crater::CraterPlugin)
        .add_plugins(weather::WeatherPlugin)
        .add_plugins(screenrain::ScreenRainPlugin)
        .add_plugins(input::InputPlugin)
        .add_plugins(options::OptionsPlugin)
        .add_plugins(drone::DronePlugin)
        .add_plugins(perf::PerfPlugin)
        .add_plugins(rigdebug::RigDebugPlugin)
        .add_plugins(bots::BotsPlugin)
        .add_plugins(botextras::BotExtrasPlugin)
        .add_plugins(botdrones::BotDronesPlugin)
        .add_plugins(debug::DebugPlugin)
        .add_plugins(sfx::SfxPlugin)
        .add_plugins(vehicles::VehiclesPlugin)
        .add_plugins((destruction::DestructionPlugin, dmgfeedback::DmgFeedbackPlugin, levelcars::LevelCarsPlugin))
        .insert_resource(sfx::GameRoot(args.game.clone()))
        .init_resource::<sfx::WeaponSfx>()
        .add_plugins(treewind::TreeWindPlugin)
        .init_resource::<particles::Particles>()
        .add_systems(Update, particles::update)
        .init_resource::<impacts::PlayerSteps>()
        .add_systems(Update, impacts::footsteps)
        .add_plugins(MaterialPlugin::<grass::GrassMat> { prepass_enabled: false, shadows_enabled: false, ..default() })
        // NOT from data: user wish 2026-10-18 "reduce all sounds by 20 percent"
        .insert_resource(ClearColor(Color::BLACK))
        // Bevy's flat ambient is replaced by the UE3 hemisphere sky light in hf.wgsl.
        .insert_resource(AmbientLight { color: Color::WHITE, brightness: 0.0, ..default() })
        .insert_non_send_resource(args)
        .insert_resource(first_level)
        .init_resource::<LevelSpawn>()
        .init_resource::<Levels>()
        .add_systems(PreStartup, gamemat::detect_bc)
        .add_systems(Startup, |mut t: ResMut<Time<Virtual>>| {
            // HB_TIMESCALE=<f>: game time runs at that speed (tests: watch animations frame by frame)
            if let Some(f) = std::env::var("HB_TIMESCALE").ok().and_then(|v| v.parse::<f32>().ok()) {
                t.set_relative_speed(f);
            }
        })
        .init_resource::<MeshEntities>()
        .add_systems(PostUpdate, index_mesh_entities)
        .add_systems(Last, touch_modified_meshes)
        .add_systems(Update, load_maps.run_if(resource_exists::<menu::LevelRequest>))
        .init_resource::<PostVolumes>()
        .add_systems(Update, (fly_camera, shot_system, post_delta_time, post_volume_select));
    bevy::asset::embedded_asset!(app, "hf.wgsl");
    app.run();
}

/// UE3 sky light (hemisphere) + terrain layer blending, see hf.wgsl.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
struct HfExt {
    #[uniform(100)]
    params: HfParams,
    #[texture(101)]
    #[sampler(102)]
    weightmap: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(104)]
    layer0: Option<Handle<Image>>,
    #[texture(105)]
    #[sampler(106)]
    layer1: Option<Handle<Image>>,
    #[texture(107)]
    #[sampler(108)]
    layer2: Option<Handle<Image>>,
    #[texture(109)]
    #[sampler(110)]
    layer3: Option<Handle<Image>>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Reflect, ShaderType)]
struct HfParams {
    sky_upper: Vec4,
    sky_lower: Vec4,
    bounds: Vec4,
    origin: Vec4,
    scales: Vec4,
    flags: UVec4,
}

impl MaterialExtension for HfExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://homebots/hf.wgsl".into()
    }
}

type HfMat = ExtendedMaterial<StandardMaterial, HfExt>;

fn hf(params: HfParams) -> HfExt {
    HfExt { params, weightmap: None, layer0: None, layer1: None, layer2: None, layer3: None }
}

#[derive(Component)]
struct FlyCam {
    yaw: f32,
    pitch: f32,
}


/// UE (left-handed, Z up) -> Bevy (right-handed, Y up): swap Y and Z.
pub(crate) fn ue_to_bevy(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[2], v[1])
}

/// UE rotator (pitch, yaw, roll; 65536 = 360 deg) to a Bevy rotation.
pub(crate) fn rotator_to_quat(r: [i32; 3]) -> Quat {
    let k = std::f32::consts::TAU / 65536.0;
    let (sp, cp) = (r[0] as f32 * k).sin_cos();
    let (sy, cy) = (r[1] as f32 * k).sin_cos();
    let (sr, cr) = (r[2] as f32 * k).sin_cos();
    // Rotated UE basis axes (in UE coordinates).
    let ax = [cp * cy, cp * sy, sp];
    let ay = [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp];
    let az = [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp];
    // Conjugate with the Y/Z swap: column j of the Bevy matrix is swap(axis[swap(j)]).
    let m = Mat3::from_cols(ue_to_bevy(ax), ue_to_bevy(az), ue_to_bevy(ay));
    Quat::from_mat3(&m).normalize()
}

fn build_mesh(lod: &upk::mesh::MeshLod, section: &upk::mesh::Section) -> Option<Mesh> {
    let start = section.first_index as usize;
    let end = start + section.num_triangles as usize * 3;
    let idx = lod.indices.get(start..end)?;
    if idx.is_empty() {
        return None;
    }
    let pos: Vec<[f32; 3]> = lod.positions.iter().map(|p| ue_to_bevy(*p).to_array()).collect();
    let nrm: Vec<[f32; 3]> = lod.normals.iter().map(|n| ue_to_bevy(*n).normalize_or_zero().to_array()).collect();
    // Checked on decal data: UE front faces are CCW seen from the normal side, and the
    // Y/Z swap keeps them CCW in Bevy, so indices are used as-is.
    let tri: Vec<u32> = idx.to_vec();
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, lod.uvs.clone())
            .with_inserted_indices(Indices::U32(tri)),
    )
}


/// Mesh for the game-shader path: positions in Bevy axes (for culling), the packed UE3
/// tangent basis, two UV sets and white vertex colour (UE3's null colour stream).
/// Drone killstreaks (drone.rs) by cost key. Everything else comes from the data: the vehicle
/// class (GCWeap_DRO_<X>_C m_DroneVehClass), its defaults over the parent classes', the gun
/// (Seats[0].GunClass VehicleWeaponInfo), audio (VehicleAudio), fuel (VehicleFuelInfo), flying
/// (bCanFly).
pub const DRONE_KINDS: [&str; 4] = ["cfg_dro_gassault", "cfg_dro_gantitank", "cfg_dro_aassault", "cfg_dro_arecon"];

/// SCEngineIdle and the first seat's first SCFireSound of a vehicle class's VehicleAudio setup
/// (GCVehicleAudio; the A2_* audio packages are cooked into the maps).
fn vehicle_audio(lib: &Library, content: &upk::Package, class: &str) -> (String, String) {
    let Some(setup_path) = weaponcfg::class_defaults(content, class).iter().find_map(|p| p.object("VehicleAudio").filter(|&o| o != 0)).map(|o| content.object_path(o)) else {
        return Default::default();
    };
    let Some(pk) = setup_path.split('.').next() else { return Default::default() };
    let (ap, own) = match lib.open(pk) {
        Some(p) => (std::sync::Arc::new(p), true),
        None => match sfx::cooked_package(lib, pk) {
            Some(p) => (p, false),
            None => return Default::default(),
        },
    };
    let inner = if own { setup_path.split_once('.').map(|x| x.1).unwrap_or("").to_string() } else { setup_path.clone() };
    let Some(i) = ap.exports.iter().enumerate().position(|(i, e)| ap.export_class(e) == "GCVehicleAudio" && ap.object_path(i as i32 + 1).eq_ignore_ascii_case(&inner)) else { return Default::default() };
    let Some(pr) = ap.export_props(i) else { return Default::default() };
    let path = |o: i32| if o > 0 && own { format!("{pk}.{}", ap.object_path(o)) } else { ap.object_path(o) };
    let Some(setup) = ap.struct_array(&pr, "VehicleAudioSetups").into_iter().next() else { return Default::default() };
    let engine = setup.object("SCEngineIdle").filter(|&o| o != 0).map(path).unwrap_or_default();
    let fire = ap.struct_array(&setup, "Seats").first().and_then(|seat| ap.struct_array(seat, "FireModes").into_iter().next()).and_then(|fm| fm.object("SCFireSound").filter(|&o| o != 0)).map(path).unwrap_or_default();
    (engine, fire)
}

fn load_drone(lib: &Library, kind: usize, spawn: &mut dyn FnMut(&str, bool) -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)>) -> Option<drone::DroneAssets> {
    let key = DRONE_KINDS[kind];
    let content = lib.open("GCGameContent")?;
    let gcgame = lib.open("GCGame")?;
    let wc = lib.open("WeaponConfigs")?;
    let find = |p: &Package, name: &str| p.exports.iter().enumerate().position(|(i, _)| p.object_path(i as i32 + 1) == name);
    let (class_s, gun_s) = weaponcfg::drone_classes(&content, key)?;
    let class = class_s.as_str();
    // the class's defaults, then its parents' (same package)
    let chain = weaponcfg::class_defaults(&content, class);
    let def = upk::props::Props(chain.into_iter().flat_map(|p| p.0).collect());
    // the script classes above (GCGame, e.g. GCVeh_RCDroneHeli): numbers only
    let mut classes = Vec::new();
    let script_def = {
        let mut ci = (0..content.exports.len()).find(|&i| content.object_name_of(&content.exports[i]) == class && content.export_class(&content.exports[i]) == "Class").map(|i| i as i32 + 1).unwrap_or(0);
        while ci > 0 {
            classes.push(content.object_name(ci));
            ci = content.exports[ci as usize - 1].super_index;
        }
        let name = if ci < 0 { content.object_name(ci) } else { String::new() };
        upk::props::Props(weaponcfg::class_defaults(&gcgame, &name).into_iter().flat_map(|p| p.0).collect())
    };
    let flying = def.bool("bCanFly").unwrap_or(false);
    let audio_path = def.object("VehicleAudio").filter(|&o| o != 0).map(|o| content.object_path(o)).unwrap_or_default();
    let audio = audio_path.rsplit('.').next().unwrap_or("").to_string();
    let fuel_path = def.object("VehicleFuelInfo").filter(|&o| o != 0).map(|o| content.object_path(o)).unwrap_or_else(|| "UCV_GroundDrones.VehicleSetup.Fuel".into());
    let gun_cfg = gun_s.unwrap_or_default();
    let detect_sq = match def.get("m_nDetectionRadiusSq").or_else(|| script_def.get("m_nDetectionRadiusSq")) {
        Some(upk::props::Value::Int(v)) => *v as f32,
        _ => 0.0,
    };
    let parent = (0..content.exports.len()).find(|&i| content.object_name_of(&content.exports[i]) == class && content.export_class(&content.exports[i]) == "Class").map(|i| content.exports[i].super_index).filter(|&s| s > 0).map(|s| content.object_name(s)).unwrap_or_default();
    let sub = |name: &str| -> upk::props::Props {
        let mut v = Vec::new();
        for c in [class, parent.as_str()] {
            if let Some(pr) = find(&content, &format!("Default__{c}.{name}")).and_then(|i| content.export_props(i)) {
                v.extend(pr.0);
            }
        }
        upk::props::Props(v)
    };
    let mesh_comp = sub("SVehicleMesh");
    let mesh_path = content.object_path(mesh_comp.object("SkeletalMesh")?);
    let mesh_z = mesh_comp.vector("Translation").map(|t| t[2]).unwrap_or(0.0);
    let cyl = sub("CollisionCylinder");
    let seat = content.struct_array(&def, "Seats").into_iter().next();
    let cam_offset = seat.as_ref().and_then(|s| s.vector("CameraOffset")).unwrap_or([0.0, 0.0, 50.0]);
    let cam_tag = seat.as_ref().and_then(|s| s.enum_name("CameraTag").map(String::from)).unwrap_or_default();
    // fuel: the setup's values over the class default GCVehicleFuelInfo
    let fuel_def = find(&gcgame, "Default__GCVehicleFuelInfo").and_then(|i| gcgame.export_props(i));
    let fuel = fuel_path.split_once('.').and_then(|(pkg, inner)| {
        let p = lib.open(pkg)?;
        let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "GCVehicleFuelInfo" && p.object_path(i as i32 + 1) == inner)?;
        p.export_props(i)
    });
    let fr = |n: &str, d: f32| fuel.as_ref().and_then(|f| f.float(n)).or_else(|| fuel_def.as_ref().and_then(|f| f.float(n))).unwrap_or(d);
    let fuel_max = fr("MaxFuel", 1000.0);
    let gp = wc.exports.iter().enumerate().position(|(i, e)| wc.export_class(e) == "GCVehicleWeaponInfo" && wc.object_path(i as i32 + 1) == gun_cfg).and_then(|gi| wc.export_props(gi)).unwrap_or_default();
    let fm = wc.struct_array(&gp, "FireModes").into_iter().next().unwrap_or_default();
    let mut gun = vehicles::GunCfg {
        fire_interval: fm.float("FireInterval").unwrap_or(0.15),
        range: fm.float("WeaponRange").unwrap_or(5500.0),
        spread: fm.float("FireSpread").unwrap_or(2.0),
        dmg_type: fm.object("InstantHitDamageType").filter(|&o| o != 0).map(|o| wc.object_name(o)).unwrap_or_default(),
        overheat_max: fm.float("OverheatMaxTime").unwrap_or(3.0),
        overheat_add: fm.float("OverheatAddTime").unwrap_or(0.3),
        overheat_recover: fm.float("OverheatMinRecoverTime").unwrap_or(1.5),
        sound: String::new(),
        sound_looping: false,
        engine_idle: String::new(),
        engine_shutdown: String::new(),
        engine_sprint: String::new(),
        tracer: None,
        emp_start: String::new(),
        emp_loop: String::new(),
        splash: None,
        full_range: fm.float("FullDamageRange").unwrap_or(0.0),
    };
    let full_range = fm.float("FullDamageRange").unwrap_or(gun.range);
    // fire sounds per fire mode (VehicleAudioSetups[0].Seats[0].FireModes[])
    let mut mode_sounds = Vec::new();
    // A2_Droids_C has no file of its own in the install: it is cooked into the maps (object
    // paths there carry the package name already)
    let droids = lib.open("A2_Droids_C").map(|p| (std::sync::Arc::new(p), true)).or_else(|| sfx::cooked_package(lib, "A2_Droids_C").map(|p| (p, false)));
    if let Some((ap, own)) = droids {
        if let Some(i) = ap.exports.iter().enumerate().position(|(i, e)| !audio.is_empty() && ap.export_class(e) == "GCVehicleAudio" && ap.object_path(i as i32 + 1).ends_with(&audio)) {
            if let Some(pr) = ap.export_props(i) {
                let path = |o: i32| if o > 0 && own { format!("A2_Droids_C.{}", ap.object_path(o)) } else { ap.object_path(o) };
                if let Some(setup) = ap.struct_array(&pr, "VehicleAudioSetups").into_iter().next() {
                    gun.engine_idle = setup.object("SCEngineIdle").filter(|&o| o != 0).map(path).unwrap_or_default();
                    if let Some(seat) = ap.struct_array(&setup, "Seats").first() {
                        for (k, fm) in ap.struct_array(seat, "FireModes").into_iter().enumerate() {
                            let snd = fm.object("SCFireSound").filter(|&o| o != 0).map(path).unwrap_or_default();
                            if k == 0 {
                                gun.sound = snd.clone();
                                gun.sound_looping = fm.bool("bLooping").unwrap_or(false);
                            }
                            mode_sounds.push(snd);
                        }
                    }
                }
            }
        }
    }
    let (h, m) = spawn(&mesh_path, false)?;
    let skeleton = m.bones.clone();
    let pose = drone::ref_pose(&skeleton);
    let rig = load_drone_rig(lib, &content, class, &skeleton);
    // GCVeh_RCDroneTank tread scroll: Left/RightTreadMaterialIndex (2 / 1), TexPan2UnrealScale 5;
    // only slots whose material reads TreadLocation (checked in the mesh's own package)
    let tread_slots: Option<([i32; 2], f32)> = match (def.get("LeftTreadMaterialIndex").or_else(|| script_def.get("LeftTreadMaterialIndex")), def.get("RightTreadMaterialIndex").or_else(|| script_def.get("RightTreadMaterialIndex")), def.float("TexPan2UnrealScale").or_else(|| script_def.float("TexPan2UnrealScale"))) {
        (Some(upk::props::Value::Int(l)), Some(upk::props::Value::Int(r)), Some(tp)) => Some(([*l, *r], tp)),
        _ => None,
    };
    let tread_ok: [bool; 2] = tread_slots
        .and_then(|(sl, _)| {
            let (pk, _) = mesh_path.split_once('.')?;
            let mp = lib.open(pk)?;
            let mi = mp.exports.iter().enumerate().position(|(i, e)| mp.export_class(e) == "SkeletalMesh" && mp.object_path(i as i32 + 1).eq_ignore_ascii_case(mesh_path.split_once('.').map(|x| x.1).unwrap_or("")))?;
            let mats = mp.read_skeletal_mesh(mi)?.materials;
            Some(sl.map(|k| mats.get(k as usize).is_some_and(|&r| vehload::tread_param(&[], &mp, r))))
        })
        .unwrap_or([false; 2]);
    // third-person view: Seats[0] BehindCameraMaxOffset from the camera socket (ESeatAllowedViews
    // SEATVIEW_BOTH)
    let behind = seat.as_ref().and_then(|s| s.vector("BehindCameraMaxOffset")).unwrap_or([-200.0, 0.0, 0.0]);
    // lock-on box of the gun (LockOn.BigRectangle LockTopLeftCorner / LockBottomRightCorner,
    // 1280x720 units around the screen centre): GCUIObject_TargetingBox
    let vec2 = |p: &upk::props::Props, n: &str| match p.get(n) {
        Some(upk::props::Value::Raw { data, .. }) if data.len() >= 8 => Some([f32::from_le_bytes(data[0..4].try_into().ok()?), f32::from_le_bytes(data[4..8].try_into().ok()?)]),
        _ => None,
    };
    let lock = wc.struct_prop(&gp, "LockOn").filter(|l| l.bool("bEnabled").unwrap_or(true));
    let lock_box = lock.as_ref().and_then(|l| wc.struct_prop(l, "BigRectangle")).and_then(|b| Some((vec2(&b, "LockTopLeftCorner")?, vec2(&b, "LockBottomRightCorner")?))).filter(|(a, b)| b[0] > a[0] && b[1] > a[1]);
    let lock_small = lock.as_ref().and_then(|l| wc.struct_prop(l, "SmallRectangle")).and_then(|b| Some((vec2(&b, "MaterialTopLeftCorner")?, vec2(&b, "MaterialBottomRightCorner")?)));
    let lock_dist = lock.as_ref().and_then(|l| l.float("MaxLockDist")).unwrap_or(0.0);
    // the air recon drone's gun (EWFT_Custom with a lock-on, GCVW_SH_ARecon LockOnTargeterClass
    // GCLockOnTargeterRecon): marks the enemies in its box (DroneLockOnDistanceSQ) for
    // FL2GameMP DroneMarkDuration seconds while the fire button is held
    // (UI_GameplayHints ReconDroneMarkTarget "HALTEN, UM EIN ZIEL ZU MARKIEREN")
    let marks = fm.enum_name("WeaponFireType") == Some("EWFT_Custom") && lock_box.is_some();
    let mark = marks.then(|| {
        let sq = gcgame.exports.iter().position(|e| gcgame.object_name_of(e) == "Default__GCLockOnTargeterRecon").and_then(|i| gcgame.export_props(i)).and_then(|p| p.float("DroneLockOnDistanceSQ")).unwrap_or(1.0e7);
        let cooked = [lib.root.join("GCGame").join("CookedPC"), lib.root.clone()].into_iter().find(|d| d.is_dir()).unwrap_or_default();
        let secs = weaponcfg::ini_f32(&cooked, "gcgame.gcgame.fl2gamemp.dronemarkduration").unwrap_or(10.0);
        (sq.sqrt().min(if lock_dist > 0.0 { lock_dist } else { f32::MAX }), secs)
    });
    // the camera: Seats[0] CameraTag socket (DriverViewSocket on Turret_Pitch, no offset) +
    // CameraOffset; without a tag CameraOffset above the cylinder centre
    let sockets = lib.open(mesh_path.split('.').next().unwrap_or("")).and_then(|p| {
        let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SkeletalMesh" && mesh_path.to_ascii_lowercase().ends_with(&format!(".{}", p.object_path(i as i32 + 1)).to_ascii_lowercase()))?;
        Some(p.mesh_sockets(i))
    }).unwrap_or_default();
    let socket_z = (!cam_tag.is_empty() && cam_tag != "None")
        .then(|| sockets.iter().find(|s| s.name.eq_ignore_ascii_case(&cam_tag)))
        .flatten()
        .and_then(|s| skeleton.iter().position(|b| b.name.eq_ignore_ascii_case(&s.bone)))
        .map(|k| -pose[k].1.y);
    let (camera_z, camera_back) = match socket_z {
        // pose y is -up (drone.rs mesh_to_comp); the mesh sits mesh_z above the cylinder centre
        Some(z) => (mesh_z + z + cam_offset[2], -cam_offset[0]),
        None => (cam_offset[2], -cam_offset[0]),
    };
    // rocket sockets Seat0FireMode<m>Socket<k> (on Turret_Pitch like the camera socket)
    let fire_sockets: Vec<Vec<[f32; 3]>> = (0..2)
        .map(|m| {
            let mut v: Vec<(usize, [f32; 3])> = sockets.iter().filter_map(|s| s.name.strip_prefix(&format!("Seat0FireMode{m}Socket")).and_then(|k| k.parse::<usize>().ok()).map(|k| (k, s.loc))).collect();
            v.sort_by_key(|x| x.0);
            v.into_iter().map(|x| x.1).collect()
        })
        .collect();
    // first-person cockpit (CockpitMesh: SkeletalMesh, Translation; SDPG_Foreground), placed at the
    // camera socket's bone
    let cockpit_comp = sub("CockpitMesh");
    let cam_bone = (!cam_tag.is_empty() && cam_tag != "None").then(|| sockets.iter().find(|s| s.name.eq_ignore_ascii_case(&cam_tag)).map(|s| s.bone.clone())).flatten().unwrap_or_default();
    let cockpit = cockpit_comp.object("SkeletalMesh").filter(|&o| o != 0).map(|o| content.object_path(o)).and_then(|path| {
        let (ch, cm) = spawn(&path, true)?;
        let cbones = cm.bones.clone();
        let cpose = drone::ref_pose(&cbones);
        let anchor = cbones.iter().position(|b| b.name.eq_ignore_ascii_case(&cam_bone)).map(|k| cpose[k].1).unwrap_or(Vec3::ZERO);
        let tr = cockpit_comp.vector("Translation").unwrap_or([0.0; 3]);
        info!("drone {key} cockpit {path}: anchor {cam_bone} {anchor:?}, translation {tr:?}");
        Some(drone::Cockpit { part: viewmodel::SkinPart::new(ch, cm, &cbones, &[]), pose: cpose, anchor, translation: Vec3::from(tr) })
    });
    info!("drone {key} {class} gun {gun_cfg} flying {flying} detect {} fuel {fuel_path} behind {behind:?} lock box {lock_box:?} small {lock_small:?} dist {lock_dist}", detect_sq.sqrt());
    info!("drone {key} {mesh_path}: {} bones, fire {} (looping {}), modes {mode_sounds:?}, engine {}, fuel {fuel_max}, camera z {camera_z} back {camera_back} ({cam_tag})", skeleton.len(), gun.sound, gun.sound_looping, gun.engine_idle);
    let part = viewmodel::SkinPart::new(h, m, &skeleton, &[]);
    let tread = tread_slots.filter(|_| tread_ok.iter().any(|&x| x)).map(|(sl, tp)| (tp, [0, 1].map(|i| if tread_ok[i] { part.slot_meshes(sl[i]) } else { Vec::new() })));
    // the treads' lowest vertex (mesh space: up = -y): the drone stands on its treads (with the
    // cylinder centre + Translation z they sank 5.9 units into the ground, user 2026-10-20)
    let tread_low = tread_slots.filter(|_| tread.is_some()).and_then(|(sl, _)| {
        let m = &part.mesh;
        m.lod
            .sections
            .iter()
            .filter(|sec| {
                let k = m.lod0_material_map.get(sec.material as usize).copied().unwrap_or(sec.material);
                (0..2).any(|i| tread_ok[i] && sl[i] == k)
            })
            .flat_map(|sec| m.lod.indices[sec.first_index as usize..(sec.first_index + sec.num_triangles * 3) as usize].iter())
            .map(|&v| m.lod.positions[v as usize][1])
            .reduce(f32::max)
    });
    info!("drone {key} treads: slots {tread_slots:?} TreadLocation {tread_ok:?} sections {:?} lowest {tread_low:?}", tread.as_ref().map(|t| [t.1[0].len(), t.1[1].len()]));
    // mirror attachments (GCV_SH_AAssault wingleft / wingright rocket pods on Missile_1 /
    // Missile_0 by its PostBeginPlay): vehload.rs
    let specs = lib.open(mesh_path.split('.').next().unwrap_or("")).and_then(|p| {
        let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SkeletalMesh" && mesh_path.to_ascii_lowercase().ends_with(&format!(".{}", p.object_path(i as i32 + 1)).to_ascii_lowercase()))?;
        let chain: Vec<String> = classes.iter().filter(|c| find(&content, &format!("Default__{c}")).is_some()).cloned().collect();
        let specs = vehload::attachment_specs(&content, &chain, &p, i);
        info!("drone {key} attachments {:?}", specs.iter().map(|a| (&a.0, &a.4)).collect::<Vec<_>>());
        Some(specs)
    });
    let attach = specs.as_ref().map(|sp| vehload::attachment_parts(sp, &skeleton, &mut |r: &str| spawn(r, false))).unwrap_or_default();
    // hit box of the bots' drones (botdrones.rs): the mesh's bounds around its origin (mesh space:
    // z forward, x left, -y up). NOT from data: a box instead of the collision.
    let hit_box = {
        let ps = &part.mesh.lod.positions;
        let fwd = ps.iter().map(|p| p[2].abs()).fold(0.0f32, f32::max).max(10.0);
        let side = ps.iter().map(|p| p[0].abs()).fold(0.0f32, f32::max).max(10.0);
        let top = ps.iter().map(|p| -p[1]).fold(f32::MIN, f32::max).max(10.0);
        let bottom = ps.iter().map(|p| p[1]).fold(f32::MIN, f32::max).max(0.0);
        (Vec3::new(-fwd, -side, -bottom), Vec3::new(fwd, side, top))
    };
    // further drawable copies for the bots' drones (botdrones::INSTANCES, NOT from data)
    let mut bots = Vec::new();
    for _ in 0..botdrones::INSTANCES {
        let Some((h, m)) = spawn(&mesh_path, false) else { break };
        let p = viewmodel::SkinPart::new(h, m, &skeleton, &[]);
        let tread: [Vec<viewmodel::TreadPart>; 2] = tread_slots.filter(|_| tread_ok.iter().any(|&x| x)).map(|(sl, _)| [0, 1].map(|i| if tread_ok[i] { p.slot_meshes(sl[i]) } else { Vec::new() })).unwrap_or_default();
        let attach = specs.as_ref().map(|sp| vehload::attachment_parts(sp, &skeleton, &mut |r: &str| spawn(r, false))).unwrap_or_default();
        bots.push(drone::BotInstance { part: p, attach, tread });
    }
    info!("drone {key}: {} copies for the bots, hit box {hit_box:?}", bots.len());
    Some(drone::DroneAssets {
        key,
        attach,
        tread,
        tread_low,
        part,
        pose,
        mesh_z,
        radius: cyl.float("CollisionRadius").unwrap_or(25.0),
        half_height: cyl.float("CollisionHeight").unwrap_or(25.0),
        speed: if flying { def.float("AirSpeed").unwrap_or(425.0) } else { def.float("GroundSpeed").unwrap_or(275.0) },
        flying,
        detect_radius: detect_sq.sqrt(),
        classes,
        health: match def.get("Health") {
            Some(upk::props::Value::Int(h)) => *h as f32,
            _ => 730.0,
        },
        camera_z,
        camera_back,
        fuel_max,
        fuel_moving: fr("FuelConsumptionRateMoving", 10.0),
        fuel_idle: fr("FuelConsumptionRateIdle", 1.0),
        fuel_shot: fr("FuelConsumptionAmountPerWeaponFire", 3.0),
        gun,
        fire_sockets,
        mode_sounds,
        behind: Vec3::from(behind),
        lock_box,
        lock_small,
        lock_dist,
        mark,
        cockpit,
        full_range,
        skeleton,
        rig,
        bots,
        hit_box,
    })
}

/// The drone's AnimTree (SVehicleMesh AnimTreeTemplate AT_UCV_GAssault_Walking): the skeletal
/// controls and wheel nodes it carries (turret yaw / pitch, treads facing the velocity, wheel
/// roll of WHL_L* / WHL_R* via GCAnimNodeBlendSingleBones, antenna spring) with their values,
/// and the wheels' WheelRadius / RollFactor of the class default (NewWHL_L0 / NewWHL_R0).
fn load_drone_rig(lib: &Library, content: &Package, class: &str, skeleton: &[upk::skeletal::Bone]) -> drone::DroneRig {
    let bone = |n: &str| skeleton.iter().position(|b| b.name.eq_ignore_ascii_case(n));
    let mut rig = drone::DroneRig::default();
    let find = |p: &Package, name: &str| p.exports.iter().enumerate().position(|(i, _)| p.object_path(i as i32 + 1) == name);
    let mesh_comp = find(content, &format!("Default__{class}.SVehicleMesh")).and_then(|i| content.export_props(i));
    let tree = mesh_comp.as_ref().and_then(|m| m.object("AnimTreeTemplate")).map(|o| content.object_path(o)).unwrap_or_default();
    let (pkg_name, tree_path) = tree.split_once('.').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
    let Some(tp) = lib.open(&pkg_name) else {
        warn!("drone: anim tree {tree} not found");
        return rig;
    };
    // skeletal controls by the bone they sit on (AnimTree SkelControlLists)
    for i in 0..tp.exports.len() {
        let path = tp.object_path(i as i32 + 1);
        if !path.starts_with(&format!("{tree_path}.")) {
            continue;
        }
        let cls = tp.export_class(&tp.exports[i]).to_string();
        let Some(pr) = tp.export_props(i) else { continue };
        match cls.as_str() {
            "GCSkelControl_TurretConstrained" => {
                let yaw = pr.bool("bRotateYaw").unwrap_or(true);
                let pitch = pr.bool("bRotatePitch").unwrap_or(true);
                if pitch && !yaw {
                    rig.turret_pitch = bone("Turret_Pitch");
                } else if yaw && !pitch {
                    rig.turret_yaw = bone("Turret_Rotate");
                }
            }
            "GCSkelControl_LookAtVelocity" => {
                rig.treads = bone("TREADS_BASE");
                rig.rest_velocity = pr.float("RestVelocity").unwrap_or(100.0);
            }
            "GCSkelControl_HeliRotorSwap" => {
                // the spinning state (highest RotorRPS)
                let defs = tp.struct_array(&pr, "RotorDefinitions");
                let best = defs.iter().max_by(|a, b| a.float("RotorRPS").unwrap_or(0.0).total_cmp(&b.float("RotorRPS").unwrap_or(0.0)));
                if let Some(d) = best {
                    let bones: Vec<(usize, f32, bool)> = tp.struct_array(d, "AffectedBones").iter().filter_map(|b| Some((bone(b.enum_name("BoneName")?)?, b.float("BoneScale").unwrap_or(1.0), b.bool("BoneRotates").unwrap_or(false)))).collect();
                    let rps = Vec3::from(pr.vector("vMaxRotationsPerSecond").unwrap_or([0.0; 3]));
                    rig.rotors.push((bones, rps));
                }
            }
            "GCSkelControl_Spring" => {
                rig.antenna_top = bone("ANTENNA_0_TOP");
                rig.antenna_bot = bone("ANTENNA_0_BOT");
                rig.spring = (pr.float("SpringStiffness").unwrap_or(50.0), pr.float("SpringDamping").unwrap_or(4.0), pr.float("MaxDisplacement").unwrap_or(0.0));
            }
            _ => {}
        }
    }
    // wheel roll: GCAnimNodeBlendSingleBones children WHL_L0..2 -> VehicleWheel_0 (WHL_L0),
    // WHL_R0..2 -> VehicleWheel_1 (WHL_R0)
    for (k, b) in skeleton.iter().enumerate() {
        let n = b.name.to_ascii_uppercase();
        if n.starts_with("WHL_L") {
            rig.wheels_l.push(k);
        } else if n.starts_with("WHL_R") {
            rig.wheels_r.push(k);
        }
    }
    if let Some(w) = find(content, &format!("Default__{class}.NewWHL_L0")).and_then(|i| content.export_props(i)) {
        rig.wheel_radius = w.float("WheelRadius").unwrap_or(rig.wheel_radius);
        rig.roll_factor = w.float("RollFactor").unwrap_or(rig.roll_factor);
    }
    rig.track_half = rig.wheels_r.first().map(|&k| skeleton[k].pos[1].abs()).unwrap_or(20.0);
    info!(
        "drone rig {tree}: turret yaw {:?} pitch {:?}, treads {:?} (rest velocity {}), wheels {:?}/{:?} radius {} roll {}, antenna {:?}/{:?} spring {:?}, rotors {:?}",
        rig.turret_yaw, rig.turret_pitch, rig.treads, rig.rest_velocity, rig.wheels_l, rig.wheels_r, rig.wheel_radius, rig.roll_factor, rig.antenna_bot, rig.antenna_top, rig.spring, rig.rotors
    );
    rig
}

pub(crate) fn build_game_mesh(lod: &upk::mesh::MeshLod, section: &upk::mesh::Section) -> Option<Mesh> {
    let start = section.first_index as usize;
    let end = start + section.num_triangles as usize * 3;
    let idx = lod.indices.get(start..end)?;
    if idx.is_empty() {
        return None;
    }
    let n = lod.positions.len();
    let pos: Vec<[f32; 3]> = lod.positions.iter().map(|p| ue_to_bevy(*p).to_array()).collect();
    let uv1 = lod.uv_sets.get(1).cloned().unwrap_or_else(|| lod.uvs.clone());
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(gameshader::ATTR_UE_POS, lod.positions.clone())
            .with_inserted_attribute(gamemat::ATTR_TANGENT_X, bevy::render::mesh::VertexAttributeValues::Unorm8x4(lod.tangent_x.clone()))
            .with_inserted_attribute(gamemat::ATTR_TANGENT_Z, bevy::render::mesh::VertexAttributeValues::Unorm8x4(lod.tangent_z.clone()))
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, lod.uvs.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uv1)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32, 1.0, 1.0, 1.0]; n])
            .with_inserted_indices(Indices::U32(idx.to_vec())),
    )
}

/// Vertices a section of a CPU-skinned mesh uses (sorted), None for an empty / broken section.
pub(crate) fn section_vertex_map(lod: &upk::mesh::MeshLod, section: &upk::mesh::Section) -> Option<Vec<u32>> {
    let start = section.first_index as usize;
    let idx = lod.indices.get(start..start + section.num_triangles as usize * 3)?;
    if idx.is_empty() || idx.iter().any(|&i| i as usize >= lod.positions.len()) {
        return None;
    }
    let mut v = idx.to_vec();
    v.sort_unstable();
    v.dedup();
    Some(v)
}

/// A section of a CPU-skinned mesh with only the vertices it uses (`map`: section vertex ->
/// mesh vertex). The skinning rewrites the vertex data every frame; before every section mesh
/// carried all vertices of the mesh (10 sections of SKW_ATW_PROX_1P = 10 copies uploaded).
pub(crate) fn build_skinned_section(lod: &upk::mesh::MeshLod, section: &upk::mesh::Section, map: &[u32]) -> Option<Mesh> {
    let start = section.first_index as usize;
    let idx = lod.indices.get(start..start + section.num_triangles as usize * 3)?;
    let mut remap = vec![u32::MAX; lod.positions.len()];
    for (k, &v) in map.iter().enumerate() {
        remap[v as usize] = k as u32;
    }
    let pick = |a: usize| map.iter().map(move |&v| v as usize).map(move |v| v.min(a.saturating_sub(1)));
    let n = lod.positions.len();
    let sel_f3 = |src: &Vec<[f32; 3]>| pick(n).map(|v| src[v]).collect::<Vec<_>>();
    let sel_u8 = |src: &Vec<[u8; 4]>| pick(n).map(|v| src.get(v).copied().unwrap_or([128, 128, 255, 255])).collect::<Vec<_>>();
    let sel_uv = |src: &Vec<[f32; 2]>| pick(n).map(|v| src.get(v).copied().unwrap_or([0.0; 2])).collect::<Vec<_>>();
    let ue = sel_f3(&lod.positions);
    let pos: Vec<[f32; 3]> = ue.iter().map(|p| ue_to_bevy(*p).to_array()).collect();
    let uv0 = sel_uv(&lod.uvs);
    let uv1 = lod.uv_sets.get(1).map(&sel_uv).unwrap_or_else(|| uv0.clone());
    let m = map.len();
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(gameshader::ATTR_UE_POS, ue)
            .with_inserted_attribute(gamemat::ATTR_TANGENT_X, bevy::render::mesh::VertexAttributeValues::Unorm8x4(sel_u8(&lod.tangent_x)))
            .with_inserted_attribute(gamemat::ATTR_TANGENT_Z, bevy::render::mesh::VertexAttributeValues::Unorm8x4(sel_u8(&lod.tangent_z)))
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv0)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uv1)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32, 1.0, 1.0, 1.0]; m])
            .with_inserted_indices(Indices::U32(idx.iter().map(|&i| remap[i as usize]).collect())),
    )
}

/// Terrain height (UE units) below a point, from the component height grids.
struct HeightField(Vec<upk::terrain::TerrainGrid>);

impl HeightField {
    fn height(&self, x: f32, y: f32) -> Option<f32> {
        for g in &self.0 {
            if g.nx < 2 || g.ny < 2 {
                continue;
            }
            let p0 = g.positions[0];
            let dx = g.positions[1][0] - p0[0];
            let dy = g.positions[g.nx][1] - p0[1];
            let fx = (x - p0[0]) / dx;
            let fy = (y - p0[1]) / dy;
            if fx < 0.0 || fy < 0.0 || fx > (g.nx - 1) as f32 || fy > (g.ny - 1) as f32 {
                continue;
            }
            let (ix, iy) = ((fx.floor() as usize).min(g.nx - 2), (fy.floor() as usize).min(g.ny - 2));
            let (tx, ty) = (fx - ix as f32, fy - iy as f32);
            let z = |i: usize, j: usize| g.positions[j * g.nx + i][2];
            let a = z(ix, iy) * (1.0 - tx) + z(ix + 1, iy) * tx;
            let b = z(ix, iy + 1) * (1.0 - tx) + z(ix + 1, iy + 1) * tx;
            return Some(a * (1.0 - ty) + b * ty);
        }
        None
    }
}

/// UE3 lights are linear multipliers: lit = albedo * LightColor * Brightness * NdotL.
/// Bevy (0.16 source): lit = exposure * albedo * (1/pi) * Burley * color * illuminance * NdotL,
/// exposure = 2^-ev100 / 1.2. With ev100 = 0 the camera exposure is 1/1.2, so
/// illuminance = Brightness * pi * 1.2 reproduces UE3's value (Burley's roughness term aside).
fn spawn_lighting(commands: &mut Commands, suns: &[upk::level::Sun], shadow: &sunshadow::SunShadow) {
    let sun = suns.first();
    let dir = sun.map(|s| rotator_to_quat(s.rotation) * Vec3::X).unwrap_or(Vec3::NEG_Y).normalize();
    let (color, lux) = sun
        .map(|s| (Color::srgb_u8(s.color[0], s.color[1], s.color[2]), s.brightness * std::f32::consts::PI * 1.2))
        .unwrap_or((Color::WHITE, std::f32::consts::PI * 1.2));
    // shadow maps: Homefront's cascades (sunshadow.rs overrides Bevy's fit)
    commands.spawn((LevelEntity, 
        DirectionalLight { illuminance: lux, color, shadows_enabled: true, shadow_depth_bias: 0.0, shadow_normal_bias: 0.0, ..default() },
        Transform::IDENTITY.looking_to(dir, if dir.y.abs() > 0.99 { Vec3::X } else { Vec3::Y }),
        bevy::pbr::CascadeShadowConfig {
            bounds: vec![(shadow.split1 + shadow.fade * 0.5) * 0.01, (shadow.split2 + shadow.fade * 0.5) * 0.01, shadow.level_radius * 0.02],
            overlap_proportion: 0.0,
            minimum_distance: 0.1,
        },
    ));
    info!("sun brightness {:?}", sun.map(|s| s.brightness));
}

/// UE3 SkyLights add up: upper = sum(LightColor * Brightness), lower = sum(LowerColor * LowerBrightness).
fn sky_terms(skies: &[upk::level::SkyLight]) -> (Vec4, Vec4) {
    let lin = |c: [u8; 4]| Vec3::new(srgb_to_lin(c[0]), srgb_to_lin(c[1]), srgb_to_lin(c[2]));
    let mut up = Vec3::ZERO;
    let mut lo = Vec3::ZERO;
    for k in skies {
        up += lin(k.color) * k.brightness;
        lo += lin(k.lower_color) * k.lower_brightness;
    }
    (up.extend(0.0), lo.extend(0.0))
}

/// StandardMaterial from a UE material: the material's node graph is evaluated (with the
/// instance's parameters and static switches) and baked into the base colour texture.
fn make_material(
    pkg: &Package,
    lib: &Library,
    material: i32,
    args: &Args,
    images: &mut Assets<Image>,
    cache: &mut upk::material::TexCache,
) -> StandardMaterial {
    use upk::level::Blend;
    let Some(baked) = pkg.bake_material(lib, material, args.max_tex, cache) else {
        return StandardMaterial { base_color: Color::srgb(0.5, 0.5, 0.5), ..default() };
    };
    let info = &baked.info;
    let emissive = baked.emissive.as_ref().map(|(px, scale)| {
        let mut img = Image::new(
            Extent3d { width: baked.width, height: baked.height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            px.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
        (images.add(img), *scale)
    });
    let two_sided = args.no_cull || info.two_sided || info.blend != Blend::Opaque;
    let mut img = Image::new(
        Extent3d { width: baked.width, height: baked.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        baked.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    StandardMaterial {
        base_color_texture: Some(images.add(img)),
        perceptual_roughness: 0.85,
        // Unlit UE materials output EmissiveColor directly; modulate/additive overlays too.
        unlit: info.unlit || matches!(info.blend, Blend::Modulate | Blend::Additive),
        alpha_mode: match info.blend {
            Blend::Opaque => AlphaMode::Opaque,
            Blend::Masked => AlphaMode::Mask(info.mask_clip),
            Blend::Translucent => AlphaMode::Blend,
            Blend::Additive => AlphaMode::Add,
            Blend::Modulate => AlphaMode::Multiply,
        },
        cull_mode: if two_sided { None } else { Some(Face::Back) },
        double_sided: two_sided,
        // UE3 emissive is added in the same units as lighting: scale by 1.2 and let
        // it follow the camera exposure (weight 1), like the lit part.
        emissive: emissive.as_ref().map(|(_, k)| LinearRgba::rgb(k * 1.2, k * 1.2, k * 1.2)).unwrap_or(LinearRgba::BLACK),
        emissive_texture: emissive.map(|(h, _)| h),
        emissive_exposure_weight: 1.0,
        ..default()
    }
}

fn grid_bounds(g: &upk::terrain::TerrainGrid) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MIN, f32::MAX, f32::MIN];
    for p in &g.positions {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].max(p[0]);
        b[2] = b[2].min(p[1]);
        b[3] = b[3].max(p[1]);
    }
    b
}

fn srgb_to_lin(b: u8) -> f32 {
    (b as f32 / 255.0).powf(2.2)
}

/// Grid mesh for one terrain component (vertices in UE units, scaled by the entity).
/// Map packages read and decompressed in parallel. Two cores stay free (the movie and the
/// renderer run meanwhile when this is the background pre-read).
fn open_parallel(maps: &[PathBuf]) -> Vec<(PathBuf, Package)> {
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(2).clamp(1, 8);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<(usize, PathBuf, Package)> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(m) = maps.get(k) else { break };
                        match Package::open(m) {
                            Ok(p) => mine.push((k, m.clone(), p)),
                            Err(e) => warn!("{}: {e}", m.display()),
                        }
                    }
                    mine
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    out.sort_by_key(|x| x.0);
    out.into_iter().map(|(_, m, p)| (m, p)).collect()
}

type Preopen = (Vec<PathBuf>, std::thread::JoinHandle<Vec<(PathBuf, Package)>>);
/// Game folder and --max-tex for the background preload (set in main).
static GAME_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static MAX_TEX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1024);
static PREOPEN: std::sync::Mutex<Option<Preopen>> = std::sync::Mutex::new(None);

/// Start reading a match level's packages in the background (menu.rs, when the match is
/// created), so the frame that builds the level does not also decompress them.
pub fn preopen(maps: Vec<PathBuf>) {
    let m2 = maps.clone();
    let h = std::thread::Builder::new().name("preopen".into()).stack_size(64 << 20).spawn(move || {
        let t = std::time::Instant::now();
        let opened = open_parallel(&m2);
        let t_open = t.elapsed().as_secs_f32();
        // the textures' separate-file mips (read + LZO decompress, 150 MB for Crossroads) while
        // the popup and the movie run: the level build then takes them from memory
        // the list of the reads the last build of this level made (exactly the mips the build
        // takes, also from packages outside the level such as weapons, vehicles, effects)
        let manifest = prefetch_manifest(&m2);
        if let Some(jobs) = upk::library::load_manifest(&manifest).filter(|j| !j.is_empty()) {
            let n = jobs.len();
            upk::library::prefetch_jobs(jobs);
            info!("preload: {} level packages in {t_open:.2} s, {n} texture mips prefetched (list of the last build) by {:.2} s", opened.len(), t.elapsed().as_secs_f32());
        } else if let Some(lib) = GAME_ROOT.get().and_then(|r| upk::library::Library::scan(r).ok()) {
            let max = MAX_TEX.load(std::sync::atomic::Ordering::Relaxed);
            let bc = gamemat::BC_SUPPORTED.load(std::sync::atomic::Ordering::Relaxed);
            let mut items = Vec::new();
            for (_, p) in &opened {
                for (i, e) in p.exports.iter().enumerate() {
                    if p.export_class(e) != "Texture2D" {
                        continue;
                    }
                    let Some(tx) = p.read_texture2d(i) else { continue };
                    let src = p.source_package(i);
                    // the build reads the whole chain below the cap only for DXT textures it
                    // uploads compressed (gamemat bc_image), else just the top mip (load_full)
                    let chain = bc && matches!(tx.format.as_str(), "PF_DXT1" | "PF_DXT3" | "PF_DXT5");
                    let top = tx.mips.iter().position(|m| m.width <= max && m.height <= max);
                    for (k, m) in tx.mips.iter().enumerate() {
                        if !chain && Some(k) != top {
                            continue;
                        }
                        if m.width <= max && m.height <= max && m.bulk.data.is_empty()
                            && m.bulk.flags & upk::texture::BULK_SEPARATE_FILE != 0
                            // unused mips carry offset/size 0xFFFFFFFF (flags 0x21): a 4 GB buffer
                            // per worker aborted the process on Windows (user crash 2026-10-18)
                            && m.bulk.flags & upk::texture::BULK_UNUSED == 0
                        {
                            items.push((src.clone(), upk::texture::BulkData { data: Vec::new(), ..m.bulk.clone() }));
                        }
                    }
                }
            }
            let n = items.len();
            lib.prefetch(items);
            info!("preload: {} level packages in {t_open:.2} s, {n} texture mips prefetched by {:.2} s", opened.len(), t.elapsed().as_secs_f32());
        }
        opened
    });
    let Ok(h) = h else { return };
    if let Ok(mut g) = PREOPEN.lock() {
        *g = Some((maps, h));
    }
}

/// Prefetch list of a match level: the separate-file mip reads its last build made (per map
/// set, texture cap and BC upload path; temp folder, rewritten after every build).
fn prefetch_manifest(maps: &[PathBuf]) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    maps.hash(&mut h);
    MAX_TEX.load(std::sync::atomic::Ordering::Relaxed).hash(&mut h);
    gamemat::BC_SUPPORTED.load(std::sync::atomic::Ordering::Relaxed).hash(&mut h);
    std::env::temp_dir().join("homebots_prefetch").join(format!("{:016x}.txt", h.finish()))
}

/// The background read for these maps is done (or none is running).
pub fn preopen_ready(maps: &[PathBuf]) -> bool {
    PREOPEN.lock().map(|g| g.as_ref().is_none_or(|(m, h)| m.as_slice() != maps || h.is_finished())).unwrap_or(true)
}

fn take_preopened(maps: &[PathBuf]) -> Option<Vec<(PathBuf, Package)>> {
    let (m, h) = PREOPEN.lock().ok()?.take()?;
    if m.as_slice() != maps {
        return None;
    }
    h.join().ok()
}

fn terrain_mesh(g: &upk::terrain::TerrainGrid) -> Mesh {
    let pos: Vec<[f32; 3]> = g.positions.iter().map(|p| ue_to_bevy(*p).to_array()).collect();
    // UVs span the component; its texture is composed from the blended terrain layers.
    let b = grid_bounds(g);
    let uv: Vec<[f32; 2]> =
        g.positions.iter().map(|p| [(p[0] - b[0]) / (b[1] - b[0]).max(1.0), (p[1] - b[2]) / (b[3] - b[2]).max(1.0)]).collect();
    let mut idx = Vec::with_capacity((g.nx - 1) * (g.ny - 1) * 6);
    for j in 0..g.ny - 1 {
        for i in 0..g.nx - 1 {
            let a = (j * g.nx + i) as u32;
            let (b, c, d) = (a + 1, a + g.nx as u32, a + g.nx as u32 + 1);
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx));
    m.compute_smooth_normals();
    m
}

#[allow(clippy::too_many_arguments)]
fn load_maps(
    mut commands: Commands,
    mut args: NonSendMut<Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HfMat>>,
    mut images: ResMut<Assets<Image>>,
    mut game_mats: ResMut<Assets<gamemat::GameMat>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut buffers: ResMut<Assets<bevy::render::storage::ShaderStorageBuffer>>,
    mut terrain_mats: ResMut<Assets<terrainemu::TerrainEmu>>,
    mut grass_mats: ResMut<Assets<grass::GrassMat>>,
    mut tree_wind: ResMut<treewind::TreeWind>,
    mut sun_shadow: ResMut<sunshadow::SunShadow>,
    mut particle_world: ResMut<particles::Particles>,
    scene_copy: Res<scenecopy::SceneCopy>,
    mut col: ResMut<player::CollisionWorld>,
    (request, old, mut cams, in_menu, mut spawn_state, mut levels, mut vis, mut loader, mut mm_pending): (
        Res<menu::LevelRequest>,
        Query<Entity, With<LevelEntity>>,
        Query<(Entity, &mut Transform, &mut FlyCam)>,
        Res<menu::InMenu>,
        ResMut<LevelSpawn>,
        ResMut<Levels>,
        Query<(&LevelGroup, &mut Visibility)>,
        Local<Option<(Library, Option<upk::shadercache::ShaderCache>)>>,
        ResMut<minimap::Pending>,
    ),
) {
    // texture size cap: the options' texture quality (settings.rs), for levels loaded from now on
    if !args.max_tex_cli {
        let t = settings::max_tex();
        if args.max_tex != t {
            info!("level: texture cap {t} (texture quality)");
            args.max_tex = t;
            MAX_TEX.store(t, std::sync::atomic::Ordering::Relaxed);
        }
    }
    let maps: Vec<PathBuf> = request.maps.clone();
    let (keep, show) = (request.cache, request.show);
    commands.remove_resource::<menu::LevelRequest>();
    levels.loads += 1;
    // already loaded (front end): just switch
    if let Some(c) = levels.cached.iter().find(|c| c.key == maps) {
        if show {
            for (g, mut v) in &mut vis {
                *v = if g.0 == c.group { Visibility::Inherited } else { Visibility::Hidden };
            }
            *sun_shadow = c.shadow;
            commands.insert_resource(PostVolumes::default());
            if let Some((e, ..)) = cams.iter().next() {
                if std::env::var_os("HOMEBOTS_NO_POST").is_none() {
                    commands.entity(e).insert(c.post);
                }
            }
        }
        return;
    }
    if keep {
        if show {
            for (_, mut v) in &mut vis {
                *v = Visibility::Hidden;
            }
        }
    } else {
        for e in &old {
            commands.entity(e).despawn();
        }
        mm_pending.objects.clear();
        mm_pending.dir = maps.first().and_then(|m| m.parent()).map(|d| d.to_path_buf());
        mm_pending.ready = true;
        col.clear();
        levels.cached.clear();
        tree_wind.winds.clear();
        tree_wind.objects.clear();
        tree_wind.buffer = None;
        particle_world.systems.clear();
        particle_world.pools.clear();
    }
    let group = levels.next;
    levels.next += 1;
    spawn_state.group = group;
    spawn_state.hidden = !show;
    let shown_shadow = *sun_shadow;
    *sun_shadow = sunshadow::SunShadow::default();
    let post_settings = post_settings(&maps, request.persistent.as_ref());
    // every PostProcessVolume of the level with its bounds: the one the camera is in is used
    // (post_volume_select); Bridge has an under-water volume with a higher Priority
    let post_volumes: Vec<upk::level::PpVolume> = request.persistent.iter().chain(maps.iter()).filter_map(|m| Package::open(m).ok()).flat_map(|p| p.post_process_volumes()).collect();
    info!("level: {:?}, post-process: {:?}", maps.iter().map(|m| m.file_name().unwrap_or_default().to_string_lossy().to_string()).collect::<Vec<_>>(), post_settings);
    let mut wind_index: std::collections::HashMap<(usize, usize), Option<usize>> = std::collections::HashMap::new();
    // package index and shader cache are loaded once
    if loader.is_none() {
        let lib = match Library::scan(&args.game) {
            Ok(l) => l,
            Err(e) => {
                error!("Homefront not found at {}: {e}", args.game.display());
                return;
            }
        };
        info!("{} packages indexed", lib.files.len());
        let game_shaders = lib
            .path_of("RefShaderCache-PC-D3D-SM3")
            .cloned()
            .or_else(|| Some(args.game.join("GCGame").join("CookedPC").join("RefShaderCache-PC-D3D-SM3.upk")))
            .and_then(|p| match upk::shadercache::ShaderCache::load(&p) {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!("shader cache {}: {e}", p.display());
                    None
                }
            });
        *loader = Some((lib, game_shaders));
    }
    let (lib, game_shaders) = loader.as_ref().unwrap();
    let (mut placed, mut skipped) = (0usize, 0usize);
    // Lights first: every material needs the level's sky light terms.
    // every map package is read and decompressed once, on all cores (it used to be opened
    // once per pass)
    let t_open = std::time::Instant::now();
    let opened: Vec<(PathBuf, Package)> = take_preopened(&maps).unwrap_or_else(|| open_parallel(&maps));
    info!("{} map packages opened in {:.2} s", opened.len(), t_open.elapsed().as_secs_f32());
    // match level: the build's separate-file reads become the next prefetch list (preopen)
    if !keep {
        upk::library::start_recording();
    }
    // load phase timing (user: the original's loading screen takes 21 s)
    let t_load = std::time::Instant::now();
    let _ = upk::library::take_read_stats();
    let lap = |what: &str| {
        let (secs, n, bytes, hits) = upk::library::take_read_stats();
        info!("load phase: {what} at {:.2} s (bulk reads {n}, {:.1} MB, {secs:.2} s, {hits} prefetched)", t_load.elapsed().as_secs_f32(), bytes as f64 / 1e6);
    };
    // objectives and full-map bounds from the open packages (objectives.rs / minimap.rs)
    if !keep {
        let objectives = objectives::load_pkgs(opened.iter().map(|x| &x.1));
        let minimap_bounds = objectives::minimap_bounds(opened.iter().map(|x| &x.1));
        let frontlines = objectives::load_frontlines(opened.iter().map(|x| &x.1));
        info!("front lines: {} ({:?} USA / KVA infantry starts)", frontlines.len(), frontlines.iter().map(|f| f.spawns.iter().map(|t| t.iter().filter(|s| s.0 == "GMST_Infantry").map(|s| s.1.len()).sum::<usize>()).collect::<Vec<_>>()).collect::<Vec<_>>());
        commands.insert_resource(objectives::LevelData { maps: maps.clone(), objectives, minimap_bounds, frontlines });
    }
    let mut suns: Vec<upk::level::Sun> = Vec::new();
    let mut skies: Vec<upk::level::SkyLight> = Vec::new();
    for (_, p) in &opened {
        suns.extend(p.sun());
        skies.extend(p.sky_lights());
    }
    let (sky_upper, sky_lower) = sky_terms(&skies);
    let base_params = HfParams { sky_upper, sky_lower, ..default() };
    info!("sky light upper {sky_upper:?} lower {sky_lower:?}");

    // Game-shader globals (see gamemat.rs): sky lights affecting world geometry
    // (LightingChannels BSP/Static/Dynamic), the sun, terrain bounds and grass colour.
    let mut g = gamemat::Globals::default();
    let mut falloff = 0.0f32;
    for sk in skies.iter().filter(|s| s.affects_world) {
        let u = gamemat::ue_color(sk.color, sk.brightness);
        let l = gamemat::ue_color(sk.lower_color, sk.lower_brightness);
        for k in 0..3 {
            g.upper_sky[k] += u[k];
            g.lower_sky[k] += l[k];
        }
        g.lower_sky[3] = sk.lower_terrain_brightness;
        falloff = sk.terrain_falloff;
    }
    g.upper_sky[3] = 1.0;
    g.ambient_sky = [0.0, 0.0, 0.0, 1.0];
    g.misc[0] = if falloff > 0.0 { 1.0 / falloff } else { 0.0 };
    if let Some(sun) = suns.first() {
        let c = gamemat::ue_color(sun.color, sun.brightness);
        g.light_color = [c[0], c[1], c[2], 0.0];
        // direction towards the light, UE world
        let d = rotator_to_quat(sun.rotation) * Vec3::X; // Bevy axes, direction the light travels
        let to_light = -Vec3::new(d.x, d.z, d.y);
        g.light_dir = [to_light.x, to_light.y, to_light.z, if to_light.z.abs() > 1e-4 { 1.0 / to_light.z } else { 0.0 }];
    }
    // Sun shadows (DirectionalLightComponent full-screen / full-level shadow settings)
    let mut filter = (2.5f32, 4.0f32);
    for (_, p) in &opened {
        if let Some(i) = p.exports.iter().position(|e| p.export_class(e) == "DirectionalLightComponent") {
            if let Some(cp) = p.export_props(i) {
                let s1 = cp.float("FullScreenShadowCullDistance").unwrap_or(sun_shadow.split1);
                sun_shadow.split1 = s1;
                sun_shadow.split2 = s1 * cp.float("FSShadowDistanceRatio").unwrap_or(2.25);
                sun_shadow.fade = cp.float("ShadowFadeOutDistance").unwrap_or(sun_shadow.fade);
                filter = (cp.float("FullScreenShadowFilterRadius").unwrap_or(filter.0), cp.float("FullLevelShadowFilterRadius").unwrap_or(filter.1));
                break;
            }
        }
    }
    let shadow_wgsl = sunshadow::wgsl(&sun_shadow, filter.0, filter.1);
    if let Ok(mut g) = gamemat::SHADOW_WGSL.write() {
        *g = shadow_wgsl.clone();
    }
    info!("sun shadow: splits {} / {}, fade {}, filter {:?}", sun_shadow.split1, sun_shadow.split2, sun_shadow.fade, filter);
    let mut heightfield = HeightField(Vec::new());
    let mut grass: Option<Handle<Image>> = None;
    for (_, p) in &opened {
        for t in p.terrains() {
            heightfield.0.extend(t.grids);
        }
        if let Some(tl) = p.terrain_layers() {
            let b = tl.bounds;
            g.terrain_bounds = [b[0], 1.0 / (b[1] - b[0]), b[2], 1.0 / (b[3] - b[2])];
        }
        if grass.is_none() {
            if let Some(ti) = p.exports.iter().position(|e| p.export_class(e) == "Terrain") {
                if let Some(gt) = p.export_props(ti).and_then(|pr| pr.object("GrassColorTexture")) {
                    if let Some(img) = gamemat::load_texture(&p, &lib, gt, [upk::matcompile::Address::Clamp; 2], 4096) {
                        grass = Some(images.add(img));
                    }
                }
            }
        }
    }
    // Dynamic terrain grass (Terrain.TerrainGrassLayers, see grass.rs)
    let grass_shader = shaders.add(Shader::from_wgsl(
        grass::SHADER
            .replace("MINZ", &format!("{:?}", 100.0 * scenecopy::NEAR * scenecopy::DEPTH_SCALE))
            .replace("struct GrassParams", &format!("{shadow_wgsl}\nstruct GrassParams")),
        "hf_grass.wgsl",
    ));
    for (map, p) in &opened {
        let Some(tg) = p.terrain_grass() else { continue };
        let overlay = gamemat::load_texture(&p, &lib, tg.color_texture, [upk::matcompile::Address::Wrap; 2], args.max_tex.max(1024))
            .map(|img| images.add(grass::single_mip(img)));
        let mut n = 0;
        for layer in &tg.layers {
            for (si, bb) in layer.billboards.iter().enumerate() {
                let Some(mesh) = grass::build_mesh(layer, si) else { continue };
                let billboard = gamemat::load_texture(&p, &lib, bb.texture, [upk::matcompile::Address::Wrap; 2], args.max_tex).map(|i| images.add(i));
                let mat = grass_mats.add(grass::GrassMat {
                    params: grass::params(bb, tg.bounds),
                    billboard,
                    overlay: overlay.clone(),
                    scene: Some(scene_copy.image.clone()),
                    shader: grass_shader.clone(),
                });
                grass::spawn(&mut commands, meshes.add(mesh), mat);
                n += layer.instances.len() * bb.num_per_cell as usize;
            }
        }
        info!("{}: grass {} layers, {n} billboards", map.display(), tg.layers.len());
    }
    info!("game globals {g:?}");
    let globals_handle = buffers.add(gamemat::globals_buffer(&g, &[]));
    // All map packages up front: sublevels import objects exported by the persistent level.
    let mut pkgs: Vec<Package> = Vec::new();
    let mut pkg_names: Vec<String> = Vec::new();
    for (map, p) in opened {
        pkgs.push(p);
        pkg_names.push(map.file_name().unwrap_or_default().to_string_lossy().to_string());
    }
    // *_FX sublevels: only their particle emitters are used (their static meshes are fake
    // light-shaft sprites); appended after the world packages so materials resolve normally
    let n_world = pkgs.len();
    if let Some(dir) = maps.first().and_then(|m| m.parent()) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut fx: Vec<std::path::PathBuf> = rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| f.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase().ends_with("_fx.map")).unwrap_or(false))
                .collect();
            fx.sort();
            for f in fx {
                if let Ok(p) = Package::open(&f) {
                    pkgs.push(p);
                    pkg_names.push(f.file_name().unwrap_or_default().to_string_lossy().to_string());
                }
            }
        }
    }
    // first-person weapons (weaponcfg.rs / viewmodel.rs): the default loadouts' primary
    // weapons with their scopes, the US arms; packages opened before the import scan
    let mut vm_alias: Vec<(usize, String)> = Vec::new();
    let mut vm_weapons: Vec<weaponcfg::WeaponCfg> = Vec::new();
    let mut combat_data: Option<combat::CombatData> = None;
    let mut tactical_fx: Vec<String> = Vec::new();
    let mut impact_fx: Vec<String> = Vec::new();
    // pool sizes above the default for effects that play many at once (airstrike bomblets)
    let mut fx_counts: Vec<(String, usize)> = Vec::new();
    // the airstrikes' effects (trails, payload release, bomblet explosions), pooled at level load
    let mut strike_fx: Vec<String> = Vec::new();
    if !keep {
        let cooked = args.game.join("GCGame").join("CookedPC");
        let loadouts = weaponcfg::default_loadouts(&cooked);
        if let Some(wc) = lib.open("WeaponConfigs") {
            // primaries first (index = loadout weapon), then the loadouts' launchers (CFG_ATW_*)
            for lo in &loadouts {
                if vm_weapons.iter().any(|w| w.cfg.eq_ignore_ascii_case(&lo.primary)) {
                    continue;
                }
                match weaponcfg::read_weapon(&wc, &lo.primary, lo.scope.as_deref()) {
                    Some(w) => vm_weapons.push(w),
                    None => warn!("weapon config {} not readable", lo.primary),
                }
            }
            let content = lib.open("GCGameContent");
            let gcgame_script = lib.open("GCGame");
            for lo in &loadouts {
                for it in &lo.items {
                    // launchers (CFG_ATW_*) and the sidearm (CFG_PST_*, inventory group 2, key two = EKBC_Weapon2)
                    let l = it.to_ascii_lowercase();
                    if !(l.starts_with("cfg_atw_") || l.starts_with("cfg_pst_")) || vm_weapons.iter().any(|w| w.cfg.eq_ignore_ascii_case(it)) {
                        continue;
                    }
                    if let Some(mut w) = weaponcfg::read_weapon(&wc, it, None) {
                        if let (Some(c), Some(pc)) = (content.as_ref(), w.projectile.as_mut()) {
                            if let Some(g) = gcgame_script.as_ref() {
                                weaponcfg::read_projectile_script(c, g, pc);
                            }
                            weaponcfg::read_projectile(c, pc);
                        }
                        // the PROX launcher has no LockOn in the data (a proximity-fuse rocket,
                        // GCProj_ATW_PROX_C bSeekTarget false): it gets the AAWS-M's
                        // (CFG_ATW_JAV) lock-on. NOT from data (user request 2026-10-21)
                        if w.lock_on.is_none() && l == "cfg_atw_prox" {
                            w.lock_on = weaponcfg::read_lock_on(&wc, "CFG_ATW_JAV");
                        }
                        if let (Some(lo), Some(pj)) = (w.lock_on.as_ref(), w.projectile.as_ref()) {
                            info!("lock-on {}: from {} box {:?} time {} grace {} / {} deg dist {}..{} unzoomed {} classes {:?}; seek {} delay {} interp {} lose dot {} proximity {}", w.cfg, lo.from, lo.big, lo.lock_time, lo.grace_time, lo.grace_angle, lo.min_dist, lo.max_dist, lo.allow_unzoomed, lo.classes, pj.seek, pj.seek_delay, pj.seek_interp, pj.seek_lose_dot, pj.prox_vehicle_range);
                        }
                        vm_weapons.push(w);
                    }
                }
            }
            // C4 loadouts: the detonator (CFG_EXP_C4Detonator) is a weapon of its own, drawn
            // after a charge is thrown
            for lo in &loadouts {
                let Some(det) = lo.items.get(2).and_then(|it| weaponcfg::c4_detonator(&wc, it)) else { continue };
                if vm_weapons.iter().any(|w| w.cfg.eq_ignore_ascii_case(&det)) {
                    continue;
                }
                match weaponcfg::read_weapon(&wc, &det, None) {
                    Some(w) => vm_weapons.push(w),
                    None => warn!("weapon config {det} not readable"),
                }
            }
            if let Some(nodes) = lib.open("KFX_WeaponNodes") {
                for w in vm_weapons.iter_mut() {
                    if let Some(e) = w.effects.clone() {
                        w.muzzle = weaponcfg::muzzle_flashes(&nodes, &e);
                        w.tracer = weaponcfg::tracers(&nodes, &e);
                    }
                }
            }
            let mut grid = weaponcfg::DamageGrid::load(&wc).unwrap_or_default();
            if let Some(g) = lib.open("GCGame") {
                let mut ps: Vec<&Package> = vec![&g];
                if let Some(c) = content.as_ref() {
                    ps.push(c);
                }
                grid.add_aliases(&ps);
                info!("damage grid: {} types, {} aliases (e.g. GCDmgType_LAV_Seat0_Bullet -> {})", grid.types.len(), grid.alias.len(), grid.row_name("GCDmgType_LAV_Seat0_Bullet"));
            }
            let grenade = lib.open("GCGame").map(|g| weaponcfg::read_grenade(&g, content.as_ref())).unwrap_or_default();
            let (costs, flak_scale) = weaponcfg::special_costs(&cooked);
            let special_names = weaponcfg::special_names(&cooked, &menu::language(&args.game), &costs.keys().cloned().collect::<Vec<_>>());
            let mut small_icons = std::collections::HashMap::new();
            for l in &loadouts {
                for it in &l.items {
                    if let Some(icon) = weaponcfg::small_icon(&wc, it) {
                        small_icons.insert(it.to_ascii_lowercase(), icon);
                    }
                }
            }
            // killstreak items: UAV sweep, Hellfire airstrike (missile cfg appended to the weapons)
            let gcgame = lib.open("GCGame");
            if let Some(g) = gcgame.as_ref() {
                commands.insert_resource(dynlights::ExplosionLights::load(g));
            }
            let uav_secs = gcgame.as_ref().map(weaponcfg::uav_duration).unwrap_or(30.0);
            let goggles = gcgame.as_ref().map(|g| weaponcfg::thermal_goggles(g, content.as_ref())).unwrap_or((180.0, String::new(), String::new()));
            let mut weapons = vm_weapons.clone();
            // airstrikes (Hellfire, cluster, white phosphorus, sensor fused): the bomb / missile and
            // the bomblets as weapon entries for the projectile code
            let mut strikes = Vec::new();
            let uav = gcgame.as_ref().and_then(|g| g.exports.iter().position(|e| g.object_name_of(e) == "Default__GCV_AirstrikeUAV").and_then(|i| g.export_props(i)));
            let orbit = [
                uav.as_ref().and_then(|u| u.float("FlyAltitude")).unwrap_or(5000.0),
                uav.as_ref().and_then(|u| u.float("FlyRadius")).unwrap_or(9000.0),
                uav.as_ref().and_then(|u| u.float("FlySpeed")).unwrap_or(0.02),
                uav.as_ref().and_then(|u| u.float("CenterPointDistanceToInstigator")).unwrap_or(1500.0),
            ];
            info!("airstrike UAV orbit (altitude, radius, speed, centre) {orbit:?}");
            for (key, mut h) in weaponcfg::read_strikes(&wc, content.as_ref(), lib.open("UCV_PredatorDrone").as_ref()) {
                h.orbit = orbit;
                let weapon = weapons.len();
                weapons.push(weaponcfg::WeaponCfg { cfg: key.to_uppercase(), projectile: Some(h.missile.clone()), ui_icon: h.ui_icon.clone(), ..Default::default() });
                let mut fxs = vec![h.missile.flight_fx.clone()];
                let sub = h.payload.as_ref().map(|p| {
                    fxs.extend([p.release_fx.clone(), p.bomblet.flight_fx.clone(), p.bomblet.explosion_fx.clone()]);
                    weapons.push(weaponcfg::WeaponCfg { cfg: format!("{}_bomblet", key.to_uppercase()), projectile: Some(p.bomblet.clone()), ui_icon: h.ui_icon.clone(), ..Default::default() });
                    weapons.len() - 1
                });
                for f in fxs.into_iter().flatten() {
                    if !strike_fx.contains(&f) {
                        strike_fx.push(f);
                    }
                }
                info!("airstrike {key}: payload {:?}", h.payload.as_ref().map(|p| (p.kind, p.count, p.spread)));
                let uav_class = match key.as_str() {
                    "cfg_bmb_cluster" => "GCV_AirstrikeUAV_Cluster_C",
                    "cfg_bmb_phosphorus" => "GCV_AirstrikeUAV_Phosphorus_C",
                    "cfg_bmb_sensorfused" => "GCV_AirstrikeUAV_SensorFused_C",
                    _ => "GCV_AirstrikeUAV_C",
                };
                let (engine, fire_sound) = content.as_ref().map(|c| vehicle_audio(&lib, c, uav_class)).unwrap_or_default();
                info!("airstrike {key}: {uav_class} engine {engine:?} fire {fire_sound:?}");
                strikes.push(combat::Strike { key, cfg: h, weapon, sub, engine, fire_sound });
            }
            // drones whose guns fire missiles (anti tank drone MP.Vehicle.CFG_GAntiTank, air assault
            // drone CFG_SH_AAssault): the projectile fire modes as weapon entries
            let mut drone_rockets = std::collections::HashMap::new();
            for key in DRONE_KINDS {
                let Some(gun) = content.as_ref().and_then(|c| weaponcfg::drone_classes(c, key)).and_then(|x| x.1) else { continue };
                let name = gun.rsplit('.').next().unwrap_or("").to_string();
                let mut modes = Vec::new();
                for (m, rm) in weaponcfg::read_rocket_modes(&wc, content.as_ref(), &name).into_iter().enumerate() {
                    weapons.push(weaponcfg::WeaponCfg { cfg: format!("{}_{m}", name.to_uppercase()), projectile: Some(rm.missile.clone()), ..Default::default() });
                    if let Some(f) = rm.missile.flight_fx.clone().filter(|f| !strike_fx.contains(f)) {
                        strike_fx.push(f);
                    }
                    info!("drone {key} {name} mode {m}: {} speed {} radius {} clip {} burst {} interval {} reload {} lock-on {}", rm.missile.class, rm.missile.speed, rm.missile.radius, rm.clip, rm.burst, rm.interval, rm.reload, rm.lock_on);
                    modes.push((rm, weapons.len() - 1));
                }
                if !modes.is_empty() {
                    drone_rockets.insert(key.to_string(), modes);
                }
            }
            // the vehicles' guns with projectile fire modes (tank shells, LAV rockets, helicopter
            // rockets / missiles): weapon entries per mode
            let mut vehicle_rockets = std::collections::HashMap::new();
            for class in vehload::all_classes() {
                for cfg in content.as_ref().map(|c| vehload::seat_weapons(c, class)).unwrap_or_default() {
                    if cfg.is_empty() || vehicle_rockets.contains_key(&cfg.to_ascii_lowercase()) {
                        continue;
                    }
                    let name = cfg.rsplit('.').next().unwrap_or("").to_string();
                    let mut modes = Vec::new();
                    for rm in weaponcfg::read_rocket_modes(&wc, content.as_ref(), &name) {
                        weapons.push(weaponcfg::WeaponCfg { cfg: format!("{}_{}", name.to_uppercase(), rm.mode), projectile: Some(rm.missile.clone()), ..Default::default() });
                        if let Some(f) = rm.missile.flight_fx.clone().filter(|f| !strike_fx.contains(f)) {
                            strike_fx.push(f);
                        }
                        info!("vehicle {class} {name} mode {}: {} speed {} radius {} clip {} burst {} interval {} reload {} lock-on {}", rm.mode, rm.missile.class, rm.missile.speed, rm.missile.radius, rm.clip, rm.burst, rm.interval, rm.reload, rm.lock_on);
                        modes.push((rm, weapons.len() - 1));
                    }
                    vehicle_rockets.insert(cfg.to_ascii_lowercase(), modes);
                }
            }
            let hellfire = strikes.iter().find(|s| s.key == "cfg_bmb_hellfire").map(|s| s.cfg.clone());
            let hellfire_weapon = strikes.iter().find(|s| s.key == "cfg_bmb_hellfire").map(|s| s.weapon);
            if hellfire.is_none() {
                warn!("hellfire airstrike config not readable");
            }
            let spawn_delay = weaponcfg::ini_f32(&cooked, "gcgame.gcgame.playerspawnmanagermp.serverspawndelaytime").unwrap_or(5.0);
            // tactical grenades of the loadouts (item 2: EMP / flash bang / C4)
            let mut tactical = std::collections::HashMap::new();
            if let (Some(g), Some(c)) = (gcgame.as_ref(), content.as_ref()) {
                for l in &loadouts {
                    if let Some(it) = l.items.get(2) {
                        if let Some(t) = weaponcfg::read_tactical(&wc, g, c, it) {
                            info!("tactical {it}: {t:?}");
                            tactical_fx.extend(t.trail.clone());
                            tactical_fx.push(combat::tactical_explosion_fx(t.kind).to_string());
                            tactical.insert(it.to_ascii_lowercase(), t);
                        }
                    }
                }
            }
            combat_data = Some(combat::CombatData { weapons, loadouts: loadouts.clone(), grid, grenade, costs, special_names, flak_scale, small_icons, uav_secs, goggles, hellfire, hellfire_weapon, strikes, drone_rockets, vehicle_rockets, spawn_delay, tactical });
        }
        // KFX_VHC: levelcars.rs burning wreck FX_LAV.Fire_WreckChassis (SP GMS_ExplodingCars) and the jiggle-car glass effects
        let mut need: Vec<String> = vec!["Projectiles".into(), "Quickthrows".into(), "US_1P".into(), "US_AST".into(), "CH_AST".into(), "CHC_MPAnimations".into(), "KFX_Weapons".into(), "KFX_Pyrotechnics".into(), "KFX_VHC".into(), "SD_M1114".into(), "UCV_GroundDrones".into(), "UCV_AirDrones".into(), "Interior_UCV_GAntiTank".into(), "Interior_UCV_AAssault".into(), "KFX_Electrical".into(), "KFX_Impacts".into(), "CH_3P".into(), "MAT_RD".into()];
        // projectile explosion templates (ProjExplosionTemplate: KFX_VHC for the PROX rocket)
        if let Some(cd) = combat_data.as_ref() {
            for f in cd.weapons.iter().filter_map(|w| w.projectile.as_ref().and_then(|p| p.explosion_fx.clone())) {
                let pk = f.split('.').next().unwrap_or("").to_string();
                if !pk.is_empty() && !need.iter().any(|n| n.eq_ignore_ascii_case(&pk)) {
                    need.push(pk);
                }
                if !strike_fx.contains(&f) {
                    strike_fx.push(f);
                }
            }
        }
        for w in &vm_weapons {
            let mut paths = vec![w.sk_mesh.clone(), w.anim_set.clone()];
            paths.extend(w.parts.iter().cloned());
            paths.extend(w.scope_rail.iter().cloned());
            paths.extend(w.tracer.iter().flatten().map(|t| t.template.clone()));
            if let Some(sc) = &w.scope {
                paths.extend(sc.mesh.iter().cloned());
                paths.extend(sc.base.iter().cloned());
            }
            for pth in paths {
                let pk = pth.split('.').next().unwrap_or("").to_string();
                if !pk.is_empty() && !need.iter().any(|n| n.eq_ignore_ascii_case(&pk)) {
                    need.push(pk);
                }
            }
        }
        for pk in need {
            if pkg_names.iter().any(|n| n.rsplit_once('.').map(|x| x.0).unwrap_or(n).eq_ignore_ascii_case(&pk)) {
                continue;
            }
            if let Some(p) = lib.open(&pk) {
                vm_alias.push((pkgs.len(), pk.to_ascii_lowercase()));
                pkgs.push(p);
                pkg_names.push(format!("{pk}.upk"));
            }
        }
        info!("viewmodel: {} weapons ({})", vm_weapons.len(), vm_weapons.iter().map(|w| w.cfg.as_str()).collect::<Vec<_>>().join(", "));
    }
    let mut import_pkgs: Vec<(usize, String)> = vm_alias.clone();
    // Textures / materials imported from packages that no map exports (always-loaded content
    // such as KFX_Weapons for particle materials): open those packages from the install.
    {
        let mut exported: std::collections::HashSet<String> = std::collections::HashSet::new();
        for p in &pkgs {
            for i in 0..p.exports.len() {
                exported.insert(p.object_path(i as i32 + 1).to_ascii_lowercase());
            }
        }
        let loaded: std::collections::HashSet<String> = pkg_names.iter().map(|n| n.rsplit_once('.').map(|x| x.0).unwrap_or(n).to_ascii_lowercase()).collect();
        let mut need: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in &pkgs {
            for (i, im) in p.imports.iter().enumerate() {
                let class = p.name(im.class_name);
                if !matches!(class.as_str(), "Texture2D" | "Material" | "MaterialInstanceConstant") {
                    continue;
                }
                let path = p.object_path(-(i as i32) - 1);
                let pk = path.split('.').next().unwrap_or("").to_ascii_lowercase();
                if !exported.contains(&path.to_ascii_lowercase()) && !loaded.contains(&pk) && lib.path_of(&pk).is_some() {
                    need.insert(pk);
                }
            }
        }
        for pk in need {
            if let Some(p) = lib.open(&pk) {
                info!("import package {pk}");
                import_pkgs.push((pkgs.len(), pk.clone()));
                pkgs.push(p);
                pkg_names.push(format!("{pk}.upk"));
            }
        }
    }
    // movement collision of a match level: terrain grids + PlayerStarts (static meshes below)
    if !keep {
        // terrain triangles: the physical material is looked up per hit from the layer weights
        col.add_phys = impacts::TERRAIN;
        col.add_src = player::SRC_TERRAIN;
        for g in &heightfield.0 {
            if g.nx < 2 || g.ny < 2 {
                continue;
            }
            let v = |i: usize, j: usize| ue_to_bevy(g.positions[j * g.nx + i]) * UU_TO_M;
            let z = |i: usize, j: usize| g.positions[j * g.nx + i][2];
            for j in 0..g.ny - 1 {
                for i in 0..g.nx - 1 {
                    // the rendered quad diagonal (terrainemu::component_meshes: the shorter one in
                    // height), so the feet stand on the drawn surface
                    if (z(i, j) - z(i + 1, j + 1)).abs() < (z(i + 1, j) - z(i, j + 1)).abs() {
                        col.add([v(i, j), v(i + 1, j), v(i + 1, j + 1)]);
                        col.add([v(i, j), v(i + 1, j + 1), v(i, j + 1)]);
                    } else {
                        col.add([v(i, j), v(i + 1, j), v(i, j + 1)]);
                        col.add([v(i + 1, j), v(i + 1, j + 1), v(i, j + 1)]);
                    }
                }
            }
        }
        col.add_phys = 0;
        col.add_src = player::SRC_OTHER;
        for p in pkgs.iter().take(n_world) {
            for (i, e) in p.exports.iter().enumerate() {
                if p.export_class(e) != "GameMode_PlayerStart" {
                    continue;
                }
                let Some(pr) = p.export_props(i) else { continue };
                let loc = match pr.get("Location") {
                    Some(upk::props::Value::Vector(v)) => *v,
                    _ => continue,
                };
                let yaw = match pr.get("Rotation") {
                    Some(upk::props::Value::Rotator(r)) => (r[1].rem_euclid(65536)) as f32 / 65536.0 * std::f32::consts::TAU,
                    _ => 0.0,
                };
                let mut group = match pr.get("Group") {
                    Some(upk::props::Value::Name(n)) => n.clone(),
                    _ => String::new(),
                };
                // vehicle starts with CameraTransition ECT_AirVehicle_* are the helicopter spawns
                // (Crossroads VehicleSpawnsHigh: 15 of 36, off the road): kept apart, the HUMVEE
                // uses the ground ones
                if pr.enum_name("CameraTransition").is_some_and(|c| c.contains("AirVehicle")) {
                    group.push_str("#air");
                }
                col.starts.push((group, ue_to_bevy(loc) * UU_TO_M, yaw));
            }
        }
    }
    let gs_globals = buffers.add(bevy::render::storage::ShaderStorageBuffer::new(&[0u8; 240], RenderAssetUsages::default()));
    let gs_objects = buffers.add(bevy::render::storage::ShaderStorageBuffer::new(&[0u8; 256], RenderAssetUsages::default()));
    let mut sp = world::Spawner::new(&pkgs, &lib, game_shaders.as_ref(), globals_handle.clone(), gs_globals.clone(), gs_objects.clone(), grass.clone(), args.max_tex, args.no_cull);
    // impact effects by physical material (impacts.rs); terrain layers' PhysMaterial
    let mut impacts = if keep { impacts::Impacts::default() } else { impacts::Impacts::load(&pkgs[..n_world], lib.open("GCGame").as_ref()) };
    // HB_IMPACTLIST=<damage types>: the effects of these damage types on every physical material
    if let Ok(v) = std::env::var("HB_IMPACTLIST") {
        let mut mats: Vec<String> = impacts.mats.keys().cloned().collect();
        mats.sort();
        for d in v.split(',') {
            for m in &mats {
                if let Some((ps, sc)) = impacts.effect(m, d) {
                    info!("IMPACTLIST {d} {m}: {ps:?} {sc:?}");
                }
            }
        }
    }
    if !keep {
        for pi in 0..n_world {
            if let Some(tl) = pkgs[pi].terrain_layers() {
                let mut phys = [0u16; 4];
                for (k, l) in tl.layers.iter().enumerate() {
                    if let Some(path) = l.and_then(|(m, _)| impacts::material_phys(&sp, pi, m)) {
                        phys[k] = impacts.id(&path);
                    }
                }
                info!("terrain physical materials {:?}", phys.iter().map(|&i| impacts.names.get((i as usize).wrapping_sub(1)).cloned().unwrap_or_default()).collect::<Vec<_>>());
                impacts.set_terrain(tl, phys);
                break;
            }
        }
        // pooled impact / explosion effects of the damage types in use
        if let Some(cd) = combat_data.as_ref() {
            let mut dmgs: Vec<String> = cd.weapons.iter().flat_map(|w| std::iter::once(w.dmg_type.clone()).chain(w.projectile.iter().map(|p| p.dmg_type.clone()))).collect();
            dmgs.push(cd.grenade.dmg_type.clone());
            dmgs.extend(cd.tactical.values().map(|t| t.dmg_type.clone()));
            // HUMVEE gun (MP.Vehicle.CFG_M1114_Seat1 InstantHitDamageType), ground drone gun
            dmgs.push("GCDmgType_M1114_Seat1_Bullet".into());
            dmgs.push("GCDmgType_GAssault_Bullet".into());
            dmgs.retain(|d| !d.is_empty());
            dmgs.sort();
            dmgs.dedup();
            impact_fx = impacts.systems_for(&dmgs);
            // airstrike bomblets: one instance per bomb of a strike (BombCount), up to 24
            for st in &cd.strikes {
                let (Some(p), Some(wi)) = (st.cfg.payload.as_ref(), st.sub) else { continue };
                let Some(pj) = cd.weapons.get(wi).and_then(|w| w.projectile.as_ref()) else { continue };
                let n = (p.count as usize).clamp(4, 24);
                let mut sys = impacts.systems_for(std::slice::from_ref(&pj.dmg_type));
                sys.extend(pj.explosion_fx.clone());
                for t in sys {
                    fx_counts.push((t, n));
                }
            }
            info!("impact effects: {} systems for {} damage types", impact_fx.len(), dmgs.len());
            if let Some(d) = impacts.default_exp().map(String::from) {
                for dm in &dmgs {
                    if let Some(ps) = impacts.effect_particles(&d, dm) {
                        info!("explosion {dm}: {ps:?}");
                    }
                    info!("damage grid {dm}: soldier {} armor {}", cd.grid.damage(dm, "PMA_Soldier", 0.0, 1.0, 1.0), cd.grid.damage(dm, "PMA_Armor", 0.0, 1.0, 1.0));
                    info!("damage grid {dm}: soldier {} armor {}", cd.grid.damage(dm, "PMA_Soldier", 0.0, 1.0, 1.0), cd.grid.damage(dm, "PMA_Armor", 0.0, 1.0, 1.0));
                }
            }
        }
    }
    for (pi, name) in &import_pkgs {
        sp.alias_package(*pi, name);
    }
    sp.upper_sky = g.upper_sky;
    sp.shadow_wgsl = shadow_wgsl.clone();
    sp.scene = Some(scene_copy.image.clone());
    sp.lower_sky = g.lower_sky;
    let mut center = Vec3::ZERO;
    // dev: HB_SKIP=static,skel,road (leave out whole kinds), HB_SKIPMESH=<path substring>
    let hb_skip = std::env::var("HB_SKIP").unwrap_or_default();
    let hb_skipmesh = std::env::var("HB_SKIPMESH").ok();
    for pi in 0..n_world {
        let pkg = &pkgs[pi];
        let mut texcache = upk::material::TexCache::new();
        let placements = pkg.static_mesh_placements();
        let mut no_pawn: std::collections::BTreeMap<String, u32> = Default::default();
        let mut pawn_col_cache: std::collections::HashMap<(usize, usize), Option<std::sync::Arc<Vec<[[f32; 3]; 3]>>>> = Default::default();
        for pl in &placements {
            let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
            if hb_skip.contains("static") || hb_skipmesh.as_ref().is_some_and(|m| m.split(',').any(|m| pkg.object_path(pl.mesh).contains(m))) {
                continue;
            }
            let Some((mp, mi)) = sp.resolve(pi, pl.mesh) else {
                skipped += 1;
                debug!("skipped (unresolved) {}", pkg.object_path(pl.mesh));
                continue;
            };
            let Some((parts, lod_index)) = sp.static_mesh(&mut a, mp, mi) else {
                skipped += 1;
                debug!("skipped (mesh) {}", pkg.object_path(pl.mesh));
                continue;
            };
            let s = pl.scale;
            // Negative DrawScale3D mirrors the mesh, which flips its winding.
            let mirrored = s[0] * s[1] * s[2] < 0.0;
            let transform = Transform {
                translation: ue_to_bevy(pl.location) * UU_TO_M,
                rotation: rotator_to_quat(pl.rotation),
                scale: Vec3::new(s[0], s[2], s[1]) * UU_TO_M,
            };
            if !pl.sky {
                center += transform.translation;
                placed += 1;
            }
            // Per-object TerrainHeight (terrain height below the object's origin).
            let th = match heightfield.height(pl.location[0], pl.location[1]) {
                Some(h) if !pl.sky => [h, g.misc[0], weather::sky_flag(false, &pkg.object_path(pl.mesh)), 0.0],
                _ => [0.0, 0.0, weather::sky_flag(pl.sky, &pkg.object_path(pl.mesh)), 0.0],
            };
            let l2w = gameshader::ue_l2w(pl.location, pl.rotation, pl.scale);
            let mats = pl.lod_materials.get(lod_index).unwrap_or(&pl.materials);
            // the mesh's own rule (upk static_mesh_collision_mask, HOMEFRONT.exe
            // UStaticMeshComponent::LineCheck 0x583d50): with UseSimpleBoxCollision (default)
            // pawns hit the mesh's collision mesh (rocks, walls, decks, porches), else its
            // BodySetup; with neither (grass, weeds, decals, wires) no pawn collision.
            // (dev: HB_MESHCOL_RENDER=1 = the old rule, every mesh blocks with its triangles)
            let col_rule = !keep && !pl.sky && !pl.nudgeable && pl.collide != 0 && std::env::var_os("HB_MESHCOL_RENDER").is_none();
            let mesh_mask = if col_rule { sp.pkgs[mp].static_mesh_collision_mask(mi) } else { 3 };
            if pl.collide & 1 != 0 && mesh_mask & 1 == 0 && !keep && !pl.sky && !pl.nudgeable {
                *no_pawn.entry(pkg.object_path(pl.mesh)).or_insert(0) += 1;
            }
            let road0 = col.tris.len();
            if !keep && !pl.sky && !pl.nudgeable && pl.collide & mesh_mask != 0 {
                // CollideActors / BlockZeroExtent / BlockNonZeroExtent of the component: light
                // cones, decals and graffiti were invisible walls, fences stopped shots
                let ph = part_phys(&sp, &mut impacts, mp, &parts, pi, mats);
                // pawns: the collision mesh's triangles where the rule picks it (PhysMaterial of
                // the first section, NOT from data: the collision mesh's own material)
                let pawn_tris = if col_rule && pl.collide & player::COL_PLAYER != 0 { pawn_col_cache.entry((mp, mi)).or_insert_with(|| sp.pkgs[mp].static_mesh_pawn_collision(mi).map(std::sync::Arc::new)).clone() } else { None };
                col.add_mask = pl.collide & mesh_mask & if pawn_tris.is_some() { !player::COL_PLAYER } else { 0xff };
                // debug overlay (coldebug.rs): owner name and where the triangles come from
                col.new_owner(pkg.object_path(pl.mesh));
                col.add_src = if pawn_tris.is_some() || !col_rule || col.add_mask & player::COL_PLAYER == 0 {
                    player::SRC_RENDER
                } else if sp.pkgs[mp].static_mesh_collision_model_count(mi).is_none() {
                    player::SRC_UNDECODED
                } else if sp.pkgs[mp].export_props(mi).and_then(|p| p.bool("UseSimpleBoxCollision")).unwrap_or(true) {
                    player::SRC_BODYSETUP
                } else {
                    player::SRC_RENDER
                };
                if col.add_mask != 0 {
                    add_collision(&mut col, a.meshes, &parts, &transform, &ph);
                }
                col.add_src = player::SRC_COLMESH;
                if let Some(tris) = pawn_tris {
                    col.add_mask = player::COL_PLAYER;
                    let keep_phys = col.add_phys;
                    col.add_phys = ph.first().copied().unwrap_or(0);
                    let m = transform.compute_matrix();
                    for t in tris.iter() {
                        col.add(t.map(|v| m.transform_point3(ue_to_bevy(v))));
                    }
                    col.add_phys = keep_phys;
                }
                col.add_mask = player::COL_PLAYER | player::COL_TRACE;
                col.add_src = player::SRC_OTHER;
                col.add_owner = 0;
            }
            // road surfaces (pa.rs keeps its poles off them): highway road-set lanes, road pieces
            if is_road_mesh(&pkg.object_path(pl.mesh)) {
                col.mark_road(road0);
            }
            sp.cast_shadows = !pl.sky;
            // destructible level trucks and their beds (levelcars.rs static trucks: KPA
            // NK_truck StaticMeshActors on Angel Island); spawn_mesh's first object block is the
            // placement's MeshTag (burnt flag)
            let truck = if pl.sky || pl.nudgeable { None } else { levelcars::static_kind(&pkg.object_path(pl.mesh)) }.map(|k| (k, levelcars::static_shape(&sp.pkgs[mp], a.meshes, &parts)));
            let truck_tag = sp.objects.len() as u32;
            // baked light map of the rendered LOD (upk level.rs LightMap2D, world.rs)
            sp.lightmap = pl.lightmaps.get(lod_index).copied().flatten().map(|l| (pi, l));
            sp.spawn_mesh(&mut commands, &mut a, mp, parts, pi, mats, transform, l2w, th, mirrored || pl.sky, false);
            if let Some((k, shape)) = truck {
                levelcars::spawn_static(&mut commands, k, shape, truck_tag, pl.location, pl.rotation, pl.scale);
            }
        }
        if !no_pawn.is_empty() {
            info!("{}: {} placements without pawn collision (simple collision, no collision mesh, no BodySetup): {:?}", pkg_names[pi], no_pawn.values().sum::<u32>(), no_pawn);
        }
        // BlockingVolumes: the pawn collision where the level's meshes have none for pawns. The
        // broken overpass deck on Crossroads / Overpass (HW_2Lane_2way_Traffic_brokenSeg2..4)
        // is BlockZeroExtent only (shots), the walkable surface are BlockingVolume_10 / 12 /
        // 13 / 14 of FL-Crossroads (user 2026-10-19: "i just fall through the bridge").
        // Default__BlockingVolume.BrushComponent0 (Engine.u): BlockActors, BlockNonZeroExtent
        // (base default), not BlockZeroExtent -> pawns only. GCVehicleBlockingVolume (vehicles
        // only) is left out.
        if !keep {
            let mut nbv = 0;
            col.add_mask = player::COL_PLAYER;
            col.add_src = player::SRC_BLOCKVOL;
            for v in pkg.brush_volumes("BlockingVolume") {
                col.new_owner(format!("BlockingVolume {}", v.name));
                for poly in &v.polys {
                    let q: Vec<Vec3> = poly.iter().map(|p| ue_to_bevy(*p) * UU_TO_M).collect();
                    for k in 1..q.len().saturating_sub(1) {
                        col.add([q[0], q[k], q[k + 1]]);
                    }
                }
                nbv += 1;
            }
            // vehicle-only volumes: shown by the debug overlay, not part of the pawn collision
            for v in pkg.brush_volumes("GCVehicleBlockingVolume") {
                col.new_owner(format!("GCVehicleBlockingVolume {}", v.name));
                for poly in &v.polys {
                    let q: Vec<Vec3> = poly.iter().map(|p| ue_to_bevy(*p) * UU_TO_M).collect();
                    for k in 1..q.len().saturating_sub(1) {
                        let o = col.add_owner;
                        col.extra.push(([q[0], q[k], q[k + 1]], player::SRC_VEHVOL, o));
                    }
                }
            }
            col.add_src = player::SRC_OTHER;
            col.add_owner = 0;
            col.add_mask = player::COL_PLAYER | player::COL_TRACE;
            if nbv > 0 {
                info!("{}: {nbv} blocking volumes", pkg_names[pi]);
            }
        }
        // minimap capture geometry (components with a MinimapRenderMode, minimap.rs)
        if !keep {
            for (pl, mode) in pkg.minimap_placements() {
                let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
                let Some((mp, mi)) = sp.resolve(pi, pl.mesh) else { continue };
                let Some((parts, _)) = sp.static_mesh(&mut a, mp, mi) else { continue };
                let s = pl.scale;
                let transform = Transform { translation: ue_to_bevy(pl.location) * UU_TO_M, rotation: rotator_to_quat(pl.rotation), scale: Vec3::new(s[0], s[2], s[1]) * UU_TO_M };
                mm_pending.objects.push((parts.iter().map(|p| p.0.clone()).collect(), transform, mode));
            }
        }
        // Skeletal meshes (vehicles etc.), reference pose. UE3: LocalToWorld =
        // RotationTranslation(RotOrigin, Origin) * component-to-world.
        let mut skel_n = 0;
        // interactive cars (levelcars.rs): skinned and damageable; orientation = UE3 LocalToWorld
        // with the mesh's RotOrigin and FRotationMatrix of the actor (checked offline for every
        // placed car, crates/upk/examples/carorient.rs). HB_STATICCARS=1: the reference-pose
        // meshes as before (no damage)
        let cars_here = if std::env::var_os("HB_STATICCARS").is_none() { levelcars::actors(&pkgs, pi) } else { Vec::new() };
        for pl in pkg.skeletal_mesh_placements() {
            if hb_skip.contains("skel") {
                continue;
            }
            let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
            // Component archetype exported by another loaded package (persistent level):
            // mesh and material overrides come from there (UE3 archetype instancing).
            let (mut mat_pi, mut mats) = (pi, pl.materials.clone());
            let mesh_ref = if pl.mesh == 0 {
                let Some((ap, ai)) = sp.resolve(pi, pl.comp_import) else { continue };
                let Some(acp) = sp.pkgs[ap].export_props(ai) else { continue };
                let Some(m) = acp.object("SkeletalMesh").filter(|&m| m != 0) else { continue };
                if mats.iter().all(|&m| m == 0) {
                    mat_pi = ap;
                    mats = sp.pkgs[ap].component_materials(&acp);
                }
                sp.resolve(ap, m)
            } else {
                sp.resolve(pi, pl.mesh)
            };
            let Some((mp, mi)) = mesh_ref else { continue };
            let Some((parts, local, local_l2w)) = sp.skeletal_mesh(&mut a, mp, mi) else { continue };
            let s = pl.scale;
            let actor = Transform {
                translation: ue_to_bevy(pl.location) * UU_TO_M,
                rotation: rotator_to_quat(pl.rotation),
                scale: Vec3::new(s[0], s[2], s[1]) * UU_TO_M,
            };
            let transform = actor.mul_transform(local);
            let th = match heightfield.height(pl.location[0], pl.location[1]) {
                Some(h) => [h, g.misc[0], 0.0, 0.0],
                None => [0.0; 4],
            };
            let l2w = gameshader::l2w_mul(local_l2w, gameshader::ue_l2w(pl.location, pl.rotation, pl.scale));
            // the triangles this mesh adds (levelcars: its detached parts ignore them)
            let col_from = col.tris.len() as u32;
            if !keep {
                let ph = part_phys(&sp, &mut impacts, mp, &parts, mat_pi, &mats);
                col.new_owner(sp.pkgs[mp].object_path(mi as i32 + 1));
                col.add_src = player::SRC_SKEL;
                add_collision(&mut col, a.meshes, &parts, &transform, &ph);
                col.add_src = player::SRC_OTHER;
                col.add_owner = 0;
            }
            let col_tris = col_from..col.tris.len() as u32;
            if let Some(car) = cars_here.iter().find(|c| c.location == pl.location && c.rotation == pl.rotation) {
                if levelcars::spawn(&mut sp, &mut a, &mut commands, mp, mi, mat_pi, &mats, car, col_tris) {
                    skel_n += 1;
                    continue;
                }
            }
            sp.cast_shadows = true;
            sp.spawn_mesh(&mut commands, &mut a, mp, parts, mat_pi, &mats, transform, l2w, th, s[0] * s[1] * s[2] < 0.0, false);
            skel_n += 1;
        }
        if skel_n > 0 {
            info!("{}: {skel_n} skeletal meshes", pkg_names[pi]);
        }
        // Fluid surfaces (water): FFluidVertexFactory with the game's shaders; flat grid
        // (EnableSimulation false in Overpass), GridSize = (FluidWidth, FluidHeight).
        for (mat, fw, fh, loc, rot, scale) in pkg.fluid_surfaces() {
            let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
            let Some(m) = sp.game_shader_material(&mut a, pi, mat, "FFluidVertexFactory", true) else {
                warn!("fluid surface: no game shader for {}", pkg.object_path(mat));
                continue;
            };
            let l2w = gameshader::ue_l2w(loc, rot, scale);
            let tag = sp.push_object(l2w, [0.0; 4]);
            if let Some(b) = sp.gs_blocks.last_mut() {
                b[15] = [fw, fh, 1.0, 0.0];
            }
            let n = 32usize;
            let (mut uv, mut ue, mut pos) = (Vec::new(), Vec::new(), Vec::new());
            for j in 0..=n {
                for i in 0..=n {
                    let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                    let lp = [(u - 0.5) * fw, (v - 0.5) * fh];
                    let w = [0, 1, 2].map(|k| l2w[0][k] * lp[0] + l2w[1][k] * lp[1] + l2w[3][k]);
                    uv.push([u, v]);
                    ue.push([lp[0], lp[1], 0.0]);
                    pos.push((ue_to_bevy(w) * UU_TO_M).to_array());
                }
            }
            let mut idx = Vec::new();
            for j in 0..n as u32 {
                for i in 0..n as u32 {
                    let v = |i: u32, j: u32| j * (n as u32 + 1) + i;
                    idx.extend_from_slice(&[v(i, j), v(i + 1, j), v(i + 1, j + 1), v(i, j), v(i + 1, j + 1), v(i, j + 1)]);
                }
            }
            let cnt = pos.len();
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
                .with_inserted_attribute(gameshader::ATTR_UE_POS, ue)
                .with_inserted_attribute(gamemat::ATTR_TANGENT_X, bevy::render::mesh::VertexAttributeValues::Unorm8x4(vec![[255, 128, 128, 255]; cnt]))
                .with_inserted_attribute(gamemat::ATTR_TANGENT_Z, bevy::render::mesh::VertexAttributeValues::Unorm8x4(vec![[128, 128, 255, 255]; cnt]))
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv.clone())
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uv)
                .with_inserted_indices(Indices::U32(idx));
            commands.spawn((LevelEntity, Mesh3d(meshes.add(mesh)), MeshMaterial3d(m), Transform::IDENTITY, bevy::render::mesh::MeshTag(tag), NotShadowCaster, NotShadowReceiver));
            info!("fluid surface {} ({fw} x {fh})", pkg.object_path(mat));
        }
        // SpeedTrees (Overpass_Speedtrees): cooked geometry with the game's shaders; LOD 0,
        // wind from the game's CWind (speedwind.rs / treewind.rs). LocalToWorld = scale + translation only, the rotation goes to
        // RotationOnlyMatrix (rows = columns of the rotation matrix), as in the trace.
        let mut st_n = 0;
        for pl in pkg.speedtree_placements() {
            let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
            let Some((tp, parts)) = sp.speedtree_parts(&mut a, pi, pl.tree) else { continue };
            let th = match heightfield.height(pl.location[0], pl.location[1]) {
                Some(h) => [h, g.misc[0], 0.0, 0.0],
                None => [0.0; 4],
            };
            let s = pl.scale;
            let l2w = [[s[0], 0.0, 0.0], [0.0, s[1], 0.0], [0.0, 0.0, s[2]], pl.location];
            let tag = sp.push_object(l2w, th);
            // one CWind per USpeedTree, shared by its placements (as in the game)
            if let Some((wp, wi)) = sp.resolve(pi, pl.tree) {
                let idx = *wind_index.entry((wp, wi)).or_insert_with(|| {
                    let params = pkgs[wp].read_speedtree(wi).and_then(|t| t.wind)?;
                    tree_wind.winds.push(speedwind::SpeedWind::new(params));
                    Some(tree_wind.winds.len() - 1)
                });
                if let Some(idx) = idx {
                    tree_wind.objects.push((tag, idx));
                }
            }
            let rot = gameshader::ue_l2w([0.0; 3], pl.rotation, [1.0; 3]);
            if let Some(b) = sp.gs_blocks.last_mut() {
                for k in 0..3 {
                    b[16 + k] = [rot[0][k], rot[1][k], rot[2][k], 0.0];
                }
                b[19] = [0.0; 4];
            }
            for (group, mesh, mat) in parts {
                let (mpi, mref) = match pl.materials[group] {
                    0 => (tp, mat),
                    o => (pi, o),
                };
                let raw = gameshader::RawLayout(speedtree_layout(group).unwrap().iter().map(|e| e.usages.to_vec()).collect());
                let Some(m) = sp.game_shader_material_raw(&mut a, mpi, mref, upk::speedtree::GROUP_VF[group], true, Some(&raw)) else { continue };
                let cast = a.terrain_mats.get(&m).is_some_and(|x| x.prepass_shader.is_some());
                let mut e = commands.spawn((LevelEntity, Mesh3d(mesh), MeshMaterial3d(m), Transform::IDENTITY, bevy::render::mesh::MeshTag(tag), bevy::render::view::NoFrustumCulling, NotShadowReceiver, crater::PlacedAt(Vec3::new(pl.location[0], pl.location[2], pl.location[1]) * 0.01)));
                if !cast {
                    e.insert(NotShadowCaster);
                }
            }
            st_n += 1;
        }
        if st_n > 0 {
            info!("{}: {st_n} speedtrees", pkg_names[pi]);
        }
        let roads = pkg.road_spline_meshes();
        for r in &roads {
            if hb_skip.contains("road") || hb_skipmesh.as_ref().is_some_and(|m| m.split(',').any(|m| pkg.object_path(r.mesh).contains(m))) {
                continue;
            }
            let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
            let Some((mp, mi)) = sp.resolve(pi, r.mesh) else { continue };
            let Some(parts) = sp.road_mesh(&mut a, mp, mi, r) else { continue };
            let transform = Transform {
                translation: ue_to_bevy(r.location) * UU_TO_M,
                rotation: rotator_to_quat(r.rotation),
                scale: Vec3::splat(UU_TO_M),
            };
            let th = match heightfield.height(r.location[0], r.location[1]) {
                Some(h) => [h, g.misc[0], 0.0, 0.0],
                None => [0.0; 4],
            };
            let l2w = gameshader::ue_l2w(r.location, r.rotation, [1.0; 3]);
            // the road surface lies on the terrain: walkable (CPU-deformed like the drawn mesh)
            if !keep {
                let ph = part_phys(&sp, &mut impacts, mp, &parts, pi, &r.materials);
                let road0 = col.tris.len();
                // the RoadSpline's own collision: none with DisableCollision (Crossroads
                // RoadSpline_13 over the creek bridge: its surface stuck out beside the bridge),
                // else the deformed mesh at the collision width (world.rs road_collision)
                col.new_owner(format!("RoadSpline {}", pkg.object_path(r.mesh)));
                col.add_phys = ph.first().copied().unwrap_or(0);
                let m = transform.compute_matrix();
                for t in sp.road_collision(mp, mi, r) {
                    col.add(t.map(|v| m.transform_point3(ue_to_bevy(v))));
                }
                col.add_phys = 0;
                col.add_owner = 0;
                col.mark_road(road0);
            }
            sp.cast_shadows = false;
            sp.spawn_mesh(&mut commands, &mut a, mp, parts, pi, &r.materials, transform, l2w, th, false, true);
        }
        if !roads.is_empty() {
            info!("{}: {} road spline meshes", pkg_names[pi], roads.len());
        }
        let emulated = match &game_shaders {
            Some(sc) => spawn_terrain_emu(
                &mut commands,
                pkg,
                &lib,
                sc,
                &globals_handle,
                args.max_tex,
                &mut meshes,
                &mut images,
                &mut shaders,
                &mut buffers,
                &mut terrain_mats,
            ),
            None => false,
        };
        let terrains = if emulated { Vec::new() } else { pkg.terrains() };
        if !terrains.is_empty() {
            let tl = pkg.terrain_layers();
            let mut ext = hf(base_params);
            if let Some(tl) = &tl {
                // Weightmap: linear data, clamped; layers: baked material graphs, tiled.
                let mut wimg = Image::new(
                    Extent3d { width: tl.weight_w as u32, height: tl.weight_h as u32, depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    tl.weights.clone(),
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::default(),
                );
                wimg.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
                ext.weightmap = Some(images.add(wimg));
                let mut scales = [1.0f32; 4];
                let mut handles: [Option<Handle<Image>>; 4] = Default::default();
                for k in 0..4 {
                    let Some((mat, scale)) = tl.layers[k] else { continue };
                    let Some(bk) = pkg.bake_material(&lib, mat, args.max_tex, &mut texcache) else { continue };
                    let mut img = Image::new(
                        Extent3d { width: bk.width, height: bk.height, depth_or_array_layers: 1 },
                        TextureDimension::D2,
                        bk.rgba,
                        TextureFormat::Rgba8UnormSrgb,
                        RenderAssetUsages::default(),
                    );
                    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        ..ImageSamplerDescriptor::linear()
                    });
                    handles[k] = Some(images.add(img));
                    scales[k] = scale;
                }
                let [h0, h1, h2, h3] = handles;
                ext.layer0 = h0;
                ext.layer1 = h1;
                ext.layer2 = h2;
                ext.layer3 = h3;
                ext.params.bounds = Vec4::new(tl.bounds[0], tl.bounds[1], tl.bounds[2], tl.bounds[3]);
                ext.params.origin = Vec4::new(tl.location[0], tl.location[1], tl.patch_size[0], tl.patch_size[1]);
                ext.params.scales = Vec4::from_array(scales);
                ext.params.flags = UVec4::new(1, 0, 0, 0);
            }
            let mat = materials.add(HfMat {
                base: StandardMaterial { perceptual_roughness: 0.95, ..default() },
                extension: ext,
            });
            for t in &terrains {
                for g in &t.grids {
                    commands.spawn((LevelEntity, 
                        Mesh3d(meshes.add(terrain_mesh(g))),
                        MeshMaterial3d(mat.clone()),
                        Transform::from_scale(Vec3::splat(UU_TO_M)),
                    ));
                }
                info!("terrain: {} components, layers: {:?}", t.grids.len(), tl.as_ref().map(|l| l.layers));
            }
        }
        info!("{}: {} placements", pkg_names[pi], placements.len());
    }
    lap("level objects placed");
    // the vehicle guns' tracers (filled with the vehicles below, pooled with the effects)
    let mut veh_fx: Vec<String> = Vec::new();
    // first-person weapons + arms (viewmodel.rs), CPU-skinned, hidden until the player spawns
    if !keep && !vm_weapons.is_empty() {
        let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
        // object path "Package.Group.Name" -> (package index, export index)
        let locate = |path: &str, class: &str| -> Option<(usize, usize)> {
            let (pk, rest) = path.split_once('.')?;
            let own = pkg_names.iter().position(|n| n.rsplit_once('.').map(|x| x.0).unwrap_or(n).eq_ignore_ascii_case(pk)).and_then(|pi| {
                let p = &pkgs[pi];
                let ei = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == class && p.object_path(i as i32 + 1).eq_ignore_ascii_case(rest))?;
                Some((pi, ei))
            });
            // cooked into a map (US_AH64, SD_MD600_v2 live in <map>_FrontlinesData only): the
            // export's full path there
            own.or_else(|| {
                pkgs.iter().enumerate().find_map(|(pi, p)| {
                    let ei = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == class && p.object_path(i as i32 + 1).eq_ignore_ascii_case(path))?;
                    Some((pi, ei))
                })
            })
        };
        sp.cast_shadows = false;
        let l2w = gameshader::ue_l2w([0.0; 3], [0; 3], [1.0; 3]);
        let mut spawn_sk_fg = |sp: &mut world::Spawner, a: &mut world::Assets3, path: &str, fg: bool| -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)> {
            // "SM|<static mesh>|<bone>|<3x3 cols, translation, bone quat, bone pos>": a vehicle
            // mirror attachment (vehload.rs) as a one-bone skinned mesh
            let (pi, ei, parts, mesh) = if let Some(req) = path.strip_prefix("SM|") {
                let mut it = req.split('|');
                let (smp, bone, nums) = (it.next()?, it.next()?, it.next()?);
                let f: Vec<f32> = nums.split(',').filter_map(|x| x.parse().ok()).collect();
                if f.len() != 19 {
                    return None;
                }
                let (pi, ei) = locate(smp, "StaticMesh")?;
                let lin = bevy::math::Mat3::from_cols_slice(&f[0..9]);
                let b = upk::skeletal::Bone { name: bone.to_string(), rot: [f[12], f[13], f[14], f[15]], pos: [f[16], f[17], f[18]], parent: 0 };
                let (parts, mesh) = sp.static_as_skinned(a, pi, ei, lin, Vec3::new(f[9], f[10], f[11]), b)?;
                (pi, ei, parts, mesh)
            } else {
                let (pi, ei) = locate(path, "SkeletalMesh")?;
                let (parts, mesh) = sp.skinned_mesh(a, pi, ei)?;
                (pi, ei, parts, mesh)
            };
            let _ = ei;
            // fg: first-person mesh (gamemat.rs foreground depth flag in the object's w)
            let n = sp.spawn_mesh(&mut commands, a, pi, parts.clone(), pi, &[], Transform::IDENTITY, l2w, [0.0, 0.0, 0.0, if fg { 1.0 } else { 0.0 }], false, false);
            if n == 0 {
                warn!("viewmodel {path}: no drawable section");
            }
            Some((parts.into_iter().map(|x| x.0).collect(), mesh))
        };
        let mut bot_assets: Option<bots::BotAssets> = None;
        let arms = locate("US_1P.Meshes.SKC_MP_US_1P", "SkeletalMesh").and_then(|_| spawn_sk_fg(&mut sp, &mut a, "US_1P.Meshes.SKC_MP_US_1P", true));
        // quick throw (Default__GCChar_MultiPlayer_Content): FragWeapSkelMeshComp SKW_EXP_Frag_1P
        // follows the weapon skeleton (ParentAnimComponent = WeapSkelMeshComp), whose AnimSets
        // add AS_CHC_PL_1P (grenade_cook / grenade_throw); the set is cooked into the MP maps
        let frag = spawn_sk_fg(&mut sp, &mut a, "Quickthrows.SkeletalMeshes.SKW_EXP_Frag_1P", true);
        let common_1p = pkgs.iter().find_map(|p| {
            let ei = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "AnimSet" && p.object_path(i as i32 + 1).eq_ignore_ascii_case("CHC_PlayerAnimations_1P.AnimSets.AS_CHC_PL_1P"))?;
            p.read_anim_set(ei)
        });
        // C4 throw (CFG_EXP_C4 CH_1P_AnimSet AS_EXP_C4_1P_CH)
        let c4_throw_set = combat_data.as_ref().and_then(|c| c.tactical.values().find_map(|t| t.anim_set.clone())).and_then(|path| locate(&path, "AnimSet")).and_then(|(pi, ei)| pkgs[pi].read_anim_set(ei));
        if frag.is_none() || common_1p.is_none() {
            warn!("viewmodel: quick throw mesh {} / AS_CHC_PL_1P {}", frag.is_some(), common_1p.is_some());
        }
        let mut weapons = Vec::new();
        if let Some((arm_handles, arm_mesh)) = arms {
            for cfg in &vm_weapons {
                let Some((wh, wmesh)) = spawn_sk_fg(&mut sp, &mut a, &cfg.sk_mesh, true) else {
                    warn!("viewmodel {}: mesh {} missing", cfg.cfg, cfg.sk_mesh);
                    continue;
                };
                let Some(anims) = locate(&cfg.anim_set, "AnimSet").and_then(|(pi, ei)| pkgs[pi].read_anim_set(ei)) else {
                    warn!("viewmodel {}: anim set {} missing", cfg.cfg, cfg.anim_set);
                    continue;
                };
                let skeleton = wmesh.bones.clone();
                // iron sights under a scope: GCWeaponScope bHideFrontSight, the weapon's Scopes entry
                // bHideRearSightBone (user's screenshot: rear sight inside the ACOG)
                let mut hide: Vec<&str> = Vec::new();
                if cfg.scope.as_ref().is_some_and(|s| s.hide_front_sight) {
                    hide.push("front_sight");
                }
                if cfg.scope.as_ref().is_some_and(|s| s.hide_rear_sight) {
                    hide.push("rear_sight");
                }
                let mut parts = vec![viewmodel::SkinPart::new(wh, wmesh, &skeleton, &hide)];
                for path in &cfg.parts {
                    if let Some((h, m)) = spawn_sk_fg(&mut sp, &mut a, path, true) {
                        parts.push(viewmodel::SkinPart::rigid(h, m, &skeleton, "Weapon", Vec3::ZERO));
                    }
                }
                if let Some(rail) = cfg.scope_rail.as_ref().filter(|_| cfg.scope.is_some()) {
                    if let Some((h, m)) = spawn_sk_fg(&mut sp, &mut a, rail, true) {
                        parts.push(viewmodel::SkinPart::rigid(h, m, &skeleton, "Weapon", Vec3::ZERO));
                    }
                }
                if let Some(sc) = &cfg.scope {
                    for path in sc.base.iter().chain(sc.mesh.iter()) {
                        if let Some((h, m)) = spawn_sk_fg(&mut sp, &mut a, path, true) {
                            parts.push(viewmodel::SkinPart::rigid(h, m, &skeleton, "Weapon", Vec3::from(cfg.scopes_rel)));
                        }
                    }
                }
                let arms_part = viewmodel::SkinPart::new(arm_handles.clone(), arm_mesh.clone(), &skeleton, &[]);
                info!("viewmodel {}: {} parts, {} sequences", cfg.cfg, parts.len(), anims.seqs.len());
                let mut wvm = viewmodel::WeaponVm::new(cfg.clone(), anims, skeleton, parts, arms_part);
                if let Some(c) = &common_1p {
                    wvm.merge_set(c);
                }
                if let Some(c) = &c4_throw_set {
                    wvm.merge_set_as(c, "c4_");
                }
                if let Some((h, m)) = &frag {
                    wvm.frag = Some(viewmodel::SkinPart::new(h.clone(), m.clone(), &wvm.skeleton, &[]));
                }
                weapons.push(wvm);
            }
        }
        let vm_res = viewmodel::ViewModel::new(weapons);
        let mut proj_spawns = Vec::new();
        // flying projectile meshes (combat::render_projectiles): 6 hidden instances per kind
        // 2: EMP / flash bang (both SKW_EXP_EMP_3P_static), 3: C4 (ProjectileMeshComponent)
        for (kind, path) in [(0u8, "Projectiles.Statics.SKW_PRJ_ATMissile01"), (1u8, "Quickthrows.Statics.SKW_EXP_Frag_3P_static"), (2u8, "Quickthrows.Statics.SKW_EXP_EMP_3P_static"), (3u8, "Quickthrows.Statics.SKW_EXP_C4_3P_static")] {
            // the standalone package (Quickthrows.upk / Projectiles.upk) first, then the copies
            // cooked into the loaded maps ("Quickthrows.Statics...." in FL-Crossroads.map): the
            // standalone ones do not read (their mesh data stays in the maps)
            let mut cands: Vec<(usize, usize)> = locate(path, "StaticMesh").into_iter().collect();
            for (qi, q) in pkgs.iter().enumerate() {
                if let Some(ei) = q.exports.iter().enumerate().position(|(i, e)| q.export_class(e) == "StaticMesh" && q.object_path(i as i32 + 1).eq_ignore_ascii_case(path)) {
                    cands.push((qi, ei));
                }
            }
            let Some((pi, parts)) = cands.into_iter().find_map(|(pi, ei)| sp.static_mesh(&mut a, pi, ei).filter(|(p, _)| !p.is_empty()).map(|(p, _)| (pi, p))) else {
                warn!("projectile mesh {path}: not readable");
                continue;
            };
            let mats = parts.iter().filter(|(_, mref)| sp.material(&mut a, pi, *mref, false).is_some()).count();
            info!("projectile mesh {path}: {} sections, {mats} with a material", parts.len());
            for slot in 0..6 {
                let tag = sp.push_object(gameshader::ue_l2w([0.0; 3], [0; 3], [1.0; 3]), [0.0; 4]);
                for (m, mref) in &parts {
                    if let Some(mat) = sp.material(&mut a, pi, *mref, false) {
                        proj_spawns.push((m.clone(), mat, tag, kind, slot));
                    }
                }
            }
        }
        // Hellfire target marker: Default__GCV_AirstrikeUAV_C StrikeMeshComponent
        // (DefaultStrikeMesh) StaticMesh MAT_RD.SpecialCase.UI.Targeter02 (airstrike.rs)
        let mut strike_spawns = Vec::new();
        // the copy cooked into the map (FL-Crossroads: "MAT_RD.SpecialCase.UI.Targeter02"), as the
        // projectile meshes above
        let mut cands: Vec<(usize, usize)> = locate("MAT_RD.SpecialCase.UI.Targeter02", "StaticMesh").into_iter().collect();
        for (qi, q) in pkgs.iter().enumerate() {
            if let Some(ei) = q.exports.iter().enumerate().position(|(i, e)| q.export_class(e) == "StaticMesh" && q.object_path(i as i32 + 1).eq_ignore_ascii_case("MAT_RD.SpecialCase.UI.Targeter02")) {
                cands.push((qi, ei));
            }
        }
        if let Some((pi, parts)) = cands.into_iter().find_map(|(pi, ei)| sp.static_mesh(&mut a, pi, ei).filter(|(p, _)| !p.is_empty()).map(|(p, _)| (pi, p))) {
            {
                let tag = sp.push_object(gameshader::ue_l2w([0.0; 3], [0; 3], [1.0; 3]), [0.0; 4]);
                for (m, mref) in &parts {
                    if let Some(mat) = sp.material(&mut a, pi, *mref, false) {
                        strike_spawns.push((m.clone(), mat, tag));
                    }
                }
            }
        }
        info!("airstrike: target marker {} sections", strike_spawns.len());
        if strike_spawns.is_empty() {
            warn!("airstrike: target marker Targeter02 not readable");
        }
        lap("viewmodel and projectile meshes");
        // bots (bots.rs): 3P bodies per team with the loadout weapons' 3P meshes
        let sets: Vec<upk::anim::AnimSetData> = ["AS_CHC_PL_3P", "AS_CHC_PL_RifleStand_3P", "AS_CHC_PL_RifleCrouch_3P", "AS_CHC_PL_Death_Running_3P"]
            .iter()
            .filter_map(|n| {
                let pi = pkg_names.iter().position(|x| x.eq_ignore_ascii_case("CHC_MPAnimations.upk"))?;
                let p = &pkgs[pi];
                let ei = p.exports.iter().position(|e| p.export_class(e) == "AnimSet" && p.object_name_of(e).eq_ignore_ascii_case(n))?;
                p.read_anim_set(ei)
            })
            .collect();
        lap("bot anim sets");
        if sets.len() == 4 {
            let lib = bots::AnimLib::new(sets);
            let mut rigs = Vec::new();
            let mut slots = Vec::new();
            let bodies = ["US_AST.SkeletalMeshes.SKC_MP_US_AST", "CH_AST.SkeletalMeshes.SKC_MP_CH_AST"];
            // MaxPlayers 16: 7 bots on the player's team + 8 on the other; 8 bodies per team so
            // either side can take the 8 (spawn selection "TEAM WECHSELN", bots.rs manage_bots)
            for k in 0..16usize {
                let team = if k < 8 { combat::TEAM_USA } else { combat::TEAM_KVA };
                let Some((bh, bmesh)) = spawn_sk_fg(&mut sp, &mut a, bodies[team as usize], false) else { continue };
                if rigs.len() <= team as usize {
                    rigs.push(bots::BodyRig::new(bmesh.bones.clone(), &lib));
                }
                let skeleton = bmesh.bones.clone();
                let n_prim = vm_weapons.iter().take_while(|w| w.projectile.is_none()).count().max(1);
                let wi = k % n_prim;
                let weapon = vm_weapons.get(wi).and_then(|w| w.sk_mesh_3p.clone()).and_then(|path| spawn_sk_fg(&mut sp, &mut a, &path, false)).map(|(h, m)| viewmodel::SkinPart::rigid(h, m, &skeleton, "RightWeapon", Vec3::ZERO));
                slots.push(bots::BotSlot { team, rig: team as usize, body: viewmodel::SkinPart::new(bh, bmesh, &skeleton, &[]), weapon, weapon_cfg: wi });
            }
            info!("bots: {} slots", slots.len());
            if std::env::var_os("HB_PERF").is_some() {
                info!("load: perf scopes so far {}", crate::perf::summary());
            }
            lap("bot bodies");
            // the player's own body for the kill cam (USA, first loadout weapon in the hands)
            let player_body = (!rigs.is_empty()).then(|| spawn_sk_fg(&mut sp, &mut a, bodies[0], false)).flatten().map(|(bh, bmesh)| {
                let skeleton = bmesh.bones.clone();
                let weapon = vm_weapons.first().and_then(|w| w.sk_mesh_3p.clone()).and_then(|path| spawn_sk_fg(&mut sp, &mut a, &path, false)).map(|(h, m)| viewmodel::SkinPart::rigid(h, m, &skeleton, "RightWeapon", Vec3::ZERO));
                bots::BotSlot { team: combat::TEAM_USA, rig: 0, body: viewmodel::SkinPart::new(bh, bmesh, &skeleton, &[]), weapon, weapon_cfg: 0 }
            });
            // thermal vision: GCObjectPool_Content CharacterThermalMaterial (unlit white)
            let thermal = locate("CH_3P.M_Character_Thermal_White", "Material").and_then(|(pi, ei)| sp.material(&mut a, pi, ei as i32 + 1, false));
            if thermal.is_none() {
                warn!("bots: CH_3P.M_Character_Thermal_White not readable (no thermal bodies)");
            }
            if rigs.len() == 2 {
                bot_assets = Some(bots::BotAssets { lib, rigs, slots, player: player_body, thermal });
            }
        } else {
            warn!("bots: CHC_MPAnimations sets missing ({})", sets.len());
        }
        // vehicles (vehicles.rs, vehload.rs): the buyable USA vehicles, two drawable instances
        // each (HB_VEHONLY=<class>: one class only, tests)
        let veh_only = std::env::var("HB_VEHONLY").ok();
        let rockets = combat_data.as_ref().map(|c| c.vehicle_rockets.clone()).unwrap_or_default();
        let veh_kinds: Vec<vehicles::VehicleKind> = match (lib.open("GCGameContent"), lib.open("GCGame"), lib.open("WeaponConfigs")) {
            (Some(content), Some(gcgame), Some(wc)) => vehload::all_classes()
                .iter()
                .filter(|c| veh_only.as_deref().is_none_or(|o| o.eq_ignore_ascii_case(c)))
                .filter_map(|c| {
                    let k = vehload::load(&lib, &pkgs, &content, &gcgame, &wc, c, &rockets, &mut |path: &str| spawn_sk_fg(&mut sp, &mut a, path, false));
                    if k.is_none() {
                        warn!("vehicle {c}: not readable");
                    }
                    k
                })
                .collect(),
            _ => Vec::new(),
        };
        // HB_DRONEONLY=<k>: load one drone kind only (tests)
        let only: Option<usize> = std::env::var("HB_DRONEONLY").ok().and_then(|v| v.parse().ok());
        let drones: Vec<drone::DroneAssets> = (0..DRONE_KINDS.len()).filter(|k| only.is_none_or(|o| o == *k)).filter_map(|k| load_drone(&lib, k, &mut |path: &str, fg: bool| spawn_sk_fg(&mut sp, &mut a, path, fg))).collect();
        drop(spawn_sk_fg);
        // the vehicle guns' tracers (pooled below)
        veh_fx = veh_kinds.iter().flat_map(|k| k.seats.iter()).flat_map(|s| s.modes.iter()).filter_map(|m| if let vehicles::FireKind::Instant(g) = &m.kind { g.tracer.as_ref().map(|t| t.template.clone()) } else { None }).collect();
        // muzzle flashes (socket References of the fire modes' FlashTags) and the impacts /
        // splash of the vehicle guns' damage types (the pools were filled before the vehicles)
        let mut veh_dmgs: Vec<String> = Vec::new();
        for k in &veh_kinds {
            for m in k.seats.iter().flat_map(|s| s.modes.iter()) {
                for tag in &m.flash_tags {
                    let tl = tag.to_ascii_lowercase();
                    veh_fx.extend(k.socket_fx.iter().filter(|e| e.0 == tl).map(|e| e.2.clone()));
                }
                if let vehicles::FireKind::Instant(g) = &m.kind {
                    veh_dmgs.push(g.dmg_type.clone());
                    if let Some(sp) = &g.splash {
                        veh_dmgs.push(sp.0.clone());
                    }
                }
            }
        }
        veh_dmgs.sort();
        veh_dmgs.dedup();
        let imp = impacts.systems_for(&veh_dmgs);
        info!("vehicle effects: {} muzzle / tracer systems, {} impact systems for {:?}", veh_fx.len(), imp.len(), veh_dmgs);
        veh_fx.extend(imp);
        // vehicle wrecks (destruction.rs): the vehicle meshes again with their RagdollMaterials
        sp.tex_substitute = destruction::tex_substitutes();
        let wrecks = destruction::load(&lib, &pkgs, &veh_kinds, &mut |path: &str, mats: &[String]| {
            if let Some(req) = path.strip_prefix("SM|") {
                return destruction::spawn_wreck_static(&mut sp, &mut a, &mut commands, &|p: &str, c: &str| locate(p, c), l2w, req);
            }
            let (pi, ei) = locate(path, "SkeletalMesh")?;
            destruction::spawn_wreck_mesh(&mut sp, &mut a, &mut commands, pi, ei, l2w, mats)
        });
        sp.tex_substitute.clear();
        if std::env::var_os("HB_NOWRECKFX").is_none() {
            veh_fx.extend(wrecks.templates());
            veh_fx.extend(levelcars::templates(&pkgs));
        }
        // the damage stages' effects (vehicles.rs damage_states: over-armour, DamageSmoke)
        veh_fx.extend(veh_kinds.iter().flat_map(|k| k.damage.refs.iter().filter(|r| !r.sound).map(|r| r.path.clone())));
        commands.insert_resource(wrecks);
        veh_fx.sort();
        veh_fx.dedup();
        if !veh_kinds.is_empty() {
            commands.insert_resource(vehicles::VehicleAssets { kinds: veh_kinds });
        }
        if drones.len() < DRONE_KINDS.len() {
            warn!("ground drones: {} of {} readable", drones.len(), DRONE_KINDS.len());
        }
        commands.insert_resource(drone::DroneKinds(drones));
        for (m, mat, tag) in strike_spawns {
            // hidden: the marker's geometry for airstrike.rs (drawn as HUD quads)
            commands.spawn((LevelEntity, Mesh3d(m), MeshMaterial3d(mat), Transform::default(), Visibility::Hidden, bevy::render::mesh::MeshTag(tag), bevy::pbr::NotShadowReceiver, bevy::pbr::NotShadowCaster, bevy::render::view::NoFrustumCulling, airstrike::StrikeMesh));
        }
        for (m, mat, tag, kind, slot) in proj_spawns {
            commands.spawn((LevelEntity, Mesh3d(m), MeshMaterial3d(mat), Transform::default(), Visibility::Hidden, bevy::render::mesh::MeshTag(tag), bevy::pbr::NotShadowReceiver, bevy::pbr::NotShadowCaster, bevy::render::view::NoFrustumCulling, combat::ProjVis { kind, slot }));
        }
        commands.insert_resource(vm_res);
        if let Some(b) = bot_assets.take() {
            commands.insert_resource(b);
        }
        if let Some(cd) = combat_data.take() {
            commands.insert_resource(cd);
        }
    }
    // PA system (pa.rs, user request): poles with loudspeakers and their positions
    if !keep {
        let map = maps.iter().filter_map(|m| m.file_stem()).map(|s| s.to_string_lossy().to_string()).collect::<Vec<_>>().join(",");
        let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
        let mut speakers = Vec::new();
        pa::spawn(&map, &mut sp, &mut a, &mut commands, &mut col, &mut speakers, &mut impacts);
        commands.insert_resource(pa::PaSpeakers(speakers));
    }
    info!("placed {placed} objects, skipped {skipped} (unsupported mesh layout)");
    // particle sprites' night class (particles::night_dimmed): objects[].z 3 = darkened with the
    // scene at night (daylit smoke / dust / sunbeams), 4 = unchanged (fire, sparks, flashes)
    let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]];
    let fx_tags = (sp.push_object(ident, [0.0, 0.0, 3.0, 0.0]), sp.push_object(ident, [0.0, 0.0, 4.0, 0.0]));
    let mut fx_class: std::collections::HashMap<(usize, i32, String), bool> = Default::default();
    let mut fx_tag = |sp: &world::Spawner, pi: usize, tmpl_path: &str, em: &upk::particles::Emitter| -> u32 {
        let dim = *fx_class.entry((pi, em.material, format!("{}|{}", tmpl_path, em.name))).or_insert_with(|| {
            let c = sp.resolve(pi, em.material).and_then(|(mp, mi)| sp.pkgs[mp].compile_material(mi as i32 + 1));
            particles::night_dimmed(em, tmpl_path, c.as_ref())
        });
        if dim { fx_tags.0 } else { fx_tags.1 }
    };
    buffers.insert(&globals_handle, gamemat::globals_buffer(&g, &sp.objects));
    {
        // game-shader globals: Globals + MaterialUpperSkyColor + MaterialLowerSkyColor
        let mut gs: Vec<u8> = bytemuck::bytes_of(&g).to_vec();
        let mat_sky = skies.iter().find(|s| s.use_as_material_sky_light);
        let (mu, ml) = match mat_sky {
            Some(sk) => (gamemat::ue_color(sk.color, sk.brightness), gamemat::ue_color(sk.lower_color, sk.lower_brightness)),
            None => ([0.0; 3], [0.0; 3]),
        };
        // WorldInfo.CheapAmbientColor (FColor -> linear: (c/255)^2.2)
        let cheap = pkgs.iter().find_map(|p| p.world_color("CheapAmbientColor")).map(|c| [0, 1, 2].map(|k| (c[k] as f32 / 255.0).powf(2.2))).unwrap_or([0.0; 3]);
        for v in [[mu[0], mu[1], mu[2], 0.0f32], [ml[0], ml[1], ml[2], 0.0], [cheap[0], cheap[1], cheap[2], 1.0]] {
            gs.extend_from_slice(bytemuck::cast_slice(&v));
        }
        let mut gsb = bevy::render::storage::ShaderStorageBuffer::new(&gs, RenderAssetUsages::default());
        // crater.rs writes misc.yzw at runtime
        gsb.buffer_description.usage |= bevy::render::render_resource::BufferUsages::COPY_DST;
        buffers.insert(&gs_globals, gsb);
        let blocks: Vec<u8> = sp.gs_blocks.iter().flat_map(|b| bytemuck::cast_slice::<[f32; 4], u8>(b).to_vec()).collect();
        let mut ob = bevy::render::storage::ShaderStorageBuffer::new(&blocks, RenderAssetUsages::default());
        // wind slots are patched in place every frame (treewind.rs)
        ob.buffer_description.usage |= bevy::render::render_resource::BufferUsages::COPY_DST;
        buffers.insert(&gs_objects, ob);
        tree_wind.buffer = Some(gs_objects.clone());
        // Placed particle emitters (fires etc.), including the *_FX sublevels
    {
        let defs = upk::particles::ClassDefaults::new(&lib);
        let mut n_sys = 0;
        for pi in 0..pkgs.len() {
            let pkg = &pkgs[pi];
            for pl in pkg.emitter_placements() {
                let (tmpl, mut loc, rot, scale) = (pl.template, pl.location, pl.rotation, pl.scale);
                if tmpl <= 0 {
                    continue;
                }
                // an emitter below the terrain surface starts on it (NOT from data: Angel_FX
                // Emitter_269 SeaBirds sits 2.6 m under the slope, its own TerrainLightingHeight
                // -882 vs Z -1141, so its gulls rose out of the ground; the only emitter below
                // the terrain on AngelIsland / Crossroads / Overpass, examples/emitterfit)
                if let Some(h) = heightfield.height(loc[0], loc[1]).filter(|&h| h > loc[2]) {
                    debug!("emitter {} lifted {:.0} uu onto the terrain", pkg.object_path(tmpl), h - loc[2]);
                    loc[2] = h;
                }
                let Some(ems) = pkg.read_particle_system(tmpl as usize - 1, &defs) else { continue };
                let l2w = gameshader::ue_l2w([0.0; 3], rot, [1.0; 3]);
                let rq = Quat::from_mat3(&Mat3::from_cols(Vec3::from(l2w[0]), Vec3::from(l2w[1]), Vec3::from(l2w[2])));
                let mut states = Vec::new();
                let tmpl_path = pkg.object_path(tmpl);
                for em in ems.into_iter().filter(|e| !e.mesh && e.material != 0) {
                    let tag = fx_tag(&sp, pi, &tmpl_path, &em);
                    let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
                    let Some(mat) = sp.material(&mut a, pi, em.material, true) else {
                        warn!("particle material {} not found", pkg.object_path(em.material));
                        continue;
                    };
                    // a translucent sprite without its texture would draw as a solid quad
                    if !sp.material_complete(pi, em.material) {
                        warn!("particle emitter {} skipped: texture of {} not found", em.name, pkg.object_path(em.material));
                        continue;
                    }
                    if std::env::var_os("HOMEBOTS_PARTICLE_DEBUG").is_some() {
                        info!("emitter {} at {:?}: material {}", em.name, loc, pkg.object_path(em.material));
                    }
                    let mesh = meshes.add(particles::empty_mesh());
                    commands.spawn((LevelEntity, 
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(mat),
                        // shown once it has particles (particles.rs set_shown)
                        Visibility::Hidden,
                        Transform { translation: ue_to_bevy(loc) * UU_TO_M, rotation: Quat::IDENTITY, scale: Vec3::splat(UU_TO_M) },
                        bevy::render::mesh::MeshTag(tag),
                        bevy::render::view::NoFrustumCulling,
                        NotShadowCaster,
                        NotShadowReceiver,
                    ));
                    states.push(particles::emitter_state(em, mesh));
                }
                if !states.is_empty() {
                    let mut si = particles::SystemInstance::placed(Vec3::from(loc), rq, scale, Vec3::from(pl.scale3d) * scale, pl.colors.clone(), states);
                    si.max_draw = pl.max_draw_distance;
                    particle_world.systems.push(si);
                    n_sys += 1;
                }
            }
        }
        info!("{n_sys} particle systems");
        // pooled one-shot effects (combat.rs / bots.rs): frag explosion (the frag template the
        // game content imports: GCGameContent import KFX_Pyrotechnics.FX_Grenades.Frag_Metal) and
        // the loadout weapons' muzzle flashes
        if !keep {
            let mut pools: Vec<(String, usize)> = vec![("KFX_Pyrotechnics.FX_Grenades.Frag_Metal".into(), 3)];
            for t in &tactical_fx {
                if !pools.iter().any(|p| &p.0 == t) {
                    pools.push((t.clone(), 3));
                }
            }
            for t in &impact_fx {
                if !pools.iter().any(|p| &p.0 == t) {
                    pools.push((t.clone(), 4));
                }
            }
            // rocket smoke trails (ProjFlightTemplate), followed by combat.rs
            for t in &strike_fx {
                if !pools.iter().any(|p| &p.0 == t) {
                    pools.push((t.clone(), 6));
                }
            }
            for w in &vm_weapons {
                if let Some(t) = w.projectile.as_ref().and_then(|p| p.flight_fx.clone()) {
                    if !pools.iter().any(|p| p.0 == t) {
                        pools.push((t, 4));
                    }
                }
            }
            // tracers (WeaponEffectTracer ParticleSystem, 2 s particles): 1P every 2nd shot of one
            // shooter, 3P for all bots
            for w in &vm_weapons {
                for (k, t) in w.tracer.iter().enumerate() {
                    if let Some(t) = t {
                        if !pools.iter().any(|p| p.0 == t.template) {
                            pools.push((t.template.clone(), if k == 0 { 12 } else { 24 }));
                        }
                    }
                }
            }
            for w in &vm_weapons {
                for (k, m) in w.muzzle.iter().enumerate() {
                    if let Some(m) = m {
                        if !pools.iter().any(|p| &p.0 == m) {
                            pools.push((m.clone(), if k == 1 { 6 } else { 2 }));
                        }
                    }
                }
            }
            nuke::add_pools(&mut pools);
            weather::add_pools(&mut pools);
            for t in &veh_fx {
                if !pools.iter().any(|p| &p.0 == t) {
                    pools.push((t.clone(), 12));
                }
            }
            for (t, n) in &fx_counts {
                match pools.iter_mut().find(|p| &p.0 == t) {
                    Some(p) => p.1 = p.1.max(*n),
                    None => pools.push((t.clone(), *n)),
                }
            }
            let (mut pool_read, mut pool_inst) = (0.0f32, 0.0f32);
            // resolve, then read all templates in parallel (the reads dominated the load time)
            let mut jobs: Vec<(String, usize, usize, usize)> = Vec::new();
            for (path, count) in pools {
                let Some((pk, rest)) = path.split_once('.') else { continue };
                let Some(pi) = pkg_names.iter().position(|n| n.rsplit_once('.').map(|x| x.0).unwrap_or(n).eq_ignore_ascii_case(pk)) else {
                    warn!("fx pool {path}: package not loaded");
                    continue;
                };
                let pkg = &pkgs[pi];
                let Some(ti) = pkg.exports.iter().enumerate().position(|(i, e)| pkg.export_class(e) == "ParticleSystem" && pkg.object_path(i as i32 + 1).eq_ignore_ascii_case(rest)) else {
                    warn!("fx pool {path}: template not found");
                    continue;
                };
                jobs.push((path.clone(), count, pi, ti));
            }
            let t_read = std::time::Instant::now();
            let templates: Vec<Option<Vec<upk::particles::Emitter>>> = {
                let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 16);
                let per = jobs.len().div_ceil(threads).max(1);
                let (pkgs_ref, defs_ref) = (&pkgs, &defs);
                std::thread::scope(|sc| {
                    let hs: Vec<_> = jobs.chunks(per).map(|ch| sc.spawn(move || ch.iter().map(|(_, _, pi, ti)| pkgs_ref[*pi].read_particle_system(*ti, defs_ref)).collect::<Vec<_>>())).collect();
                    hs.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
                })
            };
            pool_read += t_read.elapsed().as_secs_f32();
            for ((path, count, pi, _), template) in jobs.into_iter().zip(templates) {
                let Some(template) = template else { continue };
                let t_inst = std::time::Instant::now();
                for _ in 0..count {
                    let ems = template.clone();
                    let mut states = Vec::new();
                    // HB_FXONLY=<emitter name>[,..]: pooled effects keep only those emitters (tests)
                    let only = std::env::var("HB_FXONLY").ok();
                    for em in ems.into_iter().filter(|e| !e.mesh && e.material != 0) {
                        if only.as_ref().is_some_and(|o| !o.split(',').any(|n| n == em.name)) {
                            continue;
                        }
                        let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
                        let Some(mat) = sp.material(&mut a, pi, em.material, true) else { continue };
                        if !sp.material_complete(pi, em.material) {
                            continue;
                        }
                        let tag = fx_tag(&sp, pi, &path, &em);
                        let mesh = meshes.add(particles::empty_mesh());
                        // hidden until fired: dormant pool instances were ~5000 visible (degenerate) transparent
                        // draws, 7.5 ms of the transparent pass in the user's profile 2026-10-19
                        commands.spawn((LevelEntity, Mesh3d(mesh.clone()), MeshMaterial3d(mat), Transform { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::splat(UU_TO_M) }, Visibility::Hidden, bevy::render::mesh::MeshTag(tag), bevy::render::view::NoFrustumCulling, NotShadowCaster, NotShadowReceiver));
                        // trail emitters (GCParticleModuleTypeDataTrail): the ribbon with TrailMaterial
                        let trail_mat = em.trail.as_ref().map(|t| t.material).filter(|&m| m != 0).and_then(|m| {
                            let mut a = world::Assets3 { terrain_mats: &mut terrain_mats, meshes: &mut meshes, images: &mut images, game_mats: &mut game_mats, shaders: &mut shaders };
                            sp.material(&mut a, pi, m, true).filter(|_| sp.material_complete(pi, m))
                        });
                        let mut st = particles::emitter_state(em, mesh);
                        if let Some(tmat) = trail_mat {
                            let tmesh = meshes.add(particles::empty_mesh());
                            commands.spawn((LevelEntity, Mesh3d(tmesh.clone()), MeshMaterial3d(tmat), Transform { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::splat(UU_TO_M) }, Visibility::Hidden, bevy::render::mesh::MeshTag(0), bevy::render::view::NoFrustumCulling, NotShadowCaster, NotShadowReceiver));
                            st = st.with_trail(tmesh);
                        }
                        states.push(st);
                    }
                    if states.is_empty() {
                        break;
                    }
                    let mut si = particles::SystemInstance::placed(Vec3::ZERO, Quat::IDENTITY, 1.0, Vec3::ONE, Vec::new(), states);
                    si.anchor = Vec3::ZERO;
                    si.pooled = true;
                    si.dormant = true;
                    let idx = particle_world.systems.len();
                    particle_world.systems.push(si);
                    particle_world.pools.entry(path.clone()).or_default().push(idx);
                }
                pool_inst += t_inst.elapsed().as_secs_f32();
            }
            info!("fx pools built: templates read {pool_read:.2} s, instances {pool_inst:.2} s");
            lap("fx pools");
            if std::env::var_os("HB_PERF").is_some() {
                info!("load: perf scopes {}", crate::perf::summary());
            }
            // prefetch vs build: the reads become the next prefetch list for this level, so from
            // the second load on every mip comes from memory and nothing is read for nothing
            let left = upk::library::clear_prefetched();
            let reads = upk::library::stop_recording();
            let hits = reads.iter().filter(|r| r.1).count();
            info!("preload: build read {} texture mips, {hits} from the prefetch, {} from disk during the build, {left} prefetched but not used", reads.len(), reads.len() - hits);
            let manifest = prefetch_manifest(&maps);
            let jobs: Vec<upk::library::BulkJob> = reads.into_iter().map(|r| r.0).collect();
            if let Err(e) = upk::library::save_manifest(&manifest, &jobs) {
                warn!("preload: prefetch list {}: {e}", manifest.display());
            }
            info!("fx pools: {:?}", particle_world.pools.iter().map(|(k, v)| (k.as_str(), v.len())).collect::<Vec<_>>());
            commands.insert_resource(std::mem::take(&mut impacts));
        }
    }
    let mut probs: Vec<_> = sp.gs_problems.iter().collect();
        probs.sort();
        for (p, n) in probs {
            warn!("game shaders: {p} ({n}x)");
        }
    }
    info!("{} game shaders; stubbed material nodes: {:?}", sp.shader_count(), sp.stub_log);
    let center = if placed > 0 { center / placed as f32 } else { Vec3::ZERO };
    spawn_lighting(&mut commands, &suns, &sun_shadow);
    // Homefront's own post-process chain (hfpost.rs) replaces Bevy's tonemapping.
    // DisplayGamma = 2.2 ([Engine.Client] in GCEngine.ini / BaseEngine.ini).
    let post = hfpost::HfPost::from_level(&post_settings.clone().unwrap_or_default(), 2.2);
    if keep {
        levels.cached.push(CachedLevel { key: maps.clone(), group, post, shadow: *sun_shadow });
    }
    if !show {
        *sun_shadow = shown_shadow;
        return;
    }
    if post_volumes.len() > 1 {
        info!("post-process volumes: {:?}", post_volumes.iter().map(|v| (v.name.as_str(), v.priority, v.min, v.max)).collect::<Vec<_>>());
    }
    commands.insert_resource(PostVolumes { list: post_volumes, default: post_settings.clone(), active: None });
    let (tf, fly) = match args.shot.as_ref().and_then(|s| s.1).or_else(|| in_menu.0.then(|| menu_camera(&args.game)).flatten()) {
        // UE coordinates of the shot camera (x, y, z in UE units, yaw/pitch in degrees, UE convention).
        Some(c) => {
            let yaw = (-c[3] - 90.0).to_radians();
            let pitch = c[4].to_radians();
            (Transform::from_translation(ue_to_bevy([c[0], c[1], c[2]]) * UU_TO_M), FlyCam { yaw, pitch })
        }
        None => (Transform::from_translation(center + Vec3::new(0.0, 15.0, 30.0)).looking_at(center, Vec3::Y), FlyCam { yaw: 0.0, pitch: -0.45 }),
    };
    let cam_entity = if let Some((e, mut t, mut f)) = cams.iter_mut().next() {
        *t = tf;
        *f = fly;
        e
    } else {
        commands
            .spawn((
                // depth readable for the scene copy (SceneColorTexture alpha), no MSAA like the game
                Camera3d { depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING).into(), ..default() },
                Msaa::Off,
                Camera { hdr: true, ..default() },
                Tonemapping::None,
                Exposure { ev100: 0.0 },
                Projection::Perspective(PerspectiveProjection { far: 100_000.0, ..default() }),
                tf,
                fly,
            ))
            .id()
    };
    let mut cam = commands.entity(cam_entity);
    // HOMEBOTS_NO_POST: raw scene colour (debugging)
    if std::env::var_os("HOMEBOTS_NO_POST").is_none() {
        cam.insert(post);
    }
    if in_menu.0 {
        cam.insert(menu::MenuFov);
    } else {
        cam.remove::<menu::MenuFov>();
        cam.insert(Projection::Perspective(PerspectiveProjection { far: 100_000.0, ..default() }));
    }
}

/// The .map files of a map folder that make up the level (minimap helper planes, fake
/// light-shaft sprites and audio-only sublevels are not world geometry).
pub fn level_files(dir: &std::path::Path) -> Vec<PathBuf> {
    // cached per folder: finding the streaming list opens the persistent map
    static CACHE: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, Vec<PathBuf>>>> = std::sync::Mutex::new(None);
    if let Some(v) = CACHE.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(dir).cloned())) {
        return v;
    }
    let v = level_files_uncached(dir);
    if let Ok(mut g) = CACHE.lock() {
        g.get_or_insert_with(Default::default).insert(dir.to_path_buf(), v.clone());
    }
    v
}

fn level_files_uncached(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("map")))
                .filter(|f| {
                    let n = f.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    !(n.ends_with("_minimap") || n.ends_with("_fx") || n.ends_with("_audio"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    // The persistent map's LevelStreamingKismet list: sublevels flagged
    // bShouldBeLoadedAsPersistent are always in the world; the others only when the game code
    // streams them for a mode (TDM_<map>_c, <map>_FrontlinesData, <map>_bcsettings - names in
    // GCGame.u / the EXE's package list). Crossroads_removedTrees (198 cut SpeedTrees, DEAD ASSET
    // placeholder meshes) and the empty Crossroads_subpersistent are never streamed in.
    let mut keep: Option<std::collections::HashSet<String>> = None;
    // the persistent map ("FL-Crossroads") first: sublevel names carry an underscore
    let mut order: Vec<&PathBuf> = v.iter().collect();
    // (Harbor also has fl-harbor-suburb / fl-harbor-terrain / fl-harbor_StripMall next to
    // fl-harbor: "fl-<folder>" goes first)
    let persistent_name = format!("fl-{}", dir.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default());
    order.sort_by_key(|f| (f.file_stem().is_none_or(|s| s.to_string_lossy().to_ascii_lowercase() != persistent_name), f.file_stem().is_some_and(|s| s.to_string_lossy().contains('_'))));
    for f in order {
        let Ok(p) = Package::open(f) else { continue };
        let mut set = std::collections::HashSet::new();
        for (i, e) in p.exports.iter().enumerate() {
            if !p.export_class(e).starts_with("LevelStreaming") {
                continue;
            }
            let Some(pr) = p.export_props(i) else { continue };
            let Some(name) = pr.enum_name("PackageName").map(|n| n.to_ascii_lowercase()) else { continue };
            let persistent = pr.bool("bShouldBeLoadedAsPersistent") == Some(true);
            let mode = mapmode::is_tdm_data(&name) || mapmode::is_fl_data(&name) || name.ends_with("_bcsettings");
            if persistent || mode {
                set.insert(name);
            }
        }
        if !set.is_empty() {
            set.insert(f.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default());
            keep = Some(set);
            break;
        }
    }
    if let Some(k) = keep {
        v.retain(|f| f.file_stem().is_some_and(|s| k.contains(&s.to_string_lossy().to_ascii_lowercase())));
    }
    v
}

/// Post-process settings: first map that has a PostProcessVolume (or WorldInfo
/// defaults if none of them has one).
fn post_settings(maps: &[PathBuf], persistent: Option<&PathBuf>) -> Option<upk::level::PostProcess> {
    let mut world_default = None;
    for m in persistent.into_iter().chain(maps) {
        if let Ok(p) = Package::open(m) {
            let has_volume = p.exports.iter().any(|e| p.export_class(e) == "PostProcessVolume");
            match p.post_process_settings() {
                Some(s) if has_volume => return Some(s),
                Some(s) if world_default.is_none() => world_default = Some(s),
                _ => {}
            }
        }
    }
    world_default
}

/// Everything a loaded level spawned (despawned when another level is loaded). The add hook
/// files the entity under the level being loaded (`LevelSpawn`) and hides it when that level is
/// loaded in the background (front-end levels are kept loaded and switched like the game's
/// level streaming, without a reload).
#[derive(Component)]
#[component(on_add = level_on_add)]
pub struct LevelEntity;

#[derive(Component)]
pub struct LevelGroup(pub u32);

#[derive(Resource, Default)]
pub struct LevelSpawn {
    group: u32,
    hidden: bool,
}

fn level_on_add(mut world: bevy::ecs::world::DeferredWorld, ctx: bevy::ecs::component::HookContext) {
    let (g, hidden) = world.get_resource::<LevelSpawn>().map(|s| (s.group, s.hidden)).unwrap_or((0, false));
    let mut c = world.commands();
    let mut e = c.entity(ctx.entity);
    e.insert(LevelGroup(g));
    if hidden {
        e.insert(Visibility::Hidden);
    }
}

struct CachedLevel {
    key: Vec<PathBuf>,
    group: u32,
    post: hfpost::HfPost,
    shadow: sunshadow::SunShadow,
}

/// Levels kept loaded (front end).
#[derive(Resource, Default)]
pub struct Levels {
    next: u32,
    cached: Vec<CachedLevel>,
    /// number of finished load requests (the loader runs synchronously)
    pub loads: u32,
}

/// Main-menu background level: "HFstream" + the campaign chapter's map (GCUIInfo.m_SPMapInfos
/// MapName, EXE FUN_006a7900 builds "HFStream..."), the game's own pick follows campaign progress
/// (the user's game and trace frame 400 show chapter 1, HFstreamDevil_P). Without a campaign the
/// offline build uses chapter 1; U_MainMenuScreen.m_LevelName (HFstreamMainMenu) is the fallback.
fn menu_background(game: &std::path::Path) -> String {
    let chapter1 = (|| {
        let p = Package::open(game.join("GCGame").join("CookedPC").join("UI").join("GCUIInfos.upk")).ok()?;
        let i = p.exports.iter().position(|e| p.object_name_of(e) == "GCUIInfo" && p.export_class(e) == "GCUIInfo")?;
        let pr = p.export_props(i)?;
        let first = p.array_structs(pr.get("m_SPMapInfos")?).into_iter().next()?;
        match first.get("MapName") {
            Some(upk::props::Value::Name(n)) => Some(n.clone()),
            _ => None,
        }
    })();
    let name = chapter1.map(|c| format!("HFstream{c}.map")).unwrap_or_else(|| "HFstreamMainMenu.map".into());
    if game.join("GCGame").join("CookedPC").join("Maps").join(&name).exists() { name } else { "HFstreamMainMenu.map".into() }
}

/// Front-end camera: CameraActor_0 of HFGameEntry.map (the persistent front-end level that
/// streams HFstreamMainMenu/HFstreamOnlineMenu); trace frames 400-1200 show exactly its
/// location (0,-2064,0) and yaw 16384 in CameraPosition / ViewProjection.
fn menu_camera(game: &std::path::Path) -> Option<[f32; 5]> {
    let p = Package::open(game.join("GCGame").join("CookedPC").join("Maps").join("HFGameEntry.map")).ok()?;
    let i = p.exports.iter().position(|e| p.export_class(e) == "CameraActor")?;
    let pr = p.export_props(i)?;
    let loc = match pr.get("Location") {
        Some(upk::props::Value::Vector(v)) => *v,
        _ => [0.0; 3],
    };
    let rot = match pr.get("Rotation") {
        Some(upk::props::Value::Rotator(r)) => *r,
        _ => [0; 3],
    };
    let deg = |u: i32| u as f32 * 360.0 / 65536.0;
    Some([loc[0], loc[1], loc[2], deg(rot[1]), deg(rot[0])])
}

/// Texture AddressX/AddressY (UTexture2D defaults TA_Wrap).
fn tex_address(pkg: &Package, obj: i32) -> [upk::matcompile::Address; 2] {
    use upk::matcompile::Address;
    let conv = |v: Option<&str>| match v {
        Some("TA_Clamp") => Address::Clamp,
        Some("TA_Mirror") => Address::Mirror,
        _ => Address::Wrap,
    };
    match (obj > 0).then(|| pkg.export_props(obj as usize - 1)).flatten() {
        Some(p) => [conv(p.enum_name("AddressX")), conv(p.enum_name("AddressY"))],
        None => [Address::Wrap; 2],
    }
}

/// Terrain through the game's own shaders (terrainemu.rs). Returns false if the package has
/// no terrain or its shaders are not in the cache.
#[allow(clippy::too_many_arguments)]
fn spawn_terrain_emu(
    commands: &mut Commands,
    pkg: &Package,
    lib: &Library,
    sc: &upk::shadercache::ShaderCache,
    globals: &Handle<bevy::render::storage::ShaderStorageBuffer>,
    max_tex: u32,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    shaders: &mut Assets<Shader>,
    buffers: &mut Assets<bevy::render::storage::ShaderStorageBuffer>,
    terrain_mats: &mut Assets<terrainemu::TerrainEmu>,
) -> bool {
    use terrainemu::TexSrc;
    let Some(t) = pkg.terrain_render(&|id| sc.is_record(id)) else { return false };
    // palette -> (lighting resource MatType:1, colour resource MatType:0)
    let find = |pal: &str, mat_type: u32| {
        let want = format!("TerrainMaterialResource:{pal} (MatType:{mat_type})");
        t.resources.iter().find(|r| sc.record(&r.id).is_some_and(|rec| rec.friendly_name == want))
    };
    let mut tex_cache: HashMap<TexSrc, Option<Handle<Image>>> = HashMap::new();
    let mut smp_cache: HashMap<[upk::matcompile::Address; 2], Handle<Image>> = HashMap::new();
    let mut pal_mats: HashMap<u8, (Handle<Shader>, Vec<Option<Handle<Image>>>, Vec<Handle<Image>>)> = HashMap::new();
    for (pi, pal) in t.palette_names.iter().enumerate() {
        let (Some(rl), Some(rc)) = (find(pal, 1), find(pal, 0)) else {
            warn!("terrain palette {pal}: lighting/colour resources not found");
            continue;
        };
        let (Some(recl), Some(recc)) = (sc.record(&rl.id), sc.record(&rc.id)) else { continue };
        let vs = recl.shader("FTerrainLightingPassVertexFactory", "TBasePassVertexShaderFDirectionalLightLightMapPolicyFNoDensityPolicy");
        let psl = recl.shader("FTerrainLightingPassVertexFactory", "TBasePassPixelShaderFDirectionalLightLightMapPolicySkyLight");
        let psc = recc.shader("FTerrainVertexFactory", "FTerrainColorPixelShader");
        let (Some(vs), Some(psl), Some(psc)) = (vs.and_then(|g| sc.code(&g)), psl.and_then(|g| sc.code(&g)), psc.and_then(|g| sc.code(&g))) else {
            warn!("terrain palette {pal}: shaders missing in the cache");
            continue;
        };
        let addr = |o: i32| tex_address(pkg, o);
        let Some(code) = terrainemu::generate(vs, psl, psc, &rl.uniforms, &rc.uniforms, &t.weight_maps, &addr) else {
            warn!("terrain palette {pal}: shader generation failed");
            continue;
        };
        for p in &code.problems {
            warn!("terrain palette {pal}: {p}");
        }
        if std::env::var_os("HOMEBOTS_DUMP_WGSL").is_some() {
            let dir = std::env::var_os("HOMEBOTS_DUMP_WGSL").unwrap_or_default();
            let _ = std::fs::write(std::path::Path::new(&dir).join(format!("terrain_{pi}.wgsl")), &code.wgsl);
        }
        if let Err(e) = gamemat::validate_wgsl(&code.wgsl) {
            warn!("terrain palette {pal}: generated shader invalid:\n{e}");
            continue;
        }
        let sh = shaders.add(Shader::from_wgsl(code.wgsl.clone(), format!("hf_terrain_{pi}.wgsl")));
        let mut texs = Vec::new();
        for src in &code.textures {
            let h = tex_cache
                .entry(*src)
                .or_insert_with(|| match *src {
                    TexSrc::White => Some(images.add(Image::new_fill(
                        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                        TextureDimension::D2,
                        &[255, 255, 255, 255],
                        TextureFormat::Rgba8Unorm,
                        RenderAssetUsages::default(),
                    ))),
                    TexSrc::Object(o) => gamemat::load_texture(pkg, lib, o, tex_address(pkg, o), max_tex.max(1024)).map(|i| images.add(i)),
                    TexSrc::Scene | TexSrc::Grass | TexSrc::Black => None,
                })
                .clone();
            if h.is_none() {
                if let TexSrc::Object(o) = src {
                    warn!("terrain texture {} not loaded", pkg.object_path(*o));
                }
            }
            texs.push(h);
        }
        if std::env::var_os("HOMEBOTS_TERRAIN_TEX").is_some() {
            let names: Vec<String> = code.textures.iter().map(|s| match s { TexSrc::Object(o) => pkg.object_name(*o), o => format!("{o:?}") }).collect();
            eprintln!("terrain palette {pi} {pal}: {}", names.join(", "));
        }
        let mut smps = Vec::new();
        for a in &code.samplers {
            let h = smp_cache
                .entry(*a)
                .or_insert_with(|| {
                    let mut img = Image::new_fill(
                        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                        TextureDimension::D2,
                        &[255, 255, 255, 255],
                        TextureFormat::Rgba8Unorm,
                        RenderAssetUsages::default(),
                    );
                    if let Some(i) = gamemat::load_texture_sampler(*a) {
                        img.sampler = i;
                    }
                    images.add(img)
                })
                .clone();
            smps.push(h);
        }
        pal_mats.insert(pi as u8, (sh, texs, smps));
    }
    if pal_mats.is_empty() {
        return false;
    }
    let mut n = 0;
    // HOMEBOTS_TERRAIN_COMPS=a..b: only these components (debugging)
    let comp_range = std::env::var("HOMEBOTS_TERRAIN_COMPS").ok().and_then(|v| {
        let (a, b) = v.split_once("..")?;
        Some(a.parse::<usize>().ok()?..b.parse::<usize>().ok()?)
    });
    for (ci, c) in t.components.iter().enumerate() {
        if comp_range.as_ref().is_some_and(|r| !r.contains(&ci)) {
            continue;
        }
        let consts = terrainemu::draw_consts(&t, c);
        let draw = buffers.add(bevy::render::storage::ShaderStorageBuffer::new(bytemuck::cast_slice(&consts), RenderAssetUsages::default()));
        for (pal, mesh) in terrainemu::component_meshes(&t, c) {
            let pal = std::env::var("HOMEBOTS_TERRAIN_PAL").ok().and_then(|v| v.parse::<u8>().ok()).unwrap_or(pal);
            let Some((sh, texs, smps)) = pal_mats.get(&pal).or_else(|| pal_mats.get(&0)) else { continue };
            let mut m = terrainemu::TerrainEmu::new(globals.clone(), draw.clone(), sh.clone());
            for (k, h) in texs.iter().enumerate() {
                if let Some(h) = h {
                    m.set_tex(k, h.clone());
                }
            }
            for (k, h) in smps.iter().enumerate() {
                m.set_smp(k, h.clone());
            }
            commands.spawn((LevelEntity, Mesh3d(meshes.add(mesh)), MeshMaterial3d(terrain_mats.add(m)), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver));
            n += 1;
        }
    }
    info!("terrain (game shaders): {} components, {n} draws, palettes {:?}", t.components.len(), t.palette_names);
    true
}

/// The level's PostProcessVolumes (main.rs load_maps) and the one in use.
#[derive(Resource, Default)]
pub struct PostVolumes {
    list: Vec<upk::level::PpVolume>,
    /// the level's pick at load (highest Priority / WorldInfo defaults): used outside every volume
    default: Option<upk::level::PostProcess>,
    /// index into `list` in use (-1 = `default`), None = not chosen yet
    active: Option<i64>,
}

/// Uses the post-process settings of the volume the camera is in, like UE3 (the highest Priority
/// among the volumes that contain the view point). Before, the volume with the highest Priority
/// of the whole level was used everywhere: on Bridge that is the under-water volume (blue tint,
/// 40 % desaturation) and the whole map looked blue / like night (user 2026-10-22 19:43).
/// Containment is tested against the brush's bounding box (the volumes seen are boxes; NOT from
/// data for other shapes). The switch is immediate (Scene_InterpolationDuration not applied).
fn post_volume_select(mut pv: ResMut<PostVolumes>, mut q: Query<(&GlobalTransform, &mut hfpost::HfPost), With<Camera3d>>) {
    if pv.list.len() < 2 && pv.active.is_some() {
        return;
    }
    for (gt, mut p) in &mut q {
        let t = gt.translation();
        let ue = [t.x * 100.0, t.z * 100.0, t.y * 100.0];
        let pick = pv.list.iter().enumerate().filter(|(_, v)| (0..3).all(|k| ue[k] >= v.min[k] && ue[k] <= v.max[k])).max_by(|a, b| a.1.priority.total_cmp(&b.1.priority)).map(|(i, _)| i);
        let key = pick.map(|i| i as i64).unwrap_or(-1);
        if pv.active == Some(key) {
            continue;
        }
        pv.active = Some(key);
        let s = match pick {
            Some(i) => pv.list[i].settings.clone(),
            None => pv.default.clone().unwrap_or_default(),
        };
        // gamma (options) and DeltaTime stay; the bloom option is re-applied by options.rs
        let (g, dt) = (p.gamma.w, p.misc.y);
        *p = hfpost::HfPost::from_level(&s, 2.2);
        p.gamma.w = g;
        p.misc.y = dt;
        info!("post-process: {}", pick.map(|i| pv.list[i].name.as_str()).unwrap_or("level default (no volume)"));
    }
}

/// DeltaTime for the eye adaptation. In screenshot mode the adaptation is run to
/// its steady state (the value a player standing still converges to).
fn post_delta_time(time: Res<Time>, args: NonSend<Args>, mut q: Query<&mut hfpost::HfPost>) {
    for mut p in &mut q {
        p.misc.y = if args.shot.is_some() { 1.0e9 } else { time.delta_secs() };
    }
}

/// Screenshot mode: wait for assets to settle, save one frame, exit.
fn shot_system(mut commands: Commands, args: NonSend<Args>, mut frames: Local<u32>, mut exit: EventWriter<AppExit>) {
    let Some((path, _)) = &args.shot else { return };
    *frames += 1;
    // HOMEBOTS_SHOT_SEQ=100,400,900: one screenshot per listed frame (<path stem>_<frame>.png)
    if let Ok(seq) = std::env::var("HOMEBOTS_SHOT_SEQ") {
        let list: Vec<u32> = seq.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        use bevy::render::view::screenshot::{save_to_disk, Screenshot};
        let stem = path.with_extension("");
        let file = |f: u32| PathBuf::from(format!("{}_{f}.png", stem.display()));
        if list.contains(&*frames) {
            let _ = std::fs::remove_file(file(*frames));
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(file(*frames)));
        }
        let last = list.iter().copied().max().unwrap_or(0);
        if *frames > last && (file(last).exists() || *frames > last + 3000) {
            exit.write(AppExit::Success);
        }
        return;
    }
    let at: u32 = std::env::var("HOMEBOTS_SHOT_FRAME").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
    if *frames == at {
        use bevy::render::view::screenshot::{save_to_disk, Screenshot};
        let _ = std::fs::remove_file(path);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
    }
    // exit once the readback has been written (software rasterisers can take many frames)
    if *frames > at && (path.exists() || *frames > at + 3000) {
        exit.write(AppExit::Success);
    }
}

fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cams: Query<(&mut Transform, &mut FlyCam)>,
    in_menu: Option<Res<menu::InMenu>>,
    flow: Option<Res<menu::MatchFlow>>,
) {
    // in a match the player pawn owns the camera (player.rs)
    if flow.is_some_and(|f| f.phase == menu::Phase::Playing) {
        return;
    }
    let Ok((mut tf, mut cam)) = cams.single_mut() else { return };
    // front end: fixed camera, the mouse belongs to the menus
    if in_menu.is_some_and(|m| m.0) {
        tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
        return;
    }
    let looking = mouse.pressed(MouseButton::Right);
    if let Ok(mut w) = windows.single_mut() {
        let (grab, vis) = if looking { (CursorGrabMode::Locked, false) } else { (CursorGrabMode::None, true) };
        if w.cursor_options.grab_mode != grab {
            w.cursor_options.grab_mode = grab;
            w.cursor_options.visible = vis;
        }
    }
    if looking {
        cam.yaw -= motion.delta.x * 0.003;
        cam.pitch = (cam.pitch - motion.delta.y * 0.003).clamp(-1.54, 1.54);
    }
    tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    let mut dir = Vec3::ZERO;
    let (f, r) = (tf.forward().as_vec3(), tf.right().as_vec3());
    if keys.pressed(KeyCode::KeyW) { dir += f; }
    if keys.pressed(KeyCode::KeyS) { dir -= f; }
    if keys.pressed(KeyCode::KeyD) { dir += r; }
    if keys.pressed(KeyCode::KeyA) { dir -= r; }
    if keys.pressed(KeyCode::KeyE) { dir += Vec3::Y; }
    if keys.pressed(KeyCode::KeyQ) { dir -= Vec3::Y; }
    let mut speed = 12.0;
    if keys.pressed(KeyCode::ShiftLeft) { speed *= 5.0; }
    if keys.pressed(KeyCode::ControlLeft) { speed *= 0.2; }
    tf.translation += dir.normalize_or_zero() * speed * time.delta_secs();
}

/// One D3D vertex element of a SpeedTree vertex declaration (trace 2026-10-15).
pub struct D3dElem {
    pub stream: u8,
    pub offset: usize,
    pub ty: D3dType,
    pub usages: &'static [(upk::sm3::Usage, u32)],
}

#[derive(Clone, Copy)]
pub enum D3dType {
    Float1,
    Float2,
    Float3,
    Float4,
    UByte4,
}

/// Vertex element as the shader sees it (D3D defaults for missing components; UBYTE4 as
/// raw byte values).
pub fn d3d_element(b: &[u8], ty: D3dType) -> [f32; 4] {
    let f = |i: usize| f32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
    match ty {
        D3dType::Float1 => [f(0), 0.0, 0.0, 1.0],
        D3dType::Float2 => [f(0), f(1), 0.0, 1.0],
        D3dType::Float3 => [f(0), f(1), f(2), 1.0],
        D3dType::Float4 => [f(0), f(1), f(2), f(3)],
        D3dType::UByte4 => [b[0] as f32, b[1] as f32, b[2] as f32, b[3] as f32],
    }
}

/// Vertex declaration of a SpeedTree element group (upk::speedtree::GROUP_VF), from the
/// trace's CreateVertexDeclaration calls. None = not seen yet (leaf meshes).
pub fn speedtree_layout(group: usize) -> Option<&'static [D3dElem]> {
    use upk::sm3::Usage::*;
    use D3dType::*;
    const BRANCH: &[D3dElem] = &[
        D3dElem { stream: 0, offset: 0, ty: Float3, usages: &[(Position, 0)] },
        D3dElem { stream: 1, offset: 0, ty: UByte4, usages: &[(Tangent, 0)] },
        D3dElem { stream: 1, offset: 4, ty: UByte4, usages: &[(Binormal, 0)] },
        D3dElem { stream: 1, offset: 8, ty: UByte4, usages: &[(Normal, 0)] },
        D3dElem { stream: 1, offset: 12, ty: Float4, usages: &[(TexCoord, 0)] },
        D3dElem { stream: 1, offset: 28, ty: Float4, usages: &[(BlendIndices, 0)] },
        D3dElem { stream: 1, offset: 44, ty: Float4, usages: &[(TexCoord, 2), (TexCoord, 3), (TexCoord, 1)] },
        D3dElem { stream: 1, offset: 60, ty: Float1, usages: &[(TexCoord, 6)] },
    ];
    const FROND: &[D3dElem] = &[
        D3dElem { stream: 0, offset: 0, ty: Float3, usages: &[(Position, 0)] },
        D3dElem { stream: 1, offset: 0, ty: UByte4, usages: &[(Tangent, 0)] },
        D3dElem { stream: 1, offset: 4, ty: UByte4, usages: &[(Binormal, 0)] },
        D3dElem { stream: 1, offset: 8, ty: UByte4, usages: &[(Normal, 0)] },
        D3dElem { stream: 1, offset: 12, ty: Float4, usages: &[(TexCoord, 0)] },
        D3dElem { stream: 1, offset: 28, ty: Float4, usages: &[(BlendIndices, 0)] },
        D3dElem { stream: 1, offset: 44, ty: Float4, usages: &[(TexCoord, 1)] },
        D3dElem { stream: 1, offset: 60, ty: Float1, usages: &[(TexCoord, 6)] },
        D3dElem { stream: 1, offset: 64, ty: Float2, usages: &[(TexCoord, 3), (TexCoord, 2)] },
    ];
    const LEAF_CARD: &[D3dElem] = &[
        D3dElem { stream: 0, offset: 0, ty: Float3, usages: &[(Position, 0)] },
        D3dElem { stream: 1, offset: 0, ty: UByte4, usages: &[(Tangent, 0)] },
        D3dElem { stream: 1, offset: 4, ty: UByte4, usages: &[(Binormal, 0)] },
        D3dElem { stream: 1, offset: 8, ty: UByte4, usages: &[(Normal, 0)] },
        D3dElem { stream: 1, offset: 12, ty: Float2, usages: &[(TexCoord, 0)] },
        D3dElem { stream: 1, offset: 28, ty: Float4, usages: &[(BlendIndices, 0)] },
        D3dElem { stream: 1, offset: 44, ty: Float4, usages: &[(TexCoord, 1)] },
        D3dElem { stream: 1, offset: 60, ty: Float1, usages: &[(TexCoord, 6)] },
        D3dElem { stream: 1, offset: 64, ty: Float3, usages: &[(TexCoord, 3), (TexCoord, 2)] },
    ];
    const BILLBOARD: &[D3dElem] = &[
        D3dElem { stream: 0, offset: 0, ty: Float3, usages: &[(Position, 0)] },
        D3dElem { stream: 1, offset: 0, ty: UByte4, usages: &[(Tangent, 0)] },
        D3dElem { stream: 1, offset: 4, ty: UByte4, usages: &[(Binormal, 0)] },
        D3dElem { stream: 1, offset: 8, ty: UByte4, usages: &[(Normal, 0)] },
        D3dElem { stream: 1, offset: 12, ty: Float2, usages: &[(TexCoord, 0)] },
        D3dElem { stream: 1, offset: 64, ty: Float1, usages: &[(TexCoord, 2), (TexCoord, 3), (TexCoord, 6), (TexCoord, 1)] },
    ];
    match group {
        0 | 1 => Some(BRANCH),
        2 => Some(FROND),
        3 => Some(LEAF_CARD),
        // billboards replace LOD 0 only far away; LOD switching is not done yet
        _ => None,
    }
}

/// Triangles of placed mesh sections for player movement (player.rs).
/// Physical material ids of a mesh's sections (impacts.rs): the placement's material override,
/// else the section's own material.
/// Static meshes that are road surface (pa.rs: no poles on them): the highway road set's lanes
/// (STA_highway.roadSet.HW_2Lane_2way_Traffic*, also the overpass decks) and road pieces
/// (STA_Commercial.Road.Road_Parking_1); road spline meshes are marked where they are placed.
fn is_road_mesh(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    (p.contains("roadset.") && p.contains("lane")) || p.contains(".road.road_")
}

fn part_phys(sp: &world::Spawner, impacts: &mut impacts::Impacts, mesh_pkg: usize, parts: &[(Handle<Mesh>, i32)], mat_pkg: usize, overrides: &[i32]) -> Vec<u16> {
    parts
        .iter()
        .enumerate()
        .map(|(k, (_, m))| {
            let path = match overrides.get(k).copied().filter(|&o| o != 0) {
                Some(o) => impacts::material_phys(sp, mat_pkg, o),
                None => impacts::material_phys(sp, mesh_pkg, *m),
            };
            path.map(|p| impacts.id(&p)).unwrap_or(0)
        })
        .collect()
}

/// Bevy 0.16 GPU mesh preprocessing caches each entity's vertex / index slab offsets and only
/// refreshes them when the entity's components change. A mesh asset rewritten every frame
/// (particle sprites, CPU-skinned bots / vehicles / weapons) is freed and reallocated in the slab,
/// so an entity that did not move kept drawing the old offsets: other meshes' vertices with its
/// indices = huge stretched triangles (seen on the PC's GPU, not with the container's software
/// driver). Entities whose mesh was modified are marked changed so they are re-extracted.
fn touch_modified_meshes(mut ev: EventReader<AssetEvent<Mesh>>, me: Res<MeshEntities>, mut q: Query<&mut Mesh3d>, mut set: Local<std::collections::HashSet<AssetId<Mesh>>>) {
    set.clear();
    if std::env::var_os("HB_NO_TOUCH").is_some() {
        ev.clear();
        return;
    }
    for e in ev.read() {
        if let AssetEvent::Modified { id } = e {
            set.insert(*id);
        }
    }
    if set.is_empty() {
        return;
    }
    for id in set.iter() {
        for &e in me.0.get(id).into_iter().flatten() {
            if let Ok(mut m) = q.get_mut(e) {
                m.set_changed();
            }
        }
    }
}

/// Entities drawing each mesh asset (skinned parts look their entities up here instead of
/// scanning every mesh entity of the level each frame).
#[derive(Resource, Default)]
pub struct MeshEntities(pub std::collections::HashMap<AssetId<Mesh>, Vec<Entity>>);

fn index_mesh_entities(mut me: ResMut<MeshEntities>, added: Query<(Entity, &Mesh3d), Added<Mesh3d>>, mut removed: RemovedComponents<Mesh3d>) {
    let gone: std::collections::HashSet<Entity> = removed.read().collect();
    if !gone.is_empty() {
        me.0.retain(|_, v| {
            v.retain(|e| !gone.contains(e));
            !v.is_empty()
        });
    }
    for (e, h) in &added {
        let v = me.0.entry(h.id()).or_default();
        if !v.contains(&e) {
            v.push(e);
        }
    }
}

/// Entities of the given mesh handles.
pub fn mesh_ents<'a>(me: &'a MeshEntities, meshes: &'a [Handle<Mesh>]) -> impl Iterator<Item = Entity> + 'a {
    meshes.iter().flat_map(move |h| me.0.get(&h.id()).into_iter().flatten().copied())
}

pub(crate) fn add_collision(col: &mut player::CollisionWorld, meshes: &Assets<Mesh>, parts: &[(Handle<Mesh>, i32)], t: &Transform, phys: &[u16]) {
    let m = t.compute_matrix();
    let keep_phys = col.add_phys;
    for (k, (h, _)) in parts.iter().enumerate() {
        col.add_phys = phys.get(k).copied().unwrap_or(0);
        let Some(mesh) = meshes.get(h) else { continue };
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        let pos: Vec<Vec3> = pos.iter().map(|p| m.transform_point3(Vec3::from(*p))).collect();
        let Some(Indices::U32(idx)) = mesh.indices() else { continue };
        for tri in idx.chunks_exact(3) {
            let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            if a < pos.len() && b < pos.len() && c < pos.len() {
                col.add([pos[a], pos[b], pos[c]]);
            }
        }
    }
    col.add_phys = keep_phys;
}
