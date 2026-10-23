//! Homefront's dynamic sun shadows ("full screen" cascades + "full level" shadow), as in the
//! Overpass trace (frame 3600, projection draws 60461491 / 60463216 / 60466228, PS blob_call20973):
//!
//! * DirectionalLightComponent (Overpass_sky): FullScreenShadowSections = 2,
//!   FullScreenShadowCullDistance = 800, FSShadowDistanceRatio = 2.25 (-> 1800),
//!   ShadowFadeOutDistance = 30, FullScreenShadowFilterRadius = 2.5, FullLevelShadowFilterRadius = 4.
//! * cascades: near [near plane, 815], mid [785, 1815] (1024^2 each), full level beyond 1785 (1600^2,
//!   fixed, centre/radius measured). Weights (ShadowLerpInfo): near sat(d) * sat((815 - d) / 30),
//!   mid sat((d - 785) / 30) * sat((1815 - d) / 30), full sat((d - 1785) / 30) * sat(200000 - d).
//! * fitting (WorldToShadow solved from ScreenToShadowMatrix): bounding sphere of the 8 slice
//!   corners (centre = corner mean, radius = max distance), box 2r x 2r, depth range 2r centred,
//!   centre snapped to texels.
//! * per cascade 4 hardware-PCF taps at uv +- (R / (res * 2 sqrt 2)) on each axis (R = filter
//!   radius), clamped to ShadowBufferValidRect; bias = min(max(d - 0, 0) * 0.0001, 0.001) in
//!   shadow depth units. Sum of weight * shadow = LightAttenuationTexture.
//!
//! Bevy renders the depth maps (DirectionalLight shadow pass); its cascades are replaced by the
//! game's fit after Bevy computes them. Deviation: Bevy uses one size for all cascades, the full
//! level map is 1024^2 here instead of 1600^2. NOT from data (2026-10-21): the full-level map
//! follows the camera (measured radius), and points outside a cascade count as lit.

use bevy::{
    render::camera::CameraProjection,
    pbr::{Cascade, Cascades, DirectionalLightShadowMap, SimulationLightSystems},
    prelude::*,
};

pub const NEAR_RES: f32 = 1024.0;

#[derive(Resource, Clone, Copy)]
pub struct SunShadow {
    /// first split (FullScreenShadowCullDistance), UE units
    pub split1: f32,
    /// second split (split1 * FSShadowDistanceRatio)
    pub split2: f32,
    /// cross-fade width (ShadowFadeOutDistance)
    pub fade: f32,
    /// full-level shadow sphere (UE world), measured from the trace for Overpass
    pub level_center: [f32; 3],
    pub level_radius: f32,
}

impl Default for SunShadow {
    fn default() -> Self {
        SunShadow { split1: 800.0, split2: 1800.0, fade: 30.0, level_center: [-6417.23, -4959.99, 4938.45], level_radius: 20128.0 }
    }
}

