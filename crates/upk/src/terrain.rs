//! UE3 Terrain as cooked by Homefront.
//!
//! Observed layout: each TerrainComponent covers SectionSizeX x SectionSizeY "section
//! units" of 32 terrain patches starting at patch (SectionBaseX, SectionBaseY), and stores
//! a height grid of (SizeX * Tess + 1) x (SizeY * Tess + 1) u16 samples, where
//! Tess = MaxTessellationLevel (samples per section unit). Native data after the
//! properties: u32, u32, TArray<u16> heights.

use crate::props::Value;
use crate::Package;

const UNIT_PATCHES: f32 = 32.0;
const TERRAIN_ZSCALE: f32 = 1.0 / 128.0;

#[derive(Debug, Clone)]
pub struct TerrainGrid {
    pub nx: usize,
    pub ny: usize,
    /// World-space (UE) positions, row-major with X fastest.
    pub positions: Vec<[f32; 3]>,
}

#[derive(Debug, Clone)]
pub struct Terrain {
    pub grids: Vec<TerrainGrid>,
    /// Export index of the first layer's material (TerrainMaterial's Material), if found.
    pub material: Option<i32>,
}

impl Package {
    pub fn terrains(&self) -> Vec<Terrain> {
        let mut out = Vec::new();
        for (ti, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != "Terrain" {
                continue;
            }
            let mut r = self.export_reader(ti);
            let Some(props) = self.read_props(&mut r) else { continue };
            let loc = props.vector("Location").unwrap_or([0.0; 3]);
            let ds = props.float("DrawScale").unwrap_or(1.0);
            // Class default from Engine.u Default__Terrain: DrawScale3D = (64, 64, 256).
            let s3 = props.vector("DrawScale3D").unwrap_or([64.0, 64.0, 256.0]);
            let scale = [s3[0] * ds, s3[1] * ds, s3[2] * ds];
            let comps: Vec<i32> = props.get("TerrainComponents").map(|v| self.array_objects(v)).unwrap_or_default();
            let mut grids = Vec::new();
            for c in comps.into_iter().filter(|&c| c > 0) {
                if let Some(g) = self.terrain_component(c as usize - 1, loc, scale) {
                    grids.push(g);
                }
            }
            let material = self.terrain_layer_material(&props);
            out.push(Terrain { grids, material });
        }
        out
    }

    fn terrain_component(&self, idx: usize, loc: [f32; 3], scale: [f32; 3]) -> Option<TerrainGrid> {
        let mut r = self.export_reader(idx);
        let p = self.read_props(&mut r)?;
        let bx = p.int("SectionBaseX").unwrap_or(0) as f32;
        let by = p.int("SectionBaseY").unwrap_or(0) as f32;
        let sx = p.int("SectionSizeX").unwrap_or(1) as usize;
        let sy = p.int("SectionSizeY").unwrap_or(1) as usize;
        let tess = p.int("MaxTessellationLevel").unwrap_or(1).max(1) as usize;
        let _a = r.u32()?;
        let _b = r.u32()?;
        let n = r.i32()? as usize;
        let (nx, ny) = (sx * tess + 1, sy * tess + 1);
        if n != nx * ny {
            return None;
        }
        let step = UNIT_PATCHES / tess as f32;
        let mut positions = Vec::with_capacity(n);
        for j in 0..ny {
            for i in 0..nx {
                let h = r.u16()? as f32;
                positions.push([
                    loc[0] + (bx + i as f32 * step) * scale[0],
                    loc[1] + (by + j as f32 * step) * scale[1],
                    loc[2] + (h - 32768.0) * scale[2] * TERRAIN_ZSCALE,
                ]);
            }
        }
        Some(TerrainGrid { nx, ny, positions })
    }

    /// Layers[0].Setup -> Materials[0].Material (TerrainMaterial) -> Material.
    fn terrain_layer_material(&self, props: &crate::props::Props) -> Option<i32> {
        let layers = self.array_structs(props.get("Layers")?);
        for layer in layers {
            let Some(setup) = layer.object("Setup").filter(|&s| s > 0) else { continue };
            let mut r = self.export_reader(setup as usize - 1);
            let sp = self.read_props(&mut r)?;
            for m in self.array_structs(sp.get("Materials")?) {
                let Some(tm) = m.object("Material").filter(|&t| t > 0) else { continue };
                let mut r = self.export_reader(tm as usize - 1);
                let tp = self.read_props(&mut r)?;
                if let Some(Value::Object(mat)) = tp.get("Material") {
                    return Some(*mat);
                }
            }
        }
        None
    }
}

/// Layer blending data. Verified against the terrain itself: the weightmap is aligned
/// with world X→U / Y→V over TerrainBounds, and AlphaMapIndex 0..3 map to the decoded
/// channels A, R, G, B (index 2 = CLIFF has by far the highest mean slope, index 3 =
/// RIVERBANK the lowest mean height).
pub struct TerrainLayers {
    pub weight_w: usize,
    pub weight_h: usize,
    /// Decoded RGBA weightmap.
    pub weights: Vec<u8>,
    pub bounds: [f32; 4], // min x, max x, min y, max y (UE units)
    pub location: [f32; 3],
    pub patch_size: [f32; 2],
    /// Per AlphaMapIndex 0..3: (material export, MappingScale in patches per tile).
    pub layers: [Option<(i32, f32)>; 4],
}

