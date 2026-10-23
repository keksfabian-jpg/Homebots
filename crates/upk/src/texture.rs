//! Texture2D: properties + bulk-data mip chain, decoded to RGBA8.

use crate::props::{Props, Rd};
use crate::{decompress_chunk_stream, Package};

pub const BULK_SEPARATE_FILE: u32 = 0x01;
pub const BULK_ZLIB: u32 = 0x02;
pub const BULK_LZO: u32 = 0x10;
pub const BULK_UNUSED: u32 = 0x20;

#[derive(Debug, Clone)]
pub struct BulkData {
    pub flags: u32,
    pub element_count: u32,
    pub size_on_disk: u32,
    pub offset_in_file: u32,
    /// Inline payload (decompressed), empty if the data lives in another file.
    pub data: Vec<u8>,
}

pub fn read_bulk(r: &mut Rd) -> Option<BulkData> {
    let flags = r.u32()?;
    let element_count = r.u32()?;
    let size_on_disk = r.u32()?;
    let offset_in_file = r.u32()?;
    let mut data = Vec::new();
    if flags & BULK_SEPARATE_FILE == 0 && flags & BULK_UNUSED == 0 && size_on_disk > 0 {
        let raw = r.bytes(size_on_disk as usize)?;
        data = if flags & BULK_LZO != 0 {
            decompress_chunk_stream(0x2, raw).ok()?
        } else if flags & BULK_ZLIB != 0 {
            return None; // not seen in Homefront yet
        } else {
            raw.to_vec()
        };
    }
    Some(BulkData { flags, element_count, size_on_disk, offset_in_file, data })
}

#[derive(Debug, Clone)]
pub struct Mip {
    pub width: u32,
    pub height: u32,
    pub bulk: BulkData,
}

#[derive(Debug, Clone)]
pub struct Texture2D {
    pub props: Props,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub mips: Vec<Mip>,
}

impl Texture2D {
    /// Largest mip whose pixel data is present inline.
    pub fn best_mip(&self) -> Option<&Mip> {
        self.mips.iter().find(|m| !m.bulk.data.is_empty())
    }

    /// Fill the largest mip from its separate package file (if needed) so that
    /// `decode_rgba` returns full resolution. `max_dim` caps the chosen size.
    pub fn load_full(&mut self, lib: &crate::library::Library, source_package: &str, max_dim: u32) {
        if let Some(m) = self.mips.iter_mut().find(|m| m.width <= max_dim && m.height <= max_dim) {
            if m.bulk.data.is_empty() && m.bulk.flags & BULK_SEPARATE_FILE != 0 {
                if let Some(d) = lib.read_separate(source_package, &m.bulk) {
                    m.bulk.data = d;
                }
            }
        }
    }

    /// Fill mip `k` from its separate package file when it is not inline.
    pub fn load_mip(&mut self, lib: &crate::library::Library, source_package: &str, k: usize) {
        if let Some(m) = self.mips.get_mut(k) {
            if m.bulk.data.is_empty() && m.bulk.flags & BULK_SEPARATE_FILE != 0 {
                if let Some(d) = lib.read_separate(source_package, &m.bulk) {
                    m.bulk.data = d;
                }
            }
        }
    }

    /// Decode the best available mip to RGBA8. Returns (w, h, pixels).
    pub fn decode_rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        let m = self.best_mip()?;
        let (w, h) = (m.width as usize, m.height as usize);
        let mut px = vec![0u32; w * h];
        let src = &m.bulk.data;
        let ok = match self.format.as_str() {
            "PF_DXT1" => texture2ddecoder::decode_bc1(src, w, h, &mut px).is_ok(),
            "PF_DXT3" => texture2ddecoder::decode_bc2(src, w, h, &mut px).is_ok(),
            "PF_DXT5" => texture2ddecoder::decode_bc3(src, w, h, &mut px).is_ok(),
            "PF_A8R8G8B8" => {
                for (i, c) in src.chunks_exact(4).take(w * h).enumerate() {
                    // stored B,G,R,A -> pack as BGRA u32 like the decoder output
                    px[i] = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                }
                true
            }
            "PF_G8" => {
                for (i, &g) in src.iter().take(w * h).enumerate() {
                    px[i] = u32::from_le_bytes([g, g, g, 255]);
                }
                true
            }
            _ => false,
        };
        if !ok {
            return None;
        }
        // texture2ddecoder yields BGRA packed little-endian; convert to RGBA bytes.
        let mut out = Vec::with_capacity(w * h * 4);
        for p in px {
            let [b, g, r, a] = p.to_le_bytes();
            out.extend_from_slice(&[r, g, b, a]);
        }
        Some((m.width, m.height, out))
    }
}

impl Package {
    pub fn read_texture2d(&self, export_index: usize) -> Option<Texture2D> {
        let mut r = self.export_reader(export_index);
        let props = self.read_props(&mut r)?;
        let format = props.enum_name("Format").unwrap_or("PF_Unknown").to_string();
        let width = props.int("SizeX").unwrap_or(0) as u32;
        let height = props.int("SizeY").unwrap_or(0) as u32;
        let _source_art = read_bulk(&mut r)?;
        let count = r.i32()?;
        let mut mips = Vec::new();
        for _ in 0..count.max(0) {
            let bulk = read_bulk(&mut r)?;
            let mw = r.u32()?;
            let mh = r.u32()?;
            mips.push(Mip { width: mw, height: mh, bulk });
        }
        Some(Texture2D { props, format, width, height, mips })
    }
}