/// WGSL: `hf_sun_shadow(frag_position) -> f32` (LightAttenuationTexture value).
pub fn wgsl(s: &SunShadow, filter_near: f32, filter_level: f32) -> String {
    let a_near = filter_near / (NEAR_RES * 2.0 * std::f32::consts::SQRT_2);
    let a_level = filter_level / (1600.0 * 2.0 * std::f32::consts::SQRT_2);
    let lo = 1.0 / NEAR_RES;
    format!(
        r#"
#import bevy_pbr::mesh_view_bindings::{{lights, directional_shadow_textures, directional_shadow_textures_comparison_sampler}}

fn hf_pcf(ci: u32, wp: vec4<f32>, bias: f32, a: f32) -> f32 {{
    let dl = lights.directional_lights[0];
    let p = dl.cascades[ci].clip_from_world * wp;
    let uv = p.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    // reverse Z: larger = closer to the light; the game moves the receiver towards the light
    let z = p.z + bias;
    // outside the cascade's box / depth range: lit (NOT from data; the game's full-level map
    // covers its whole level, here the edge texels were stretched over the rest of the map:
    // hard black triangles on Angel Island)
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || p.z < 0.0 || p.z > 1.0) {{
        return 1.0;
    }}
    let layer = i32(dl.depth_texture_base_index + ci);
    let lo = vec2<f32>({lo:?});
    let hi = vec2<f32>(1.0 - {lo:?});
    var s = textureSampleCompareLevel(directional_shadow_textures, directional_shadow_textures_comparison_sampler, clamp(uv + vec2<f32>(-a, 0.0), lo, hi), layer, z);
    s += textureSampleCompareLevel(directional_shadow_textures, directional_shadow_textures_comparison_sampler, clamp(uv + vec2<f32>(0.0, -a), lo, hi), layer, z);
    s += textureSampleCompareLevel(directional_shadow_textures, directional_shadow_textures_comparison_sampler, clamp(uv + vec2<f32>(0.0, a), lo, hi), layer, z);
    s += textureSampleCompareLevel(directional_shadow_textures, directional_shadow_textures_comparison_sampler, clamp(uv + vec2<f32>(a, 0.0), lo, hi), layer, z);
    return s * 0.25;
}}

fn hf_sun_shadow(frag: vec4<f32>) -> f32 {{
    let ndc = vec2<f32>((frag.x - view.viewport.x) / view.viewport.z * 2.0 - 1.0, 1.0 - (frag.y - view.viewport.y) / view.viewport.w * 2.0);
    let wh = view.world_from_clip * vec4<f32>(ndc, frag.z, 1.0);
    return hf_sun_shadow_at(wh.xyz / wh.w);
}}

// the same for a Bevy world position (1P meshes: their depth is squeezed, gamemat.rs)
fn hf_sun_shadow_at(bevy_world: vec3<f32>) -> f32 {{
    if (lights.n_directional_lights == 0u) {{
        return 1.0;
    }}
    let wp = vec4<f32>(bevy_world, 1.0);
    let d = -(view.view_from_world * wp).z * 100.0;
    let bias = min(max(d, 0.0) * 0.0001, 0.001);
    let w0 = clamp(d, 0.0, 1.0) * clamp(({e1:?} - d) / {f:?}, 0.0, 1.0);
    let w1 = clamp((d - {s1:?}) / {f:?}, 0.0, 1.0) * clamp(({e2:?} - d) / {f:?}, 0.0, 1.0);
    let w2 = clamp((d - {s2:?}) / {f:?}, 0.0, 1.0) * clamp(200000.0 - d, 0.0, 1.0);
    var att = 0.0;
    if (w0 > 0.0) {{ att += w0 * hf_pcf(0u, wp, bias, {a_near:?}); }}
    if (w1 > 0.0) {{ att += w1 * hf_pcf(1u, wp, bias, {a_near:?}); }}
    if (w2 > 0.0) {{ att += w2 * hf_pcf(2u, wp, bias, {a_level:?}); }}
    return clamp(att, 0.0, 1.0);
}}
"#,
        e1 = s.split1 + s.fade * 0.5,
        s1 = s.split1 - s.fade * 0.5,
        e2 = s.split2 + s.fade * 0.5,
        s2 = s.split2 - s.fade * 0.5,
        f = s.fade,
    )
}

/// naga validation stubs for the bindings `wgsl()` uses (gamemat::validate_wgsl).
pub const STUBS: &str = r#"
struct DirectionalCascade { clip_from_world: mat4x4<f32>, texel_size: f32, far_bound: f32 }
struct DirectionalLight { cascades: array<DirectionalCascade, 4>, color: vec4<f32>, direction_to_light: vec3<f32>, flags: u32, soft_shadow_size: f32, shadow_depth_bias: f32, shadow_normal_bias: f32, num_cascades: u32, cascades_overlap_proportion: f32, depth_texture_base_index: u32 }
struct Lights { directional_lights: array<DirectionalLight, 10u>, n_directional_lights: u32 }
@group(0) @binding(3) var<uniform> lights: Lights;
@group(0) @binding(5) var directional_shadow_textures: texture_depth_2d_array;
@group(0) @binding(6) var directional_shadow_textures_comparison_sampler: sampler_comparison;
"#;

pub struct SunShadowPlugin;

