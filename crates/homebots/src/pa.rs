//! PA system (user request, NOT from data): a pole with loudspeaker horns near the overpass at
//! objective B on Crossroads, playing an OGG file in a loop. The volume follows the distance from
//! the listener to the nearest pole.
//!
//! - pole: STO_energy.highPower.Statics.telePole_Conduit (cooked into Crossroads_construction);
//!   the game has no megaphone / PA mesh in the install (searched all packages), so the horns are
//!   built here (open bell + driver housing) with STA_highway.roadSigns M_signPostMetal_C
//! - audio: assets/pa/*.ogg next to the project (Hanoi Hannah, converted from the user's MP3),
//!   played as one looping voice; volume 1 inside FULL_RADIUS, linear to 0 at MAX_RADIUS (the
//!   UE3 SoundNodeAttenuation linear model), times the options' music volume

use bevy::audio::{AudioSinkPlayback, PlaybackMode, PlaybackSettings, Volume};
use bevy::prelude::*;

/// UE units
const FULL_RADIUS: f32 = 500.0;
const MAX_RADIUS: f32 = 5500.0;
/// level of the clip relative to the other sounds
const LEVEL: f32 = 1.0;

/// Pole placements per map (UE x, y, height the ground search starts at; yaw degrees; search a
/// free spot around the point); z comes from the ground below. Crossroads: one pole per front
/// line (the three stages of a Ground Control match), next to its objectives (user 2026-10-19).
/// Line 1: beside the overpass at B, ~10 m off the road towards the KVA side (+x); lines 0 / 2:
/// a free spot 6-16 m (up to 25 m) from the line's objective B (data: the objectives' locations).
/// Every pole keeps 1.5 m off road surfaces (free_spot / spot_ok; the objectives B of lines 0 / 2
/// lie on the road). NOT from data: the poles themselves.
fn placements(map: &str) -> Vec<([f32; 3], f32, bool)> {
    let m = map.to_ascii_lowercase();
    // HB_PA_AT=x,y,from,yaw[;x,y,from,yaw...]: other placements (tests)
    if let Ok(v) = std::env::var("HB_PA_AT") {
        return v
            .split(';')
            .filter_map(|p| {
                let f: Vec<f32> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                (f.len() >= 4).then(|| ([f[0], f[1], f[2]], f[3], false))
            })
            .collect();
    }
    if m.contains("crossroads") {
        vec![
            // line 0: objective B (-9909.9, 1309.2, -293)
            ([-9909.9, 1309.2, 1207.0], 140.0, true),
            // line 1: the overpass at B
            ([1750.0, 300.0, -400.0], 200.0, false),
            // line 2: objective B (9456.4, -6160.0, -443)
            ([9456.4, -6160.0, 1057.0], 320.0, true),
        ]
    } else {
        Vec::new()
    }
}

/// Clearance of the pole from road surfaces (UE units): 1.5 m past the road edge plus the pole's
/// radius (user 2026-10-21: the pole stood on the centre line at B, "neben die Straße"). NOT from
/// data.
const ROAD_CLEAR: f32 = 170.0;

