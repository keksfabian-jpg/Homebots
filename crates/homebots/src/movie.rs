//! Full-screen Bink movies (GCGame/Movies/*.bik), e.g. the game-mode intro that hides the map
//! loading (DefaultEngine.ini GameModeTypes: GroundControl -> "GroundControl",
//! TeamDeathMatch -> "battlePoints").
//!
//! Decoding: infinitier_bik_decoder (FFmpeg bink.c port; trace t3: Y planes of movie frames 0,
//! 94 and 288 match the game's uploads byte for byte). The game uploads Y (1280x720) and the
//! two 640x360 chroma planes as L8 textures every frame and converts in a pixel shader on a
//! full-viewport quad, alpha blending off, straight to the gamma-space back buffer (trace t3
//! frame 1216); movie.wgsl does the same.
//!
//! Subtitles: <movie>.txt lists "start_ms,end_ms,key" lines, the text is
//! Localization Subtitles.<key> (trace t3 frames 1316 and 1425: "Bodenkontrolle", GC2 wrapped
//! to two lines), drawn with PC_UI_Fonts.TradGothicBoldCondensed (DefaultEngine.ini
//! DefaultLoadingSequence FontName) as an outlined string, see subtitle_quads.
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::{AlphaMode2d, Material2d, Material2dPlugin};
use bevy::image::{ImageSampler, ImageSamplerDescriptor, ImageAddressMode, ImageFilterMode};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};

pub struct MoviePlugin;

impl Plugin for MoviePlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "movie.wgsl");
        app.add_plugins(Material2dPlugin::<MovieMat>::default())
            .init_resource::<MovieState>()
            .add_systems(Startup, prewarm)
            .add_systems(Update, (start_movie.run_if(resource_exists::<MovieRequest>), play_movie).chain());
    }
}

/// The movie material's render pipeline is built at start-up: a 1x1 quad scaled to nothing
/// stays in the scene for good. Before, the pipeline was first requested when the loading
/// movie's quad appeared and waited in the pipeline cache behind the match preload's ~400 game
/// pipelines (warmup.rs): the first ~5 s of the movie were black while its sound already ran
/// (user 2026-10-23 00:29). NOT from data.
fn prewarm(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut mats: ResMut<Assets<MovieMat>>, mut meshes: ResMut<Assets<Mesh>>) {
    let mat = mats.add(MovieMat { y: images.add(plane_image(2, 2)), cr: images.add(plane_image(1, 1)), cb: images.add(plane_image(1, 1)) });
    commands.spawn((Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))), MeshMaterial2d(mat), Transform::from_xyz(0.0, 0.0, -0.9).with_scale(Vec3::splat(1.0e-4)), Visibility::Visible));
}

/// Start playing GCGame/Movies/<name>.bik.
#[derive(Resource, Clone, Debug)]
pub struct MovieRequest {
    pub file: PathBuf,
}

