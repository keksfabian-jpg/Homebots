//! SpeedTree 5 CWind as linked into HOMEFRONT.exe (decompiled, see MODLOG "SpeedTree wind"):
//! constructor 0xf84e90, SetParams 0xf85a60, SetDirection 0xf85b90, SetStrength 0xf85a80,
//! Advance 0xf85250, gust update 0xf85d60, smoothing 0xf862f0, random 0xf861e0 (MT19937,
//! seed 0, 0xf85050). UE3 drives every USpeedTree's wind once per frame from the vertex
//! factory's SetMesh (0x40a810 -> 0xc51b50): SetDirection((1,0,0)), SetStrength(0.5),
//! Advance(true, time) with the time step clamped to < 1 s.
//!
//! Field names follow the byte offsets in the game's CWind object (p[k] = SParams + 4k).

struct Mt {
    s: [u32; 624],
    i: usize,
}

impl Mt {
    fn new(seed: u32) -> Self {
        let mut s = [0u32; 624];
        s[0] = seed;
        for i in 1..624 {
            s[i] = 1812433253u32.wrapping_mul(s[i - 1] ^ (s[i - 1] >> 30)).wrapping_add(i as u32);
        }
        Mt { s, i: 624 }
    }
    fn next(&mut self) -> u32 {
        if self.i >= 624 {
            for k in 0..624 {
                let y = (self.s[k] & 0x8000_0000) | (self.s[(k + 1) % 624] & 0x7fff_ffff);
                self.s[k] = self.s[(k + 397) % 624] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.i = 0;
        }
        let mut y = self.s[self.i];
        self.i += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }
    /// FUN_00f861e0: lo + (hi - lo) * (u + 0.5) / 2^32
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        let u = self.next() as f64;
        (hi - lo) * ((u + 0.5) * 2.328_306_436_538_696_3e-10) as f32 + lo
    }
}

/// FUN_00f862f0: blend between a logistic ease (12x - 6) and linear by `w`.
fn smooth(x: f32, w: f32) -> f32 {
    let sig = 1.0 / ((-((6.0 - -6.0) * x + -6.0) as f64).exp() as f32 + 1.0);
    (x - sig) * w + sig
}

pub struct SpeedWind {
    p: [f32; 38],
    strength: f32,       // +0x98
    dir: [f32; 3],       // +0x9c
    rng: Mt,             // +0xac
    last_time: f32,      // +0xa7c
    dt: f32,             // +0xa80
    gust: f32,           // +0xa84
    gust_target: f32,    // +0xa88
    gust_rise_end: f32,  // +0xa8c
    gust_end: f32,       // +0xa90
    gust_start: f32,     // +0xa94
    gust_from: f32,      // +0xa98
    gust_hold_end: f32,  // +0xa9c
    str_target: f32,     // +0xaa0
    str_t0: f32,         // +0xaa4
    str_t1: f32,         // +0xaa8
    str_from: f32,       // +0xaac
    dir_target: [f32; 3], // +0xab0
    dir_mid: [f32; 3],   // +0xabc
    dir_t0: f32,         // +0xac8
    dir_t1: f32,         // +0xacc
    dir_from: [f32; 3],  // +0xad0
    osc: [f32; 4],       // +0xadc
    times: [f32; 4],     // +0xaec
    eff: f32,            // +0xafc
    /// wrapper (0xc51b50): last raw time +0xe68, offset +0xe6c
    raw_last: f32,
    raw_off: f32,
}

impl SpeedWind {
    pub fn new(params: [f32; 38]) -> Self {
        SpeedWind {
            p: params,
            strength: 0.0,
            dir: [1.0, 0.0, 0.0],
            rng: Mt::new(0),
            last_time: -1.0,
            dt: 0.0,
            gust: 0.0,
            gust_target: 0.0,
            gust_rise_end: 0.0,
            gust_end: 0.0,
            gust_start: 0.0,
            gust_from: 1.0,
            gust_hold_end: 0.0,
            str_target: 0.0,
            str_t0: 0.0,
            str_t1: 0.0,
            str_from: 0.0,
            dir_target: [1.0, 0.0, 0.0],
            dir_mid: [1.0, 0.0, 0.0],
            dir_t0: 0.0,
            dir_t1: 0.0,
            dir_from: [1.0, 0.0, 0.0],
            osc: [0.0; 4],
            times: [0.0; 4],
            eff: 0.0,
            raw_last: 0.0,
            raw_off: 0.0,
        }
    }

    fn set_direction(&mut self, d: [f32; 3]) {
        if self.dir == d {
            return;
        }
        self.dir_target = d;
        self.dir_t0 = self.last_time;
        let half = self.p[1] * 0.5;
        let dot = self.dir[0] * d[0] + self.dir[1] * d[1] + self.dir[2] * d[2];
        self.dir_t1 = self.dir_t0 + (self.p[1] - half) * (1.0 - (dot + 1.0) * 0.5) + half;
        self.dir_from = self.dir;
        let mut m = [0.0; 3];
        for k in 0..3 {
            m[k] = (self.dir_from[k] + self.dir_target[k]) * 0.5;
        }
        self.dir_mid = normalize(m);
    }

    fn set_strength(&mut self, s: f32) {
        let s = s.min(1.0).max(0.0);
        if self.strength != s {
            self.str_t0 = self.last_time;
            let half = self.p[0] * 0.5;
            self.str_t1 = self.str_t0 + (self.p[0] - half) * (s - self.strength).abs() + half;
            self.str_from = self.strength;
            self.str_target = s;
        }
    }

