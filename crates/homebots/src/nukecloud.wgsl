// Volumetric nuclear cloud (nuke.rs / nukecloud.rs, NOT from data): one ray march through a
// box around ground zero, clipped by the scene depth (scene copy alpha, scenecopy.rs).
// Shapes in metres relative to ground zero (Bevy axes, y up):
// - fireball / cap: a torus (ring radius R, tube radius r) at height h; R = 0 is a sphere, so the
//   fireball grows into the rolling cap as R grows and h rises
// - dome over the cap, stem from the ground into the cap (flared at both ends)
// - base surge: a ground torus at the shock radius, dust skirt inside it
// - condensation shell (Wilson cloud): a thin hemisphere just outside the shock front
// Noise: a tileable 64^3 Perlin-Worley texture; the cap's noise rolls around the tube (toroidal
// coordinates with integer periods, so the texture wraps without seams).
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}

struct CloudParams {
    // ground zero (Bevy m), seconds since the detonation
    a: vec4<f32>,
    // direction to the sun, shock radius (m)
    b: vec4<f32>,
    // cap height, ring radius, tube radius, stem radius (m)
    c: vec4<f32>,
    // heat 0..1, base surge strength, condensation shell strength, overall fade
    d: vec4<f32>,
    // sun colour, ambient
    e: vec4<f32>,
    // box half size xz, box top, march steps, stem growth 0..1
    f: vec4<f32>,
}

@group(2) @binding(0) var<uniform> P: CloudParams;
@group(2) @binding(1) var scene_t: texture_2d<f32>;
@group(2) @binding(2) var noise_t: texture_3d<f32>;
@group(2) @binding(3) var noise_s: sampler;

const PI: f32 = 3.14159265;

