// HomeBots: rain drops on the screen ("lens") while it rains (screenrain.rs).
//
// Homefront has no rain-on-lens effect (it has no rain at all). The only water-on-lens
// effect in the data is the bullet-in-water splash (KFX_Impacts FX_Aftermath.AFM_Bullet_Water,
// emitter 0, material Shaders_Aftermath.LensFX_Waterdroplets: BLEND_Translucent, MLM_Unlit,
// Emissive = SceneTexture(ScreenPosition + (Tex.g - 0.4) * VertexColor.a), opacity 1, alpha
// over life 0.1 -> 0). FROM DATA here: the method - the drop shows the scene re-sampled at a
// shifted screen position, no tint, no added colour - and the size of the shift (the game's
// peak is 0.06 of the screen; ours stays below 0.03).
// NOT from data: the drops themselves (procedural round beads and slowly sliding drops with
// a wet trail), their sizes, counts, life times, speeds, the lens-like inverted shift, the
// slight blur and the rim darkening.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct ScreenRain {
    // x = amount 0..1 (rain strength, roof / cockpit / fade applied), y = time (s, wrapped),
    // z = strength (opacity of the drops), w = unused
    p: vec4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: ScreenRain;

fn h21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q += dot(q, q + 45.32);
    return fract(q.x * q.y);
}

fn h24(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(h21(p), h21(p + 17.13), h21(p + 39.71), h21(p + 73.37));
}

// One layer of round beads in a grid of `cell` (screen heights). At most one bead per cell,
// kept inside it, so only the pixel's own cell is checked.
// Returns (shift.xy in screen heights, mask, rim).
fn beads(q: vec2<f32>, cell: f32, t: f32, density: f32, seed: f32) -> vec4<f32> {
    let g = q / cell;
    let id = floor(g);
    let f = fract(g);
    // every cell has its own cycle: appear quickly, sit, shrink while evaporating
    let period = mix(7.0, 15.0, h21(id + seed));
    let ph = t / period + h21(id + seed + 5.7);
    let cyc = floor(ph);
    let life = fract(ph);
    let r = h24(id + vec2<f32>(cyc * 7.31, cyc * 1.97) + seed);
    // cells below the density threshold carry a bead; soft edge so beads fade, not pop,
    // when the amount changes
    let a = clamp((density - r.x) / 0.04, 0.0, 1.0);
    if a <= 0.0 {
        return vec4<f32>(0.0);
    }
    var rr = mix(0.07, 0.16, r.y * r.y);
    rr *= mix(0.55, 1.0, smoothstep(0.0, 0.03, life)) * (1.0 - 0.4 * smoothstep(0.55, 1.0, life));
    let c = vec2<f32>(0.2, 0.2) + 0.6 * r.zw;
    // slightly flatter at the top (gravity)
    var d = (f - c) / rr;
    d.y *= select(1.0, 1.15, d.y < 0.0);
    let l = length(d);
    if l >= 1.0 {
        return vec4<f32>(0.0);
    }
    let fade = 1.0 - smoothstep(0.8, 1.0, life);
    let edge = 1.0 - smoothstep(0.7, 1.0, l);
    // a drop on a lens works like a small lens: inverted view of its surroundings
    let shift = -d * rr * cell * 1.6;
    return vec4<f32>(shift, edge * a * fade, smoothstep(0.5, 0.95, l));
}

