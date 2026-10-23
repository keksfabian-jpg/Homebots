//! HB_SLOPETEST=1: vehicle slope bench without rendering (tests). The buyable vehicles are
//! loaded from the game data as in a match and driven with full throttle by
//! vehicles::ground_step (the same code as in the match) over synthetic lanes: ramps of
//! 5..45 degrees (20 m long, flat before and after) and steps of 5..45 units. Prints per
//! vehicle and lane whether the top was reached, the time, the lowest speed on the way and the
//! frames the wall spheres blocked. HB_SLOPETEST_V=<class> one vehicle only; HB_SLOPETEST_LOG=<lane>
//! prints every 10th frame of that lane.
//! NOT from data: the lanes (test geometry), 60 frames per second.
use crate::player::CollisionWorld;
use crate::vehicles::{ground_step, new_vehicle, top_speed, VehicleKind};
use bevy::prelude::*;
use upk::library::Library;

const DT: f32 = 1.0 / 60.0;
const LANE_GAP: f32 = 1500.0;
const LANE_HALF: f32 = 400.0;
const RAMP_LEN: f32 = 2000.0;

#[derive(Clone, Copy, Debug)]
enum Lane {
    Ramp(f32),
    Step(f32),
}

/// One lane at UE y = `y`: UE profile x -> height, as quads (Bevy metres).
fn add_lane(col: &mut CollisionWorld, lane: Lane, y: f32) {
    let quad = |col: &mut CollisionWorld, a: Vec3, b: Vec3, c: Vec3, d: Vec3| {
        let m = |p: Vec3| Vec3::new(p.x, p.z, p.y) * 0.01;
        col.add([m(a), m(b), m(c)]);
        col.add([m(a), m(c), m(d)]);
    };
    let (y0, y1) = (y - LANE_HALF, y + LANE_HALF);
    let strip = |col: &mut CollisionWorld, x0: f32, h0: f32, x1: f32, h1: f32| {
        quad(col, Vec3::new(x0, y0, h0), Vec3::new(x1, y0, h1), Vec3::new(x1, y1, h1), Vec3::new(x0, y1, h0));
    };
    match lane {
        Lane::Ramp(deg) => {
            let h = RAMP_LEN * deg.to_radians().tan();
            strip(col, -3000.0, 0.0, 0.0, 0.0);
            strip(col, 0.0, 0.0, RAMP_LEN, h);
            strip(col, RAMP_LEN, h, RAMP_LEN + 3000.0, h);
        }
        Lane::Step(hgt) => {
            strip(col, -3000.0, 0.0, 0.0, 0.0);
            // the riser
            quad(col, Vec3::new(0.0, y0, 0.0), Vec3::new(0.0, y1, 0.0), Vec3::new(0.0, y1, hgt), Vec3::new(0.0, y0, hgt));
            strip(col, 0.0, hgt, 3000.0, hgt);
        }
    }
}

fn lane_end(lane: Lane) -> (f32, f32) {
    match lane {
        Lane::Ramp(deg) => (RAMP_LEN, RAMP_LEN * deg.to_radians().tan()),
        Lane::Step(h) => (0.0, h),
    }
}

