//! Navigation for the bots. The MP maps carry no path network (Crossroads: 16 ReachSpecs, no
//! PathNodes; Homefront's MP bots were a debug feature), so a walk grid is built from the
//! collision triangles after loading: 1 m columns over the area of the PlayerStarts, every
//! walkable surface (normal.y >= WalkableFloorZ 0.7) with standing room above (cylinder height
//! 83 uu) is a node, neighbours connect when the height step is climbable and nothing blocks
//! a knee- and chest-high line between them. Paths: A* plus line-of-sight smoothing.
use crate::player::CollisionWorld;
use bevy::prelude::*;
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex};

const CELL: f32 = 1.0;
const HEADROOM: f32 = 0.85;
const MAX_STEP: f32 = 0.55;
const WALKABLE: f32 = 0.7;

#[derive(Default)]
pub struct NavGrid {
    pub origin: Vec2,
    pub w: i32,
    pub h: i32,
    /// node positions (Bevy metres, on the floor)
    pub nodes: Vec<Vec3>,
    /// first node index per column (w*h+1 entries)
    col_start: Vec<u32>,
    pub edges: Vec<Vec<u32>>,
    /// connected component per node
    pub comp: Vec<u32>,
    /// nodes per component
    pub comp_size: Vec<u32>,
    /// extra path cost of a node next to a wall or an edge: 0.4 m per missing neighbour of the
    /// 8 (keeps paths off walls, corners and ledges the walker catches on; NOT from data)
    pub wall: Vec<f32>,
}

#[derive(Resource, Default)]
pub struct Nav {
    pub grid: Option<Arc<NavGrid>>,
    /// learned extra cost per node: where bots got stuck (bots.rs), reset with the grid
    pub penalty: Vec<f32>,
    pending: Option<Arc<Mutex<Option<NavGrid>>>>,
    pub built_for: u32,
}

impl NavGrid {
    pub fn build(col: &CollisionWorld, lo: Vec2, hi: Vec2) -> NavGrid {
        let t0 = std::time::Instant::now();
        let w = ((hi.x - lo.x) / CELL).ceil() as i32 + 1;
        let h = ((hi.y - lo.y) / CELL).ceil() as i32 + 1;
        let mut nodes = Vec::new();
        let mut col_start = Vec::with_capacity((w * h + 1) as usize);
        let mut hits = Vec::new();
        for j in 0..h {
            for i in 0..w {
                col_start.push(nodes.len() as u32);
                let x = lo.x + i as f32 * CELL + CELL * 0.5;
                let z = lo.y + j as f32 * CELL + CELL * 0.5;
                col.column_hits(x, z, &mut hits);
                hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                for (k, &(y, ny)) in hits.iter().enumerate() {
                    if ny < WALKABLE {
                        continue;
                    }
                    // standing room: nothing above within the cylinder height
                    let above = hits[k + 1..].iter().find(|h| h.0 > y + 0.05).map(|h| h.0);
                    if above.is_some_and(|a| a < y + HEADROOM) {
                        continue;
                    }
                    // collapse near-duplicate surfaces
                    if hits[k + 1..].iter().any(|h| h.1 >= WALKABLE && h.0 - y < 0.3 && h.0 > y) {
                        continue;
                    }
                    nodes.push(Vec3::new(x, y, z));
                }
            }
        }
        col_start.push(nodes.len() as u32);
        let mut g = NavGrid { origin: lo, w, h, nodes, col_start, edges: Vec::new(), comp: Vec::new(), comp_size: Vec::new(), wall: Vec::new() };
        let mut buf = Vec::new();
        let n = g.nodes.len();
        let mut edges: Vec<Vec<u32>> = vec![Vec::new(); n];
        let dirs = [(1, 0), (0, 1), (1, 1), (1, -1)];
        for j in 0..h {
            for i in 0..w {
                for a in g.column(i, j) {
                    let pa = g.nodes[a as usize];
                    for &(di, dj) in &dirs {
                        let (ni, nj) = (i + di, j + dj);
                        if ni < 0 || nj < 0 || ni >= w || nj >= h {
                            continue;
                        }
                        // best matching layer in the neighbour column
                        let Some(b) = g.column(ni, nj).min_by(|&x, &y| (g.nodes[x as usize].y - pa.y).abs().partial_cmp(&(g.nodes[y as usize].y - pa.y).abs()).unwrap()) else { continue };
                        let pb = g.nodes[b as usize];
                        if (pb.y - pa.y).abs() > MAX_STEP * if di != 0 && dj != 0 { 1.41 } else { 1.0 } {
                            continue;
                        }
                        if !clear(col, pa, pb, &mut buf) || !steps_ok(col, pa, pb, &mut buf) {
                            continue;
                        }
                        edges[a as usize].push(b);
                        edges[b as usize].push(a);
                    }
                }
            }
        }
        g.wall = edges.iter().map(|e| (8usize.saturating_sub(e.len())) as f32 * 0.4).collect();
        g.edges = edges;
        // components
        let mut comp = vec![u32::MAX; n];
        let mut c = 0;
        for s in 0..n {
            if comp[s] != u32::MAX {
                continue;
            }
            let mut stack = vec![s as u32];
            comp[s] = c;
            while let Some(v) = stack.pop() {
                for &u in &g.edges[v as usize] {
                    if comp[u as usize] == u32::MAX {
                        comp[u as usize] = c;
                        stack.push(u);
                    }
                }
            }
            c += 1;
        }
        let mut size = vec![0u32; c as usize];
        for &k in &comp {
            size[k as usize] += 1;
        }
        let mut top: Vec<u32> = size.clone();
        top.sort_unstable_by(|a, b| b.cmp(a));
        info!("nav: largest components {:?}", &top[..top.len().min(6)]);
        g.comp = comp;
        g.comp_size = size;
        info!("nav: {} x {} columns, {} nodes, {} components, {:.1} s", w, h, n, c, t0.elapsed().as_secs_f32());
        g
    }