// Slowly sliding drops, one possible drop per column of width W, each with a thin wet trail
// along its wobbling path. Returns (shift.xy, mask, rim).
fn sliders(q: vec2<f32>, t: f32, density: f32) -> vec4<f32> {
    let W = 0.07;
    let col = floor(q.x / W);
    let hc = h24(vec2<f32>(col, 3.7));
    let speed = mix(0.025, 0.06, hc.x);
    let span = 1.6;
    let ph = t * speed / span + hc.y;
    let cyc = floor(ph);
    let r = h24(vec2<f32>(col * 1.31 + cyc * 9.17, cyc + 1.3));
    let a = clamp((density - r.x) / 0.04, 0.0, 1.0);
    if a <= 0.0 {
        return vec4<f32>(0.0);
    }
    // drop centre (y grows downwards); off screen when the cycle wraps
    let y0 = fract(ph) * span - 0.3;
    let base = (col + 0.5) * W;
    let k1 = r.y * 6.283;
    let k2 = r.z * 6.283;
    let xd = base + W * 0.2 * sin(y0 * 9.0 + k1) + W * 0.06 * sin(y0 * 23.0 + k2);
    let rad = vec2<f32>(0.010, 0.013) * mix(0.8, 1.15, r.w);
    var d = (q - vec2<f32>(xd, y0)) / rad;
    d.y *= select(1.0, 1.25, d.y < 0.0);
    let l = length(d);
    var out = vec4<f32>(0.0);
    if l < 1.0 {
        out = vec4<f32>(-d * rad * 1.6, (1.0 - smoothstep(0.7, 1.0, l)) * a, smoothstep(0.5, 0.95, l));
    }
    // trail above the drop: a thin wet line and a few tiny beads left behind
    let L = 0.2;
    let dy = y0 - q.y;
    if dy > 0.0 && dy < L && out.z <= 0.0 {
        let xp = base + W * 0.2 * sin(q.y * 9.0 + k1) + W * 0.06 * sin(q.y * 23.0 + k2);
        let dx = q.x - xp;
        let age = dy / L;
        let tw = 0.0022 * (1.0 - age);
        let line = (1.0 - smoothstep(tw * 0.3, tw, abs(dx))) * (1.0 - age) * 0.35;
        // tiny beads every 0.02 along the trail
        let seg = floor(q.y / 0.02);
        let hb = h21(vec2<f32>(col + seg * 0.37, cyc));
        let br = 0.0035 * (1.0 - age) * step(0.45, hb);
        let bd = vec2<f32>(dx, q.y - (seg + 0.5) * 0.02) / max(br, 1e-4);
        let bl = length(bd);
        let bead = select(0.0, (1.0 - smoothstep(0.6, 1.0, bl)) * (1.0 - age), bl < 1.0 && br > 0.0);
        if bead > line {
            out = vec4<f32>(-bd * br * 1.6, bead * a, smoothstep(0.5, 0.95, bl));
        } else if line > 0.0 {
            out = vec4<f32>(vec2<f32>(-sign(dx) * tw, 0.0), line * a, 0.0);
        }
    }
    return out;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(scene));
    let px = vec2<i32>(floor(in.position.xy));
    let base = textureLoad(scene, px, 0);
    let amount = u.p.x;
    let t = u.p.y;
    let aspect = size.x / size.y;
    let uv = in.position.xy / size;
    // screen-height units, so drops stay round on any aspect
    let q = vec2<f32>(uv.x * aspect, uv.y);

    var d = beads(q, 0.09, t, 0.16 * amount, 0.0);
    let d2 = beads(q + vec2<f32>(0.37, 0.61), 0.15, t * 0.8, 0.12 * amount, 11.0);
    if d2.z > d.z {
        d = d2;
    }
    let d3 = sliders(q, t, 0.3 * amount);
    if d3.z > d.z {
        d = d3;
    }
    let m = d.z * u.p.z;
    if m < 0.002 {
        return base;
    }
    let ruv = clamp(uv + vec2<f32>(d.x / aspect, d.y), vec2<f32>(0.0), vec2<f32>(1.0));
    // slight blur: 4 bilinear taps 1.25 px around the shifted position
    let o = 1.25 / size;
    var c = textureSampleLevel(scene, samp, ruv + vec2<f32>(o.x, o.y), 0.0).rgb;
    c += textureSampleLevel(scene, samp, ruv + vec2<f32>(-o.x, o.y), 0.0).rgb;
    c += textureSampleLevel(scene, samp, ruv + vec2<f32>(o.x, -o.y), 0.0).rgb;
    c += textureSampleLevel(scene, samp, ruv + vec2<f32>(-o.x, -o.y), 0.0).rgb;
    c *= 0.25;
    // faint darker rim, so a drop is readable on a flat sky
    c *= 1.0 - 0.12 * d.w;
    return vec4<f32>(mix(base.rgb, c, m), base.a);
}
