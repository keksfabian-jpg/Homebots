//! External models (NOT from data, not game files): `assets/<..>/*.hbm`, a small mesh format
//! written by the offline converter from downloaded CC-licensed models (credits in the folder's
//! CREDITS.txt). Layout (little endian):
//! - "HBM1", u32 texture count, per texture: u32 length + PNG bytes
//! - u32 part count, per part: u8 kind (0 paint, 1 glass, 2 propeller blade, 3 beacon), i32
//!   texture (-1 none), f32x4 colour (linear), f32 roughness, f32 metallic, i32 group (propeller,
//!   -1 none), f32x3 pivot (the part's vertices are relative to it), u32 vertex count, u32 index
//!   count, vertices (position xyz, normal xyz, uv; f32), indices (u32)
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology},
};

pub struct Part {
    pub kind: u8,
    pub mesh: Mesh,
    pub material: StandardMaterial,
    pub group: i32,
    pub pivot: Vec3,
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.p..self.p + n)?;
        self.p += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}

/// Reads a .hbm file; textures go into `images`.
pub fn load(path: &str, images: &mut Assets<Image>) -> Option<Vec<Part>> {
    let bytes = std::fs::read(path).ok()?;
    let mut r = Rd { b: &bytes, p: 0 };
    if r.take(4)? != b"HBM1" {
        return None;
    }
    let mut tex = Vec::new();
    for _ in 0..r.u32()? {
        let n = r.u32()? as usize;
        let png = r.take(n)?;
        let img = Image::from_buffer(png, ImageType::Extension("png"), CompressedImageFormats::NONE, true, ImageSampler::Default, RenderAssetUsages::RENDER_WORLD).ok()?;
        tex.push(images.add(img));
    }
    let mut parts = Vec::new();
    for _ in 0..r.u32()? {
        let kind = r.take(1)?[0];
        let ti = r.i32()?;
        let col = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
        let (rough, metal) = (r.f32()?, r.f32()?);
        let group = r.i32()?;
        let pivot = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
        let (nv, ni) = (r.u32()? as usize, r.u32()? as usize);
        let vb = r.take(nv * 32)?;
        let fv: Vec<f32> = vb.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let ib = r.take(ni * 4)?;
        let idx: Vec<u32> = ib.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let pos: Vec<[f32; 3]> = fv.chunks_exact(8).map(|v| [v[0], v[1], v[2]]).collect();
        let nrm: Vec<[f32; 3]> = fv.chunks_exact(8).map(|v| [v[3], v[4], v[5]]).collect();
        let uv: Vec<[f32; 2]> = fv.chunks_exact(8).map(|v| [v[6], v[7]]).collect();
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx));
        let base = Color::linear_rgba(col[0], col[1], col[2], col[3]);
        let mut material = StandardMaterial {
            base_color: if ti >= 0 { Color::WHITE } else { base },
            base_color_texture: usize::try_from(ti).ok().and_then(|i| tex.get(i).cloned()),
            perceptual_roughness: rough,
            metallic: metal,
            double_sided: true,
            cull_mode: None,
            ..default()
        };
        match kind {
            1 => {
                material.alpha_mode = AlphaMode::Blend;
                material.reflectance = 0.8;
            }
            3 => {
                material.emissive = LinearRgba::rgb(4.0, 0.3, 0.1);
            }
            _ => {}
        }
        parts.push(Part { kind, mesh, material, group, pivot });
    }
    Some(parts)
}