/// Playback state other systems read (match flow, subtitles).
#[derive(Resource, Default, Debug)]
pub struct MovieState {
    /// a movie is loaded (decoder running or holding its last shown frame)
    pub active: bool,
    /// audio decoded and playback clock running
    pub started: bool,
    /// index of the frame on screen
    pub frame: usize,
    pub frame_count: usize,
    /// movie time of the frame on screen in seconds
    pub time: f32,
    /// subtitle cues: (start s, end s, Subtitles key)
    pub cues: Vec<(f32, f32, String)>,
    /// set by the owner to stop and remove the movie
    pub stop: bool,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct MovieMat {
    #[texture(0)]
    #[sampler(1)]
    pub y: Handle<Image>,
    /// Cr (decoder plane "v")
    #[texture(2)]
    pub cr: Handle<Image>,
    /// Cb (decoder plane "u")
    #[texture(3)]
    pub cb: Handle<Image>,
}

impl Material2d for MovieMat {
    fn fragment_shader() -> bevy::render::render_resource::ShaderRef {
        "embedded://homebots/movie.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

struct Planes {
    y: Vec<u8>,
    cr: Vec<u8>,
    cb: Vec<u8>,
}

enum Msg {
    Header { w: u32, h: u32, frame_us: u32, frames: u32 },
    Audio(Option<(u16, u32, Vec<i16>)>),
    Frame(Planes),
    End,
    Error(String),
}

/// Non-send: owns the decoder channel.
struct Player {
    rx: Receiver<Msg>,
    size: (u32, u32),
    frame_us: u32,
    start: Option<f64>,
    shown: Option<usize>,
    next: usize,
    ended: bool,
    images: Option<[Handle<Image>; 3]>,
    mat: Option<Handle<MovieMat>>,
    entity: Option<Entity>,
    audio: Option<Entity>,
}

fn decoder_thread(path: PathBuf, tx: SyncSender<Msg>) {
    use infinitier_bik_decoder::{AudioDecoder, BikPixels, BikStreamingDecoder};
    use std::io::{Read, Seek, SeekFrom};
    let fail = |tx: &SyncSender<Msg>, e: String| {
        let _ = tx.send(Msg::Error(e));
    };
    // 1) the whole audio track first (the playback clock starts with it)
    let audio = (|| -> Result<Option<(u16, u32, Vec<i16>)>, String> {
        let mut f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        let header = infinitier_bik_decoder::parse_header(&mut f).map_err(|e| e.to_string())?;
        let Some(track) = header.audio_tracks.first() else { return Ok(None) };
        let mut dec = AudioDecoder::new(track).map_err(|e| e.to_string())?;
        let mut pcm = Vec::new();
        let mut packet = Vec::with_capacity(header.max_frame_size as usize);
        for fr in &header.frames {
            packet.resize(fr.size as usize, 0);
            f.seek(SeekFrom::Start(fr.pos as u64)).map_err(|e| e.to_string())?;
            f.read_exact(&mut packet).map_err(|e| e.to_string())?;
            if packet.len() < 4 {
                continue;
            }
            let n = u32::from_le_bytes([packet[0], packet[1], packet[2], packet[3]]) as usize;
            if n > 0 && 4 + n <= packet.len() {
                pcm.extend(dec.decode_packet(&packet[4..4 + n]).map_err(|e| e.to_string())?);
            }
        }
        Ok(Some((track.flags.channels(), track.sample_rate as u32, pcm)))
    })();
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => return fail(&tx, e.to_string()),
    };
    let mut d = match BikStreamingDecoder::new(std::io::BufReader::new(file), path.to_string_lossy()) {
        Ok(d) => d,
        Err(e) => return fail(&tx, e.to_string()),
    };
    if tx.send(Msg::Header { w: d.width(), h: d.height(), frame_us: d.frame_duration_us(), frames: d.frame_count() }).is_err() {
        return;
    }
    match audio {
        Ok(a) => {
            if tx.send(Msg::Audio(a)).is_err() {
                return;
            }
        }
        Err(e) => {
            warn!("movie audio: {e}");
            if tx.send(Msg::Audio(None)).is_err() {
                return;
            }
        }
    }
    // 2) video frames, a few ahead of the clock (bounded channel)
    let pack = |p: &infinitier_bik_decoder::Plane| -> Vec<u8> {
        let mut out = Vec::with_capacity((p.width * p.height) as usize);
        for r in 0..p.height as usize {
            out.extend_from_slice(&p.data[r * p.stride..r * p.stride + p.width as usize]);
        }
        out
    };
    loop {
        match d.next_frame() {
            Ok(Some(fr)) => {
                if let BikPixels::Yuv(v) = &fr.video.pixels {
                    if tx.send(Msg::Frame(Planes { y: pack(&v.y), cr: pack(&v.v), cb: pack(&v.u) })).is_err() {
                        return;
                    }
                }
            }
            Ok(None) => {
                let _ = tx.send(Msg::End);
                return;
            }
            Err(e) => return fail(&tx, e.to_string()),
        }
    }
}

/// Subtitle cues of <movie>.txt: a header line, then "start_ms,end_ms,key".
pub fn read_cues(txt: &std::path::Path) -> Vec<(f32, f32, String)> {
    let Ok(s) = std::fs::read_to_string(txt) else { return Vec::new() };
    let mut cues = Vec::new();
    for l in s.lines() {
        let p: Vec<&str> = l.split(',').map(|x| x.trim()).collect();
        if p.len() == 3 {
            if let (Ok(a), Ok(b)) = (p[0].parse::<f32>(), p[1].parse::<f32>()) {
                cues.push((a / 1000.0, b / 1000.0, p[2].to_string()));
            }
        }
    }
    cues
}

fn start_movie(world: &mut World) {
    let req = world.remove_resource::<MovieRequest>().unwrap();
    // replace a running movie
    if let Some(old) = world.remove_non_send_resource::<Player>() {
        for e in [old.entity, old.audio].into_iter().flatten() {
            if let Ok(ec) = world.get_entity_mut(e) {
                ec.despawn();
            }
        }
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let path = req.file.clone();
    std::thread::spawn(move || decoder_thread(path, tx));
    let cues = read_cues(&req.file.with_extension("txt"));
    info!("movie: {} ({} subtitle cues)", req.file.display(), cues.len());
    let mut st = world.resource_mut::<MovieState>();
    *st = MovieState { active: true, cues, ..default() };
    world.insert_non_send_resource(Player { rx, size: (0, 0), frame_us: 33333, start: None, shown: None, next: 0, ended: false, images: None, mat: None, entity: None, audio: None });
}

fn plane_image(w: u32, h: u32) -> Image {
    let mut img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0u8; (w * h) as usize],
        TextureFormat::R8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    // trace t3: MIN/MAG LINEAR, MIP POINT, ADDRESS CLAMP
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    img
}

#[allow(clippy::too_many_arguments)]
fn play_movie(
    mut commands: Commands,
    player: Option<NonSendMut<Player>>,
    mut state: ResMut<MovieState>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<MovieMat>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut audio: ResMut<Assets<AudioSource>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut tf: Query<&mut Transform>,
    time: Res<Time<Real>>,
) {
    let Some(mut p) = player else { return };
    if state.stop {
        for e in [p.entity.take(), p.audio.take()].into_iter().flatten() {
            commands.entity(e).despawn();
        }
        *state = MovieState::default();
        commands.queue(|w: &mut World| {
            w.remove_non_send_resource::<Player>();
        });
        return;
    }
    let now = time.elapsed_secs_f64();
    // messages up to the frame due now
    let due = p.start.map(|s| ((now - s) * 1e6 / p.frame_us as f64) as usize);
    let mut latest: Option<Planes> = None;
    loop {
        if p.start.is_some() && (p.ended || due.is_some_and(|d| p.next > d)) {
            break;
        }
        match p.rx.try_recv() {
            Ok(Msg::Header { w, h, frame_us, frames }) => {
                p.size = (w, h);
                p.frame_us = frame_us.max(1);
                state.frame_count = frames as usize;
                let imgs = [images.add(plane_image(w, h)), images.add(plane_image(w / 2, h / 2)), images.add(plane_image(w / 2, h / 2))];
                let mat = mats.add(MovieMat { y: imgs[0].clone(), cr: imgs[1].clone(), cb: imgs[2].clone() });
                let mesh = meshes.add(Rectangle::new(1.0, 1.0));
                let e = commands.spawn((Mesh2d(mesh), MeshMaterial2d(mat.clone()), Transform::from_xyz(0.0, 0.0, -0.5), Visibility::Hidden, MovieQuad)).id();
                p.images = Some(imgs);
                p.mat = Some(mat);
                p.entity = Some(e);
            }
            Ok(Msg::Audio(a)) => {
                if let Some((ch, rate, pcm)) = a {
                    let mut wav = Vec::with_capacity(44 + pcm.len() * 2);
                    let data_len = (pcm.len() * 2) as u32;
                    wav.extend_from_slice(b"RIFF");
                    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
                    wav.extend_from_slice(b"WAVEfmt ");
                    wav.extend_from_slice(&16u32.to_le_bytes());
                    wav.extend_from_slice(&1u16.to_le_bytes());
                    wav.extend_from_slice(&ch.to_le_bytes());
                    wav.extend_from_slice(&rate.to_le_bytes());
                    wav.extend_from_slice(&(rate * ch as u32 * 2).to_le_bytes());
                    wav.extend_from_slice(&(ch * 2).to_le_bytes());
                    wav.extend_from_slice(&16u16.to_le_bytes());
                    wav.extend_from_slice(b"data");
                    wav.extend_from_slice(&data_len.to_le_bytes());
                    for s in pcm {
                        wav.extend_from_slice(&s.to_le_bytes());
                    }
                    let h = audio.add(AudioSource { bytes: wav.into() });
                    // the movie groups (MovieEffects / MovieVoice) hang under Master only
                    p.audio = Some(commands.spawn((AudioPlayer(h), PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::settings::volume("master"))))).id());
                }
                p.start = Some(now);
                state.started = true;
                break;
            }
            Ok(Msg::Frame(f)) => {
                p.next += 1;
                latest = Some(f);
            }
            Ok(Msg::End) => {
                p.ended = true;
            }
            Ok(Msg::Error(e)) => {
                error!("movie: {e}");
                p.ended = true;
                state.started = true;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                p.ended = true;
                break;
            }
        }
    }
    if let (Some(f), Some(imgs)) = (latest, p.images.as_ref()) {
        for (h, data) in imgs.iter().zip([f.y, f.cr, f.cb]) {
            if let Some(img) = images.get_mut(h) {
                img.data = Some(data);
            }
        }
        // the material's bind group holds the texture views: re-prepare it with the new images
        if let Some(m) = p.mat.as_ref() {
            let _ = mats.get_mut(m);
        }
        if p.shown.is_none() {
            info!("movie: first frame on screen {:.2} s after the sound started", p.start.map(|s| now - s).unwrap_or(0.0));
        }
        p.shown = Some(p.next - 1);
        if let Some(e) = p.entity {
            commands.entity(e).insert(Visibility::Visible);
        }
    }
    if let Some(i) = p.shown {
        state.frame = i;
        state.time = i as f32 * p.frame_us as f32 * 1e-6;
    }
    // full-viewport quad: the game's clip-space quad (-1..1-2/vw, D3D9 pixel centres) covers
    // canvas pixels 0.5..vw-0.5 x 0.5..vh+0.5
    if let (Some(e), Ok(win)) = (p.entity, windows.single()) {
        let (vw, vh, s) = (win.physical_width() as f32, win.physical_height() as f32, win.scale_factor());
        if let Ok(mut t) = tf.get_mut(e) {
            let (x0, x1, y0, y1) = (0.5, vw - 0.5, 0.5, vh + 0.5);
            let cx = ((x0 + x1) * 0.5 - vw * 0.5) / s;
            let cy = (vh * 0.5 - (y0 + y1) * 0.5) / s;
            *t = Transform::from_xyz(cx, cy, -0.5).with_scale(Vec3::new((x1 - x0) / s, (y1 - y0) / s, 1.0));
        }
    }
}

