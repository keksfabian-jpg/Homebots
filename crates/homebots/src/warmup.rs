//! Render pipeline warm-up and the match preload.
//!
//! Bevy builds a mesh's render pipeline the first time the mesh is drawn and compiles it in the
//! background; until then the mesh is skipped (user 2026-10-20: weapons "popped in" on the
//! first spawn; 2026-10-21: rocks, houses and the sky appeared frame by frame after the first
//! spawn). Materials whose textures are not on the GPU yet are skipped the same way.
//!
//! Preload (user 2026-10-21 18:27 "Ich will, dass alles vorlädt"): once the match level is
//! built, the scene camera renders behind the black loading screen / the movie (its picture is
//! not written to the window), and everything of the match that has not been drawn yet
//! (viewmodel weapons and arms, vehicles, drones, projectiles, effect pools, grass, every part of
//! the level) gets a stand-in: the same mesh and material, drawn 50 km behind the camera
//! (clipped, no pixels). One stand-in per material and vertex layout. The loading screen / movie
//! stays until all textures and materials are on the GPU and Bevy's pipeline cache has had
//! nothing left to compile for some frames; the stand-ins stay through the spawn selection and
//! are removed when the match runs. HB_NOWARMUP=1 switches it off. NOT from data (renderer detail).
use crate::menu::{MatchFlow, Phase};
use bevy::pbr::{MeshMaterial3d, NotShadowCaster, NotShadowReceiver, PreparedMaterial};
use bevy::prelude::*;
use bevy::render::mesh::{MeshTag, MeshVertexAttributeId};
use bevy::render::render_asset::{RenderAssetUsages, RenderAssets};
use bevy::render::render_resource::{CachedPipelineState, PipelineCache};
use bevy::render::texture::GpuImage;
use bevy::render::view::NoFrustumCulling;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSet};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;

pub struct WarmUpPlugin;

/// Wall-clock seconds since the first call. The preload's timers, its cap and its log lines use
/// this, not `Time<Real>`: under HB_RECORD (tests) `TimeUpdateStrategy::ManualDuration` advances
/// `Time<Real>` by 1/fps per frame, so at 5 fps every rendered frame counted 0.2 s (the "96 s
/// preload" of test d2 was 482 frames in 7.6 s on the clock).
pub fn wall() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// Material types that get stand-ins (and whose GPU preparation the preload waits for).
const WARM_TYPES: usize = 5;

impl Plugin for WarmUpPlugin {
    fn build(&self, app: &mut App) {
        let probe = Probe::default();
        app.init_resource::<WarmUp>().init_resource::<Preload>().insert_resource(probe.clone()).add_systems(
            Update,
            (
                warm_up::<crate::gamemat::GameMat>,
                warm_up::<crate::terrainemu::TerrainEmu>,
                warm_up::<crate::HfMat>,
                warm_up::<crate::grass::GrassMat>,
                warm_up::<StandardMaterial>,
                place_and_end,
                preload_tick,
            )
                .chain(),
        );
        if let Some(ra) = app.get_sub_app_mut(RenderApp) {
            ra.insert_resource(probe).add_systems(ExtractSchedule, probe_assets).add_systems(Render, probe_pipelines.in_set(RenderSet::Cleanup));
        }
    }
}

#[derive(Resource, Default)]
pub struct WarmUp {
    /// level load the stand-ins were made for (Levels.loads)
    done_for: Option<u32>,
    /// material types handled for that load
    types: HashSet<std::any::TypeId>,
    /// (material, vertex layout) pairs with a stand-in
    seen: HashSet<(bevy::asset::UntypedAssetId, u64)>,
    count: usize,
}

/// Stand-in for the warm-up (removed when the player spawns).
#[derive(Component)]
pub struct WarmClone;

/// Match preload state (menu.rs starts it when the match level is built and holds the loading
/// screen / movie until `pending()` is false).
#[derive(Resource, Default)]
pub struct Preload {
    active: bool,
    /// wall clock (`wall()`) when the level build finished
    since: f64,
    /// render frames seen when the level build finished
    since_frame: Option<u32>,
    /// main frames with nothing pending in a row
    stable: u32,
    /// render frames seen when the stand-ins were spawned
    clones_frame: Option<u32>,
    /// pipelines in the cache when the preload started
    pipelines_at_start: Option<usize>,
    /// last time anything finished (pipeline compiled, texture / material uploaded)
    progress: (usize, usize, usize, f64),
    /// wall clock (`wall()`) when the loading screen started waiting for the preload (after the
    /// movie / skip)
    pub held_since: Option<f64>,
}

