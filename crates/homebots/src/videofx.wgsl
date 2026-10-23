// MAT_RD.postFX.M_VideoFX (the remote video feed of the drones), node by node. The material is
// translucent (Opacity 0.99) on the drone's cockpit mesh and samples the scene colour; here it
// runs as a full-screen pass over the HDR scene before Homefront's post chain.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var scene_t: texture_2d<f32>;
@group(0) @binding(1) var scene_s: sampler;
@group(0) @binding(2) var wrap_s: sampler;
// FX_PostProcess.Textures.VideoTexturesCombined, VideoNoise, interference, ScreenScratches,
// MAT_RD.SpecialCase.UI.lineGradient03
@group(0) @binding(3) var t_combined: texture_2d<f32>;
@group(0) @binding(4) var t_noise: texture_2d<f32>;
@group(0) @binding(5) var t_interf: texture_2d<f32>;
@group(0) @binding(6) var t_scratch: texture_2d<f32>;
@group(0) @binding(7) var t_grad: texture_2d<f32>;
// v: time (s), Desaturation parameter, on, goggles (M_VideoFX_Goggles); tint: TintColor
struct Params { v: vec4<f32>, tint: vec4<f32> }
@group(0) @binding(8) var<uniform> p: Params;
// UI_PlayerHUD.Textures.HUD.ThermalGoggles_corner01 (TA_Mirror)
@group(0) @binding(9) var t_corner: texture_2d<f32>;
@group(0) @binding(10) var mirror_s: sampler;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let orig = textureSample(scene_t, scene_s, uv);
    let t = p.v.x;
    // static bursts: lineGradient03.G (TexCoord VTiling 0.0001, Panner SpeedY 0.1 * StaticTiming 1 * Time) ^ 50
    let g = textureSample(t_grad, wrap_s, vec2<f32>(uv.x, uv.y * 0.0001 + 0.1 * t)).g;
    let c = clamp(pow(g, 50.0), 0.0, 1.0);
    // distortion (bUseDistortion): ScreenPosition + (Combined.B * m34 - m34), m34 = StaticAmount 0.0075 * c
    let m34 = 0.0075 * c;
    let tb = textureSample(t_combined, wrap_s, uv * 0.15 + vec2<f32>(0.0, -1.0 * t)).b;
    let d = uv + vec2<f32>(tb * m34 - m34);
    // chromatic offsets: lerp(Croab 1, CroabStatic 7.5, c) * 0.00125 (UseExpensiveCroab: G averaged)
    let k = mix(1.0, 7.5, c);
    let r = textureSample(scene_t, scene_s, d + k * vec2<f32>(0.00125, 0.0)).r;
    let g1 = textureSample(scene_t, scene_s, d + k * vec2<f32>(0.0, 0.00125)).g;
    let g2 = textureSample(scene_t, scene_s, d + vec2<f32>(0.0, -0.00125)).g;
    let b = textureSample(scene_t, scene_s, d + k * vec2<f32>(-0.00125, 0.0)).b;
    let col = vec3<f32>(r, mix(g1, g2, 0.5), b);
    // Desaturation (UE3 luminance factors 0.3 0.59 0.11): lerp(Desaturation, DesaturationStatic 1, c)
    let lum = dot(col, vec3<f32>(0.3, 0.59, 0.11));
    // M_VideoFX_Goggles: the desaturated colour times TintColor
    let des = mix(col, vec3<f32>(lum), mix(p.v.y, 1.0, c)) * p.tint.rgb;
    // noise: VideoNoise (x2 tiling) panned (3.5, 2.25) and (-3.5, -2.25), lerp by Noise 0.0175
    let n1 = textureSample(t_noise, wrap_s, uv * 2.0 + vec2<f32>(3.5, 2.25) * t).rgb;
    let n2 = textureSample(t_noise, wrap_s, uv * 2.0 + vec2<f32>(-3.5, -2.25) * t).rgb;
    let l6 = mix(des, n1 * n2, 0.0175);
    // scan bands: interference (Panner SpeedY 0.65) * Combined.G (x1.25, SpeedY -0.9), lerp by scanBands 0.1
    let i12 = textureSample(t_interf, wrap_s, uv + vec2<f32>(0.0, 0.65 * t)).rgb;
    let s13 = textureSample(t_combined, wrap_s, uv * 1.25 + vec2<f32>(0.0, -0.9 * t)).g;
    let l2 = mix(vec3<f32>(1.0), i12 * s13, 0.1);
    // grid: Combined.R, gridBands 1
    let l4 = textureSample(t_combined, wrap_s, uv).r;
    // fx 1 / FXStatic 1: the effect replaces the scene; ScreenScratches over it (UseHeliFXs off)
    var res = l6 * l2 * l4 * textureSample(t_scratch, wrap_s, uv).rgb;
    // goggles: times clamp(corner(((0.5, -0.5) + (uv - 0.5) * 2) + 0.5))
    let cuv = vec2<f32>(0.5, -0.5) + (uv - vec2<f32>(0.5)) * 2.0 + vec2<f32>(0.5);
    let corner = clamp(textureSample(t_corner, mirror_s, cuv).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    res = mix(res, res * corner, p.v.w);
    // Opacity 0.99 (translucent)
    return vec4<f32>(mix(orig.rgb, res, 0.99 * p.v.z), orig.a);
}