    fn column(&self, i: i32, j: i32) -> std::ops::Range<u32> {
        let k = (j * self.w + i) as usize;
        self.col_start[k]..self.col_start[k + 1]
    }

    /// Nodes in the columns within `r` (horizontal square) of `p`.
    pub fn nodes_within(&self, p: Vec3, r: f32) -> impl Iterator<Item = u32> + '_ {
        let (i0, i1) = (((p.x - r - self.origin.x) / CELL).floor() as i32, ((p.x + r - self.origin.x) / CELL).floor() as i32);
        let (j0, j1) = (((p.z - r - self.origin.y) / CELL).floor() as i32, ((p.z + r - self.origin.y) / CELL).floor() as i32);
        let (i0, i1, j0, j1) = (i0.max(0), i1.min(self.w - 1), j0.max(0), j1.min(self.h - 1));
        (j0..=j1).flat_map(move |j| (i0..=i1).flat_map(move |i| self.column(i, j)))
    }

    /// Node nearest to a floor position (same column first, then the 3x3 neighbourhood).
    pub fn nearest(&self, p: Vec3) -> Option<u32> {
        let i = ((p.x - self.origin.x) / CELL).floor() as i32;
        let j = ((p.z - self.origin.y) / CELL).floor() as i32;
        let mut best: Option<(f32, u32)> = None;
        for r in 0..4i32 {
            for dj in -r..=r {
                for di in -r..=r {
                    if di.abs() != r && dj.abs() != r {
                        continue;
                    }
                    let (ni, nj) = (i + di, j + dj);
                    if ni < 0 || nj < 0 || ni >= self.w || nj >= self.h {
                        continue;
                    }
                    for k in self.column(ni, nj) {
                        let q = self.nodes[k as usize];
                        let d = (q - p).length() + (q.y - p.y).abs() * 2.0;
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, k));
                        }
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        best.map(|b| b.1)
    }

    /// A* between two nodes (max `limit` expansions). Scratch arrays are reused per thread and
    /// stamped per search (the HashMap version cost tens of ms on long paths).
    pub fn path(&self, a: u32, b: u32, limit: usize, pen: &[f32]) -> Option<Vec<u32>> {
        if self.comp[a as usize] != self.comp[b as usize] {
            return None;
        }
        #[derive(PartialEq)]
        struct S(f32, u32);
        impl Eq for S {}
        impl PartialOrd for S {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for S {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.partial_cmp(&self.0).unwrap_or(std::cmp::Ordering::Equal)
            }
        }
        struct Scratch {
            g: Vec<f32>,
            from: Vec<u32>,
            stamp: Vec<u32>,
            cur: u32,
            open: BinaryHeap<S>,
        }
        thread_local! {
            static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::new(Scratch { g: Vec::new(), from: Vec::new(), stamp: Vec::new(), cur: 0, open: BinaryHeap::new() });
        }
        let n = self.nodes.len();
        SCRATCH.with(|sc| {
            let mut sc = sc.borrow_mut();
            let sc = &mut *sc;
            if sc.g.len() != n {
                sc.g = vec![0.0; n];
                sc.from = vec![u32::MAX; n];
                sc.stamp = vec![0; n];
                sc.cur = 0;
            }
            sc.cur = sc.cur.wrapping_add(1);
            if sc.cur == 0 {
                sc.stamp.iter_mut().for_each(|x| *x = 0);
                sc.cur = 1;
            }
            let cur = sc.cur;
            sc.open.clear();
            let goal = self.nodes[b as usize];
            sc.g[a as usize] = 0.0;
            sc.from[a as usize] = u32::MAX;
            sc.stamp[a as usize] = cur;
            sc.open.push(S(self.nodes[a as usize].distance(goal), a));
            let mut k = 0;
            while let Some(S(_, v)) = sc.open.pop() {
                if v == b {
                    let mut out = vec![b];
                    let mut c = b;
                    while sc.from[c as usize] != u32::MAX && out.len() < n {
                        c = sc.from[c as usize];
                        out.push(c);
                    }
                    out.reverse();
                    return Some(out);
                }
                k += 1;
                if k > limit {
                    return None;
                }
                let gv = sc.g[v as usize];
                let pv = self.nodes[v as usize];
                for &u in &self.edges[v as usize] {
                    let pu = self.nodes[u as usize];
                    let ng = gv + pv.distance(pu) + self.wall.get(u as usize).copied().unwrap_or(0.0) + pen.get(u as usize).copied().unwrap_or(0.0);
                    let ui = u as usize;
                    if sc.stamp[ui] != cur || ng < sc.g[ui] {
                        sc.stamp[ui] = cur;
                        sc.g[ui] = ng;
                        sc.from[ui] = v;
                        // weighted A* (heuristic x1.3): far fewer expansions on long paths,
                        // paths at most 30 % longer (bot_ai spikes up to 54 ms in the user's
                        // profile 2026-10-19; NOT from data)
                        sc.open.push(S(ng + pu.distance(goal) * 1.3, u));
                    }
                }
            }
            None
        })
    }

    /// Node of component `c` nearest to `p` within `r` metres (column rings outwards).
    pub fn nearest_in_comp(&self, p: Vec3, c: u32, r: f32) -> Option<u32> {
        let i = ((p.x - self.origin.x) / CELL).floor() as i32;
        let j = ((p.z - self.origin.y) / CELL).floor() as i32;
        let mut best: Option<(f32, u32)> = None;
        for ring in 0..=(r / CELL).ceil() as i32 {
            for dj in -ring..=ring {
                for di in -ring..=ring {
                    if di.abs() != ring && dj.abs() != ring {
                        continue;
                    }
                    let (ni, nj) = (i + di, j + dj);
                    if ni < 0 || nj < 0 || ni >= self.w || nj >= self.h {
                        continue;
                    }
                    for k in self.column(ni, nj) {
                        if self.comp[k as usize] != c {
                            continue;
                        }
                        let q = self.nodes[k as usize];
                        let d = (q - p).length() + (q.y - p.y).abs() * 2.0;
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, k));
                        }
                    }
                }
            }
            // a ring further out cannot be closer than one cell less than its radius
            if best.is_some_and(|b| b.0 < (ring as f32) * CELL) {
                break;
            }
        }
        best.map(|b| b.1)
    }

    /// Node of component `c` closest to `p` anywhere on the map (full scan: for goals the
    /// component does not reach within the ring search).
    pub fn closest_in_comp(&self, p: Vec3, c: u32) -> Option<u32> {
        let mut best: Option<(f32, u32)> = None;
        for (k, q) in self.nodes.iter().enumerate() {
            if self.comp[k] != c {
                continue;
            }
            let d = q.distance_squared(p);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, k as u32));
            }
        }
        best.map(|b| b.1)
    }

    /// Size of the component a node belongs to.
    pub fn comp_len(&self, k: u32) -> u32 {
        self.comp_size.get(self.comp[k as usize] as usize).copied().unwrap_or(0)
    }

    /// The largest component (the walkable map; small ones are roofs, car tops, ledges).
    pub fn main_comp(&self) -> Option<u32> {
        self.comp_size.iter().enumerate().max_by_key(|x| x.1).map(|x| x.0 as u32)
    }

    /// Random node of the component, near a point (within `r` metres).
    pub fn node_near(&self, p: Vec3, r: f32, seed: u32) -> Option<u32> {
        let base = self.nearest(p)?;
        let c = self.comp[base as usize];
        let mut s = seed | 1;
        for _ in 0..40 {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let a = (s % 6283) as f32 / 1000.0;
            let d = (s / 6283 % 1000) as f32 / 1000.0 * r;
            if let Some(k) = self.nearest(p + Vec3::new(a.cos() * d, 0.0, a.sin() * d)) {
                if self.comp[k as usize] == c {
                    return Some(k);
                }
            }
        }
        Some(base)
    }
}