    fn update_gust(&mut self, t: f32) {
        let mut trigger = false;
        if (self.gust_end < t || (t < self.gust_hold_end && self.gust_rise_end < t)) && self.rng.range(0.0, self.dt) < self.dt * (self.p[0x54 / 4] * 0.01) {
            self.gust_target = self.rng.range(self.p[0x60 / 4], self.p[0x64 / 4]);
            trigger = true;
        }
        if trigger {
            self.gust_start = t;
            self.gust_from = self.gust;
            if 1.0 - self.strength < self.gust_target {
                self.gust_target = 1.0 - self.strength;
            }
            let r = self.p[0];
            let f = (r - r * 0.5) * (self.gust_target - self.strength).abs() + r * 0.5;
            self.gust_rise_end = if self.gust_target <= self.gust { self.rng.range(f, f + f) } else { self.rng.range(f * 0.5, f + f) } + t;
            self.gust_hold_end = self.rng.range(self.p[0x68 / 4], self.p[0x6c / 4]) + self.gust_rise_end;
            self.gust_end = self.rng.range(f + f, f * 3.0) + self.gust_hold_end;
        }
        if self.gust_rise_end <= t {
            if self.gust_hold_end < t && 0.0 < self.gust_end && self.gust_hold_end < self.gust_end {
                let s = smooth((t - self.gust_hold_end) / (self.gust_end - self.gust_hold_end), 0.5);
                self.gust = (0.0 - self.gust_target) * s + self.gust_target;
            }
        } else {
            let s = smooth((t - self.gust_start) / (self.gust_rise_end - self.gust_start), 0.0);
            self.gust = (self.gust_target - self.gust_from) * s + self.gust_from;
        }
        self.gust = self.gust.min(1.0).max(0.0);
    }

    fn advance(&mut self, t: f32) {
        self.dt = if self.last_time == -1.0 { 0.0 } else { t - self.last_time };
        self.last_time = t;
        self.update_gust(t);
        let mut x = 0.0;
        if self.dir_t0 != self.dir_t1 {
            x = ((t - self.dir_t0) / (self.dir_t1 - self.dir_t0)).max(0.0).min(1.0);
        }
        let s = smooth(x, 0.5);
        for k in 0..3 {
            self.dir[k] = if 0.5 <= s {
                let s2 = (s - 0.5) + (s - 0.5);
                (self.dir_target[k] - self.dir_mid[k]) * s2 + self.dir_mid[k]
            } else {
                let s2 = s + s;
                (self.dir_mid[k] - self.dir_from[k]) * s2 + self.dir_from[k]
            };
        }
        self.dir = normalize(self.dir);
        let mut x = 0.0;
        if self.str_t0 != self.str_t1 {
            x = ((t - self.str_t0) / (self.str_t1 - self.str_t0)).max(0.0).min(1.0);
        }
        let s = smooth(x, 0.0);
        self.strength = (self.str_target - self.str_from) * s + self.str_from;
        self.eff = (self.strength + self.gust).min(1.0);
        for i in 0..4 {
            self.osc[i] = (self.eff as f64).powf(self.p[(0x80 + 4 * i) / 4] as f64) as f32;
            let lo = self.p[(0x1c + 16 * i) / 4];
            let hi = self.p[(0x20 + 16 * i) / 4];
            self.times[i] += self.dt * ((hi - lo) * self.osc[i] + lo);
        }
    }

    /// One frame as UE3's FSpeedTreeVertexFactory SetMesh does it (0xc51b50).
    pub fn frame(&mut self, time: f32) {
        if self.raw_last == time {
            return;
        }
        let prev = self.raw_off + self.raw_last;
        let next = self.raw_off + time;
        let adj = if prev <= next && next < prev + 1.0 { next } else if prev <= next { prev + 1.0 } else { prev };
        self.raw_off = adj - time;
        self.raw_last = time;
        self.set_direction([1.0, 0.0, 0.0]);
        self.set_strength(0.5);
        self.advance(time + self.raw_off);
    }

    /// Shader table (+0xb00..): WindDir, WindTimes, WindDistances, WindLeaves, WindFrondRipple,
    /// WindGust, WindGustHints.
    pub fn constants(&self) -> [[f32; 4]; 7] {
        let p = &self.p;
        let lerp = |a: usize, b: usize, s: f32| (p[b / 4] - p[a / 4]) * s + p[a / 4];
        [
            [self.dir[0], self.dir[1], self.dir[2], 0.0],
            [self.times[0], self.times[1], self.times[2], self.times[3]],
            [lerp(0x14, 0x18, self.osc[0]), lerp(0x24, 0x28, self.osc[1]), if p[2] == 0.0 { 0.0 } else { 1.0 / p[2] }, p[3]],
            [lerp(0x44, 0x48, self.osc[3]), p[0x90 / 4], p[0x94 / 4], 0.0],
            [lerp(0x34, 0x38, self.osc[2]), p[0x78 / 4], p[0x7c / 4], 0.0],
            [self.eff, self.osc[0] * p[0x58 / 4], p[0x5c / 4], 0.0],
            [p[4], p[0x70 / 4] * self.eff, p[0x74 / 4] * self.eff, 0.0],
        ]
    }
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f64).sqrt() as f32;
    if l == 0.0 { [0.0; 3] } else { [v[0] / l, v[1] / l, v[2] / l] }
}
