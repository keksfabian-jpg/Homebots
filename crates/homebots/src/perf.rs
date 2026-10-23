//! Frame profiler (user request 2026-10-18).
//!
//! Every Bevy system, schedule, render-graph node and render sub-app is timed through the
//! tracing spans of Bevy's `trace` feature (a tracing layer, `PerfLayer`, adds the span times
//! into lock-free slots); `perf::scope` adds own scopes, `perf::count` counters. GPU time per
//! pass comes from Bevy's RenderDiagnosticsPlugin (timestamp queries, DX12 / Vulkan), memory
//! from the sysinfo plugin.
//!
//! - F2: overlay (FPS, frame-time graph, CPU top list, GPU passes, counts)
//! - Shift+F2 (or the button / F1 menu): recording to logs/perf_<unix>.jsonl (one JSON object
//!   per frame) and, when it stops, logs/perf_<unix>_summary.txt (percentiles per system,
//!   worst frames with their causes, GPU passes, counts, machine and settings)
//! - HB_PERF=1: overlay on, recording starts with the match (HB_PERF_REC=<s>, default 180 s),
//!   plus a log line every 5 s
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::entity::Entities;
use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::{span, Subscriber};
use bevy::log::tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::ViewVisibility;
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

// ---------------------------------------------------------------------------------------------
// span / scope capture

const MAX_KEYS: usize = 8192;

struct Slot {
    ns: AtomicU64,
    n: AtomicU32,
}

static SLOTS: LazyLock<Box<[Slot]>> = LazyLock::new(|| (0..MAX_KEYS).map(|_| Slot { ns: AtomicU64::new(0), n: AtomicU32::new(0) }).collect());
static NKEYS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    System,
    Commands,
    Cond,
    Schedule,
    Node,
    Scope,
    Count,
    Other,
}

impl Kind {
    fn tag(self) -> &'static str {
        match self {
            Kind::System => "sys",
            Kind::Commands => "cmd",
            Kind::Cond => "cond",
            Kind::Schedule => "sched",
            Kind::Node => "node",
            Kind::Scope => "scope",
            Kind::Count => "count",
            Kind::Other => "span",
        }
    }
}

#[derive(Default)]
struct Keys {
    map: HashMap<Kind, HashMap<String, u32>>,
    names: Vec<(Kind, String)>,
}

static KEYS: LazyLock<Mutex<Keys>> = LazyLock::new(|| Mutex::new(Keys::default()));

fn intern(kind: Kind, name: &str) -> Option<u32> {
    let mut g = KEYS.lock().ok()?;
    if let Some(&k) = g.map.get(&kind).and_then(|m| m.get(name)) {
        return Some(k);
    }
    let k = g.names.len() as u32;
    if k as usize >= MAX_KEYS {
        return None;
    }
    g.names.push((kind, name.to_string()));
    g.map.entry(kind).or_default().insert(name.to_string(), k);
    NKEYS.store(k + 1, Relaxed);
    Some(k)
}

fn key_name(k: u32) -> (Kind, String) {
    KEYS.lock().ok().and_then(|g| g.names.get(k as usize).cloned()).unwrap_or((Kind::Other, "?".into()))
}

fn add(k: u32, ns: u64) {
    if let Some(s) = SLOTS.get(k as usize) {
        s.ns.fetch_add(ns, Relaxed);
        s.n.fetch_add(1, Relaxed);
    }
}

/// Times all Bevy spans (systems, schedules, render nodes ...) into the slots.
pub struct PerfLayer;

struct SpanKey(u32);
struct Entered(Instant);

struct NameVisit(Option<String>);

impl Visit for NameVisit {
    fn record_str(&mut self, f: &Field, v: &str) {
        if f.name() == "name" {
            self.0 = Some(v.to_string());
        }
    }
    fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
        if f.name() == "name" {
            self.0 = Some(format!("{v:?}"));
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for PerfLayer {
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let meta = attrs.metadata();
        let mut v = NameVisit(None);
        attrs.record(&mut v);
        let (kind, name) = match (meta.name(), v.0) {
            ("system", Some(n)) => (Kind::System, n),
            ("system_commands", Some(n)) => (Kind::Commands, n),
            ("check_conditions", Some(n)) => (Kind::Cond, n),
            ("schedule", Some(n)) => (Kind::Schedule, n),
            ("node", Some(n)) => (Kind::Node, n),
            (s, Some(n)) => (Kind::Other, format!("{s}: {n}")),
            (s, None) => (Kind::Other, s.to_string()),
        };
        if let (Some(k), Some(span)) = (intern(kind, &name), ctx.span(id)) {
            span.extensions_mut().insert(SpanKey(k));
        }
    }

    fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut e = span.extensions_mut();
            if e.get_mut::<SpanKey>().is_some() {
                e.replace(Entered(Instant::now()));
            }
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut e = span.extensions_mut();
            let t = e.remove::<Entered>();
            if let (Some(k), Some(t)) = (e.get_mut::<SpanKey>(), t) {
                add(k.0, t.0.elapsed().as_nanos() as u64);
            }
        }
    }
}