/// The floor between two nodes rises or falls by at most the pawn's step height (MaxStepHeight
/// 26 uu) per quarter metre: a slope is walkable, a ledge between the columns is not (the
/// column heights alone let 0.55 m kerbs through, where the bots then stood stuck).
pub fn steps_ok(col: &CollisionWorld, a: Vec3, b: Vec3, buf: &mut Vec<u32>) -> bool {
    let n = (Vec2::new(b.x - a.x, b.z - a.z).length() / 0.25).ceil().max(1.0) as usize;
    let mut prev = a.y;
    for k in 1..=n {
        let t = k as f32 / n as f32;
        let p = a.lerp(b, t);
        let top = prev.max(p.y) + 0.3;
        let Some((y, n)) = col.floor(Vec3::new(p.x, top, p.z), 1.2, buf) else { return false };
        if (y - prev).abs() > 0.27 {
            return false;
        }
        // a slope steeper than WalkableFloorZ the walker slides off (the height steps alone let
        // 47 deg through: the remaining stuck walks of HB_NAVTEST climbed such slopes)
        if n.y < WALKABLE && (y - prev).abs() > 0.05 {
            return false;
        }
        prev = y;
    }
    (prev - b.y).abs() < 0.3
}

/// Shin-, knee- and chest-high lines between two floor points are free.
pub fn clear(col: &CollisionWorld, a: Vec3, b: Vec3, buf: &mut Vec<u32>) -> bool {
    for hgt in [0.32, 0.6, 1.2] {
        let o = a + Vec3::Y * hgt;
        let e = b + Vec3::Y * hgt;
        let d = e - o;
        let len = d.length();
        if len < 1e-4 {
            continue;
        }
        if col.raycast_mask(o, d / len, len, buf, crate::player::COL_PLAYER).is_some() {
            return false;
        }
    }
    true
}

