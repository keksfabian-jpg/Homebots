//! Hellfire airstrike (loadout special CFG_BMB_Hellfire): the player flies an unmanned UAV
//! (GCV_AirstrikeUAV_C, AirSpeed) for its fuel time and fires the gun's Hellfire missiles
//! (MP.Vehicle.CFG_Airstrike_Hellfire AmmoClip / ReloadTime, GCProj_Airstrike_Hellfire_Content
//! speed / damage radii / flight effect) at the point under the crosshair (bLaserGuided: the
//! missile is launched at the designated point). See weaponcfg::read_hellfire.
//!
//! Not from data (marked in MODLOG): the UAV starts 3000 uu above the player looking down at
//! 55 degrees, pitch limits -85..-10 degrees, it keeps its height.
use crate::combat::{Actor, CombatData, Grenade, Grenades, player_team};
use crate::menu::{MatchFlow, Phase};
use crate::player::{CollisionWorld, Pawn};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;

pub struct AirstrikePlugin;

impl Plugin for AirstrikePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Airstrike>().add_systems(Update, fly.after(crate::combat::CombatSet).after(crate::drone::update_drone));
    }
}

/// The UAV's target marker (StrikeMeshComponent Targeter02): on the ground where the view points
/// while the strike runs (AbsoluteRotation: lies flat; scale 1).
#[derive(Component)]
pub struct StrikeMesh;

#[derive(Resource, Default)]
pub struct Airstrike {
    pub active: bool,
    /// index into CombatData::strikes (Hellfire, cluster, white phosphorus, sensor fused)
    pub kind: usize,
    /// UE units / radians
    pos: Vec3,
    yaw: f32,
    pitch: f32,
    fuel: f32,
    left: i32,
    cd: f32,
    fuel_max: f32,
    /// orbit centre (UE) and angle (radians)
    centre: Vec3,
    angle: f32,
    /// zoomed in (CFG_Airstrike_* bAllowZoom, EKBC_AltFire): thermal vision when
    /// bThermalVisionWhenZoomed
    pub zoomed: bool,
    /// voice lines: shots fired, the player's kills when the last one was fired (result line once
    /// the missiles are down), bingo fuel said
    shots: i32,
    kills_at_fire: Option<i32>,
    result_wait: f32,
    bingo: bool,
    started: bool,
}

/// Operator voice lines (KAOSAIChatter.MPChatterConfig GCUtt_MPAirstrike* Line = the USOPERATOR
/// GCSoundCueInstance; the KPA / MONARCH lines are Line_NK)
const VO_TRANSITION: &str = "A2_Airstrike_C.PredatorTransition.USOPERATOR.SCI_PredatorTransition";
const VO_FIRST_HELLFIRE: &str = "A2_Airstrike_C.FirstHellfire.USOPERATOR.SCI_FirstHellfire";
const VO_SECOND_HELLFIRE: &str = "A2_Airstrike_C.SecondHellfire.USOPERATOR.SCI_SecondHellfire";
const VO_CARPET: &str = "A2_Airstrike_C.CarpetBombFired.USOPERATOR.SCI_CarpetBombFired";
const VO_PHOSPHORUS: &str = "A2_Airstrike_C.WhitePhosphorousFired.USOPERATOR.SCI_WhitePhosphorousFired";
const VO_KILL: &str = "A2_Airstrike_C.AirstrikeKill.USOPERATOR.SCI_AirstrikeKill";
const VO_MULTIKILL: &str = "A2_Airstrike_C.AirstrikeMultiKill.USOPERATOR.SCI_AirstrikeMultiKill";
const VO_MISS: &str = "A2_Airstrike_C.AirstrikeMiss.USOPERATOR.SCI_AirstrikeMiss";
const VO_BINGO: &str = "A2_Airstrike_C.BingoFuel.USOPERATOR.SCI_BingoFuel";
/// NOT from data: the bingo fuel line with this share of the fuel left, the result line this
/// long after the last missile / bomblet is down
const BINGO_SHARE: f32 = 0.2;
const RESULT_DELAY: f32 = 0.6;

