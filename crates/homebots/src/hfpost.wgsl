// HomeBots: Homefront (UE3) post-process chain.
// Every pass below is a transcription of the game's own D3D9 shaders
// (GlobalShaderCache-PC-D3D-SM3.bin, confirmed by an apitrace capture of the
// running game, tutorial level, 1366x768). Pass order, buffer sizes, sample
// positions and constants are the captured ones; see hfpost.rs for the CPU side.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct HfPost {
    // HDRLumParameters: x = HDRLumLowClamp, y = HDRLumHighClamp, z = HDRLumCenter, w = HDRLumFactor
    hdr_lum: vec4<f32>,
    // x = LumAdaptTime (HDRLumTime), y = DeltaTime, z = BloomScale, w = number of blur samples
    misc: vec4<f32>,
    // blur sample offsets (bloom texels) and weights, up to 8
    blur_off: array<vec4<f32>, 2>,
    blur_w: array<vec4<f32>, 2>,
    // SceneShadowsAndDesaturation.w (= 1 - Scene_Desaturation)
    // and SceneScaledLuminanceWeights.xyz
    desat: vec4<f32>,
    // GammaColorScaleAndInverse
    gamma: vec4<f32>,
    // GammaOverlayColor
    overlay: vec4<f32>,
    // x = TintShadowCutoff, y = TintHighlightCutoff, z/w = GradientScale/GradientOffset (VignetteScaleBias)
    cut_grad: vec4<f32>,
    tint_shadow: vec4<f32>,
    tint_mid: vec4<f32>,
    tint_high: vec4<f32>,
}

@group(0) @binding(0) var t0: texture_2d<f32>;
@group(0) @binding(1) var t1: texture_2d<f32>;
@group(0) @binding(2) var t2: texture_2d<f32>;
@group(0) @binding(3) var<uniform> p: HfPost;

// HomeBots (NOT in the game): NaN / Inf texels -> 0 and at most 60000 wherever a pass reads a
// texture. One non-finite scene pixel spread through the bloom blur and the 1x1 eye adaptation
// and flashed the whole screen red / pink / green / black / white (user 2026-10-22 20:48).
// Bit test because compilers may assume finite floats and drop `x != x`.
fn fin1(x: f32) -> f32 {
    return select(x, 0.0, (bitcast<u32>(x) & 0x7f800000u) == 0x7f800000u);
}
fn fin4(c: vec4<f32>) -> vec4<f32> {
    return clamp(vec4<f32>(fin1(c.x), fin1(c.y), fin1(c.z), fin1(c.w)), vec4<f32>(0.0), vec4<f32>(60000.0));
}
fn load(t: texture_2d<f32>, p: vec2<i32>) -> vec4<f32> {
    return fin4(textureLoad(t, p, 0));
}

// D3D9 bilinear fetch with CLAMP addressing (texel centres at (i + 0.5) / size).
fn bilin(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(t));
    let c = uv * vec2<f32>(size) - 0.5;
    let fl = floor(c);
    let f = c - fl;
    let mx = size - vec2<i32>(1);
    // clamp in float first: the game feeds garbage offsets up to 4e18
    let i0 = vec2<i32>(clamp(fl, vec2<f32>(0.0), vec2<f32>(mx)));
    let i1 = vec2<i32>(clamp(fl + 1.0, vec2<f32>(0.0), vec2<f32>(mx)));
    let a = load(t, vec2<i32>(i0.x, i0.y));
    let b = load(t, vec2<i32>(i1.x, i0.y));
    let c2 = load(t, vec2<i32>(i0.x, i1.y));
    let d = load(t, vec2<i32>(i1.x, i1.y));
    let ff = select(f, vec2<f32>(0.0), abs(c) > vec2<f32>(1e7));
    return mix(mix(a, b, ff.x), mix(c2, d, ff.x), ff.y);
}

fn pix_uv(pos: vec4<f32>, t_out: vec2<f32>) -> vec2<f32> {
    return pos.xy / t_out;
}

// ---- luminance ---------------------------------------------------------------
// Sample offsets as the shader actually receives them: the CPU uploads a packed
// float2 array but the shader reads one float2 per register, so only every other
// offset is used and the tail reads past the array. Captured values; the two
// non-finite ones are +-4.4e18 (clamped to the edge) and NaN (taken as u = 0).
const BIG: f32 = 4.379188e18;
const NAN_U: f32 = -4.379188e18;