/// Whether (px, py) suits a pole: ground below `from` near the height `ground` (not a roof),
/// nearly level, nothing within 9 m above and 1.5 m around at chest height, and no road surface
/// (CollisionWorld::is_road, road spline meshes + highway lanes) below the spot or within
/// ROAD_CLEAR of it.
fn spot_ok(col: &crate::player::CollisionWorld, px: f32, py: f32, from: f32, ground: f32, buf: &mut Vec<u32>) -> bool {
    let m = crate::player::COL_PLAYER | crate::player::COL_TRACE;
    let top = Vec3::new(px, from, py) * crate::UU_TO_M;
    let Some((t, n, tri)) = col.raycast_hit(top, Vec3::NEG_Y, 450.0, buf, m) else { return false };
    if n.y < 0.85 || col.is_road(tri) {
        return false;
    }
    let g = top - Vec3::Y * t;
    if (g.y / crate::UU_TO_M - ground).abs() > 300.0 || col.raycast_mask(g + Vec3::Y * 0.3, Vec3::Y, 9.0, buf, m).is_some() {
        return false;
    }
    let chest = g + Vec3::Y * 1.5;
    if [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z].iter().any(|d| col.raycast_mask(chest, *d, 1.5, buf, m).is_some()) {
        return false;
    }
    // road edge: the surface below 8 points around the pole (from 3 m above the ground)
    (0..8).all(|k| {
        let a = k as f32 / 8.0 * std::f32::consts::TAU;
        let o = g + Vec3::new(a.cos(), 0.0, a.sin()) * ROAD_CLEAR * crate::UU_TO_M + Vec3::Y * 3.0;
        col.raycast_hit(o, Vec3::NEG_Y, 6.0, buf, m).is_none_or(|(_, _, tri)| !col.is_road(tri))
    })
}

/// A spot for a pole near (x, y) (UE): (x, y) itself when `exact`, else rings of 6-16 m (as
/// before) widening to 25 m, 12 directions each; for `exact` the point first, then rings from 2 m
/// (nearest spot off the road). `spot_ok` decides.
fn free_spot(col: &crate::player::CollisionWorld, x: f32, y: f32, from: f32, ground: f32, exact: bool, buf: &mut Vec<u32>) -> Option<(f32, f32)> {
    let rings: &[f32] = if exact { &[0.0, 200.0, 400.0, 600.0, 900.0, 1200.0, 1600.0, 2000.0, 2500.0] } else { &[600.0, 900.0, 1200.0, 1600.0, 2000.0, 2500.0] };
    for &r in rings {
        let n = if r == 0.0 { 1 } else { 12 };
        for k in 0..n {
            let a = k as f32 / 12.0 * std::f32::consts::TAU;
            let (px, py) = (x + a.cos() * r, y + a.sin() * r);
            if spot_ok(col, px, py, from, ground, buf) {
                return Some((px, py));
            }
        }
    }
    None
}

/// Horn mouth centres of the current level (UE units), set at level load.
#[derive(Resource, Default)]
pub struct PaSpeakers(pub Vec<Vec3>);

#[derive(Default)]
struct Pa {
    voice: Option<Entity>,
    clip: Option<Handle<AudioSource>>,
}

#[derive(Component)]
struct PaVoice;

pub struct PaPlugin;

impl Plugin for PaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PaSpeakers>().add_systems(PostUpdate, (update, top_view.before(bevy::transform::TransformSystem::TransformPropagate)));
    }
}

/// The audio file: HB_PA_FILE, else the first .ogg in assets/pa (working directory = project).
fn clip_path() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("HB_PA_FILE") {
        return Some(p.into());
    }
    let dir = std::path::Path::new("assets/pa");
    let mut v: Vec<_> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ogg"))).collect();
    v.sort();
    v.into_iter().next()
}

