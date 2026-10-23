//! Dynamic point lights: explosion flashes and the C4 charge's blinking light.
//!
//! - flashes: GCExplosionLight_* class defaults (GCGame.u), TimeShift keys {StartTime, Radius,
//!   Brightness, LightColor}; between two keys radius, brightness and colour are interpolated
//!   linearly, after the last key the light is off (as UT3's UTExplosionLight.Tick, which the
//!   GCExplosionLight data mirrors; the GC tick itself is native and not decoded)
//! - falloff: PointLightComponent FalloffExponent 2.0 (Engine.u Default__PointLightComponent):
//!   pow(saturate(1 - (d / Radius)^2), 2) * LightColor * Brightness * N.L
//! - which projectile gets which light class is NOT from data (the projectile explosion code
//!   passing LightClass to ObjectPool_EmitterProjExplosions.GetProjectileExplosionEmitter is
//!   native; GCProj_Airstrike.SpawnProjectileExplosionParticles passes None)
//!
//! The lights are Bevy PointLights (clustered); GameMat and the terrain shader read them through
//! the view's clusterable object lists (WGSL below), Bevy's own shading is not used.

use bevy::prelude::*;
use std::collections::HashMap;

/// Bevy point light entities kept for the dynamic lights (the strongest ones win).
const SLOTS: usize = 24;

#[derive(Clone, Copy, Debug)]
pub struct LightKey {
    pub t: f32,
    /// UE units
    pub radius: f32,
    pub brightness: f32,
    /// linear colour ((c / 255)^2.2)
    pub color: [f32; 3],
}

#[derive(Resource, Default)]
pub struct ExplosionLights {
    pub classes: HashMap<String, Vec<LightKey>>,
}

impl ExplosionLights {
    pub fn load(g: &upk::Package) -> ExplosionLights {
        let mut out = ExplosionLights::default();
        for (i, e) in g.exports.iter().enumerate() {
            let name = g.object_name_of(e);
            let Some(class) = name.strip_prefix("Default__") else { continue };
            if !class.starts_with("GCExplosionLight_") {
                continue;
            }
            let Some(pr) = g.export_props(i) else { continue };
            let keys: Vec<LightKey> = g
                .struct_array(&pr, "TimeShift")
                .iter()
                .map(|k| {
                    let c = k.color("LightColor").unwrap_or([255, 255, 255, 255]);
                    let lin = |x: u8| (x as f32 / 255.0).powf(2.2);
                    LightKey { t: k.float("StartTime").unwrap_or(0.0), radius: k.float("Radius").unwrap_or(0.0), brightness: k.float("Brightness").unwrap_or(0.0), color: [lin(c[0]), lin(c[1]), lin(c[2])] }
                })
                .collect();
            if !keys.is_empty() {
                info!("explosion light {class}: {keys:?}");
                out.classes.insert(class.to_string(), keys);
            }
        }
        out
    }
}

/// A light for this frame: UE position, linear colour * brightness, radius (UE units).
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub at: Vec3,
    pub color: [f32; 3],
    pub radius: f32,
}

#[derive(Resource, Default)]
pub struct DynLights {
    flashes: Vec<(Vec3, Vec<LightKey>, f32)>,
    /// lights for the current frame only (C4 blink), refilled by their owners every frame
    pub steady: Vec<Light>,
}

impl DynLights {
    /// Start an explosion flash of a GCExplosionLight_* class at a UE position.
    pub fn flash(&mut self, lights: &ExplosionLights, class: &str, at: Vec3) {
        if let Some(k) = lights.classes.get(class) {
            self.flashes.push((at, k.clone(), 0.0));
        }
    }

    pub fn clear(&mut self) {
        self.flashes.clear();
        self.steady.clear();
    }
}