// pass 1: 16x16 from scene colour, offsets in units of 1/48 (UV)
const L1: array<vec2<f32>, 9> = array<vec2<f32>, 9>(
    vec2(-1.0, -1.0), vec2(-1.0, 1.0), vec2(0.0, 0.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
    vec2(NAN_U, 0.0), vec2(-1.0, 0.0), vec2(0.0, 0.0), vec2(0.0, 0.0));

// passes 2/3: 16 taps; regular part (scaled per pass) + garbage tail (unscaled)
const L23: array<vec2<f32>, 8> = array<vec2<f32>, 8>(
    vec2(-3.0, -3.0), vec2(1.0, -3.0), vec2(-3.0, -1.0), vec2(1.0, -1.0),
    vec2(-3.0, 1.0), vec2(1.0, 1.0), vec2(-3.0, 3.0), vec2(1.0, 3.0));
const L23_TAIL: array<vec2<f32>, 8> = array<vec2<f32>, 8>(
    vec2(3.352481e-08, 2.97076e-38), vec2(8.421247e-27, 5.605194e-45), vec2(0.0, 0.0),
    vec2(BIG, 3.352551e-08), vec2(2.437493e-30, 5.605194e-45), vec2(NAN_U, 8.5407e-39),
    vec2(6.594688e-38, 3.0), vec2(BIG, 3.022249e-37));

@fragment
fn lum1(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = pix_uv(in.position, vec2<f32>(16.0));
    var s = 0.0;
    var taps = L1;
    for (var i = 0; i < 9; i++) {
        let o = taps[i];
        var q = uv + o / 48.0;
        if (o.x == NAN_U) { q.x = NAN_U; }
        let c = bilin(t0, q).rgb;
        s += clamp(c.r + c.g + c.b + 0.005, 0.0, 1.0);
    }
    return vec4<f32>(vec3<f32>(s * 0.111111), 1.0);
}

fn lum_down(uv: vec2<f32>, scale: f32) -> f32 {
    var s = 0.0;
    var reg = L23;
    var tail = L23_TAIL;
    for (var i = 0; i < 8; i++) {
        s += bilin(t0, uv + reg[i] * scale).r;
    }
    for (var i = 0; i < 8; i++) {
        s += bilin(t0, uv + tail[i]).r;
    }
    return s * 0.0625;
}

// pass 2: 4x4 from 16x16 (regular offsets +-0.375 / +-0.125)
@fragment
fn lum2(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(vec3<f32>(lum_down(pix_uv(in.position, vec2<f32>(4.0)), 0.125)), 1.0);
}

// pass 3: 1x1 from 4x4 (regular offsets +-1.5 / +-0.5)
@fragment
fn lum3(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(vec3<f32>(lum_down(vec2<f32>(0.5), 0.5)), 1.0);
}

// eye adaptation: t0 = current 1x1 luminance, t1 = previous adapted value
@fragment
fn adapt(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let cur = load(t0, vec2<i32>(0)).r;
    let prev = load(t1, vec2<i32>(0)).r;
    let d = cur - prev;
    let r = abs(d) / p.misc.x;
    let up = min(cur, r * p.misc.y + prev);
    let down = max(cur, r * -p.misc.y + prev);
    return vec4<f32>(vec3<f32>(select(down, up, d >= 0.0)), 1.0);
}

fn exposure(l: f32) -> f32 {
    let x = p.hdr_lum.z / l;
    var lg = -19.9316;
    if (abs(x) - 1e-6 >= 0.0) { lg = log2(abs(x)); }
    return min(p.hdr_lum.y, max(exp2(lg * p.hdr_lum.w), p.hdr_lum.x));
}

// ---- bloom -------------------------------------------------------------------
// Filter buffer is (W/4 + 2) x (H/4 + 2); the quad covers texels 1..W/4, the
// border stays cleared to 0. t0 = scene colour, t1 = adapted luminance,
// t2 = any texture of the filter-buffer size (gives the size).
@fragment
fn bloom_down(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let bsize = vec2<f32>(textureDimensions(t2));
    let inner = bsize - 2.0;
    let k = floor(in.position.xy);
    if (any(k < vec2<f32>(1.0)) || any(k > inner)) { return vec4<f32>(0.0); }
    let uv = (k - 0.5) / inner;
    let ssize = vec2<f32>(textureDimensions(t0));
    let d = 1.0 / ssize;
    let e = exposure(load(t1, vec2<i32>(0)).r);
    var taps = array<vec2<f32>, 4>(vec2(-d.x, -d.y), vec2(d.x, -d.y), vec2(-d.x, d.y), vec2(d.x, d.y));
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < 4; i++) {
        let c = bilin(t0, uv + taps[i]).rgb;
        let w = clamp((dot(vec3<f32>(0.3, 0.59, 0.11), c) * e - 1.0) * 0.5, 0.0, 1.0);
        acc += c * w;
    }
    // A16B16G16R16 (UNORM) render target
    return vec4<f32>(clamp(acc * p.misc.z * 0.0625, vec3<f32>(0.0), vec3<f32>(1.0)), 0.0);
}