/// Open loudspeaker horn in local UE units (x forward = sound direction): bell from the throat
/// (radius 7 at x = 0) to the mouth (radius 26 at x = 62), driver housing behind (radius 11,
/// x -26..0) with a closed back. NOT from data (no such mesh in the install).
fn horn_lod() -> upk::mesh::MeshLod {
    let mut lod = upk::mesh::MeshLod::default();
    let seg = 20usize;
    let pack = |v: Vec3, w: u8| -> [u8; 4] { [((v.x.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8, ((v.y.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8, ((v.z.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8, w] };
    // profile (x, radius) rings; flare of the bell as a curve
    let mut rings: Vec<(f32, f32)> = vec![(-26.0, 0.0), (-26.0, 11.0), (-4.0, 11.0), (-4.0, 7.0)];
    for k in 0..=8 {
        let t = k as f32 / 8.0;
        rings.push((t * 62.0, 7.0 + 19.0 * t * t));
    }
    let mut vlen = 0.0;
    for r in 0..rings.len() {
        if r > 0 {
            let (a, b) = (rings[r - 1], rings[r]);
            vlen += ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        }
        let (x, rad) = rings[r];
        // profile slope for the normal
        let (pa, pb) = (rings[r.saturating_sub(1)], rings[(r + 1).min(rings.len() - 1)]);
        let (dx, dr) = (pb.0 - pa.0, pb.1 - pa.1);
        for s in 0..=seg {
            let ang = s as f32 / seg as f32 * std::f32::consts::TAU;
            let (sn, cs) = ang.sin_cos();
            let radial = Vec3::new(0.0, cs, sn);
            lod.positions.push([x, rad * cs, rad * sn]);
            // outward normal of the surface of revolution: (-dr, dx * radial)
            let n = (Vec3::new(-dr, 0.0, 0.0) + radial * dx).normalize_or(Vec3::NEG_X);
            let tangent = Vec3::new(0.0, -sn, cs);
            lod.tangent_x.push(pack(tangent, 255));
            lod.tangent_z.push(pack(n, 255));
            lod.normals.push(n.to_array());
            lod.uvs.push([s as f32 / seg as f32 * 2.0, vlen / 40.0]);
        }
    }
    let row = seg as u32 + 1;
    for r in 0..rings.len() as u32 - 1 {
        for s in 0..seg as u32 {
            let (a, b, c, d) = (r * row + s, r * row + s + 1, (r + 1) * row + s, (r + 1) * row + s + 1);
            lod.indices.extend([a, c, b, b, c, d]);
        }
    }
    lod.uv_sets = vec![lod.uvs.clone(), lod.uvs.clone()];
    lod.sections.push(upk::mesh::Section { material: 0, first_index: 0, num_triangles: lod.indices.len() as u32 / 3, min_vertex: 0, max_vertex: lod.positions.len() as u32 - 1 });
    lod
}

/// A mounting bar between the pole and the horns (box, local UE units).
fn bar_lod(len: f32, w: f32) -> upk::mesh::MeshLod {
    let mut lod = upk::mesh::MeshLod::default();
    let pack = |v: Vec3| -> [u8; 4] { [((v.x + 1.0) * 127.5).round() as u8, ((v.y + 1.0) * 127.5).round() as u8, ((v.z + 1.0) * 127.5).round() as u8, 255] };
    let h = w * 0.5;
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (Vec3::NEG_X, Vec3::NEG_Y, Vec3::Z),
        (Vec3::Y, Vec3::NEG_X, Vec3::Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::X, Vec3::NEG_Y),
    ];
    let size = Vec3::new(len * 0.5, h, h);
    for (n, u, v) in faces {
        let base = lod.positions.len() as u32;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let p = (n + u * a + v * b) * size + Vec3::X * len * 0.5;
            lod.positions.push(p.to_array());
            lod.normals.push(n.to_array());
            lod.tangent_x.push(pack(u));
            lod.tangent_z.push(pack(n));
            lod.uvs.push([(a + 1.0) * 0.5, (b + 1.0) * 0.5]);
        }
        lod.indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }
    lod.uv_sets = vec![lod.uvs.clone(), lod.uvs.clone()];
    lod.sections.push(upk::mesh::Section { material: 0, first_index: 0, num_triangles: lod.indices.len() as u32 / 3, min_vertex: 0, max_vertex: lod.positions.len() as u32 - 1 });
    lod
}

fn find_export(pkgs: &[upk::Package], class: &str, path_suffix: &str) -> Option<(usize, usize)> {
    pkgs.iter().enumerate().find_map(|(pi, p)| p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == class && p.object_path(i as i32 + 1).ends_with(path_suffix)).map(|i| (pi, i)))
}

/// Place the poles of `map` (level load, before the object constants are uploaded).
#[allow(clippy::too_many_arguments)]
pub fn spawn(map: &str, sp: &mut crate::world::Spawner, a: &mut crate::world::Assets3, commands: &mut Commands, col: &mut crate::player::CollisionWorld, speakers: &mut Vec<Vec3>, impacts: &mut crate::impacts::Impacts) {
    speakers.clear();
    let places = placements(map);
    if places.is_empty() {
        return;
    }
    let pole = find_export(sp.pkgs, "StaticMesh", "highPower.Statics.telePole_Conduit");
    let metal = find_export(sp.pkgs, "MaterialInstanceConstant", "roadSigns.Materials.M_signPostMetal_C").or_else(|| find_export(sp.pkgs, "Material", "SpecialCase.M_Metal"));
    info!("pa: pole {pole:?} metal {metal:?}");
    let horn = a.meshes.add(crate::build_game_mesh(&horn_lod(), &horn_lod().sections[0]).unwrap());
    let bar = bar_lod(70.0, 8.0);
    let bar_mesh = a.meshes.add(crate::build_game_mesh(&bar, &bar.sections[0]).unwrap());
    let mut buf = Vec::new();
    for ([x, y, from], yaw, search) in places {
        // the ground height the spot must stay near: objectives (search) 15 m below `from`, the
        // exact points at the ground below them
        let ground = if search {
            from - 1500.0
        } else {
            match col.raycast(Vec3::new(x, from, y) * crate::UU_TO_M, Vec3::NEG_Y, 450.0, &mut buf) {
                Some((t, _)) => from - t / crate::UU_TO_M,
                None => from - 1500.0,
            }
        };
        let (x, y) = match free_spot(col, x, y, from, ground, !search, &mut buf) {
            Some(p) => p,
            None => {
                warn!("pa: no free spot off the road near {x} {y}");
                continue;
            }
        };
        // ground below (collision is in Bevy metres)
        let top = Vec3::new(x, from, y) * crate::UU_TO_M;
        let Some((t, _)) = col.raycast(top, Vec3::NEG_Y, 450.0, &mut buf) else {
            warn!("pa: no ground at {x} {y}");
            continue;
        };
        let z = (top.y - t) / crate::UU_TO_M;
        let base = Vec3::new(x, y, z - 5.0);
        let rot = [0, (yaw / 360.0 * 65536.0) as i32, 0];
        let place = |loc: Vec3, rot: [i32; 3], scale: f32| {
            let transform = Transform { translation: crate::ue_to_bevy(loc.to_array()) * crate::UU_TO_M, rotation: crate::rotator_to_quat(rot), scale: Vec3::splat(scale * crate::UU_TO_M) };
            let l2w = crate::gameshader::ue_l2w(loc.to_array(), rot, [scale; 3]);
            (transform, l2w)
        };
        let mut pole_top = 700.0;
        if let Some((pi, mi)) = pole {
            if let Some((parts, _)) = sp.static_mesh(a, pi, mi) {
                let (transform, l2w) = place(base, rot, 1.0);
                let ph = vec![impacts.id("pm_library.env.wood.pm_wood"); parts.len()];
                crate::add_collision(col, a.meshes, &parts, &transform, &ph);
                sp.cast_shadows = true;
                sp.spawn_mesh(commands, a, pi, parts, pi, &[], transform, l2w, [0.0; 4], false, false);
                pole_top = 724.0;
            }
        }
        // three horns on a short bar below the crossarm, facing yaw, yaw + 120, yaw - 120,
        // tilted 12 degrees down
        let (Some((mpi, mmi)), true) = (metal, true) else { continue };
        let mat = mmi as i32 + 1;
        let hz = pole_top - 150.0;
        for k in 0..3 {
            let hy = yaw + k as f32 * 120.0;
            let dir = Vec3::new(hy.to_radians().cos(), hy.to_radians().sin(), 0.0);
            let brot = [0, (hy / 360.0 * 65536.0) as i32, 0];
            let (bt, bl) = place(base + Vec3::Z * hz, brot, 1.0);
            sp.spawn_mesh(commands, a, mpi, vec![(bar_mesh.clone(), mat)], mpi, &[], bt, bl, [0.0; 4], false, false);
            let hrot = [(-12.0f32 / 360.0 * 65536.0) as i32, brot[1], 0];
            let at = base + Vec3::Z * hz + dir * 92.0;
            let (ht, hl) = place(at, hrot, 1.0);
            sp.spawn_mesh(commands, a, mpi, vec![(horn.clone(), mat)], mpi, &[], ht, hl, [0.0; 4], true, false);
            speakers.push(at + dir * 60.0);
        }
        info!("pa: pole at {base:?}, {} speakers", speakers.len());
    }
}

fn update(
    mut commands: Commands,
    speakers: Res<PaSpeakers>,
    mut pa: Local<Pa>,
    mut sources: ResMut<Assets<AudioSource>>,
    cams: Query<&GlobalTransform, (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    mut sinks: Query<&mut AudioSink, With<PaVoice>>,
    in_menu: Option<Res<crate::menu::InMenu>>,
) {
    let menu = in_menu.is_some_and(|m| m.0);
    if speakers.0.is_empty() || menu {
        if let Some(e) = pa.voice.take() {
            commands.entity(e).despawn();
        }
        return;
    }
    if pa.clip.is_none() {
        let Some(path) = clip_path() else { return };
        match std::fs::read(&path) {
            Ok(b) => {
                info!("pa: {} ({} bytes)", path.display(), b.len());
                pa.clip = Some(sources.add(AudioSource { bytes: b.into() }));
            }
            Err(e) => {
                warn!("pa: {}: {e}", path.display());
                pa.clip = Some(Handle::default());
                return;
            }
        }
    }
    let Some(listener) = cams.iter().next().map(|g| {
        let t = g.translation();
        Vec3::new(t.x, t.z, t.y) * 100.0
    }) else {
        return;
    };
    let d = speakers.0.iter().map(|s| s.distance(listener)).fold(f32::MAX, f32::min);
    let g = (1.0 - (d - FULL_RADIUS) / (MAX_RADIUS - FULL_RADIUS)).clamp(0.0, 1.0);
    // music slider (user 2026-10-19: "The PA system should be controlled over Musicvolume")
    let vol = (g * LEVEL * crate::settings::volume("music")).min(1.5);
    match pa.voice {
        None => {
            let h = pa.clip.clone().unwrap();
            pa.voice = Some(commands.spawn((AudioPlayer::new(h), PaVoice, PlaybackSettings { mode: PlaybackMode::Loop, volume: Volume::Linear(vol), ..default() })).id());
        }
        Some(_) => {
            for mut s in sinks.iter_mut() {
                s.set_volume(Volume::Linear(vol));
            }
        }
    }
}

/// HB_PA_TOPVIEW=<height m>[,<x>,<y> UE]: the camera looks straight down at the first pole (or
/// x, y) from that height (tests: placement).
fn top_view(speakers: Res<PaSpeakers>, mut cams: Query<&mut Transform, (With<Camera3d>, Without<crate::minimap::MinimapCam>)>) {
    let Ok(v) = std::env::var("HB_PA_TOPVIEW") else { return };
    let f: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
    let Some(&h) = f.first() else { return };
    let c = match (f.get(1), f.get(2), speakers.0.first()) {
        (Some(&x), Some(&y), _) => Vec3::new(x, y, 0.0),
        (_, _, Some(s)) => *s,
        _ => return,
    };
    for mut t in cams.iter_mut() {
        t.translation = Vec3::new(c.x * 0.01, h, c.y * 0.01);
        t.look_to(Vec3::NEG_Y, Vec3::X);
    }
}