impl Airstrike {
    /// Take control of the UAV for the player at `eye` (UE units) facing `yaw` (radians): it
    /// circles the point `orbit[3]` in front of the player, starting behind it, looking at the
    /// centre (NOT from data: the start angle, the fly-in is left out).
    pub fn start(&mut self, kind: usize, eye: Vec3, yaw: f32, fuel: f32, clip: i32, orbit: [f32; 4]) {
        let centre = eye + Vec3::new(yaw.cos(), yaw.sin(), 0.0) * orbit[3];
        let angle = yaw + std::f32::consts::PI;
        let pos = centre + Vec3::new(angle.cos() * orbit[1], angle.sin() * orbit[1], orbit[0]);
        let to = centre - pos;
        let pitch = to.z.atan2(Vec2::new(to.x, to.y).length());
        *self = Airstrike { active: true, kind, pos, yaw: to.y.atan2(to.x), pitch, fuel, left: clip, cd: 0.0, fuel_max: fuel.max(0.01), centre, angle, zoomed: false, ..Default::default() };
    }
}

/// Top faces of the marker mesh (Targeter02: the ring's segments and the cross bars) as quads,
/// 4 corners each in the mesh's own (Bevy-axis, UE-unit) space.
fn marker_quads(m: &Mesh) -> Vec<[Vec3; 4]> {
    let Some(pos) = m.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()) else { return Vec::new() };
    let Some(idx) = m.indices() else { return Vec::new() };
    let idx: Vec<usize> = idx.iter().collect();
    let top = pos.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    let v = |i: usize| Vec3::from(pos[i]);
    // the upper faces: facing up, in the top half of the ring's height (ring 4.65..5.38, the
    // cross arms slope from 5.38 to 5.23)
    let tris: Vec<[Vec3; 3]> = idx
        .chunks(3)
        .filter(|t| t.len() == 3)
        .map(|t| [v(t[0]), v(t[1]), v(t[2])])
        .filter(|t| {
            let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
            n.y.abs() > 0.95 && (t[0].y + t[1].y + t[2].y) / 3.0 > top - 0.3
        })
        .collect();
    let same = |a: Vec3, b: Vec3| a.distance(b) < 1e-3;
    let mut used = vec![false; tris.len()];
    let mut out = Vec::new();
    // a quad's two triangles share its diagonal: first pairs where it is the longest edge of
    // both, then where it is the longest of one (the ring's trapezoids)
    let longest = |t: &[Vec3; 3]| (0..3).map(|k| t[k].distance(t[(k + 1) % 3])).fold(0.0f32, f32::max);
    for pass in 0..2 {
        for i in 0..tris.len() {
            if used[i] {
                continue;
            }
            for j in i + 1..tris.len() {
                if used[j] {
                    continue;
                }
                let shared: Vec<Vec3> = tris[i].iter().copied().filter(|&a| tris[j].iter().any(|&b| same(a, b))).collect();
                if shared.len() != 2 {
                    continue;
                }
                let d = shared[0].distance(shared[1]);
                let (li, lj) = (d >= longest(&tris[i]) - 1e-3, d >= longest(&tris[j]) - 1e-3);
                if !(li && lj || pass == 1 && (li || lj)) {
                    continue;
                }
                let a = tris[i].iter().copied().find(|&x| !shared.iter().any(|&s| same(s, x))).unwrap_or(shared[0]);
                let b = tris[j].iter().copied().find(|&x| !shared.iter().any(|&s| same(s, x))).unwrap_or(shared[1]);
                out.push([a, shared[0], b, shared[1]]);
                used[i] = true;
                used[j] = true;
                break;
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn fly(
    time: Res<Time>,
    motion: Res<AccumulatedMouseMotion>,
    mut flow: ResMut<MatchFlow>,
    data: Option<Res<CombatData>>,
    col: Res<CollisionWorld>,
    mut st: ResMut<Airstrike>,
    mut pawns: Query<&mut Pawn>,
    mut cams: Query<(&Camera, &mut Projection, &mut Transform), (With<Camera3d>, Without<crate::minimap::MinimapCam>, Without<StrikeMesh>)>,
    (marker, meshes, mut quads): (Query<&Mesh3d, With<StrikeMesh>>, Res<Assets<Mesh>>, Local<Option<Vec<[Vec3; 4]>>>),
    (mut gr, mut fx, mut vm): (ResMut<Grenades>, ResMut<crate::particles::Particles>, Option<ResMut<crate::viewmodel::ViewModel>>),
    mut buf: Local<Vec<u32>>,
    (mut sfx, pc): (ResMut<crate::sfx::SfxQueue>, Res<crate::combat::PlayerCombat>),
) {
    flow.hud.predator = None;
    flow.hud.strike_marker.clear();
    // the marker's geometry, read once (the mesh itself stays hidden: drawn as HUD quads below)
    if quads.is_none() {
        if let Some(m) = marker.iter().next().and_then(|h| meshes.get(&h.0)) {
            let q = marker_quads(m);
            info!("airstrike: target marker {} quads", q.len());
            *quads = Some(q);
        }
    }
    // HB_HELLFIRE=1: the strike once, 3 s into the match (HUD tests)
    if std::env::var_os("HB_HELLFIRE").is_some() && !st.active && flow.phase == Phase::Playing {
        static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        // HB_HELLFIRE=<cost key> picks another strike (cfg_bmb_cluster, ...)
        let want = std::env::var("HB_HELLFIRE").ok().filter(|v| v.starts_with("cfg_")).unwrap_or_else(|| "cfg_bmb_hellfire".into());
        let ready = (pawns.single().ok().filter(|p| !p.dead), data.as_ref().and_then(|d| d.strikes.iter().position(|s| s.key == want).map(|k| (k, d.strikes[k].cfg.clone()))));
        if let (Some(p), Some((k, h))) = ready {
            if !DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                st.start(k, p.eye_ue(), p.yaw / 65536.0 * std::f32::consts::TAU, h.fuel, h.clip, h.orbit);
                info!("test: hellfire strike (fuel {}, clip {})", h.fuel, h.clip);
            }
        }
    }
    if !st.active {
        return;
    }
    let Ok(mut p) = pawns.single_mut() else { return };
    let (Some(_data), Some(strike)) = (data.as_ref(), data.as_ref().and_then(|d| d.strikes.get(st.kind).cloned())) else {
        st.active = false;
        return;
    };
    let hf = strike.cfg.clone();
    let dt = time.delta_secs().min(0.1);
    let missiles_flying = gr.list.iter().any(|g| g.rocket.is_some() && (g.rocket == Some(strike.weapon) || g.rocket == strike.sub)) || !gr.pending.is_empty();
    st.fuel -= dt;
    st.cd = (st.cd - dt).max(0.0);
    // voice: the operator takes over (PredatorTransition), bingo fuel, hit / multi kill / miss once
    // the fired missiles or bomblets are down
    if !st.started {
        st.started = true;
        sfx.play(VO_TRANSITION, None);
    }
    if !st.bingo && st.fuel < st.fuel_max * BINGO_SHARE {
        st.bingo = true;
        sfx.play(VO_BINGO, None);
    }
    if let Some(k0) = st.kills_at_fire {
        if missiles_flying {
            st.result_wait = 0.0;
        } else {
            st.result_wait += dt;
            if st.result_wait >= RESULT_DELAY {
                let n = pc.kills - k0;
                sfx.play(if n >= 2 { VO_MULTIKILL } else if n == 1 { VO_KILL } else { VO_MISS }, None);
                st.kills_at_fire = None;
            }
        }
    }
    // the UAV's engine (VehicleAudio SCEngineIdle), heard in the cockpit
    if !strike.engine.is_empty() {
        sfx.looping("strike_engine", &strike.engine, None);
    }
    let over = flow.phase != Phase::Playing || p.dead || st.fuel <= 0.0 || (st.left <= 0 && st.cd <= 0.0 && !missiles_flying && st.kills_at_fire.is_none());
    if over {
        st.active = false;
        st.zoomed = false;
        p.remote = false;
        p.fov = crate::settings::vfov();
        if let Some(vm) = vm.as_mut() {
            vm.lowered = false;
            vm.play(crate::viewmodel::WeaponAnim::Draw);
        }
        return;
    }
    p.remote = true;
    if let Some(vm) = vm.as_mut() {
        vm.lowered = true;
    }
    flow.hud.predator = Some(((st.fuel / st.fuel_max).clamp(0.0, 1.0), st.left));
    if !st.zoomed && flow.hud.hint.is_none() {
        flow.hud.hint = Some("strike_zoom");
    }
    if !flow.paused {
        // mouse look (GCPlayerInput sensitivity, rotation units -> radians)
        let k = 45.0 / 65536.0 * std::f32::consts::TAU;
        let (sx, sy) = crate::settings::mouse_scale_for("Heli", motion.delta, false);
        st.yaw += motion.delta.x * k * sx;
        st.pitch = (st.pitch - motion.delta.y * k * sy).clamp(-85f32.to_radians(), -10f32.to_radians());
        // zoom (bAllowZoom): held EKBC_AltFire (NOT from data: the zoomed FOV is half the view's)
        st.zoomed = crate::input::down(crate::input::Set::Heli, "EKBC_AltFire") || std::env::var_os("HB_STRIKEZOOM").is_some();
    }
    // GCV_AirstrikeUAV: circles the centre at FlySpeed (taken as radians per second, NOT from
    // data), FlyRadius away and FlyAltitude above it
    // (the view turns with it, BlendingMode ECBM_FixedOnUAV)
    st.angle += hf.orbit[2] * dt;
    st.yaw += hf.orbit[2] * dt;
    st.pos = st.centre + Vec3::new(st.angle.cos() * hf.orbit[1], st.angle.sin() * hf.orbit[1], hf.orbit[0]);
    p.fov = if st.zoomed { crate::settings::vfov() * 0.5 } else { crate::settings::vfov() };
    let d = Vec3::new(st.pitch.cos() * st.yaw.cos(), st.pitch.cos() * st.yaw.sin(), st.pitch.sin());
    let cam_b = Vec3::new(st.pos.x, st.pos.z, st.pos.y) * 0.01;
    let dir_b = Vec3::new(d.x, d.z, d.y);
    if let Ok((_, mut proj, mut t)) = cams.single_mut() {
        *t = Transform::from_translation(cam_b).looking_to(dir_b, Vec3::Y);
        if let Projection::Perspective(pp) = &mut *proj {
            pp.fov = p.fov.to_radians();
        }
    }
    let aim = match col.raycast(cam_b, dir_b, 327.68, &mut buf) {
        Some((t, _)) => cam_b + dir_b * t,
        None => cam_b + dir_b * 327.68,
    };
    // marker size: Targeter02 has a radius of 9.87 uu, the native code scales it. The user's video
    // shows the Hellfire ring about as wide as its DamageRadiusFull (200) and all bomb rings about
    // as wide as the cluster bomb's StrikeWidth (800) (NOT from data)
    let cluster_width = data.as_ref().and_then(|d| d.strikes.iter().find_map(|s| s.cfg.payload.as_ref().filter(|p| p.kind == 1).map(|p| p.spread))).unwrap_or(800.0);
    let area = if hf.payload.is_some() { cluster_width } else if hf.missile.radius_full > 0.0 { hf.missile.radius_full } else { hf.missile.radius }.max(50.0);
    let scale = 0.01 * area / 9.87;
    // the marker (StrikeMeshComponent Targeter02, M_laserSphere01: additive, EmissiveColor
    // lineGradient03 + 1, Opacity 0.5) lies flat on the ground at the aim point. Drawn as HUD
    // quads projected from its top faces (NOT from data: the mesh does not show through the
    // game-material path here; white at alpha 0.75 stands in for the material)
    if let (Some(q), Ok((camera, _, _))) = (quads.as_ref(), cams.single()) {
        let view = Transform::from_translation(cam_b).looking_to(dir_b, Vec3::Y);
        let gt = GlobalTransform::from(view);
        let vp = camera.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
        let k = Vec2::new(1280.0 / vp.x.max(1.0), 720.0 / vp.y.max(1.0));
        let top = q.iter().flat_map(|c| c.iter().map(|p| p.y)).fold(f32::MIN, f32::max);
        for c in q {
            let world: Vec<Vec3> = c.iter().map(|p| aim + Vec3::new(p.x, p.y - top, p.z) * scale + Vec3::Y * 0.05).collect();
            // hidden behind the world (the user's video: the ring is cut by roofs)
            let mid = world.iter().copied().sum::<Vec3>() / 4.0;
            let to = mid - cam_b;
            let l = to.length();
            // (NOT from data: 1.5 m margin so the ground the ring lies on does not hide it)
            if l > 2.0 && col.raycast(cam_b, to / l, l - 1.5, &mut buf).is_some() {
                continue;
            }
            let pts: Option<Vec<Vec2>> = world.iter().map(|w| camera.world_to_viewport(&gt, *w).ok().map(|s| s * k)).collect();
            let Some(pts) = pts else { continue };
            let centre = pts.iter().copied().sum::<Vec2>() / 4.0;
            // long axis: the longer of the two side directions
            let (s1, s2) = (((pts[1] - pts[0]) + (pts[2] - pts[3])) * 0.5, ((pts[3] - pts[0]) + (pts[2] - pts[1])) * 0.5);
            let (along, across) = if s1.length() >= s2.length() { (s1, s2) } else { (s2, s1) };
            let len = along.length();
            let width = across.length().clamp(1.5, 30.0);
            if len < 0.5 {
                continue;
            }
            flow.hud.strike_marker.push((centre, len + width * 0.5, width, along.y.atan2(along.x)));
        }
    }
    // fire at the designated point
    // HB_STRIKEFIRE=1: fire 3 s into the strike (tests)
    let test_fire = std::env::var_os("HB_STRIKEFIRE").is_some() && st.fuel_max - st.fuel > 3.0;
    if std::env::var_os("HB_STRIKEDBG").is_some() && (st.fuel * 2.0).floor() != ((st.fuel + dt) * 2.0).floor() {
        info!("strike: fuel {:.1} / {:.1} left {} cd {:.2} test_fire {test_fire} paused {}", st.fuel, st.fuel_max, st.left, st.cd, flow.paused);
    }
    if !flow.paused && (crate::input::hit(crate::input::Set::Heli, "EKBC_Fire") || test_fire) && st.left > 0 && st.cd <= 0.0 {
        let aim_ue = Vec3::new(aim.x, aim.z, aim.y) * 100.0;
        let start = st.pos + d * 150.0;
        let md = (aim_ue - start).normalize_or_zero();
        let trail = hf.missile.flight_fx.as_ref().and_then(|t| fx.fire_tracked(t, start, crate::combat::dir_quat(md)));
        gr.list.push(Grenade { rocket: Some(strike.weapon), pos: start, vel: md * hf.missile.speed, fuse: 14.0, owner: Actor::Player, team: player_team(), resting: false, trail, tac: None, payload: hf.payload.as_ref().map(|_| (st.kind, aim_ue)) });
        st.left -= 1;
        st.cd = hf.reload;
        // the seat's SCFireSound (Hellfire: SC_AH64_Missile_Fire) and the operator's line
        if !strike.fire_sound.is_empty() {
            sfx.play(&strike.fire_sound, None);
        }
        let vo = match (hf.payload.as_ref().map(|p| p.kind), st.shots) {
            (None, 0) => VO_FIRST_HELLFIRE,
            (None, _) => VO_SECOND_HELLFIRE,
            (Some(2), _) => VO_PHOSPHORUS,
            // cluster and sensor fused: the carpet bomb line (the sensor fused UAV has the
            // AudioSetup_PredatorDrone_Carpet; for the cluster bomb NOT from data)
            _ => VO_CARPET,
        };
        sfx.play(vo, None);
        st.shots += 1;
        if st.kills_at_fire.is_none() {
            st.kills_at_fire = Some(pc.kills);
        }
        st.result_wait = 0.0;
    }
}