static LD_STATS: Mutex<Option<HashMap<&'static str, (f64, f64, u32)>>> = Mutex::new(None);

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("HB_PERF").is_some())
}

/// Times a scope (shown as "scope" in the profiler; the ld_* load scopes are also summed for
/// the load log).
pub struct Scope(&'static str, Instant);

pub fn scope(name: &'static str) -> Scope {
    Scope(name, Instant::now())
}

impl Drop for Scope {
    fn drop(&mut self) {
        let el = self.1.elapsed();
        if let Some(k) = intern(Kind::Scope, self.0) {
            add(k, el.as_nanos() as u64);
        }
        if self.0.starts_with("ld_") {
            let ms = el.as_secs_f64() * 1000.0;
            if let Ok(mut g) = LD_STATS.lock() {
                let e = g.get_or_insert_with(HashMap::new).entry(self.0).or_insert((0.0, 0.0, 0));
                e.0 += ms;
                e.1 = e.1.max(ms);
                e.2 += 1;
            }
        }
    }
}

/// Adds `n` to a per-frame counter (e.g. bytes of skinned vertices written).
pub fn count(name: &'static str, n: u64) {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<&'static str, u32>> = std::cell::RefCell::new(HashMap::new());
    }
    let k = CACHE.with(|c| {
        if let Some(&k) = c.borrow().get(name) {
            return Some(k);
        }
        let k = intern(Kind::Count, name)?;
        c.borrow_mut().insert(name, k);
        Some(k)
    });
    if let Some(k) = k.and_then(|k| SLOTS.get(k as usize)) {
        k.ns.fetch_add(n, Relaxed);
        k.n.fetch_add(1, Relaxed);
    }
}

