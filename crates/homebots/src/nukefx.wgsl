// NUKE killstreak post pass (nuke.rs, NOT from data): radial blur and chromatic aberration from
// the blast, glow of the fireball, flash, impact frame, white-out, grain, desaturation. Runs after
// the tonemapping, before Homefront's post chain.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var scene_t: texture_2d<f32>;
@group(0) @binding(1) var scene_s: sampler;
// a: flash, impact frame, white-out, fireball glow; b: time, radial blur, blast centre uv;
// c: aberration, impact polarity (1 = negative), grain, desaturation
struct Params { a: vec4<f32>, b: vec4<f32>, c: vec4<f32> }
@group(0) @binding(2) var<uniform> p: Params;

fn hash(v: vec2<f32>) -> f32 {
    return fract(sin(dot(v, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn hash1(x: f32) -> f32 {
    return fract(sin(x * 127.1) * 43758.5453);
}

// smooth 1d noise (angular rays)
fn noise1(x: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(hash1(i), hash1(i + 1.0), u);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var uv = in.uv;
    let dims = vec2<f32>(textureDimensions(scene_t));
    let aspect = dims.x / max(dims.y, 1.0);
    let imp = clamp(p.a.y, 0.0, 1.0);
    // impact frame: the camera sensor overloads, rows tear sideways
    if (imp > 0.0) {
        let row = floor(uv.y * 90.0);
        let tear = hash(vec2<f32>(row, floor(p.b.x * 30.0)));
        uv.x += select(0.0, (tear - 0.5) * 0.06, tear > 0.82) * imp;
    }
    let center = p.b.zw;
    let d = uv - center;
    let blur = p.b.y;
    let ab = p.c.x;
    var col = vec3<f32>(0.0);
    let taps = 10;
    for (var i = 0; i < taps; i++) {
        let k = f32(i) / f32(taps - 1);
        let s = 1.0 - blur * k * 0.18;
        col.r += textureSampleLevel(scene_t, scene_s, center + d * s * (1.0 + ab * 0.01), 0.0).r;
        col.g += textureSampleLevel(scene_t, scene_s, center + d * s, 0.0).g;
        col.b += textureSampleLevel(scene_t, scene_s, center + d * s * (1.0 - ab * 0.01), 0.0).b;
    }
    col /= f32(taps);
    // fireball glow: a hot bloom around the blast, light rays and a horizontal lens streak
    let dd = vec2<f32>(d.x * aspect, d.y);
    let r = length(dd);
    let ang = atan2(dd.y, dd.x);
    let rays = pow(noise1(ang * 9.0 + 3.0), 3.0) * 0.6 + pow(noise1(ang * 23.0 + 7.0), 6.0) * 0.6;
    let g = p.a.w;
    if (g > 0.0) {
        let core = exp(-r / 0.035) * 3.0 + exp(-r / 0.12) * 1.1 + exp(-r / 0.4) * 0.35;
        let streak = exp(-abs(dd.y) / 0.006) * exp(-abs(dd.x) / 0.45) * 0.9;
        let glow = (core + streak + rays * exp(-r / 0.3) * 0.8) * g;
        col += vec3<f32>(1.0, 0.86, 0.68) * glow;
    }
    let lum = dot(col, vec3<f32>(0.2126, 0.7152, 0.0722));
    col = mix(col, vec3<f32>(lum), clamp(p.c.w, 0.0, 1.0));
    // impact frame: hard black and white, the blast and its rays white, everything else black;
    // negative frames swap the two
    if (imp > 0.0) {
        let disc = smoothstep(0.09, 0.05, r);
        let beam = smoothstep(0.55, 0.85, rays) * exp(-r / 0.9);
        let lit = max(smoothstep(0.55, 0.8, lum), max(disc, beam));
        var v = select(lit, 1.0 - lit, p.c.y > 0.5);
        // sensor noise
        v = clamp(v + (hash(uv * 731.0 + vec2<f32>(p.b.x)) - 0.5) * 0.18, 0.0, 1.0);
        col = mix(col, vec3<f32>(v), imp);
    }
    // flash and white-out
    col = mix(col, vec3<f32>(14.0, 13.0, 12.0), clamp(p.a.x, 0.0, 1.0));
    col = mix(col, vec3<f32>(10.0, 10.0, 10.0), clamp(p.a.z, 0.0, 1.0));
    let gr = hash(uv * 1000.0 + vec2<f32>(p.b.x, p.b.x * 1.3)) - 0.5;
    col += vec3<f32>(gr * p.c.z * (0.1 + lum * 0.4));
    return vec4<f32>(max(col, vec3<f32>(0.0)), 1.0);
}