#[derive(Component)]
struct MovieQuad;

/// Subtitle text as the loading movie draws it (trace t3 frames 1316, 1425; 1366x768):
/// - horizontal centre X = trunc(0.1 vw) + (trunc(0.9 vw) - trunc(0.1 vw)) / 2 (682), a line
///   starts at round(X - width / 2) (615 for "Bodenkontrolle", 414 for the 537 px GC2 line);
/// - lines are stacked up from the bottom edge, line pitch = tallest glyph + 3 (41), glyphs at
///   line top + VerticalOffset;
/// - greedy word wrap; the GC2 line broke between 537 and 614 px, half the centred region
///   width (546) is used as the limit;
/// - outline: black copies at (-1,-1), (-1,+1), (+1,+1), (+1,-1), then white.
pub fn subtitle_quads(ui: &upk::ui::Ui, font_ref: upk::ui::GRef, text: &str, vw: f32, vh: f32, out: &mut Vec<upk::ui::Quad>) {
    let Some(font) = ui.font(font_ref) else { return };
    let max_h = font.chars.iter().map(|g| g.h).max().unwrap_or(0) as f32;
    let pitch = max_h + 3.0;
    let width = |t: &str| t.chars().filter_map(|c| font.glyph(c)).map(|g| g.w as f32).sum::<f32>();
    let x0 = (vw * 0.1).trunc();
    let x1 = (vw * 0.9).trunc();
    let cx = x0 + ((x1 - x0) / 2.0).trunc();
    let limit = ((x1 - x0) / 2.0).trunc();
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if !line.is_empty() && width(&cand) > limit {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        } else {
            line = cand;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    let n = lines.len() as f32;
    for (li, l) in lines.iter().enumerate() {
        let top = vh - (n - li as f32) * pitch;
        let left = (cx - width(l) / 2.0 + 0.5).floor();
        for (dx, dy, c) in [(-1.0, -1.0, 0.0), (-1.0, 1.0, 0.0), (1.0, 1.0, 0.0), (1.0, -1.0, 0.0), (0.0, 0.0, 1.0)] {
            let mut x = left + dx;
            for ch in l.chars() {
                let Some(g) = font.glyph(ch) else { continue };
                if g.w > 0 && g.h > 0 {
                    let tex = font.textures.get(g.tex as usize).and_then(|&t| ui.world.gref(font_ref.pkg, t));
                    let (tw, th) = tex.and_then(|t| ui.tex_size(t)).unwrap_or((1024.0, 256.0));
                    let y = top + dy + g.voff as f32;
                    out.push(upk::ui::Quad {
                        rot: 0.0,
                        rect: [x, y, x + g.w as f32, y + g.h as f32],
                        uv: [g.u as f32 / tw, g.v as f32 / th, (g.u + g.w) as f32 / tw, (g.v + g.h) as f32 / th],
                        tex,
                        color: [c, c, c, 1.0],
                    });
                }
                x += g.w as f32;
            }
        }
    }
}