fn sample(keys: &[LightKey], t: f32) -> Option<(f32, f32, [f32; 3])> {
    let i = keys.iter().rposition(|k| k.t <= t)?;
    let a = keys[i];
    let b = *keys.get(i + 1)?;
    let f = ((t - a.t) / (b.t - a.t).max(1e-4)).clamp(0.0, 1.0);
    let l = |x: f32, y: f32| x + (y - x) * f;
    Some((l(a.radius, b.radius), l(a.brightness, b.brightness), [l(a.color[0], b.color[0]), l(a.color[1], b.color[1]), l(a.color[2], b.color[2])]))
}

#[derive(Component)]
struct Slot;

pub struct DynLightsPlugin;

impl Plugin for DynLightsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DynLights>().init_resource::<ExplosionLights>().add_systems(Startup, setup).add_systems(PostUpdate, update.before(bevy::render::view::VisibilitySystems::CheckVisibility));
    }
}

fn setup(mut commands: Commands) {
    for _ in 0..SLOTS {
        commands.spawn((Slot, PointLight { intensity: 0.0, range: 1.0, radius: 0.0, shadows_enabled: false, ..default() }, Transform::default(), Visibility::Hidden));
    }
}

/// C4 blink: the colour of KFX_Weapons.HandThrown.Lights_C4 (ColorOverLife (25, 1, 1), on
/// M_SphereLight_Particle_48_LightsFlicker, MatControlledFlickerSpeed 2). NOT from data: the
/// on / off pattern (the parent material FX_MeshLight.M_SphereLight is not in the install),
/// radius and brightness (the material lights 48 units; made stronger on request)
const C4_BLINK_HZ: f32 = 2.0;
const C4_BLINK_ON: f32 = 0.35;
const C4_RADIUS: f32 = 450.0;
const C4_BRIGHTNESS: f32 = 6.0;
/// rocket motor light (NOT from data)
const ROCKET_RADIUS: f32 = 700.0;
const ROCKET_BRIGHTNESS: f32 = 8.0;
/// flying tracer light (NOT from data, user request): short range, at most this many at once
const TRACER_RADIUS: f32 = 300.0;
const TRACER_BRIGHTNESS: f32 = 4.0;
const TRACER_MAX: usize = 8;

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn update(time: Res<Time>, mut dl: ResMut<DynLights>, el: Res<ExplosionLights>, gr: Option<Res<crate::combat::Grenades>>, cams: Query<&GlobalTransform, (With<Camera3d>, Without<crate::minimap::MinimapCam>)>, mut slots: Query<(&mut PointLight, &mut Transform, &mut Visibility), With<Slot>>) {
    let dt = time.delta_secs().min(0.1);
    if let Some(gr) = gr.as_ref() {
        for (at, class) in &gr.exploded {
            if !class.is_empty() {
                dl.flash(&el, class, *at);
            }
        }
        let on = (time.elapsed_secs() * C4_BLINK_HZ).fract() < C4_BLINK_ON;
        if std::env::var_os("HB_LIGHTDBG").is_some() {
            info!("c4 blink {on} t {:.2}", time.elapsed_secs());
        }
        // rocket motors (launcher rockets, drone rockets, Hellfire / airstrike missiles): a
        // flickering orange light just behind the projectile while it flies. NOT from data
        // (user 2026-10-19: "let the rockets Projectile emit light while flying aswell like the
        // c4"); the game's GCProj classes have no light class
        let tt = time.elapsed_secs();
        for (k, n) in gr.list.iter().enumerate().filter(|(_, n)| (n.rocket.is_some() || n.payload.is_some()) && !n.resting) {
            let fl = 0.8 + 0.12 * (tt * 31.0 + k as f32 * 1.7).sin() + 0.08 * (tt * 57.0 + k as f32).sin();
            let back = n.vel.normalize_or_zero() * -45.0;
            let b = ROCKET_BRIGHTNESS * fl;
            dl.steady.push(Light { at: n.pos + back, color: [b, b * 0.5, b * 0.16], radius: ROCKET_RADIUS });
        }
        if on {
            for n in gr.list.iter().filter(|n| n.tac.as_ref().is_some_and(|t| t.kind == 3)) {
                let k = C4_BRIGHTNESS / 25.0;
                // a little off the surface the charge sits on
                dl.steady.push(Light { at: n.pos + Vec3::Z * 8.0, color: [25.0 * k, 1.0 * k, 1.0 * k], radius: C4_RADIUS });
            }
        }
    }
    // flying tracers (combat::fire_tracer): a small light in the tracer's colour at its head,
    // the TRACER_MAX nearest to the camera. NOT from data - user request 2026-10-21 ("let
    // Bullets emit lights when flying like rockets"); radius / brightness / count are not
    let cam_ue = cams.iter().next().map(|c| {
        let p = c.translation();
        Vec3::new(p.x, p.z, p.y) * 100.0
    });
    crate::combat::set_camera_ue(cam_ue);
    let tracer_scale = 0.8 * (0.8 + (0.6 - 0.8) * crate::weather::daylight().clamp(0.0, 1.0));
    if let Ok(mut tl) = crate::combat::TRACER_LIGHTS.lock() {
        for t in tl.iter_mut() {
            t.age += dt;
        }
        tl.retain(|t| t.age < t.life);
        let mut near: Vec<(f32, Light)> = tl
            .iter()
            .map(|t| {
                let at = t.from + t.dir * (t.speed * t.age);
                let m = t.color.iter().cloned().fold(0.0f32, f32::max).max(1e-3);
                // user request 2026-10-21 ("looks like star wars"): tracer lights 20 % weaker
                // at night, 40 % weaker by day (NOT from data; rockets / launchers unchanged)
                // No light within 2 m of the camera / the muzzle, full from 6 m (NOT from data):
                // a tracer light next to the camera (the player's own shots leaving the muzzle,
                // bot tracers flying past the head) lit everything within 3 m - gun, floor,
                // walls - flat red / green / yellow for a frame: the whole picture flashed
                // ("flickering", user 2026-10-22 20:48 / 21:18, video in a barn).
                let near_cam = cam_ue.map(|c| smooth(200.0, 600.0, c.distance(at))).unwrap_or(1.0);
                let near_muzzle = smooth(200.0, 600.0, t.speed * t.age);
                let k = TRACER_BRIGHTNESS / m * tracer_scale * near_cam * near_muzzle;
                (cam_ue.map(|c| c.distance_squared(at)).unwrap_or(0.0), Light { at, color: t.color.map(|x| x * k), radius: TRACER_RADIUS })
            })
            .collect();
        if near.len() > TRACER_MAX {
            near.select_nth_unstable_by(TRACER_MAX, |a, b| a.0.total_cmp(&b.0));
            near.truncate(TRACER_MAX);
        }
        dl.steady.extend(near.into_iter().map(|x| x.1));
    }
    // HB_TEST_LIGHT=<brightness>,<radius>: a steady white light 4 m in front of the camera (tests)
    if let Ok(v) = std::env::var("HB_TEST_LIGHT") {
        let mut it = v.split(',').filter_map(|x| x.parse::<f32>().ok());
        let (b, r) = (it.next().unwrap_or(10.0), it.next().unwrap_or(800.0));
        if let Ok(c) = cams.single() {
            let p = c.translation() + c.forward() * 4.0;
            dl.steady.push(Light { at: Vec3::new(p.x, p.z, p.y) * 100.0, color: [b, b, b], radius: r });
        }
    }
    let mut list: Vec<Light> = std::mem::take(&mut dl.steady);
    // HB_NODYNLIGHTS=1: no dynamic lights (diagnosis)
    if crate::diag("HB_NODYNLIGHTS") {
        list.clear();
        dl.flashes.clear();
    }
    for f in dl.flashes.iter_mut() {
        f.2 += dt;
    }
    dl.flashes.retain(|f| f.1.last().is_some_and(|k| f.2 < k.t));
    for (at, keys, t) in &dl.flashes {
        if let Some((r, b, c)) = sample(keys, *t) {
            if r > 0.0 && b > 0.0 {
                list.push(Light { at: *at, color: c.map(|x| x * b), radius: r });
            }
        }
    }
    let power = |l: &Light| (l.color[0] + l.color[1] + l.color[2]) * l.radius;
    list.sort_by(|a, b| power(b).total_cmp(&power(a)));
    let mut it = list.into_iter();
    for (mut pl, mut tr, mut vis) in slots.iter_mut() {
        match it.next() {
            Some(l) => {
                let m = l.color.iter().cloned().fold(0.0f32, f32::max).max(1e-6);
                // stored colour = colour * intensity / 4pi (bevy_pbr light.rs): carries the
                // UE brightness unchanged; range = Radius
                pl.color = Color::linear_rgb(l.color[0] / m, l.color[1] / m, l.color[2] / m);
                pl.intensity = m * 4.0 * std::f32::consts::PI;
                pl.range = l.radius * 0.01;
                tr.translation = Vec3::new(l.at.x, l.at.z, l.at.y) * 0.01;
                *vis = Visibility::Visible;
            }
            None => {
                // unused: dark and tiny (a hidden light was not reliably dropped from the clusters)
                if pl.intensity != 0.0 {
                    pl.intensity = 0.0;
                    pl.range = 0.01;
                }
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

/// WGSL: `hf_dyn_lights(frag_position, ue_world_position, ue_world_normal) -> vec3` (summed
/// irradiance of the dynamic lights, multiplied with the diffuse colour by the callers).
pub const WGSL: &str = r#"
#import bevy_pbr::mesh_view_bindings::clusterable_objects
#import bevy_pbr::clustered_forward::{fragment_cluster_index, unpack_clusterable_object_index_ranges, get_clusterable_object_id}

fn hf_dyn_lights(frag: vec4<f32>, wue: vec3<f32>, nue: vec3<f32>) -> vec3<f32> {
    let wb = vec4<f32>(wue.x * 0.01, wue.z * 0.01, wue.y * 0.01, 1.0);
    let vz = (view.view_from_world * wb).z;
    let ci = fragment_cluster_index(frag.xy, vz, false);
    let r = unpack_clusterable_object_index_ranges(ci);
    var sum = vec3<f32>(0.0);
    for (var k = r.first_point_light_index_offset; k < r.first_spot_light_index_offset; k = k + 1u) {
        let l = clusterable_objects.data[get_clusterable_object_id(k)];
        let p = vec3<f32>(l.position_radius.x, l.position_radius.z, l.position_radius.y) * 100.0;
        let rr = 10000.0 / max(l.color_inverse_square_range.w, 1e-12);
        let d = p - wue;
        let q = dot(d, d) / rr;
        if (q < 1.0) {
            let a = (1.0 - q) * (1.0 - q);
            sum += l.color_inverse_square_range.rgb * a * clamp(dot(nue, d * inverseSqrt(max(dot(d, d), 1e-4))), 0.0, 1.0);
        }
    }
    return sum;
}
"#;

/// naga validation stubs for `WGSL` (gamemat::validate_wgsl).
pub const STUBS: &str = r#"
struct HfClusterRanges { first_point_light_index_offset: u32, first_spot_light_index_offset: u32 }
struct HfClusterable { light_custom_data: vec4<f32>, color_inverse_square_range: vec4<f32>, position_radius: vec4<f32> }
struct HfClusterables { data: array<HfClusterable> }
@group(0) @binding(8) var<storage> clusterable_objects: HfClusterables;
fn fragment_cluster_index(a: vec2<f32>, z: f32, o: bool) -> u32 { return 0u; }
fn unpack_clusterable_object_index_ranges(c: u32) -> HfClusterRanges { return HfClusterRanges(0u, 0u); }
fn get_clusterable_object_id(i: u32) -> u32 { return i; }
"#;
