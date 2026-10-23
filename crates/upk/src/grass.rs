//! Homefront dynamic terrain grass (Terrain.TerrainGrassLayers + cooked per-component instances).
//!
//! Verified against the Overpass D3D9 trace (2026-10-16, draws 60495358.., 9 draws = 3 layers x 3
//! billboard sets): 640 of 645 traced billboards reproduced to < 0.07 units / 0.0007 UV by the
//! generator in homebots' grass.rs (the EXE job FUN_004e6180), the rest are jitter-table entries
//! at rounding boundaries.
//!
//! * TerrainComponent native, after the per-quad info flags: u32 layer count, then per grass layer
//!   a TArray of 8-byte instances: x, y, z as half floats (world UE units) and a u16 with four
//!   signed 3-bit slope terms (bits 0-3, 4-7, 8-11, 12-15; bit 3 of each nibble = sign) * 0.125.
//! * Layer k of the instances belongs to Terrain.TerrainGrassLayers[k].

use crate::Package;

#[derive(Debug, Clone)]
pub struct GrassBillboard {
    pub texture: i32,
    pub min_draw_dist: f32,
    pub max_draw_dist: f32,
    pub near_width: f32,
    pub far_width: f32,
    pub near_width_dist: f32,
    pub far_width_dist: f32,
    pub height: f32,
    pub near_height_dist: f32,
    pub far_height_dist: f32,
    pub num_per_cell: u32,
    pub camera_aligned_y: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct GrassInstance {
    pub pos: [f32; 3],
    /// Slope terms of the u16 (bits 0-3, 4-7, 8-11, 12-15), already * 0.125.
    pub slope: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct GrassLayer {
    pub name: String,
    pub billboards: Vec<GrassBillboard>,
    pub instances: Vec<GrassInstance>,
}

#[derive(Debug, Clone)]
pub struct TerrainGrass {
    pub layers: Vec<GrassLayer>,
    /// TerrainBoundsMinX, MaxX, MinY, MaxY (TerrainOverlayTexture UV = (pos - min) / (max - min)).
    pub bounds: [f32; 4],
    /// Terrain.GrassColorTexture (sampled as TerrainOverlayTexture).
    pub color_texture: i32,
}

pub fn half_to_f32(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    match e {
        0 => s * m / 1024.0 * 2f32.powi(-14),
        31 => s * f32::INFINITY,
        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

fn slope_term(v: u16) -> f32 {
    let m = (v & 7) as f32;
    (if v & 8 != 0 { -m } else { m }) * 0.125
}

impl Package {
    pub fn terrain_grass(&self) -> Option<TerrainGrass> {
        let ti = self.exports.iter().position(|e| self.export_class(e) == "Terrain")?;
        let props = self.export_props(ti)?;
        let mut layers = Vec::new();
        for l in self.array_structs(props.get("TerrainGrassLayers")?) {
            let billboards = l
                .get("TerrainGrassBillboards")
                .map(|v| self.array_structs(v))
                .unwrap_or_default()
                .iter()
                .map(|b| GrassBillboard {
                    texture: b.object("Texture").unwrap_or(0),
                    min_draw_dist: b.float("MinDrawDist").unwrap_or(0.0),
                    max_draw_dist: b.float("MaxDrawDist").unwrap_or(0.0),
                    near_width: b.float("NearWidth").unwrap_or(0.0),
                    far_width: b.float("FarWidth").unwrap_or(0.0),
                    near_width_dist: b.float("NearWidthDist").unwrap_or(0.0),
                    far_width_dist: b.float("FarWidthDist").unwrap_or(0.0),
                    height: b.float("Height").unwrap_or(0.0),
                    near_height_dist: b.float("NearHeightDist").unwrap_or(0.0),
                    far_height_dist: b.float("FarHeightDist").unwrap_or(0.0),
                    num_per_cell: b.int("NumBillboardsPerCell").unwrap_or(1).max(0) as u32,
                    camera_aligned_y: b.bool("CameraAlignedY").unwrap_or(false),
                })
                .collect();
            layers.push(GrassLayer { name: match l.get("Name") { Some(crate::props::Value::Str(s)) => s.clone(), _ => String::new() }, billboards, instances: Vec::new() });
        }
        let comps: Vec<i32> = props.get("TerrainComponents").map(|v| self.array_objects(v)).unwrap_or_default();
        for c in comps.into_iter().filter(|&c| c > 0) {
            for (l, inst) in self.grass_instances(c as usize - 1).into_iter().enumerate() {
                if let Some(layer) = layers.get_mut(l) {
                    layer.instances.extend(inst);
                }
            }
        }
        Some(TerrainGrass {
            layers,
            bounds: [
                props.float("TerrainBoundsMinX")?,
                props.float("TerrainBoundsMaxX")?,
                props.float("TerrainBoundsMinY")?,
                props.float("TerrainBoundsMaxY")?,
            ],
            color_texture: props.object("GrassColorTexture").unwrap_or(0),
        })
    }

    /// Cooked grass instances of one TerrainComponent, per grass layer.
    fn grass_instances(&self, idx: usize) -> Vec<Vec<GrassInstance>> {
        let mut r = self.export_reader(idx);
        if self.read_props(&mut r).is_none() {
            return Vec::new();
        }
        (|| -> Option<Vec<Vec<GrassInstance>>> {
            // u32, u32, TArray<u16> heights, TArray<u32>, TArray<u16>, TArray<u16> quad flags
            r.u32()?;
            r.u32()?;
            for es in [2usize, 4, 2, 2] {
                let k = r.i32()?.max(0) as usize;
                r.bytes(k * es)?;
            }
            let nl = r.i32()?.max(0) as usize;
            if nl > 64 {
                return None;
            }
            let mut out = Vec::with_capacity(nl);
            for _ in 0..nl {
                let n = r.i32()?.max(0) as usize;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    let (x, y, z, s) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?);
                    v.push(GrassInstance {
                        pos: [half_to_f32(x), half_to_f32(y), half_to_f32(z)],
                        slope: [slope_term(s), slope_term(s >> 4), slope_term(s >> 8), slope_term(s >> 12)],
                    });
                }
                out.push(v);
            }
            Some(out)
        })()
        .unwrap_or_default()
    }
}
