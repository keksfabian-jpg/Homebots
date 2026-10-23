//! Homefront's cooked USpeedTree data and SpeedTreeActor placements.
//!
//! Native data after the properties (verified on PineTree / Tree_deciduous_lrg_2 / GrassMix01
//! against the Overpass trace 2026-10-15, draw ranges identical):
//! * a parameter block of varying length ending with FBoxSphereBounds (7 floats)
//! * 5 position streams (bulk TArray: elem size 12, count) and 5 data streams (64/72/76/64/68)
//!   in the order branch, frond, leaf card, leaf mesh, billboard
//! * the index buffer (bulk TArray of u16)
//! * per element group a TArray of raw 216-byte FMeshElement structs (FirstIndex at +152,
//!   NumPrimitives +156, MinVertexIndex +160, MaxVertexIndex +164), groups: branch 1,
//!   branch 2, frond, leaf card, leaf mesh; then one billboard element.
//!
//! Vertex declarations (trace): stream 0 = FLOAT3 position, stream 1 = the data stream.

use crate::props::Value;
use crate::Package;

pub const GROUPS: usize = 6;
/// Stream index used by each element group.
pub const GROUP_STREAM: [usize; GROUPS] = [0, 0, 1, 2, 3, 4];
/// Vertex factory name of each element group.
pub const GROUP_VF: [&str; GROUPS] = [
    "FSpeedTreeBranchVertexFactory",
    "FSpeedTreeBranchVertexFactory",
    "FSpeedTreeFrondVertexFactory",
    "FSpeedTreeLeafCardVertexFactory",
    "FSpeedTreeLeafMeshVertexFactory",
    "FSpeedTreeBillboardVertexFactory",
];
/// USpeedTree material property of each element group.
pub const GROUP_MATERIAL: [&str; GROUPS] = ["Branch1Material", "Branch2Material", "FrondMaterial", "LeafCardMaterial", "LeafMeshMaterial", "BillboardMaterial"];
const DATA_STRIDE: [usize; 5] = [64, 72, 76, 64, 68];

#[derive(Debug, Clone, Copy)]
pub struct Element {
    pub first_index: u32,
    pub num_primitives: u32,
    pub min_vertex: u32,
    pub max_vertex: u32,
}

#[derive(Debug, Clone)]
pub struct SpeedTreeMesh {
    pub positions: Vec<Vec<[f32; 3]>>,
    pub data: Vec<Vec<u8>>,
    pub strides: [usize; 5],
    pub indices: Vec<u16>,
    /// Per group, one element per LOD (LOD 0 first).
    pub elements: Vec<Vec<Element>>,
    /// Per group, the material object reference (0 = none).
    pub materials: [i32; GROUPS],
    /// SpeedTree CWind::SParams in memory order (index = byte offset / 4), see `wind_params`.
    pub wind: Option<[f32; 38]>,
}

#[derive(Debug, Clone)]
pub struct SpeedTreePlacement {
    pub tree: i32,
    pub location: [f32; 3],
    pub rotation: [i32; 3],
    pub scale: [f32; 3],
    pub max_draw_distance: f32,
    /// Per group material overrides from the component (0 = the tree's own).
    pub materials: [i32; GROUPS],
}

impl Package {
    pub fn read_speedtree(&self, idx: usize) -> Option<SpeedTreeMesh> {
        let mut r = self.export_reader(idx);
        let props = self.read_props(&mut r)?;
        let b = r.b;
        let u = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
        // locate the stream arrays: element sizes 12 x5, then the data strides, then 2
        let expect: [u32; 11] = [12, 12, 12, 12, 12, 64, 72, 76, 64, 68, 2];
        let walk = |mut o: usize| -> Option<(Vec<(usize, usize)>, usize)> {
            let mut v = Vec::new();
            for &es in &expect {
                if u(o)? != es {
                    return None;
                }
                let n = u(o + 4)? as usize;
                let start = o + 8;
                let end = start.checked_add(n.checked_mul(es as usize)?)?;
                if end > b.len() {
                    return None;
                }
                v.push((start, n));
                o = end;
            }
            Some((v, o))
        };
        let mut found = None;
        let mut o = r.pos;
        while o + 8 <= b.len() && o < r.pos + 4096 {
            if let Some(x) = walk(o) {
                found = Some(x);
                break;
            }
            o += 4;
        }
        let (arrs, mut o) = found?;
        let wind = wind_params(b, arrs[0].0 - 8);
        let f = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let positions = (0..5).map(|k| (0..arrs[k].1).map(|i| [0, 1, 2].map(|c| f(arrs[k].0 + i * 12 + c * 4))).collect()).collect();
        let data = (0..5).map(|k| b[arrs[5 + k].0..arrs[5 + k].0 + arrs[5 + k].1 * DATA_STRIDE[k]].to_vec()).collect();
        let (is, ni) = arrs[10];
        let indices = (0..ni).map(|i| u16::from_le_bytes(b[is + 2 * i..is + 2 * i + 2].try_into().unwrap())).collect();
        let elem = |p: usize| -> Option<Element> {
            Some(Element { first_index: u(p + 152)?, num_primitives: u(p + 156)?, min_vertex: u(p + 160)?, max_vertex: u(p + 164)? })
        };
        let mut elements = Vec::new();
        for _ in 0..5 {
            let n = u(o)? as usize;
            if n > 16 {
                return None;
            }
            o += 4;
            let mut g = Vec::new();
            for _ in 0..n {
                g.push(elem(o)?);
                o += 216;
            }
            elements.push(g);
        }
        // single billboard element
        let bb = elem(o)?;
        elements.push(if bb.num_primitives > 0 { vec![bb] } else { Vec::new() });
        let materials = GROUP_MATERIAL.map(|n| props.object(n).unwrap_or(0));
        Some(SpeedTreeMesh { positions, data, strides: DATA_STRIDE, indices, elements, materials, wind })
    }