/// Totals of the load scopes so far (name total ms / calls), without resetting.
pub fn summary() -> String {
    let g = LD_STATS.lock().ok();
    let mut v: Vec<(&'static str, f64, u32)> = g.as_ref().and_then(|g| g.as_ref()).map(|m| m.iter().map(|(k, x)| (*k, x.0, x.2)).collect()).unwrap_or_default();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    v.iter().map(|(k, t, n)| format!("{k} {t:.0} ms / {n}")).collect::<Vec<_>>().join(", ")
}

/// Slot values since the last call: (key, value, calls); times in ms, counters raw.
fn take_slots() -> Vec<(u32, f64, u32)> {
    let n = NKEYS.load(Relaxed) as usize;
    let mut out = Vec::new();
    for (k, s) in SLOTS.iter().enumerate().take(n) {
        let c = s.n.swap(0, Relaxed);
        if c == 0 {
            continue;
        }
        let v = s.ns.swap(0, Relaxed);
        out.push((k as u32, v as f64, c));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// frame data

#[derive(Clone, Copy, Default)]
struct Sample {
    dt: f32,
    main: f32,
    render: f32,
    present: f32,
    gpu: f32,
}

#[derive(Default, Clone)]
struct Counts {
    entities: u32,
    meshes: u32,
    visible: u32,
    mesh_mod: u32,
    lights: u32,
    mem_gib: f32,
    cpu_pct: f32,
}

struct Recording {
    base: String,
    w: std::io::BufWriter<std::fs::File>,
    start: f64,
    limit: f64,
    samples: Vec<Sample>,
    /// per key: (frame, value, calls)
    keys: HashMap<u32, Vec<(u32, f32, u32)>>,
    /// per frame: the 12 largest system / node / scope times
    tops: Vec<Vec<(u32, f32)>>,
    gpu: HashMap<String, Vec<f32>>,
    stats: HashMap<String, Vec<f64>>,
    counts: Vec<Counts>,
    note: String,
}

#[derive(Resource)]
pub struct Profiler {
    pub overlay: bool,
    hist: VecDeque<Sample>,
    /// display averages over ~0.5 s: key -> (sum, max, calls)
    acc: HashMap<u32, (f64, f64, u32)>,
    acc_frames: u32,
    acc_t: f64,
    disp: Vec<(u32, f64, f64, f64)>,
    disp_frames: u32,
    gpu: Vec<(String, f32)>,
    stats: Vec<(String, f64)>,
    counts: Counts,
    rec: Option<Recording>,
    pub status: String,
    names: HashMap<u32, (Kind, String)>,
    special: Option<[Option<u32>; 3]>,
    auto_started: bool,
    log_t: f64,
    log_acc: HashMap<u32, (f64, f64, u32)>,
    log_frames: (u32, f64, f64),
}

impl Default for Profiler {
    fn default() -> Self {
        Profiler {
            overlay: enabled(),
            hist: VecDeque::new(),
            acc: HashMap::new(),
            acc_frames: 0,
            acc_t: 0.0,
            disp: Vec::new(),
            disp_frames: 0,
            gpu: Vec::new(),
            stats: Vec::new(),
            counts: Counts::default(),
            rec: None,
            status: String::new(),
            names: HashMap::new(),
            special: None,
            auto_started: false,
            log_t: 0.0,
            log_acc: HashMap::new(),
            log_frames: (0, 0.0, 0.0),
        }
    }
}

impl Profiler {
    fn name(&mut self, k: u32) -> (Kind, String) {
        self.names.entry(k).or_insert_with(|| key_name(k)).clone()
    }

    pub fn recording(&self) -> Option<(String, f64)> {
        self.rec.as_ref().map(|r| (format!("{}.jsonl", r.base), r.samples.len() as f64))
    }

    /// Start a recording of `secs` seconds (0: until stopped, max 10 min).
    pub fn start(&mut self, now: f64, secs: f64, note: String) {
        if self.rec.is_some() {
            return;
        }
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = std::fs::create_dir_all("logs");
        let base = format!("logs/perf_{unix}");
        match std::fs::File::create(format!("{base}.jsonl")) {
            Ok(f) => {
                self.status = format!("Aufnahme läuft: {base}.jsonl");
                info!("perf: recording to {base}.jsonl ({note})");
                self.rec = Some(Recording {
                    base,
                    w: std::io::BufWriter::new(f),
                    start: now,
                    limit: if secs <= 0.0 { 600.0 } else { secs.min(600.0) },
                    samples: Vec::new(),
                    keys: HashMap::new(),
                    tops: Vec::new(),
                    gpu: HashMap::new(),
                    stats: HashMap::new(),
                    counts: Vec::new(),
                    note,
                });
            }
            Err(e) => self.status = format!("Aufnahme nicht möglich: {e}"),
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Short display name: the last path segment of a system name and of each generic argument
/// ("ViewNodeRunner<MainTransparentPass3dNode>").
pub fn short(name: &str) -> String {
    let mut out = String::new();
    let mut tok = String::new();
    let flush = |tok: &mut String, out: &mut String| {
        out.push_str(tok.rsplit("::").next().unwrap_or(tok));
        tok.clear();
    };
    for c in name.chars() {
        if matches!(c, '<' | '>' | ',' | ' ' | '(' | ')' | '&') {
            flush(&mut tok, &mut out);
            out.push(c);
        } else {
            tok.push(c);
        }
    }
    flush(&mut tok, &mut out);
    // keep the module of a plain system name ("bots::render_bots")
    if !name.contains('<') {
        let segs: Vec<&str> = name.split("::").collect();
        if segs.len() >= 2 {
            return format!("{}::{}", segs[segs.len() - 2], segs[segs.len() - 1]);
        }
    }
    out
}

pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        // GPU timestamp / pipeline statistics queries only when profiling (HB_PERF or
        // HB_GPUDIAG): they cost GPU time and are a driver path we do not need otherwise
        // (the user's PC froze after closing the game once with them on, cause unknown)
        if enabled() || std::env::var_os("HB_GPUDIAG").is_some() {
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
        }
        app.add_plugins(bevy::diagnostic::SystemInformationDiagnosticsPlugin)
            .init_resource::<Profiler>()
            .add_systems(First, collect.after(bevy::time::TimeSystem))
            .add_systems(Update, keys)
            .add_systems(EguiPrimaryContextPass, overlay)
            .add_systems(Last, on_exit);
        app.add_systems(Last, visible_breakdown);
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(bevy::render::Render, count_phases.in_set(bevy::render::RenderSet::PhaseSort));
        }
    }
}

/// HB_PHASEDBG: visible meshes by material and blend mode, every 2 s (what the transparent pass
/// draws).
fn visible_breakdown(
    q: Query<(&ViewVisibility, &MeshMaterial3d<crate::gamemat::GameMat>, Option<&Name>)>,
    mats: Res<Assets<crate::gamemat::GameMat>>,
    grass: Query<&ViewVisibility, With<MeshMaterial3d<crate::grass::GrassMat>>>,
    std_q: Query<&ViewVisibility, With<MeshMaterial3d<StandardMaterial>>>,
    time: Res<Time<Real>>,
    mut last: Local<f64>,
) {
    if std::env::var_os("HB_PHASEDBG").is_none() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let mut by: HashMap<String, usize> = HashMap::new();
    for (v, m, _) in &q {
        if !v.get() {
            continue;
        }
        let a = mats.get(&m.0).map(|m| format!("{:?}", m.alpha)).unwrap_or_default();
        *by.entry(a).or_default() += 1;
    }
    let g = grass.iter().filter(|v| v.get()).count();
    let st = std_q.iter().filter(|v| v.get()).count();
    info!("visible: GameMat by blend {by:?}, grass {g}, standard {st}");
}

/// Render phase sizes per frame (all views): transparent items (each one a draw of its own),
/// by material pipeline, so the profile shows what fills the transparent pass.
fn count_phases(t: Res<bevy::render::render_phase::ViewSortedRenderPhases<bevy::core_pipeline::core_3d::Transparent3d>>, mut last: Local<Option<std::time::Instant>>) {
    // (render world: no Time<Real> there)
    let mut n = 0usize;
    let mut by_pipe: HashMap<bevy::render::render_resource::CachedRenderPipelineId, usize> = HashMap::new();
    for (_, ph) in t.iter() {
        n += ph.items.len();
        for it in &ph.items {
            *by_pipe.entry(it.pipeline).or_default() += 1;
        }
    }
    count("transparent_items", n as u64);
    count("transparent_pipelines", by_pipe.len() as u64);
    if std::env::var_os("HB_PHASEDBG").is_some() {
        if last.is_none_or(|l| l.elapsed().as_secs_f32() > 2.0) {
            *last = Some(std::time::Instant::now());
            let mut v: Vec<_> = by_pipe.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            info!("phases: {n} transparent items, top pipelines {:?}", v.iter().take(8).map(|x| (x.0.id(), x.1)).collect::<Vec<_>>());
        }
    }
}

fn keys(keys: Res<ButtonInput<KeyCode>>, mut prof: ResMut<Profiler>, time: Res<Time<Real>>, flow: Res<crate::menu::MatchFlow>) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::F2) {
        if shift {
            if prof.rec.is_some() {
                finish(&mut prof);
            } else {
                prof.start(time.elapsed_secs_f64(), 0.0, "Shift+F2".into());
            }
        } else {
            prof.overlay = !prof.overlay;
        }
    }
    // HB_PERF: the match is recorded from its start
    if enabled() && !prof.auto_started && flow.phase == crate::menu::Phase::Playing {
        prof.auto_started = true;
        let secs = std::env::var("HB_PERF_REC").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(180.0);
        prof.start(time.elapsed_secs_f64(), secs, format!("HB_PERF, {secs} s"));
    }
}

#[allow(clippy::too_many_arguments)]
fn collect(
    mut prof: ResMut<Profiler>,
    time: Res<Time<Real>>,
    diags: Res<DiagnosticsStore>,
    entities: &Entities,
    vis: Query<&ViewVisibility, With<Mesh3d>>,
    lights: Query<(), Or<(With<PointLight>, With<SpotLight>, With<DirectionalLight>)>>,
    mut mesh_ev: EventReader<AssetEvent<Mesh>>,
    flow: Res<crate::menu::MatchFlow>,
) {
    let slots = take_slots();
    let mesh_mod = mesh_ev.read().filter(|e| matches!(e, AssetEvent::Modified { .. })).count() as u32;
    let prof = &mut *prof;
    let dt = time.delta_secs_f64() * 1000.0;
    if dt <= 0.0 {
        return;
    }
    let now = time.elapsed_secs_f64();
    let special = *prof.special.get_or_insert_with(|| [None, None, None]);
    let mut special = special;
    let lookup = |k: Kind, n: &str| KEYS.lock().ok().and_then(|g| g.map.get(&k).and_then(|m| m.get(n)).copied());
    for (i, n) in ["main app", "sub app: RenderApp", "present_frames"].iter().enumerate() {
        if special[i].is_none() {
            special[i] = lookup(Kind::Other, n);
        }
    }
    prof.special = Some(special);
    let val = |k: Option<u32>| k.and_then(|k| slots.iter().find(|s| s.0 == k)).map(|s| s.1 / 1e6).unwrap_or(0.0);
    let mut gpu = Vec::new();
    let mut stats = Vec::new();
    let mut gpu_total = 0.0f32;
    for d in diags.iter() {
        let p = d.path().as_str();
        let Some(rest) = p.strip_prefix("render/") else { continue };
        let Some(v) = d.measurement().map(|m| m.value) else { continue };
        if let Some(name) = rest.strip_suffix("/elapsed_gpu") {
            if !name.contains('/') {
                gpu_total += v as f32;
            }
            gpu.push((name.to_string(), v as f32));
        } else if !rest.ends_with("/elapsed_cpu") {
            stats.push((rest.to_string(), v));
        }
    }
    gpu.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let sample = Sample { dt: dt as f32, main: val(special[0]) as f32, render: val(special[1]) as f32, present: val(special[2]) as f32, gpu: gpu_total };
    prof.hist.push_back(sample);
    while prof.hist.len() > 300 {
        prof.hist.pop_front();
    }
    let want_counts = prof.overlay || prof.rec.is_some();
    if want_counts {
        let mut c = Counts { entities: entities.len(), mesh_mod, ..default() };
        for v in &vis {
            c.meshes += 1;
            if v.get() {
                c.visible += 1;
            }
        }
        c.lights = lights.iter().count() as u32;
        c.mem_gib = diags.get_measurement(&bevy::diagnostic::SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE).map(|m| m.value as f32).unwrap_or(0.0);
        c.cpu_pct = diags.get_measurement(&bevy::diagnostic::SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE).map(|m| m.value as f32).unwrap_or(0.0);
        prof.counts = c;
    }
    // display averages, refreshed every 0.5 s
    for &(k, v, n) in &slots {
        let e = prof.acc.entry(k).or_insert((0.0, 0.0, 0));
        e.0 += v;
        e.1 = e.1.max(v);
        e.2 += n;
    }
    prof.acc_frames += 1;
    if now - prof.acc_t >= 0.5 {
        prof.acc_t = now;
        let f = prof.acc_frames.max(1) as f64;
        prof.disp = prof.acc.drain().map(|(k, (s, m, n))| (k, s / f, m, n as f64 / f)).collect();
        prof.disp.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        prof.disp_frames = prof.acc_frames;
        prof.acc_frames = 0;
        prof.gpu = gpu.clone();
        prof.stats = stats.clone();
    }
    // HB_PERF: a log line every 5 s
    if enabled() {
        for &(k, v, n) in &slots {
            let e = prof.log_acc.entry(k).or_insert((0.0, 0.0, 0));
            e.0 += v;
            e.1 = e.1.max(v);
            e.2 += n;
        }
        prof.log_frames.0 += 1;
        prof.log_frames.1 += dt;
        prof.log_frames.2 = prof.log_frames.2.max(dt);
        if now - prof.log_t >= 5.0 {
            prof.log_t = now;
            let (n, sum, worst) = std::mem::take(&mut prof.log_frames);
            let mut v: Vec<(u32, (f64, f64, u32))> = prof.log_acc.drain().collect();
            v.retain(|x| x.1.0 > 0.0);
            v.sort_by(|a, b| b.1.0.partial_cmp(&a.1.0).unwrap());
            let mut parts = Vec::new();
            for (k, (s, w, _)) in v.into_iter() {
                let (kind, name) = prof.name(k);
                if matches!(kind, Kind::System | Kind::Node | Kind::Scope) && parts.len() < 12 {
                    parts.push(format!("{} {:.2}/{:.1}", short(&name), s / 1e6 / n.max(1) as f64, w / 1e6));
                }
            }
            info!("perf: {n} frames, avg {:.1} ms, worst {:.1} ms | {}", sum / n.max(1) as f64, worst, parts.join(", "));
        }
    }
    // recording
    if prof.rec.is_some() {
        let frame = prof.rec.as_ref().unwrap().samples.len() as u32;
        let mut top: Vec<(u32, f32)> = Vec::new();
        let mut line = format!(
            "{{\"f\":{frame},\"t\":{:.3},\"dt\":{:.3},\"main\":{:.3},\"render\":{:.3},\"present\":{:.3},\"gpu\":{:.3},\"phase\":\"{:?}\"",
            now - prof.rec.as_ref().unwrap().start,
            sample.dt,
            sample.main,
            sample.render,
            sample.present,
            sample.gpu,
            flow.phase
        );
        let mut counters = Vec::new();
        for &(k, v, n) in &slots {
            let (kind, name) = prof.name(k);
            let rec = prof.rec.as_mut().unwrap();
            if kind == Kind::Count {
                rec.keys.entry(k).or_default().push((frame, v as f32, n));
                counters.push(format!("\"{}\":{}", esc(&name), v as u64));
                continue;
            }
            let ms = (v / 1e6) as f32;
            rec.keys.entry(k).or_default().push((frame, ms, n));
            if matches!(kind, Kind::System | Kind::Node | Kind::Scope | Kind::Commands) {
                top.push((k, ms));
            }
        }
        top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        top.truncate(12);
        let tops: Vec<String> = top.iter().map(|(k, ms)| format!("[\"{}\",{ms:.3}]", esc(&short(&prof.name(*k).1)))).collect();
        line.push_str(&format!(",\"top\":[{}]", tops.join(",")));
        let g: Vec<String> = gpu.iter().map(|(n, v)| format!("\"{}\":{v:.3}", esc(n))).collect();
        line.push_str(&format!(",\"gpu_pass\":{{{}}}", g.join(",")));
        let c = prof.counts.clone();
        line.push_str(&format!(
            ",\"n\":{{\"entities\":{},\"meshes\":{},\"visible\":{},\"mesh_modified\":{},\"lights\":{},\"mem_gib\":{:.3},\"cpu_pct\":{:.1}{}{}}}}}\n",
            c.entities,
            c.meshes,
            c.visible,
            c.mesh_mod,
            c.lights,
            c.mem_gib,
            c.cpu_pct,
            if counters.is_empty() { "" } else { "," },
            counters.join(",")
        ));
        let rec = prof.rec.as_mut().unwrap();
        let _ = rec.w.write_all(line.as_bytes());
        if frame % 60 == 0 {
            let _ = rec.w.flush();
        }
        rec.samples.push(sample);
        rec.tops.push(top);
        rec.counts.push(c);
        for (n, v) in &gpu {
            let e = rec.gpu.entry(n.clone()).or_default();
            e.resize(frame as usize, f32::NAN);
            e.push(*v);
        }
        for (n, v) in &stats {
            rec.stats.entry(n.clone()).or_default().push(*v);
        }
        if now - rec.start >= rec.limit {
            finish(prof);
        }
    }
}

fn on_exit(mut ev: EventReader<AppExit>, mut prof: ResMut<Profiler>) {
    if ev.read().next().is_some() && prof.rec.is_some() {
        finish(&mut prof);
    }
}

fn pct(sorted: &[f32], p: f64) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// Stop the recording and write the summary.
pub fn finish(prof: &mut Profiler) {
    let Some(mut rec) = prof.rec.take() else { return };
    let _ = rec.w.flush();
    let frames = rec.samples.len();
    let path = format!("{}_summary.txt", rec.base);
    let mut o = String::new();
    use std::fmt::Write as _;
    let _ = writeln!(o, "HomeBots perf summary  {}  ({})", rec.base, rec.note);
    let _ = writeln!(o, "{}", INFO.lock().map(|g| g.clone()).unwrap_or_default());
    if frames == 0 {
        let _ = std::fs::write(&path, o);
        return;
    }
    let mut dts: Vec<f32> = rec.samples.iter().map(|s| s.dt).collect();
    let total: f64 = dts.iter().map(|&d| d as f64).sum();
    dts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg = |f: fn(&Sample) -> f32| rec.samples.iter().map(|s| f(s) as f64).sum::<f64>() / frames as f64;
    let _ = writeln!(o, "\nframes {frames} in {:.1} s, avg {:.1} FPS", total / 1000.0, frames as f64 / (total / 1000.0));
    let _ = writeln!(
        o,
        "frame ms: avg {:.2}  p50 {:.2}  p90 {:.2}  p95 {:.2}  p99 {:.2}  max {:.2}   >25 ms: {}  >33 ms: {}  >50 ms: {}",
        total / frames as f64,
        pct(&dts, 0.5),
        pct(&dts, 0.9),
        pct(&dts, 0.95),
        pct(&dts, 0.99),
        dts.last().copied().unwrap_or(0.0),
        dts.iter().filter(|&&d| d > 25.0).count(),
        dts.iter().filter(|&&d| d > 33.3).count(),
        dts.iter().filter(|&&d| d > 50.0).count()
    );
    let _ = writeln!(
        o,
        "avg ms: main app {:.2}  render app {:.2}  present (incl. vsync wait) {:.2}  GPU passes {:.2}",
        avg(|s| s.main),
        avg(|s| s.render),
        avg(|s| s.present),
        avg(|s| s.gpu)
    );
    let ca = |f: fn(&Counts) -> f64| rec.counts.iter().map(f).sum::<f64>() / rec.counts.len().max(1) as f64;
    let _ = writeln!(
        o,
        "avg counts: entities {:.0}  Mesh3d {:.0}  visible {:.0}  mesh assets modified/frame {:.1}  lights {:.0}  process mem {:.2} GiB  process CPU {:.0} %",
        ca(|c| c.entities as f64),
        ca(|c| c.meshes as f64),
        ca(|c| c.visible as f64),
        ca(|c| c.mesh_mod as f64),
        ca(|c| c.lights as f64),
        ca(|c| c.mem_gib as f64),
        ca(|c| c.cpu_pct as f64)
    );
    // per key
    let mut rows: Vec<(Kind, String, f64, f32, f32, f64, f32)> = Vec::new();
    for (k, v) in &rec.keys {
        let (kind, name) = prof.name(*k);
        let mut vals: Vec<f32> = v.iter().map(|x| x.1).collect();
        vals.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let sum: f64 = vals.iter().map(|&x| x as f64).sum();
        // percentile over all frames (frames without a run count as 0)
        let rank = |p: f64| {
            let i = ((1.0 - p) * frames as f64) as usize;
            vals.get(i).copied().unwrap_or(0.0)
        };
        let calls: f64 = v.iter().map(|x| x.2 as f64).sum::<f64>() / frames as f64;
        rows.push((kind, name, sum / frames as f64, rank(0.95), vals.first().copied().unwrap_or(0.0), calls, rank(0.99)));
    }
    rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    let main_avg = avg(|s| s.main).max(1e-6);
    for (title, kinds) in [
        ("systems / scopes / render nodes (ms per frame)", &[Kind::System, Kind::Scope, Kind::Node, Kind::Commands, Kind::Cond][..]),
        ("schedules and other spans (ms per frame)", &[Kind::Schedule, Kind::Other][..]),
    ] {
        let _ = writeln!(o, "\n== {title}");
        let _ = writeln!(o, "{:>8} {:>8} {:>8} {:>8} {:>7} {:>6}  {:<5} name", "avg", "p95", "p99", "max", "calls", "%main", "kind");
        for r in rows.iter().filter(|r| kinds.contains(&r.0)).take(120) {
            let _ = writeln!(o, "{:8.3} {:8.3} {:8.3} {:8.3} {:7.2} {:5.1}%  {:<5} {}", r.2, r.3, r.6, r.4, r.5, r.2 / main_avg * 100.0, r.0.tag(), r.1);
        }
    }
    let _ = writeln!(o, "\n== counters (per frame)");
    for r in rows.iter().filter(|r| r.0 == Kind::Count) {
        let _ = writeln!(o, "{:12.1} avg {:12.1} max  {}", r.2, r.4, r.1);
    }
    // GPU
    let _ = writeln!(o, "\n== GPU passes (ms, timestamp queries)");
    let mut g: Vec<(String, f32, f32, f32)> = rec
        .gpu
        .iter()
        .map(|(n, v)| {
            let mut s: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let a = s.iter().sum::<f32>() / s.len().max(1) as f32;
            (n.clone(), a, pct(&s, 0.95), s.last().copied().unwrap_or(0.0))
        })
        .collect();
    g.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    if g.is_empty() {
        let _ = writeln!(o, "(no GPU timestamps: the adapter does not offer TIMESTAMP_QUERY)");
    }
    for (n, a, p, m) in &g {
        let _ = writeln!(o, "{a:8.3} avg {p:8.3} p95 {m:8.3} max  {n}");
    }
    let _ = writeln!(o, "\n== render statistics (avg per frame)");
    let mut st: Vec<(&String, f64)> = rec.stats.iter().map(|(n, v)| (n, v.iter().sum::<f64>() / v.len().max(1) as f64)).collect();
    st.sort_by(|a, b| a.0.cmp(b.0));
    for (n, v) in st {
        let _ = writeln!(o, "{v:14.0}  {n}");
    }
    // worst frames
    let _ = writeln!(o, "\n== 30 worst frames (frame, t, ms, main / render / present / gpu, top systems)");
    let mut idx: Vec<usize> = (0..frames).collect();
    idx.sort_by(|&a, &b| rec.samples[b].dt.partial_cmp(&rec.samples[a].dt).unwrap());
    for &i in idx.iter().take(30) {
        let s = rec.samples[i];
        let tops: Vec<String> = rec.tops[i].iter().take(8).map(|(k, ms)| format!("{} {ms:.2}", short(&prof.name(*k).1))).collect();
        let _ = writeln!(o, "#{i:<6} {:7.2} ms  {:6.2} / {:6.2} / {:6.2} / {:6.2}  {}", s.dt, s.main, s.render, s.present, s.gpu, tops.join(", "));
    }
    let _ = std::fs::write(&path, o);
    info!("perf: recording stopped, {frames} frames, summary {path}");
    prof.status = format!("gespeichert: {path}");
}

/// Machine / settings line for the summary (filled by the overlay system).
static INFO: Mutex<String> = Mutex::new(String::new());

// ---------------------------------------------------------------------------------------------
// overlay

#[allow(clippy::too_many_arguments)]
fn overlay(
    mut contexts: EguiContexts,
    mut prof: ResMut<Profiler>,
    time: Res<Time<Real>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    flow: Res<crate::menu::MatchFlow>,
    args: Option<NonSend<crate::Args>>,
) -> Result {
    if let (Ok(mut g), Some(a)) = (INFO.lock(), adapter.as_ref()) {
        if g.is_empty() {
            let w = windows.single().ok();
            *g = format!(
                "adapter: {} ({:?}, {:?}, driver {} {})  window {}  present {:?}  max_tex {}  build {}",
                a.name,
                a.backend,
                a.device_type,
                a.driver,
                a.driver_info,
                w.map(|w| format!("{}x{} (scale {})", w.resolution.physical_width(), w.resolution.physical_height(), w.resolution.scale_factor())).unwrap_or_default(),
                w.map(|w| w.present_mode),
                args.as_ref().map(|a| a.max_tex).unwrap_or(0),
                if cfg!(debug_assertions) { "debug" } else { "release" }
            );
        }
    }
    if !prof.overlay {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let prof = &mut *prof;
    let n = prof.hist.len().max(1) as f32;
    let last: Vec<Sample> = prof.hist.iter().rev().take(60).copied().collect();
    let m = last.len().max(1) as f32;
    let avg = |f: fn(&Sample) -> f32| last.iter().map(f).sum::<f32>() / m;
    let worst = prof.hist.iter().map(|s| s.dt).fold(0.0f32, f32::max);
    let dt = avg(|s| s.dt);
    let _ = n;
    let txt = |s: String| egui::RichText::new(s).monospace().size(12.5);
    egui::Window::new(egui::RichText::new("Performance").strong())
        .id(egui::Id::new("hb_perf"))
        .default_pos(egui::pos2(ctx.screen_rect().width() - 470.0, 16.0))
        .default_width(450.0)
        .resizable(true)
        .collapsible(true)
        .frame(egui::Frame::window(&ctx.style()).fill(egui::Color32::from_rgba_unmultiplied(16, 18, 24, 215)))
        .show(ctx, |ui| {
            let fps = 1000.0 / dt.max(0.01);
            let c = if fps >= 55.0 { egui::Color32::from_rgb(120, 220, 140) } else if fps >= 30.0 { egui::Color32::from_rgb(255, 180, 70) } else { egui::Color32::from_rgb(255, 105, 105) };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{fps:.0} FPS")).size(20.0).strong().color(c));
                ui.label(txt(format!("{dt:.2} ms  (max 5 s {worst:.1})")));
            });
            ui.label(txt(format!("main {:.2}  render {:.2}  present/vsync {:.2}  GPU {:.2} ms", avg(|s| s.main), avg(|s| s.render), avg(|s| s.present), avg(|s| s.gpu))));
            // frame-time graph: the last 300 frames, lines at 16.7 / 33.3 ms
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 70.0), egui::Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 4.0, egui::Color32::from_black_alpha(120));
            let scale = 50.0f32;
            let y = |ms: f32| rect.bottom() - (ms / scale).min(1.0) * rect.height();
            for (ms, col) in [(16.7, egui::Color32::from_rgb(80, 160, 90)), (33.3, egui::Color32::from_rgb(170, 120, 50))] {
                p.line_segment([egui::pos2(rect.left(), y(ms)), egui::pos2(rect.right(), y(ms))], egui::Stroke::new(1.0_f32, col));
            }
            let w = rect.width() / 300.0;
            for (i, s) in prof.hist.iter().enumerate() {
                let x = rect.left() + i as f32 * w;
                let col = if s.dt > 33.3 { egui::Color32::from_rgb(255, 105, 105) } else if s.dt > 17.5 { egui::Color32::from_rgb(255, 180, 70) } else { egui::Color32::from_rgb(110, 170, 255) };
                p.line_segment([egui::pos2(x, rect.bottom()), egui::pos2(x, y(s.dt))], egui::Stroke::new(w.max(1.0), col));
            }
            ui.add_space(2.0);
            let cn = prof.counts.clone();
            ui.label(txt(format!(
                "Entities {}  Mesh3d {} (sichtbar {})  Mesh-Uploads {}  Lichter {}  RAM {:.2} GiB  CPU {:.0} %",
                cn.entities, cn.meshes, cn.visible, cn.mesh_mod, cn.lights, cn.mem_gib, cn.cpu_pct
            )));
            ui.separator();
            ui.label(egui::RichText::new("CPU (ms pro Frame, Ø 0,5 s)").strong());
            let rows: Vec<(u32, f64, f64, f64)> = prof.disp.clone();
            let mut shown = 0;
            for (k, a, mx, calls) in rows.iter() {
                let (kind, name) = prof.name(*k);
                if !matches!(kind, Kind::System | Kind::Node | Kind::Scope) {
                    continue;
                }
                let a = a / 1e6;
                if shown >= 16 || a < 0.02 {
                    break;
                }
                shown += 1;
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(60.0, 12.0), egui::Sense::hover());
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2((a as f32 / 4.0).min(1.0) * 60.0, 12.0)), 2.0, egui::Color32::from_rgb(64, 156, 255));
                    ui.label(txt(format!("{a:6.2} {:6.2} {:4.1}x {:<5} {}", mx / 1e6, calls, kind.tag(), short(&name))));
                });
            }
            let sched: Vec<String> = rows
                .iter()
                .filter_map(|(k, a, _, _)| {
                    let (kind, name) = key_name(*k);
                    (kind == Kind::Schedule && a / 1e6 >= 0.05).then(|| format!("{} {:.2}", name, a / 1e6))
                })
                .take(10)
                .collect();
            if !sched.is_empty() {
                ui.label(txt(format!("Schedules: {}", sched.join("  "))));
            }
            let counters: Vec<String> = rows
                .iter()
                .filter_map(|(k, a, _, _)| {
                    let (kind, name) = key_name(*k);
                    (kind == Kind::Count).then(|| format!("{name} {:.0}", a))
                })
                .collect();
            if !counters.is_empty() {
                ui.label(txt(format!("Zähler/Frame: {}", counters.join("  "))));
            }
            ui.separator();
            ui.label(egui::RichText::new("GPU (ms)").strong());
            if prof.gpu.is_empty() {
                ui.label(txt("keine GPU-Zeitstempel (nur mit HB_PERF=1 oder HB_GPUDIAG=1)".into()));
            }
            for (name, v) in prof.gpu.iter().take(12) {
                ui.label(txt(format!("{v:6.2}  {name}")));
            }
            ui.separator();
            ui.horizontal(|ui| {
                if prof.rec.is_some() {
                    let secs = prof.rec.as_ref().map(|r| time.elapsed_secs_f64() - r.start).unwrap_or(0.0);
                    ui.label(egui::RichText::new(format!("REC {secs:.0} s")).color(egui::Color32::from_rgb(255, 90, 90)).strong());
                    if ui.button("Stopp + Auswertung").clicked() {
                        finish(prof);
                    }
                } else {
                    for (label, s) in [("Aufnahme 60 s", 60.0), ("180 s", 180.0), ("bis Stopp", 0.0)] {
                        if ui.button(label).clicked() {
                            prof.start(time.elapsed_secs_f64(), s, format!("button {label}, phase {:?}", flow.phase));
                        }
                    }
                }
            });
            if !prof.status.is_empty() {
                ui.label(egui::RichText::new(&prof.status).small().color(egui::Color32::from_rgb(150, 156, 166)));
            }
            ui.label(egui::RichText::new("F2 Overlay · Shift+F2 Aufnahme · Maus: F1").small().color(egui::Color32::from_rgb(150, 156, 166)));
        });
    Ok(())
}