impl Plugin for SunShadowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SunShadow>().insert_resource(DirectionalLightShadowMap { size: NEAR_RES as usize }).add_systems(
            PostUpdate,
            override_cascades
                .after(SimulationLightSystems::UpdateDirectionalLightCascades)
                .before(SimulationLightSystems::UpdateLightFrusta),
        );
    }
}

fn cascade(light_from_world: Mat4, center: Vec3, r: f32, res: f32) -> Cascade {
    let cl = light_from_world.transform_point3(center);
    let texel = 2.0 * r / res;
    let snapped = Vec3::new((cl.x / texel).round() * texel, (cl.y / texel).round() * texel, cl.z);
    let cascade_from_world = Mat4::from_translation(-snapped) * light_from_world;
    // x, y: [-r, r] -> [-1, 1]; z: depth range 2r centred, 1 = towards the light (reverse Z)
    let clip_from_cascade = Mat4::from_cols(
        Vec4::new(1.0 / r, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0 / r, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 0.5 / r, 0.0),
        Vec4::new(0.0, 0.0, 0.5, 1.0),
    );
    // Bevy keeps the fields crate-private: set them through reflection
    let mut c = Cascade::default();
    set_field(&mut c, "world_from_cascade", cascade_from_world.inverse());
    set_field(&mut c, "clip_from_cascade", clip_from_cascade);
    set_field(&mut c, "clip_from_world", clip_from_cascade * cascade_from_world);
    set_field(&mut c, "texel_size", texel);
    c
}

fn set_field<T: bevy::reflect::Reflect + Clone>(target: &mut dyn bevy::reflect::Struct, name: &str, v: T) {
    if let Some(f) = target.field_mut(name).and_then(|f| f.try_downcast_mut::<T>()) {
        *f = v;
    }
}

fn slice_sphere(proj: &Projection, cam: &GlobalTransform, z0: f32, z1: f32) -> (Vec3, f32) {
    let corners = proj.get_frustum_corners(-z0, -z1);
    let m = cam.compute_matrix();
    let w: Vec<Vec3> = corners.iter().map(|c| m.transform_point3(Vec3::from(*c))).collect();
    let center = w.iter().copied().sum::<Vec3>() / w.len() as f32;
    let r = w.iter().map(|p| p.distance(center)).fold(0.0f32, f32::max);
    (center, r)
}

fn override_cascades(
    s: Res<SunShadow>,
    cams: Query<(Entity, &GlobalTransform, &Projection, &Camera), Without<crate::minimap::MinimapCam>>,
    mut lights: Query<(&GlobalTransform, &DirectionalLight, &mut Cascades)>,
) {
    for (lt, dl, mut cascades) in &mut lights {
        if !dl.shadows_enabled {
            continue;
        }
        let light_from_world = Mat4::from_quat(lt.compute_transform().rotation).inverse();
        for (e, gt, proj, cam) in &cams {
            if !cam.is_active {
                continue;
            }
            let near = match proj {
                Projection::Perspective(p) => p.near,
                _ => 0.1,
            };
            let (c0, r0) = slice_sphere(proj, gt, near, (s.split1 + s.fade * 0.5) * 0.01);
            let (c1, r1) = slice_sphere(proj, gt, (s.split1 - s.fade * 0.5) * 0.01, (s.split2 + s.fade * 0.5) * 0.01);
            // full-level map: the measured Overpass sphere radius, centred on the camera (NOT
            // from data: the fixed Overpass centre left most of Angel Island / Crossroads outside)
            let _ = s.level_center;
            let lc = gt.translation();
            let v = vec![
                cascade(light_from_world, c0, r0, NEAR_RES),
                cascade(light_from_world, c1, r1, NEAR_RES),
                cascade(light_from_world, lc, s.level_radius * 0.01, NEAR_RES),
            ];
            use bevy::reflect::Struct;
            if let Some(map) = cascades.field_mut("cascades").and_then(|f| f.try_downcast_mut::<bevy::ecs::entity::EntityHashMap<Vec<Cascade>>>()) {
                map.insert(e, v);
            }
        }
    }
}