    /// Placed SpeedTreeActors (archetypes excluded: they live outside TheWorld).
    pub fn speedtree_placements(&self) -> Vec<SpeedTreePlacement> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != "SpeedTreeActor" || !self.object_path(i as i32 + 1).starts_with("TheWorld.") {
                continue;
            }
            let Some(ap) = self.export_props(i) else { continue };
            if matches!(ap.get("bHidden"), Some(Value::Bool(true))) {
                continue;
            }
            let Some(c) = ap.object("SpeedTreeComponent").filter(|&c| c > 0) else { continue };
            let Some(cp) = self.export_props(c as usize - 1) else { continue };
            if matches!(cp.get("HiddenGame"), Some(Value::Bool(true))) {
                continue;
            }
            let Some(tree) = cp.object("SpeedTree").filter(|&t| t != 0) else { continue };
            let ds = ap.float("DrawScale").unwrap_or(1.0);
            let s3 = ap.vector("DrawScale3D").unwrap_or([1.0; 3]);
            out.push(SpeedTreePlacement {
                tree,
                location: ap.vector("Location").unwrap_or([0.0; 3]),
                rotation: ap.rotator("Rotation").unwrap_or([0; 3]),
                scale: [s3[0] * ds, s3[1] * ds, s3[2] * ds],
                max_draw_distance: cp.float("MaxDrawDistance").unwrap_or(0.0),
                materials: GROUP_MATERIAL.map(|n| cp.object(n).unwrap_or(0)),
            });
        }
        out
    }
}

impl Package {
    /// A Color property of the level's WorldInfo (e.g. CheapAmbientColor), RGBA bytes.
    pub fn world_color(&self, name: &str) -> Option<[u8; 4]> {
        let i = self.exports.iter().position(|e| self.export_class(e) == "WorldInfo")?;
        self.export_own_props(i)?.color(name)
    }
}

/// CWind::SParams of a tree. Native layout before the stream arrays: ... 38 wind floats,
/// 7 dwords, FBoxSphereBounds (7 floats). The file order differs from the struct order;
/// mapping solved against the Overpass trace (PineTree, Tree_deciduous_lrg_2, grasses: all
/// state-independent shader constants and the lerps with pow(strength, exponent) match):
/// f0..f4 = +0x00 +0x04 +0x08 +0x0c +0x10, f5..f17 = +0x54 +0x58 +0x5c +0x60 +0x64 +0x68 +0x6c
/// +0x70 +0x74 +0x78 +0x7c +0x90 +0x94, then per oscillation component i (0..4):
/// exponent (+0x80+4i), distance lo/hi (+0x14+16i, +0x18+16i), time rate lo/hi (+0x1c+16i, +0x20+16i).
pub fn wind_params(b: &[u8], streams_at: usize) -> Option<[f32; 38]> {
    let start = streams_at.checked_sub((14 + 38) * 4)?;
    let f = |k: usize| f32::from_le_bytes(b[start + 4 * k..start + 4 * k + 4].try_into().unwrap());
    let mut p = [0.0f32; 38];
    for (k, off) in [0x00, 0x04, 0x08, 0x0c, 0x10, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x78, 0x7c, 0x90, 0x94].iter().enumerate() {
        p[off / 4] = f(k);
    }
    for i in 0..4 {
        let k = 18 + 5 * i;
        p[(0x80 + 4 * i) / 4] = f(k);
        p[(0x14 + 16 * i) / 4] = f(k + 1);
        p[(0x18 + 16 * i) / 4] = f(k + 2);
        p[(0x1c + 16 * i) / 4] = f(k + 3);
        p[(0x20 + 16 * i) / 4] = f(k + 4);
    }
    p.iter().all(|x| x.is_finite()).then_some(p)
}