impl Preload {
    /// The match level is built: hold the loading screen until everything is on the GPU.
    pub fn begin(&mut self, _now: f64) {
        let off = std::env::var_os("HB_NOWARMUP").is_some();
        *self = Preload { active: !off, since: wall(), ..default() };
    }
    /// Still loading (the loading screen / movie stays).
    pub fn pending(&self) -> bool {
        self.active
    }
    pub fn cancel(&mut self) {
        self.active = false;
    }
}

/// Render-world progress the preload waits for (written by the render app, read in Update).
#[derive(Default)]
pub struct ProbeState {
    armed: AtomicBool,
    frame: AtomicU32,
    waiting: AtomicUsize,
    pipelines: AtomicUsize,
    ok: AtomicUsize,
    images_pending: AtomicUsize,
    images_total: AtomicUsize,
    mats_pending: AtomicUsize,
    mats_total: AtomicUsize,
}

#[derive(Resource, Clone, Default)]
pub struct Probe(Arc<ProbeState>);

fn layout_key(m: &Mesh) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (m.primitive_topology() as u8).hash(&mut h);
    let mut ids: Vec<MeshVertexAttributeId> = m.attributes().map(|(a, _)| a.id).collect();
    ids.sort_by_key(|id| format!("{id:?}"));
    for id in ids {
        format!("{id:?}").hash(&mut h);
    }
    h.finish()
}

/// The warm-up runs during the spawn selection and during the match preload.
fn warming(flow: &MatchFlow, pre: &Preload) -> bool {
    flow.phase == Phase::Spawn || (pre.active && matches!(flow.phase, Phase::Loading | Phase::Movie))
}

#[allow(clippy::type_complexity)]
fn warm_up<M: Material>(
    mut commands: Commands,
    flow: Res<MatchFlow>,
    pre: Res<Preload>,
    levels: Res<crate::Levels>,
    mut wu: ResMut<WarmUp>,
    meshes: Res<Assets<Mesh>>,
    // all 3D entities, not only the level's (the nuke's plane and props are made at start-up)
    ents: Query<(&Mesh3d, &MeshMaterial3d<M>, &ViewVisibility, Option<&MeshTag>, Has<NotShadowCaster>, Has<NotShadowReceiver>), Without<WarmClone>>,
) {
    // the match's spawn selection or preload, after its level load
    if !warming(&flow, &pre) || levels.loads == 0 || std::env::var_os("HB_NOWARMUP").is_some() {
        return;
    }
    if wu.done_for != Some(levels.loads) {
        wu.done_for = Some(levels.loads);
        wu.types.clear();
        wu.seen.clear();
        wu.count = 0;
    }
    if !wu.types.insert(std::any::TypeId::of::<M>()) {
        return;
    }
    let before = wu.count;
    for (mesh, mat, vis, tag, no_cast, no_recv) in &ents {
        // drawn already: its pipeline exists
        if vis.get() {
            continue;
        }
        let Some(m) = meshes.get(&mesh.0) else { continue };
        let key = (mat.0.id().untyped(), layout_key(m) ^ ((no_cast as u64) << 1) ^ (no_recv as u64));
        if !wu.seen.insert(key) {
            continue;
        }
        let mut e = commands.spawn((crate::LevelEntity, WarmClone, Mesh3d(mesh.0.clone()), MeshMaterial3d(mat.0.clone()), Transform::default(), Visibility::Visible, NoFrustumCulling));
        if let Some(t) = tag {
            e.insert(MeshTag(t.0));
        }
        if no_cast {
            e.insert(NotShadowCaster);
        }
        if no_recv {
            e.insert(NotShadowReceiver);
        }
        wu.count += 1;
    }
    info!("warm-up: {} stand-ins for {} (load {})", wu.count - before, std::any::type_name::<M>().rsplit("::").next().unwrap_or(""), levels.loads);
}