pub fn run(game: &std::path::Path) {
    if std::env::var("HB_SLOPETEST").is_ok_and(|v| v == "heli") {
        heli_bench(game);
        return;
    }
    let Ok(lib) = Library::scan(game) else {
        eprintln!("slopetest: no game data at {game:?}");
        return;
    };
    let (Some(content), Some(gcgame), Some(wc)) = (lib.open("GCGameContent"), lib.open("GCGame"), lib.open("WeaponConfigs")) else {
        eprintln!("slopetest: script packages missing");
        return;
    };
    let pkgs: Vec<upk::Package> = ["SD_M1114", "SD_LAV25", "US_M1A3"].iter().filter_map(|n| lib.open(n)).collect();
    let only = std::env::var("HB_SLOPETEST_V").ok();
    let log_lane: Option<usize> = std::env::var("HB_SLOPETEST_LOG").ok().and_then(|v| v.parse().ok());
    let rockets = std::collections::HashMap::new();
    let mut lanes = Vec::new();
    for d in (5..=45).step_by(5) {
        lanes.push(Lane::Ramp(d as f32));
    }
    for h in [5.0, 10.0, 15.0, 20.0, 30.0, 45.0] {
        lanes.push(Lane::Step(h));
    }
    let mut col = CollisionWorld::default();
    for (i, l) in lanes.iter().enumerate() {
        add_lane(&mut col, *l, i as f32 * LANE_GAP);
    }
    for class in crate::vehload::CLASSES.iter().filter(|c| !c.contains("MD600") && !c.contains("AH64")) {
        if only.as_deref().is_some_and(|o| !class.eq_ignore_ascii_case(o)) {
            continue;
        }
        let mut spawn = |path: &str| -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)> {
            let rest = path.split_once('.').map(|x| x.1).unwrap_or(path);
            pkgs.iter().find_map(|p| {
                let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SkeletalMesh" && {
                    let op = p.object_path(i as i32 + 1);
                    op.eq_ignore_ascii_case(path) || op.eq_ignore_ascii_case(rest)
                })?;
                let m = p.read_skeletal_mesh(i)?;
                Some((vec![Handle::default(); m.lod.sections.len()], m))
            })
        };
        let Some(k) = crate::vehload::load(&lib, &pkgs, &content, &gcgame, &wc, class, &rockets, &mut spawn) else {
            eprintln!("slopetest: {class} not loadable");
            continue;
        };
        println!(
            "== {} ({:?}) ride {:.1} wheel r {:.1} base {:.1} track {:.1} body {:.0}x{:.0} top {:.0} probes {:?}",
            k.class,
            k.motion,
            k.ride_height,
            k.wheel_radius,
            k.wheel_base,
            k.track_half,
            k.half_len,
            k.half_width,
            top_speed(&k),
            k.probes
        );
        for (li, lane) in lanes.iter().enumerate() {
            drive_lane(&k, &col, *lane, li, log_lane == Some(li));
        }
    }
}

fn drive_lane(k: &VehicleKind, col: &CollisionWorld, lane: Lane, li: usize, log: bool) {
    let y = li as f32 * LANE_GAP;
    let start = Vec3::new(-2000.0, y, k.ride_height);
    let mut v = new_vehicle(k, 0, 0, start, 0.0);
    let mut pos = start;
    let mut buf = (Vec::new(), Vec::new());
    let top = top_speed(k);
    let (end_x, end_h) = lane_end(lane);
    let goal = end_x + k.half_len + 200.0;
    let (mut t, mut min_speed, mut blocked_frames, mut reached) = (0.0f32, f32::MAX, 0, None);
    let mut first_block: Option<(f32, f32, f32)> = None;
    while t < 25.0 {
        let b = ground_step(k, &mut v, &mut pos, col, &mut buf, 1.0, 0.0, top, top, DT);
        v.pos = pos;
        t += DT;
        if b {
            blocked_frames += 1;
            first_block.get_or_insert((t, pos.x, pos.z));
        }
        if t > 2.0 && pos.x > -1000.0 && pos.x < goal {
            min_speed = min_speed.min(v.speed);
        }
        let frame = (t / DT).round() as i32;
        if log && frame % 10 == 0 {
            println!("   t {t:5.2} x {:7.1} z {:7.1} speed {:6.1} pitch {:5.1} blocked {b}", pos.x, pos.z, v.speed, v.pitch.to_degrees());
        }
        if pos.x > goal && reached.is_none() {
            reached = Some(t);
            break;
        }
    }
    let on_top = (pos.z - k.ride_height - end_h).abs() < 30.0;
    println!(
        "  {:<12} {} t {:>5} min speed {:6.1} end x {:7.1} z {:6.1} (top {:6.1}) blocked {:4} frames{}",
        format!("{lane:?}"),
        if reached.is_some() && on_top { "OK  " } else { "FAIL" },
        reached.map(|t| format!("{t:.1}")).unwrap_or("-".into()),
        if min_speed == f32::MAX { 0.0 } else { min_speed },
        pos.x,
        pos.z,
        end_h + k.ride_height,
        blocked_frames,
        first_block.map(|(t, x, z)| format!(", first at t {t:.2} x {x:.0} z {z:.0}")).unwrap_or_default()
    );
}