fn nz(p: vec3<f32>) -> f32 {
    return textureSampleLevel(noise_t, noise_s, p, 0.0).r;
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

// cap (torus + dome) distance
fn d_cap(p: vec3<f32>) -> f32 {
    let h = P.c.x;
    let R = P.c.y;
    let r = P.c.z;
    let q = vec2<f32>(length(p.xz) - R, (p.y - h) * 1.12);
    let tor = length(q) - r;
    let dome = length(vec3<f32>(p.x, (p.y - (h + r * 0.3)) * 1.7, p.z)) - (R * 0.9 + r * 0.25);
    return smin(tor, dome, r * 0.45);
}

fn stem_radius(y: f32) -> f32 {
    let h = P.c.x;
    let r = P.c.z;
    let rs = P.c.w;
    return rs * (1.0 + 1.5 * exp(-max(y, 0.0) / max(rs * 1.4, 1.0))) * (1.0 + 0.9 * smoothstep(h - r * 1.8, h - r * 0.3, y));
}

fn d_stem(p: vec3<f32>) -> f32 {
    let top = P.c.x * P.f.w;
    let ds = length(p.xz) - stem_radius(p.y);
    return max(ds, max(-p.y - 5.0, p.y - top));
}

fn surge_radius() -> f32 {
    return 18.0 + 32.0 * (1.0 - exp(-P.a.w / 3.0));
}

fn d_surge(p: vec3<f32>) -> f32 {
    let s = P.b.w;
    let rr = surge_radius();
    let q = vec2<f32>(length(p.xz) - s * 0.96, (p.y - rr * 0.35) * 1.3);
    let ring = length(q) - rr;
    // thin dust skirt just inside the ring
    let skirt = max(abs(length(p.xz) - s * 0.78) - s * 0.16, max(-p.y - 3.0, p.y - rr * 0.45));
    return min(ring, skirt);
}

// coarse distance to anything (for skipping empty space)
fn d_all(p: vec3<f32>) -> f32 {
    var d = min(d_cap(p), d_stem(p));
    if (P.d.y > 0.01) {
        d = min(d, d_surge(p));
    }
    if (P.d.z > 0.01) {
        d = min(d, abs(length(p) - P.b.w * 1.03) - 10.0);
    }
    return d;
}

struct Sample {
    dens: f32,
    albedo: vec3<f32>,
    heat: f32,
}

fn sample_at(p: vec3<f32>, detail: bool) -> Sample {
    var o: Sample;
    o.dens = 0.0;
    o.albedo = vec3<f32>(0.0);
    o.heat = 0.0;
    let tb = P.a.w;
    let h = P.c.x;
    let R = P.c.y;
    let r = max(P.c.z, 1.0);
    // ---- cap: rolling toroidal noise (u around the ring, v around the tube)
    let q = vec2<f32>(length(p.xz) - R, (p.y - h) * 1.12);
    let th = atan2(p.z, p.x) / (2.0 * PI);
    let ph = atan2(q.y, q.x) / (2.0 * PI);
    let roll = tb * 0.045;
    let tc = vec3<f32>(th * 5.0, (ph - roll) * 3.0, length(q) / r * 0.45 + tb * 0.01);
    let wc = p / 140.0 + vec3<f32>(0.0, -tb * 0.05, 0.0);
    let ring_w = clamp(R / (R + r * 0.6), 0.0, 1.0);
    var n = mix(nz(wc), nz(tc), ring_w);
    if (detail) {
        n = n * 0.68 + 0.32 * mix(nz(wc * 3.1), nz(tc * vec3<f32>(3.0, 3.0, 2.9)), ring_w);
    }
    let dc = d_cap(p) + (0.55 - n) * r * 0.95;
    let cap = smoothstep(0.0, -r * 0.22, dc);
    // ---- stem: noise rising with the column
    let sn = nz(vec3<f32>(p.x / 70.0, p.y / 90.0 - tb * 0.09, p.z / 70.0));
    let ds = d_stem(p) + (0.5 - sn) * stem_radius(p.y) * 0.9;
    let stem = smoothstep(0.0, -stem_radius(p.y) * 0.35, ds) * 0.85;
    // ---- base surge and skirt
    var surge = 0.0;
    if (P.d.y > 0.01) {
        let un = nz(vec3<f32>(p.x / 55.0, p.y / 40.0 - tb * 0.03, p.z / 55.0) + vec3<f32>(0.0, 0.0, tb * 0.02));
        let du = d_surge(p) + (0.55 - un) * surge_radius() * 1.3;
        surge = smoothstep(0.0, -surge_radius() * 0.4, du) * P.d.y * 0.7;
    }
    // ---- condensation shell (upper hemisphere)
    var shell = 0.0;
    if (P.d.z > 0.01) {
        let cn = nz(p / 60.0 + vec3<f32>(tb * 0.05));
        let sd = abs(length(p) - P.b.w * 1.03) - 6.0 - 10.0 * cn;
        shell = smoothstep(0.0, -6.0, sd) * smoothstep(15.0, 80.0, p.y) * P.d.z * 0.3;
    }
    o.dens = cap + stem + surge + shell;
    if (o.dens < 1e-4) {
        return o;
    }
    // smoke darkens with soot towards the inside, dust is brown, the shell white
    let smoke = vec3<f32>(0.52, 0.47, 0.43);
    let dust = vec3<f32>(0.58, 0.5, 0.41);
    o.albedo = (smoke * (cap + stem) + dust * surge + vec3<f32>(0.95) * shell) / o.dens;
    // heat: the core of the tube and the underside of the cap stay hot longest
    let inner = smoothstep(0.15, -0.6, d_cap(p) / r);
    let under = smoothstep(h + r * 0.4, h - r * 0.8, p.y);
    o.heat = P.d.x * (cap * mix(0.6, 1.0, inner) * mix(0.75, 1.0, under) + stem * 0.3 * smoothstep(h * 0.3, h, p.y)) / o.dens;
    return o;
}

fn fire_col(t: f32) -> vec3<f32> {
    // dark red -> orange -> yellow-white, HDR
    let c1 = vec3<f32>(1.0, 0.28, 0.04) * 3.0;
    let c2 = vec3<f32>(1.0, 0.5, 0.12) * 11.0;
    let c3 = vec3<f32>(1.0, 0.82, 0.55) * 45.0;
    if (t < 0.5) {
        return c1 * smoothstep(0.0, 0.5, t);
    }
    if (t < 0.8) {
        return mix(c1, c2, (t - 0.5) / 0.3);
    }
    return mix(c2, c3, (t - 0.8) / 0.2);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // HB_CLOUDDBG: the box itself (red)
    if (P.f.z < 0.0) {
        return vec4<f32>(0.4, 0.0, 0.0, 0.4);
    }
    let gz = P.a.xyz;
    let ro = view.world_position - gz;
    let rd = normalize(in.world_position.xyz - view.world_position);
    // box
    let bmin = vec3<f32>(-P.f.x, -10.0, -P.f.x);
    let bmax = vec3<f32>(P.f.x, P.f.y, P.f.x);
    let inv = 1.0 / rd;
    let t0v = (bmin - ro) * inv;
    let t1v = (bmax - ro) * inv;
    let tn = max(max(min(t0v.x, t1v.x), min(t0v.y, t1v.y)), min(t0v.z, t1v.z));
    let tf = min(min(max(t0v.x, t1v.x), max(t0v.y, t1v.y)), max(t0v.z, t1v.z));
    var t0 = max(tn, 0.0);
    var t1 = tf;
    // scene depth (scene copy alpha = reverse-Z depth * 1000, depth = near / view distance)
    let sc = textureLoad(scene_t, vec2<i32>(in.position.xy), 0);
    if (sc.a > 1e-5) {
        let fwd = normalize(-view.world_from_view[2].xyz);
        let vd = 100.0 / sc.a;
        t1 = min(t1, vd / max(dot(rd, fwd), 0.05));
    }
    if (t1 <= t0) {
        discard;
    }
    let sun = normalize(P.b.xyz);
    let mu = dot(rd, sun);
    let g = 0.45;
    let hg = (1.0 - g * g) / pow(1.0 + g * g - 2.0 * g * mu, 1.5) / (4.0 * PI);
    let phase = mix(1.0 / (4.0 * PI), hg, 0.7) * 4.0 * PI;
    let steps = i32(P.f.z);
    let base_dt = clamp((t1 - t0) / f32(steps), 3.0, 60.0);
    var T = 1.0;
    var col = vec3<f32>(0.0);
    var t = t0 + base_dt * fract(sin(dot(in.position.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    let sigma = 0.045;
    for (var i = 0; i < 160; i++) {
        if (t >= t1 || T < 0.015) {
            break;
        }
        let p = ro + rd * t;
        let dd = d_all(p);
        let skip = dd - P.c.z * 0.9 - 30.0;
        if (skip > base_dt) {
            t += skip;
            continue;
        }
        let s = sample_at(p, true);
        if (s.dens > 1e-3) {
            // light towards the sun (3 coarse samples)
            var od = 0.0;
            for (var k = 1; k <= 3; k++) {
                od += sample_at(p + sun * (f32(k) * 22.0), false).dens * 22.0;
            }
            let tsun = exp(-sigma * od * 0.9);
            let hgt = clamp((p.y - 0.0) / max(P.c.x + P.c.z, 1.0), 0.0, 1.0);
            let amb = P.e.w * mix(vec3<f32>(0.22, 0.2, 0.18), vec3<f32>(0.42, 0.47, 0.55), 0.35 + 0.65 * hgt);
            let lit = s.albedo * (P.e.xyz * tsun * phase + amb) + fire_col(s.heat) * s.heat;
            let a = 1.0 - exp(-sigma * s.dens * base_dt);
            col += T * a * lit;
            T *= 1.0 - a;
        }
        t += base_dt;
    }
    let alpha = (1.0 - T) * P.d.w;
    return vec4<f32>(col * P.d.w, alpha);
}
