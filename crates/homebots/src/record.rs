//! Recording mode for tests (NOT from data): HB_RECORD=<dir>[,<fps>[,<from>[,<to>]]] runs the game
//! clock in fixed steps of 1/fps (default 30) per rendered frame, so a slow machine still gives a
//! smooth, real-speed video, and saves every frame as PNG to <dir> while the nuke sequence is
//! between <from> and <to> seconds (default 0..40; without a nuke running nothing is saved).
//! The game quits after <to>. The frames become a video / GIF afterwards (ffmpeg).
use bevy::{prelude::*, time::TimeUpdateStrategy};

struct Cfg {
    dir: String,
    fps: f32,
    from: f32,
    to: f32,
}

fn cfg() -> Option<Cfg> {
    let v = std::env::var("HB_RECORD").ok()?;
    let mut it = v.split(',');
    let dir = it.next()?.trim().to_string();
    let num = |x: Option<&str>, d: f32| x.and_then(|x| x.trim().parse::<f32>().ok()).unwrap_or(d);
    // fps 0: real time (no fixed step), every frame recorded
    let raw = num(it.next(), 30.0);
    let fps = if raw <= 0.0 { 0.0 } else { raw.clamp(5.0, 120.0) };
    let from = num(it.next(), 0.0);
    let to = num(it.next(), 40.0);
    Some(Cfg { dir, fps, from, to })
}

static WARM_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
/// The HB_RECORD_PLAY clock (s, f32 bits), for scripted test events (levelcars.rs HB_TEST_CARHITS).
static PLAY_T: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The recording's match clock when recording with HB_RECORD_PLAY (0 until the warm-up is over).
pub fn play_clock() -> Option<f32> {
    (cfg().is_some() && std::env::var_os("HB_RECORD_PLAY").is_some() && std::env::var_os("HB_VEHTEST").is_none()).then(|| f32::from_bits(PLAY_T.load(std::sync::atomic::Ordering::Relaxed)))
}

pub struct RecordPlugin;

impl Plugin for RecordPlugin {
    fn build(&self, app: &mut App) {
        let Some(c) = cfg() else { return };
        let _ = std::fs::create_dir_all(&c.dir);
        info!("record: {} fps to {} (nuke {}..{} s)", c.fps, c.dir, c.from, c.to);
        if c.fps > 0.0 {
            app.insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / c.fps)));
        }
        app.add_systems(Last, record);
    }
}

#[allow(clippy::too_many_arguments)]
fn record(mut commands: Commands, nk: Res<crate::nuke::Nuke>, mut n: Local<u32>, mut wait: Local<u32>, mut exit: EventWriter<AppExit>, time: Res<Time>, flow: Res<crate::menu::MatchFlow>, mut play_t: Local<f32>) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let Some(c) = cfg() else { return };
    // HB_RECORD_PLAY=1: the clock is the time in the match (vehicle tests) instead of the nuke's
    let t = if std::env::var_os("HB_VEHTEST").is_some() {
        // the vehicle test script's clock (vehicles.rs)
        let _ = (&time, &flow, &mut play_t);
        f32::from_bits(crate::vehicles::TEST_T.load(std::sync::atomic::Ordering::Relaxed))
    } else if std::env::var_os("HB_RECORD_PLAY").is_some() {
        if flow.phase != crate::menu::Phase::Playing {
            return;
        }
        // HB_RECORD_WARMUP=<s>: real seconds in the match before the clock starts, so a fast PC
        // does not record the first frames while meshes and textures are still streaming in
        // (test setting, NOT from data)
        let warm = std::env::var("HB_RECORD_WARMUP").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let started = *WARM_START.get_or_init(std::time::Instant::now);
        if started.elapsed().as_secs_f32() < warm {
            return;
        }
        *play_t += time.delta_secs();
        PLAY_T.store(play_t.to_bits(), std::sync::atomic::Ordering::Relaxed);
        *play_t
    } else {
        let Some(t) = nk.time() else { return };
        t
    };
    if t > c.to + 0.5 {
        // the screenshots are read back a few frames late: give them time before quitting
        // (a fast GPU renders 90 frames in well under a second, before the read-backs arrive:
        // wait until every requested frame is on disk, at most ~2000 frames)
        *wait += 1;
        let saved = if std::env::var_os("HB_RECORD_NOPNG").is_some() {
            std::fs::read_to_string(format!("{}/means.csv", c.dir)).map(|t| t.lines().count() as u32).unwrap_or(0)
        } else {
            std::fs::read_dir(&c.dir).map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "png")).count() as u32).unwrap_or(0)
        };
        if (*wait >= 90 && saved >= *n) || *wait == 2000 {
            info!("record: {} frames, done", *n);
            exit.write(AppExit::Success);
        }
        return;
    }
    if t < c.from || t > c.to {
        return;
    }
    let path = format!("{}/f{:05}.png", c.dir, *n);
    let idx = *n;
    *n += 1;
    // mean colour of the frame (centre 60 % of the view, the HUD edges left out) to
    // <dir>/means.csv: frame, t, r, g, b (0..255) - flicker checks without the PNGs (NOT from data)
    let csv = format!("{}/means.csv", c.dir);
    let tt = t;
    let mut ent = commands.spawn(Screenshot::primary_window());
    if std::env::var_os("HB_RECORD_NOPNG").is_none() {
        ent.observe(save_to_disk(path));
    }
    ent.observe(move |trigger: Trigger<bevy::render::view::screenshot::ScreenshotCaptured>| {
        let img = &trigger.event().0;
        let (w, h) = (img.width() as usize, img.height() as usize);
        let Some(data) = img.data.as_ref() else { return };
        if w == 0 || h == 0 || data.len() < w * h * 4 {
            return;
        }
        let (mut sum, mut cnt) = ([0f64; 3], 0f64);
        for y in (h / 5..h * 4 / 5).step_by(4) {
            for x in (w / 5..w * 4 / 5).step_by(4) {
                let i = (y * w + x) * 4;
                for k in 0..3 {
                    sum[k] += data[i + k] as f64;
                }
                cnt += 1.0;
            }
        }
        // Bgra8 swap chains: the first channel is blue
        let bgra = matches!(img.texture_descriptor.format, bevy::render::render_resource::TextureFormat::Bgra8Unorm | bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb);
        let m = sum.map(|v| v / cnt.max(1.0));
        let (r, g, b) = if bgra { (m[2], m[1], m[0]) } else { (m[0], m[1], m[2]) };
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&csv) {
            let _ = writeln!(f, "{idx},{tt:.3},{r:.1},{g:.1},{b:.1}");
        }
    });
}