/// HB_SLOPETEST=heli: the helicopters' flight model (vehicles::heli_step) over flat ground:
/// lift-off (W 3 s), hover, nose down with the mouse, level flight, A/D yaw, mouse roll; logs
/// height, speeds and attitude every half second.
fn heli_bench(game: &std::path::Path) {
    let Ok(lib) = Library::scan(game) else { return };
    let (Some(content), Some(gcgame), Some(wc)) = (lib.open("GCGameContent"), lib.open("GCGame"), lib.open("WeaponConfigs")) else { return };
    let map = game.join("GCGame/CookedPC/Maps/MP/Crossroads/Crossroads_FrontlinesData.map");
    let Ok(fl) = upk::Package::open(&map) else { return };
    let pkgs = vec![fl];
    let rockets = std::collections::HashMap::new();
    let mut col = CollisionWorld::default();
    let m = |p: Vec3| Vec3::new(p.x, p.z, p.y) * 0.01;
    let (a, b, c, d) = (Vec3::new(-50000.0, -50000.0, 0.0), Vec3::new(50000.0, -50000.0, 0.0), Vec3::new(50000.0, 50000.0, 0.0), Vec3::new(-50000.0, 50000.0, 0.0));
    col.add([m(a), m(b), m(c)]);
    col.add([m(a), m(c), m(d)]);
    for class in ["GCV_US_AH64_Content", "GCV_US_MD600_Content"] {
        let mut spawn = |path: &str| -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)> {
            let rest = path.split_once('.').map(|x| x.1).unwrap_or(path);
            pkgs.iter().find_map(|p| {
                let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SkeletalMesh" && {
                    let op = p.object_path(i as i32 + 1);
                    op.eq_ignore_ascii_case(path) || op.eq_ignore_ascii_case(rest)
                })?;
                let m = p.read_skeletal_mesh(i)?;
                Some((vec![Handle::default(); m.lod.sections.len()], m))
            })
        };
        let Some(k) = crate::vehload::load(&lib, &pkgs, &content, &gcgame, &wc, class, &rockets, &mut spawn) else {
            eprintln!("helibench: {class} not loadable");
            continue;
        };
        let Some(h) = k.heli.clone() else { continue };
        println!("== {} ride {:.1} cap {}\n{h:?}", k.class, k.ride_height, k.max_speed);
        let start = Vec3::new(0.0, 0.0, k.ride_height);
        let mut v = new_vehicle(&k, 0, 0, start, 0.0);
        let mut pos = start;
        let mut buf = (Vec::new(), Vec::new());
        let mut t = 0.0f32;
        let ru = 65536.0 / std::f32::consts::TAU;
        while t < 24.0 {
            // script: W 0.5..3.5 s; mouse forward (nose down) 5..5.6 s; A/D right 10..12 s;
            // mouse right 14..14.4 s; S 18..19 s
            let rise = if (0.5..3.5).contains(&t) { 1.0 } else if (18.0..19.0).contains(&t) { -1.0 } else { 0.0 };
            let mut pitch_in = if (5.0..5.6).contains(&t) { -0.4 * DT * ru } else { 0.0 };
            let mut yaw_in = if (14.0..14.4).contains(&t) { 0.6 * DT * ru } else { 0.0 };
            // HB_HELIROLL=1: hard roll right 13..16 s (limit), roll back left 16..16.5 s,
            // hands off (auto-level), hard nose up 20..22 s, released
            if std::env::var_os("HB_HELIROLL").is_some() {
                yaw_in = if (13.0..16.0).contains(&t) { 3.0 * DT * ru } else if (16.0..16.5).contains(&t) { -3.0 * DT * ru } else { 0.0 };
                pitch_in = if (5.0..5.6).contains(&t) { -0.4 * DT * ru } else if (20.0..22.0).contains(&t) { 3.0 * DT * ru } else { 0.0 };
                // =2: knocked to 80 deg of bank at 8 s (no input until 13 s)
                if std::env::var("HB_HELIROLL").is_ok_and(|x| x == "2") && t < 8.0 && t + DT >= 8.0 {
                    v.roll = 80f32.to_radians();
                }
            }
            let steer = if (10.0..12.0).contains(&t) { 1.0 } else { 0.0 };
            crate::vehicles::heli_step(&h, &k, &mut v, &mut pos, rise, steer, yaw_in, pitch_in, k.max_speed, DT, &col, &mut buf);
            v.pos = pos;
            t += DT;
            if ((t - DT) * 2.0).floor() != (t * 2.0).floor() {
                println!(
                    "  t {t:5.1} alt {:6.0} vz {:6.0} fwd {:6.0} side {:6.0} pitch {:6.1} roll {:6.1} yaw {:6.1} yawrate {:5.2}",
                    pos.z - k.ride_height,
                    v.vel.z,
                    v.vel.dot(Vec3::new(v.yaw.cos(), v.yaw.sin(), 0.0)),
                    v.vel.dot(Vec3::new(-v.yaw.sin(), v.yaw.cos(), 0.0)),
                    v.pitch.to_degrees(),
                    v.roll.to_degrees(),
                    v.yaw.to_degrees(),
                    v.ang.z
                );
            }
        }
    }
}
