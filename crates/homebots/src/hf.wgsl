// HomeBots material extension (on top of Bevy's StandardMaterial):
//  * UE3 hemispherical SkyLight: albedo * lerp(lower, upper, N_up * 0.5 + 0.5)
//    (values come from the level's SkyLightComponents; Bevy's own ambient is off).
//  * Terrain (mode 1): per-pixel blend of the four layer materials with the level's
//    weightmap, layer i tiled every MappingScale_i terrain patches.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}

struct HfParams {
    sky_upper: vec4<f32>,
    sky_lower: vec4<f32>,
    // terrain bounds in UE units: min x, max x, min y, max y
    bounds: vec4<f32>,
    // terrain Location x, y and patch size x, y (UE units)
    origin: vec4<f32>,
    // MappingScale of layers 0..3 (patches per texture tile)
    scales: vec4<f32>,
    // x: mode (0 = mesh, 1 = terrain)
    flags: vec4<u32>,
}

@group(2) @binding(100) var<uniform> hf: HfParams;
@group(2) @binding(101) var weight_tex: texture_2d<f32>;
@group(2) @binding(102) var weight_smp: sampler;
@group(2) @binding(103) var layer0_tex: texture_2d<f32>;
@group(2) @binding(104) var layer0_smp: sampler;
@group(2) @binding(105) var layer1_tex: texture_2d<f32>;
@group(2) @binding(106) var layer1_smp: sampler;
@group(2) @binding(107) var layer2_tex: texture_2d<f32>;
@group(2) @binding(108) var layer2_smp: sampler;
@group(2) @binding(109) var layer3_tex: texture_2d<f32>;
@group(2) @binding(110) var layer3_smp: sampler;

const UNLIT_BIT: u32 = 32u;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    if (hf.flags.x == 1u) {
        // Bevy world (metres, Y up) -> UE units: x = X * 100, y = Z * 100.
        let ue = vec2<f32>(in.world_position.x * 100.0, in.world_position.z * 100.0);
        let wuv = (ue - vec2<f32>(hf.bounds.x, hf.bounds.z)) / vec2<f32>(hf.bounds.y - hf.bounds.x, hf.bounds.w - hf.bounds.z);
        let w = textureSample(weight_tex, weight_smp, wuv);
        // AlphaMapIndex 0..3 -> channels A, R, G, B (verified against slope/height).
        let wt = vec4<f32>(w.a, w.r, w.g, w.b);
        // ("patch" is a reserved WGSL keyword)
        let pc = (ue - hf.origin.xy) / hf.origin.zw;
        let c0 = textureSample(layer0_tex, layer0_smp, pc / hf.scales.x).rgb;
        let c1 = textureSample(layer1_tex, layer1_smp, pc / hf.scales.y).rgb;
        let c2 = textureSample(layer2_tex, layer2_smp, pc / hf.scales.z).rgb;
        let c3 = textureSample(layer3_tex, layer3_smp, pc / hf.scales.w).rgb;
        let s = max(wt.x + wt.y + wt.z + wt.w, 1e-4);
        let col = (c0 * wt.x + c1 * wt.y + c2 * wt.z + c3 * wt.w) / s;
        pbr_input.material.base_color = vec4<f32>(col, 1.0);
    }

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    // Same branch as Bevy's own pbr.wgsl: unlit materials skip lighting entirely.
    if ((pbr_input.material.flags & UNLIT_BIT) == 0u) {
        out.color = apply_pbr_lighting(pbr_input);
        // UE3 sky light, in the same units as the direct lights (see exposure setup).
        let up = clamp(pbr_input.N.y * 0.5 + 0.5, 0.0, 1.0);
        let sky = mix(hf.sky_lower.rgb, hf.sky_upper.rgb, up);
        out.color = vec4<f32>(out.color.rgb + pbr_input.material.base_color.rgb * sky * view.exposure * 1.2, out.color.a);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