/// A straight walk from `a` to `b` the walker can follow: the shin / knee / chest lines are free
/// at the centre and at the walker's sides (+-0.35 m: best of 0.25 / 0.35 / 0.45 in HB_NAVTEST), and the
/// floor along it has no step above MaxStepHeight (path smoothing; the centre line alone cut
/// corners and crossed kerbs the bots then stuck on: user's bot log 2026-10-18).
pub fn walkable_line(col: &CollisionWorld, a: Vec3, b: Vec3, buf: &mut Vec<u32>) -> bool {
    let d = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
    if d.length() < 1e-3 {
        return true;
    }
    let side = Vec3::new(-d.z, 0.0, d.x).normalize() * 0.35;
    clear(col, a, b, buf) && clear(col, a + side, b + side, buf) && clear(col, a - side, b - side, buf) && steps_ok(col, a, b, buf)
}

impl Nav {
    /// Raise the path cost around a spot where a bot got stuck (heading `dir`): the node there
    /// and the next one ahead, +15 m each, capped at 300 m (NOT from data).
    pub fn penalize(&mut self, at: Vec3, dir: Vec3) -> Option<u32> {
        let g = self.grid.clone()?;
        let mut hit = None;
        for p in [at, at + dir.normalize_or_zero() * 0.9] {
            if let Some(k) = g.nearest(p) {
                if let Some(x) = self.penalty.get_mut(k as usize) {
                    *x = (*x + 15.0).min(300.0);
                    hit = Some(k);
                }
            }
        }
        hit
    }

    /// Start building in the background (once per level load).
    pub fn start(&mut self, col: &CollisionWorld, loads: u32) {
        if self.built_for == loads {
            return;
        }
        self.built_for = loads;
        self.grid = None;
        if col.starts.is_empty() || col.tris.is_empty() {
            return;
        }
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for s in &col.starts {
            lo = lo.min(Vec2::new(s.1.x, s.1.z));
            hi = hi.max(Vec2::new(s.1.x, s.1.z));
        }
        lo -= Vec2::splat(30.0);
        hi += Vec2::splat(30.0);
        let slot = Arc::new(Mutex::new(None));
        self.pending = Some(slot.clone());
        let col = col.clone();
        std::thread::spawn(move || {
            let g = NavGrid::build(&col, lo, hi);
            if let Ok(mut s) = slot.lock() {
                *s = Some(g);
            }
        });
    }

    pub fn poll(&mut self) {
        let done = self.pending.as_ref().and_then(|p| p.lock().ok().and_then(|mut s| s.take()));
        if let Some(g) = done {
            self.penalty = vec![0.0; g.nodes.len()];
            self.grid = Some(Arc::new(g));
            self.pending = None;
        }
    }
}