fn blur(pos: vec4<f32>, dir: vec2<f32>) -> vec4<f32> {
    let bsize = vec2<f32>(textureDimensions(t0));
    let k = floor(pos.xy);
    if (any(k < vec2<f32>(1.0)) || any(k > bsize - 2.0)) { return vec4<f32>(0.0); }
    var acc = vec4<f32>(0.0);
    let n = i32(p.misc.w);
    for (var i = 0; i < n; i++) {
        let o = p.blur_off[i / 4][i % 4];
        let w = p.blur_w[i / 4][i % 4];
        acc += bilin(t0, (k + 0.5 + dir * o) / bsize) * w;
    }
    return clamp(acc, vec4<f32>(0.0), vec4<f32>(1.0));
}

@fragment
fn blur_h(in: FullscreenVertexOutput) -> @location(0) vec4<f32> { return blur(in.position, vec2<f32>(1.0, 0.0)); }
@fragment
fn blur_v(in: FullscreenVertexOutput) -> @location(0) vec4<f32> { return blur(in.position, vec2<f32>(0.0, 1.0)); }

// ---- uber blend: t0 = scene, t1 = adapted luminance, t2 = blurred bloom ---------
fn lin_from_srgb(v: vec3<f32>) -> vec3<f32> {
    // inverse of the sRGB encoding Bevy applies when writing the swapchain, so the
    // displayed value equals the game's back-buffer value
    return select(pow((v + 0.055) / 1.055, vec3<f32>(2.4)), v / 12.92, v <= vec3<f32>(0.04045));
}

@fragment
fn uber(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let ssize = vec2<f32>(textureDimensions(t0));
    let bsize = vec2<f32>(textureDimensions(t2));
    let px = vec2<i32>(floor(in.position.xy));
    let scene = load(t0, px).rgb;
    let buv = in.position.xy / ssize * ((bsize - 2.0) / bsize);
    var c = bilin(t2, buv).rgb * 4.0 + scene;

    // gradient ("vignette"): clip-space y of the D3D9 half-pixel-shifted quad
    // (the interpolated position at D3D9 pixel row r is clip y = 1 - 2r/H)
    let t = (in.position.y - 0.5) / ssize.y;
    c *= clamp(t * p.cut_grad.z + p.cut_grad.w, 0.0, 1.0);

    // three-band tint
    let sc = p.cut_grad.x;
    let hc = p.cut_grad.y;
    let mid = min(vec3<f32>(hc), max(c, vec3<f32>(sc))) * p.tint_mid.rgb;
    let sh = min(vec3<f32>(sc), c) * p.tint_shadow.rgb - vec3<f32>(sc);
    let hi = max(c, vec3<f32>(hc)) - vec3<f32>(hc);
    c = hi * p.tint_high.rgb + (mid * p.tint_mid.w + sh);

    c *= exposure(load(t1, vec2<i32>(0)).r);

    let l = dot(c, p.desat.yzw);
    c = c * p.desat.x + p.overlay.rgb + vec3<f32>(l);
    c = clamp(c * p.gamma.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    c = exp2(log2(max(c, vec3<f32>(1e-6))) * p.gamma.w);
    // gamma-space output like the game's back buffer; the UI camera draws on top and
    // converts to linear afterwards (uigamma.wgsl)
    return vec4<f32>(c, 1.0);
}