/// Keeps the stand-ins 50 km behind the camera (clipped) and removes them once the match runs.
fn place_and_end(mut commands: Commands, flow: Res<MatchFlow>, cams: Query<&GlobalTransform, (With<Camera3d>, Without<crate::minimap::MinimapCam>)>, mut clones: Query<(Entity, &mut Transform), With<WarmClone>>) {
    if clones.is_empty() {
        return;
    }
    if matches!(flow.phase, Phase::Playing | Phase::Front | Phase::Leaving) {
        let n = clones.iter().count();
        for (e, _) in &clones {
            commands.entity(e).despawn();
        }
        info!("warm-up: {n} stand-ins removed");
        return;
    }
    let Ok(cam) = cams.single() else { return };
    let at = cam.translation() - cam.forward() * 50_000.0;
    for (_, mut tf) in &mut clones {
        if tf.translation != at {
            tf.translation = at;
        }
    }
}

/// Main frames in a row with nothing to compile or upload before the preload counts as done.
const STABLE_FRAMES: u32 = 10;
/// Render frames after the stand-ins were spawned before "nothing pending" counts (extract,
/// prepare, specialise, queue the pipelines).
const SETTLE_RENDER_FRAMES: u32 = 4;
/// Nothing finished for this long while something is still pending: stop waiting (a texture
/// or pipeline that never gets ready must not hold the loading screen forever).
const STALL_SECS: f64 = 8.0;
/// Nothing compiling and no upload finished for this long: the rest never gets ready.
const QUIET_SECS: f64 = 2.0;
/// Upper bound for the whole preload in wall-clock seconds (HB_PRELOAD_MAX=<s> overrides; NOT
/// from data). The user's AMD iGPU needed ~7.6 s wall for ~485 pipelines (the "~95 s" in the
/// test logs were Time<Real> under the HB_RECORD fixed step, see `wall()`); the cap only guards
/// against a driver that takes far longer (the rest then compiles in play).
fn max_secs() -> f64 {
    std::env::var("HB_PRELOAD_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(25.0)
}

fn preload_tick(mut pre: ResMut<Preload>, probe: Res<Probe>, flow: Res<MatchFlow>, wu: Res<WarmUp>, levels: Res<crate::Levels>) {
    let p = &probe.0;
    if !pre.active || !matches!(flow.phase, Phase::Loading | Phase::Movie | Phase::Spawn | Phase::Creating) {
        if pre.active {
            pre.active = false;
        }
        p.armed.store(false, Relaxed);
        return;
    }
    p.armed.store(true, Relaxed);
    let now = wall();
    let frame = p.frame.load(Relaxed);
    let since_frame = *pre.since_frame.get_or_insert(frame);
    let (waiting, total, ok) = (p.waiting.load(Relaxed), p.pipelines.load(Relaxed), p.ok.load(Relaxed));
    let (img_pending, img_total, mat_pending, mat_total) = (p.images_pending.load(Relaxed), p.images_total.load(Relaxed), p.mats_pending.load(Relaxed), p.mats_total.load(Relaxed));
    if pre.pipelines_at_start.is_none() {
        pre.pipelines_at_start = Some(total);
    }
    // stand-ins of all material types spawned for this load
    let clones_ready = wu.done_for == Some(levels.loads) && wu.types.len() >= WARM_TYPES;
    if !clones_ready {
        return;
    }
    let cf = *pre.clones_frame.get_or_insert(frame);
    let settled = frame.wrapping_sub(cf) >= SETTLE_RENDER_FRAMES;
    let pending = waiting + img_pending + mat_pending;
    // progress: compiled pipelines, uploaded textures / materials
    let prog = (ok, img_total - img_pending.min(img_total), mat_total - mat_pending.min(mat_total));
    if (prog.0, prog.1, prog.2) != (pre.progress.0, pre.progress.1, pre.progress.2) || pre.progress.3 == 0.0 {
        pre.progress = (prog.0, prog.1, prog.2, now);
    }
    pre.stable = if settled && pending == 0 { pre.stable + 1 } else { 0 };
    let elapsed = now - pre.since;
    // nothing compiling and the uploads stand still: textures / materials that never get ready
    // (a material whose texture is missing) must not hold the loading screen
    let quiet = settled && waiting == 0 && pending > 0 && now - pre.progress.3 > QUIET_SECS;
    let stalled = pending > 0 && now - pre.progress.3 > STALL_SECS;
    if pre.stable >= STABLE_FRAMES || quiet || stalled || elapsed > max_secs() {
        if quiet {
            info!("preload: {img_pending} textures, {mat_pending} materials did not get ready (nothing left to compile)");
        } else if stalled || elapsed > max_secs() {
            warn!("preload: stopped waiting after {elapsed:.1} s ({waiting} pipelines compiling, {img_pending} textures, {mat_pending} materials not on the GPU)");
        }
        let compiled = total.saturating_sub(pre.pipelines_at_start.unwrap_or(total));
        info!(
            "preload done: {total} pipelines ({compiled} compiled during the preload, {ok} ready), {img_total} textures, {mat_total} materials on the GPU, {} warm-up stand-ins, {elapsed:.2} s (wall clock, {} render frames) after the level build, {} compile threads",
            wu.count,
            frame.wrapping_sub(since_frame),
            bevy::tasks::AsyncComputeTaskPool::get().thread_num()
        );
        pre.active = false;
        p.armed.store(false, Relaxed);
    }
}

/// Textures and materials of the main world that are not prepared on the GPU yet.
#[allow(clippy::too_many_arguments)]
fn probe_assets(
    probe: Res<Probe>,
    images: Extract<Res<Assets<Image>>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    m1: Extract<Option<Res<Assets<crate::gamemat::GameMat>>>>,
    r1: Option<Res<RenderAssets<PreparedMaterial<crate::gamemat::GameMat>>>>,
    m2: Extract<Option<Res<Assets<crate::terrainemu::TerrainEmu>>>>,
    r2: Option<Res<RenderAssets<PreparedMaterial<crate::terrainemu::TerrainEmu>>>>,
    m3: Extract<Option<Res<Assets<crate::HfMat>>>>,
    r3: Option<Res<RenderAssets<PreparedMaterial<crate::HfMat>>>>,
    m4: Extract<Option<Res<Assets<crate::grass::GrassMat>>>>,
    r4: Option<Res<RenderAssets<PreparedMaterial<crate::grass::GrassMat>>>>,
) {
    let p = &probe.0;
    if !p.armed.load(Relaxed) {
        return;
    }
    let (mut ip, mut it) = (0, 0);
    for (id, img) in images.iter() {
        if !img.asset_usage.contains(RenderAssetUsages::RENDER_WORLD) {
            continue;
        }
        it += 1;
        if gpu_images.get(id).is_none() {
            ip += 1;
        }
    }
    let (mut mp, mut mt) = (0, 0);
    fn count<M: Material>(m: &Option<Res<Assets<M>>>, r: &Option<Res<RenderAssets<PreparedMaterial<M>>>>, mp: &mut usize, mt: &mut usize) {
        let (Some(m), Some(r)) = (m, r) else { return };
        for (id, _) in m.iter() {
            *mt += 1;
            if r.get(id).is_none() {
                *mp += 1;
            }
        }
    }
    count(&m1, &r1, &mut mp, &mut mt);
    count(&m2, &r2, &mut mp, &mut mt);
    count(&m3, &r3, &mut mp, &mut mt);
    count(&m4, &r4, &mut mp, &mut mt);
    p.images_pending.store(ip, Relaxed);
    p.images_total.store(it, Relaxed);
    p.mats_pending.store(mp, Relaxed);
    p.mats_total.store(mt, Relaxed);
}

/// Pipelines still queued / compiling after this frame's pipeline cache pass (every frame:
/// the count at the start of a preload is the baseline of its log line).
fn probe_pipelines(probe: Res<Probe>, cache: Res<PipelineCache>) {
    let p = &probe.0;
    let all: Vec<_> = cache.pipelines().collect();
    let ok = all.iter().filter(|c| matches!(c.state, CachedPipelineState::Ok(_))).count();
    let total = all.len();
    // queued or compiling; failed ones that the cache keeps retrying (shader not loaded) do not count
    let compiling = cache.waiting_pipelines().filter(|&id| all.get(id).is_some_and(|c| matches!(c.state, CachedPipelineState::Queued | CachedPipelineState::Creating(_)))).count();
    p.waiting.store(compiling, Relaxed);
    p.pipelines.store(total, Relaxed);
    p.ok.store(ok, Relaxed);
    p.frame.fetch_add(1, Relaxed);
}