pub const WEIGHT_CHANNEL: [usize; 4] = [3, 0, 1, 2];

impl Package {
    pub fn terrain_layers(&self) -> Option<TerrainLayers> {
        let ti = self.exports.iter().position(|e| self.export_class(e) == "Terrain")?;
        let mut r = self.export_reader(ti);
        let props = self.read_props(&mut r)?;
        let wi = self.exports.iter().position(|e| self.export_class(e) == "TerrainWeightMapTexture")?;
        let (w, h, weights) = self.read_texture2d(wi)?.decode_rgba()?;
        let s3 = props.vector("DrawScale3D").unwrap_or([64.0, 64.0, 256.0]);
        let ds = props.float("DrawScale").unwrap_or(1.0);
        let mut layers: [Option<(i32, f32)>; 4] = [None; 4];
        for l in self.array_structs(props.get("Layers")?) {
            let ai = l.int("AlphaMapIndex").unwrap_or(-1);
            if !(0..4).contains(&ai) || layers[ai as usize].is_some() {
                continue;
            }
            let Some(setup) = l.object("Setup").filter(|&s| s > 0) else { continue };
            let mut r = self.export_reader(setup as usize - 1);
            let Some(sp) = self.read_props(&mut r) else { continue };
            let Some(first) = sp.get("Materials").map(|v| self.array_structs(v)).and_then(|v| v.into_iter().next()) else { continue };
            let Some(tm) = first.object("Material").filter(|&t| t > 0) else { continue };
            let mut r = self.export_reader(tm as usize - 1);
            let Some(tp) = self.read_props(&mut r) else { continue };
            if let Some(mat) = tp.object("Material") {
                layers[ai as usize] = Some((mat, tp.float("MappingScale").unwrap_or(1.0).max(0.01)));
            }
        }
        Some(TerrainLayers {
            weight_w: w as usize,
            weight_h: h as usize,
            weights,
            bounds: [
                props.float("TerrainBoundsMinX")?,
                props.float("TerrainBoundsMaxX")?,
                props.float("TerrainBoundsMinY")?,
                props.float("TerrainBoundsMaxY")?,
            ],
            location: props.vector("Location").unwrap_or([0.0; 3]),
            patch_size: [s3[0] * ds, s3[1] * ds],
            layers,
        })
    }
}

/// One TerrainComponent as the game renders it (FTerrainVertexFactory input).
#[derive(Debug, Clone)]
pub struct TerrainComp {
    /// SectionBaseX/Y in patches.
    pub base: [i32; 2],
    /// SectionSizeX/Y in 32-patch sections.
    pub size: [i32; 2],
    /// MaxTessellationLevel: height samples per 32-patch section edge.
    pub tess: i32,
    /// (size.x*tess+1) x (size.y*tess+1) raw heights, X fastest.
    pub heights: Vec<u16>,
    /// Per quad of the height grid (size.x*tess x size.y*tess) info flags, if present.
    pub quad_flags: Vec<u16>,
}

impl TerrainComp {
    pub fn verts(&self) -> [usize; 2] {
        [(self.size[0] * self.tess + 1) as usize, (self.size[1] * self.tess + 1) as usize]
    }
}

#[derive(Debug, Clone)]
pub struct TerrainResource {
    pub id: [u8; 16],
    pub uniforms: crate::matres::UniformSet,
}

/// Everything the game's terrain shaders need (see homebots terrainemu.rs).
#[derive(Debug, Clone)]
pub struct TerrainRender {
    pub location: [f32; 3],
    /// DrawScale3D * DrawScale
    pub scale: [f32; 3],
    pub num_vertices: [i32; 2],
    /// WeightMapMinX/MaxX/MinY/MaxY (Engine.u defaults 0, 1, 0, 1)
    pub weight_map_bounds: [f32; 4],
    /// 32-patch sections (NumPatches / 32) and their palette index.
    pub sections: [usize; 2],
    pub section_palette: Vec<u8>,
    /// TWeightMap<N> textures.
    pub weight_maps: Vec<i32>,
    pub normal_map: Option<i32>,
    /// Concatenated layer names per palette ("GRAVELGRASSCLIFFRoad"), as the cooker
    /// names the TerrainMaterialResources.
    pub palette_names: Vec<String>,
    pub resources: Vec<TerrainResource>,
    pub components: Vec<TerrainComp>,
}

impl Package {
    /// Terrain render data. `is_resource_id` tells whether a GUID is a material resource Id
    /// known to the shader cache (used to find the FTerrainMaterialResources in the native).
    pub fn terrain_render(&self, is_resource_id: &dyn Fn(&[u8; 16]) -> bool) -> Option<TerrainRender> {
        let ti = self.exports.iter().position(|e| self.export_class(e) == "Terrain")?;
        let mut r = self.export_reader(ti);
        let props = self.read_props(&mut r)?;
        let nat = &self.export_bytes(&self.exports[ti])[r.pos..];
        let ds = props.float("DrawScale").unwrap_or(1.0);
        let s3 = props.vector("DrawScale3D").unwrap_or([64.0, 64.0, 256.0]);
        let npx = props.int("NumPatchesX").unwrap_or(128);
        let npy = props.int("NumPatchesY").unwrap_or(128);
        let mut nr = crate::props::Rd::new(nat);
        let _a = nr.u32()?;
        let _b = nr.u32()?;
        let n = nr.i32()? as usize;
        let sections = [(npx / 32) as usize, (npy / 32) as usize];
        if n != sections[0] * sections[1] {
            return None;
        }
        let mut section_palette = Vec::with_capacity(n);
        for _ in 0..n {
            let e = nr.bytes(7)?;
            section_palette.push(e[6]);
        }
        let nw = nr.i32()?;
        let weight_maps: Vec<i32> = (0..nw).map(|_| nr.i32()).collect::<Option<_>>()?;
        let _nres = nr.i32()?;
        let mut resources = Vec::new();
        let mut p = nr.pos + 12;
        while p + 16 <= nat.len() {
            if nat[p - 12..p].iter().all(|&b| b == 0) {
                let mut id = [0u8; 16];
                id.copy_from_slice(&nat[p..p + 16]);
                if is_resource_id(&id) {
                    if let Some((uniforms, end)) = self.read_uniform_set(nat, p) {
                        resources.push(TerrainResource { id, uniforms });
                        p = end.max(p + 16);
                        continue;
                    }
                }
            }
            p += 1;
        }
        let mut palette_names: Vec<String> = Vec::new();
        for l in props.get("Layers").map(|v| self.array_structs(v)).unwrap_or_default() {
            let pi = l.int("PaletteIndex").unwrap_or(0).max(0) as usize;
            if palette_names.len() <= pi {
                palette_names.resize(pi + 1, String::new());
            }
            if let Some(crate::props::Value::Str(s)) = l.get("Name") {
                palette_names[pi].push_str(s);
            }
        }
        let comps: Vec<i32> = props.get("TerrainComponents").map(|v| self.array_objects(v)).unwrap_or_default();
        let components = comps.into_iter().filter(|&c| c > 0).filter_map(|c| self.terrain_comp(c as usize - 1)).collect();
        Some(TerrainRender {
            location: props.vector("Location").unwrap_or([0.0; 3]),
            scale: [s3[0] * ds, s3[1] * ds, s3[2] * ds],
            num_vertices: [props.int("NumVerticesX").unwrap_or(npx + 1), props.int("NumVerticesY").unwrap_or(npy + 1)],
            weight_map_bounds: [
                props.float("WeightMapMinX").unwrap_or(0.0),
                props.float("WeightMapMaxX").unwrap_or(1.0),
                props.float("WeightMapMinY").unwrap_or(0.0),
                props.float("WeightMapMaxY").unwrap_or(1.0),
            ],
            sections,
            section_palette,
            weight_maps,
            normal_map: props.object("NormalMap"),
            palette_names,
            resources,
            components,
        })
    }

    /// Component native: u32, u32, TArray<u16> heights, TArray<u32>, TArray<u16>,
    /// TArray<u16> per-quad info flags, ...
    fn terrain_comp(&self, idx: usize) -> Option<TerrainComp> {
        let mut r = self.export_reader(idx);
        let p = self.read_props(&mut r)?;
        let base = [p.int("SectionBaseX").unwrap_or(0), p.int("SectionBaseY").unwrap_or(0)];
        let size = [p.int("SectionSizeX").unwrap_or(1), p.int("SectionSizeY").unwrap_or(1)];
        let tess = p.int("MaxTessellationLevel").unwrap_or(1).max(1);
        let _a = r.u32()?;
        let _b = r.u32()?;
        let n = r.i32()? as usize;
        let c = TerrainComp { base, size, tess, heights: Vec::new(), quad_flags: Vec::new() };
        let [nx, ny] = c.verts();
        if n != nx * ny {
            return None;
        }
        let heights: Vec<u16> = (0..n).map(|_| r.u16()).collect::<Option<_>>()?;
        let mut quad_flags = Vec::new();
        let nq = (nx - 1) * (ny - 1);
        (|| -> Option<()> {
            let k = r.i32()? as usize;
            r.bytes(k * 4)?;
            let k = r.i32()? as usize;
            r.bytes(k * 2)?;
            let k = r.i32()? as usize;
            if k == nq {
                quad_flags = (0..k).map(|_| r.u16()).collect::<Option<_>>()?;
            }
            Some(())
        })();
        Some(TerrainComp { heights, quad_flags, ..c })
    }
}
