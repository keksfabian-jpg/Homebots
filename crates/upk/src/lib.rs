//! Reader for Unreal Engine 3 packages (.u / .upk / .map) as shipped with Homefront
//! (file version 576, licensee 100). Original implementation written from the
//! on-disk layout; it reads files from the user's own install at runtime.

use byteorder::{LittleEndian as LE, ReadBytesExt};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

pub mod props;
pub mod texture;
pub mod mesh;
pub mod library;
pub mod level;
pub mod terrain;
pub mod material;
pub mod matcompile;
pub mod sm3;
pub mod shadercache;
pub mod matres;
pub mod skeletal;
pub mod anim;
pub mod speedtree;
pub mod grass;
pub mod particles;
pub mod ui;
pub mod sound;

pub const PACKAGE_TAG: u32 = 0x9E2A_83C1;
const COMPRESS_ZLIB: u32 = 0x1;
const COMPRESS_LZO: u32 = 0x2;
const COMPRESS_LZX: u32 = 0x4;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a UE3 package (tag {0:#x})")]
    BadTag(u32),
    #[error("unsupported compression flags {0:#x}")]
    Compression(u32),
    #[error("decompression failed: {0}")]
    Decompress(String),
    #[error("bad data: {0}")]
    Bad(String),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub uncompressed_offset: u32,
    pub uncompressed_size: u32,
    pub compressed_offset: u32,
    pub compressed_size: u32,
}

#[derive(Debug, Clone)]
pub struct Summary {
    pub file_version: u16,
    pub licensee_version: u16,
    pub total_header_size: u32,
    pub folder_name: String,
    pub package_flags: u32,
    pub name_count: u32,
    pub name_offset: u32,
    pub export_count: u32,
    pub export_offset: u32,
    pub import_count: u32,
    pub import_offset: u32,
    pub depends_offset: u32,
    pub guid: [u8; 16],
    pub engine_version: u32,
    pub cooker_version: u32,
    pub compression_flags: u32,
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone)]
pub struct Name {
    pub name: String,
    pub flags: u64,
}

/// Name reference inside the package: index into the name table plus instance number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FName {
    pub index: i32,
    pub number: i32,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub class_package: FName,
    pub class_name: FName,
    pub outer_index: i32,
    pub object_name: FName,
}

#[derive(Debug, Clone)]
pub struct Export {
    pub class_index: i32,
    pub super_index: i32,
    pub outer_index: i32,
    pub object_name: FName,
    pub archetype_index: i32,
    pub object_flags: u64,
    pub serial_size: u32,
    pub serial_offset: u32,
    pub export_flags: u32,
}

pub struct Package {
    pub summary: Summary,
    /// Fully decompressed package bytes (header + body), so offsets in the tables are direct.
    pub data: Vec<u8>,
    pub names: Vec<Name>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

fn read_fstring<R: Read>(r: &mut R) -> Result<String> {
    let len = r.read_i32::<LE>()?;
    if len == 0 {
        return Ok(String::new());
    }
    if len > 0 {
        let mut buf = vec![0u8; len as usize];
        r.read_exact(&mut buf)?;
        buf.pop(); // trailing NUL
        Ok(buf.iter().map(|&c| c as char).collect())
    } else {
        let n = (-len) as usize;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(r.read_u16::<LE>()?);
        }
        v.pop();
        Ok(String::from_utf16_lossy(&v))
    }
}

fn read_fname<R: Read>(r: &mut R) -> Result<FName> {
    Ok(FName { index: r.read_i32::<LE>()?, number: r.read_i32::<LE>()? })
}

fn read_summary<R: Read + Seek>(r: &mut R) -> Result<Summary> {
    let tag = r.read_u32::<LE>()?;
    if tag != PACKAGE_TAG {
        return Err(Error::BadTag(tag));
    }
    let file_version = r.read_u16::<LE>()?;
    let licensee_version = r.read_u16::<LE>()?;
    let total_header_size = r.read_u32::<LE>()?;
    let folder_name = read_fstring(r)?;
    let package_flags = r.read_u32::<LE>()?;
    let name_count = r.read_u32::<LE>()?;
    let name_offset = r.read_u32::<LE>()?;
    let export_count = r.read_u32::<LE>()?;
    let export_offset = r.read_u32::<LE>()?;
    let import_count = r.read_u32::<LE>()?;
    let import_offset = r.read_u32::<LE>()?;
    let depends_offset = r.read_u32::<LE>()?;
    let mut guid = [0u8; 16];
    r.read_exact(&mut guid)?;
    let gen_count = r.read_u32::<LE>()?;
    for _ in 0..gen_count {
        // export count, name count, net object count
        r.seek(SeekFrom::Current(12))?;
    }
    let engine_version = r.read_u32::<LE>()?;
    let cooker_version = r.read_u32::<LE>()?;
    let compression_flags = r.read_u32::<LE>()?;
    let chunk_count = r.read_u32::<LE>()?;
    let mut chunks = Vec::with_capacity(chunk_count as usize);
    for _ in 0..chunk_count {
        chunks.push(Chunk {
            uncompressed_offset: r.read_u32::<LE>()?,
            uncompressed_size: r.read_u32::<LE>()?,
            compressed_offset: r.read_u32::<LE>()?,
            compressed_size: r.read_u32::<LE>()?,
        });
    }
    Ok(Summary {
        file_version,
        licensee_version,
        total_header_size,
        folder_name,
        package_flags,
        name_count,
        name_offset,
        export_count,
        export_offset,
        import_count,
        import_offset,
        depends_offset,
        guid,
        engine_version,
        cooker_version,
        compression_flags,
        chunks,
    })
}

fn decompress_block(flags: u32, src: &[u8], out_len: usize) -> Result<Vec<u8>> {
    match flags {
        COMPRESS_LZO => {
            lzokay_native::decompress_all(src, Some(out_len))
                .map_err(|e| Error::Decompress(format!("{e:?}")))
        }
        COMPRESS_ZLIB | COMPRESS_LZX => Err(Error::Compression(flags)),
        f => Err(Error::Compression(f)),
    }
}

/// Decompress a UE3 compressed-chunk stream (tag, block size, summary, per-block
/// sizes, then block payloads). Used for package chunks and LZO bulk data.
pub fn decompress_chunk_stream(flags: u32, src: &[u8]) -> Result<Vec<u8>> {
    let mut r = Cursor::new(src);
    let tag = r.read_u32::<LE>()?;
    if tag != PACKAGE_TAG {
        return Err(Error::BadTag(tag));
    }
    let _block_size = r.read_u32::<LE>()?;
    let _total_compressed = r.read_u32::<LE>()?;
    let total_uncompressed = r.read_u32::<LE>()? as usize;
    // a damaged header must not make us allocate gigabytes or loop forever
    if total_uncompressed > (1 << 30) {
        return Err(Error::Decompress(format!("chunk stream too large: {total_uncompressed}")));
    }
    let mut blocks = Vec::new();
    let mut acc = 0usize;
    while acc < total_uncompressed {
        let c = r.read_u32::<LE>()? as usize;
        let u = r.read_u32::<LE>()? as usize;
        if u == 0 || c > src.len() {
            return Err(Error::Decompress("bad chunk block size".into()));
        }
        acc += u;
        blocks.push((c, u));
    }
    let mut out = Vec::with_capacity(total_uncompressed);
    for (c, u) in blocks {
        let mut buf = vec![0u8; c];
        r.read_exact(&mut buf)?;
        out.extend(decompress_block(flags, &buf, u)?);
    }
    Ok(out)
}

fn decompress_chunk(flags: u32, file: &[u8], chunk: &Chunk) -> Result<Vec<u8>> {
    let start = chunk.compressed_offset as usize;
    let end = (start + chunk.compressed_size as usize + 64 * 1024).min(file.len());
    decompress_chunk_stream(flags, &file[start..end])
}

impl Package {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    /// Return the package bytes with all compressed chunks expanded in place.
    pub fn decompressed_image(file: Vec<u8>) -> Result<Vec<u8>> {
        let summary = read_summary(&mut Cursor::new(&file))?;
        Self::expand(&summary, file)
    }

    fn expand(summary: &Summary, file: Vec<u8>) -> Result<Vec<u8>> {
        Ok(if summary.compression_flags != 0 && !summary.chunks.is_empty() {
            let first = summary.chunks[0].uncompressed_offset as usize;
            let end = summary
                .chunks
                .iter()
                .map(|c| (c.uncompressed_offset + c.uncompressed_size) as usize)
                .max()
                .unwrap_or(first);
            let mut img = vec![0u8; end];
            img[..first.min(file.len())].copy_from_slice(&file[..first.min(file.len())]);
            for c in &summary.chunks {
                let d = decompress_chunk(summary.compression_flags, &file, c)?;
                let o = c.uncompressed_offset as usize;
                img[o..o + d.len()].copy_from_slice(&d);
            }
            img
        } else {
            file
        })
    }

    pub fn from_bytes(file: Vec<u8>) -> Result<Self> {
        let summary = read_summary(&mut Cursor::new(&file))?;
        let data = Self::expand(&summary, file)?;
        let mut r = Cursor::new(&data);
        r.seek(SeekFrom::Start(summary.name_offset as u64))?;
        let mut names = Vec::with_capacity(summary.name_count as usize);
        for _ in 0..summary.name_count {
            let name = read_fstring(&mut r)?;
            let flags = r.read_u64::<LE>()?;
            names.push(Name { name, flags });
        }

        r.seek(SeekFrom::Start(summary.import_offset as u64))?;
        let mut imports = Vec::with_capacity(summary.import_count as usize);
        for _ in 0..summary.import_count {
            imports.push(Import {
                class_package: read_fname(&mut r)?,
                class_name: read_fname(&mut r)?,
                outer_index: r.read_i32::<LE>()?,
                object_name: read_fname(&mut r)?,
            });
        }

        r.seek(SeekFrom::Start(summary.export_offset as u64))?;
        let mut exports = Vec::with_capacity(summary.export_count as usize);
        for _ei in 0..summary.export_count {
            let class_index = r.read_i32::<LE>()?;
            let super_index = r.read_i32::<LE>()?;
            let outer_index = r.read_i32::<LE>()?;
            let object_name = read_fname(&mut r)?;
            let archetype_index = r.read_i32::<LE>()?;
            let object_flags = r.read_u64::<LE>()?;
            let serial_size = r.read_u32::<LE>()?;
            // v576: serial offset always present, no component map (removed in earlier
            // UE3 versions); variable-length net-object list, then package GUID + flags.
            let serial_offset = r.read_u32::<LE>()?;
            let export_flags = r.read_u32::<LE>()?;
            let net_count = r.read_u32::<LE>()?;
            r.seek(SeekFrom::Current(net_count as i64 * 4))?;
            let _package_guid = { let mut g = [0u8; 16]; r.read_exact(&mut g)?; g };
            let _package_flags = r.read_u32::<LE>()?;
            exports.push(Export {
                class_index,
                super_index,
                outer_index,
                object_name,
                archetype_index,
                object_flags,
                serial_size,
                serial_offset,
                export_flags,
            });
        }

        Ok(Package { summary, data, names, imports, exports })
    }

    pub fn name(&self, n: FName) -> String {
        let base = self
            .names
            .get(n.index as usize)
            .map(|x| x.name.as_str())
            .unwrap_or("<bad-name>");
        if n.number > 0 { format!("{}_{}", base, n.number - 1) } else { base.to_string() }
    }

    /// Elements of a name array property (raw: count, then an FName per element).
    pub fn name_array(&self, v: Option<&props::Value>) -> Vec<String> {
        let Some(props::Value::Raw { data, .. }) = v else { return Vec::new() };
        if data.len() < 4 {
            return Vec::new();
        }
        let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        (0..n)
            .filter_map(|k| {
                let o = 4 + k * 8;
                let b = data.get(o..o + 8)?;
                Some(self.name(FName { index: i32::from_le_bytes(b[0..4].try_into().ok()?), number: i32::from_le_bytes(b[4..8].try_into().ok()?) }))
            })
            .collect()
    }

    /// Resolve an object reference (>0 export, <0 import, 0 null) to its short name.
    pub fn object_name(&self, idx: i32) -> String {
        if idx > 0 {
            self.exports.get(idx as usize - 1).map(|e| self.name(e.object_name)).unwrap_or_default()
        } else if idx < 0 {
            self.imports.get((-idx) as usize - 1).map(|i| self.name(i.object_name)).unwrap_or_default()
        } else {
            "None".into()
        }
    }

    /// Full dotted path (Outer.Outer.Name) of an object reference.
    pub fn object_path(&self, idx: i32) -> String {
        let mut parts = Vec::new();
        let mut cur = idx;
        let mut guard = 0;
        while cur != 0 && guard < 64 {
            parts.push(self.object_name(cur));
            cur = if cur > 0 {
                self.exports[cur as usize - 1].outer_index
            } else {
                self.imports[(-cur) as usize - 1].outer_index
            };
            guard += 1;
        }
        parts.reverse();
        parts.join(".")
    }

    /// Class name of an import (negative index).
    pub fn import_class(&self, idx: i32) -> String {
        self.imports.get((-idx - 1) as usize).map(|i| self.name(i.class_name)).unwrap_or_default()
    }

    pub fn export_class(&self, e: &Export) -> String {
        if e.class_index == 0 { "Class".into() } else { self.object_name(e.class_index) }
    }

    pub fn export_bytes(&self, e: &Export) -> &[u8] {
        let s = e.serial_offset as usize;
        &self.data[s..s + e.serial_size as usize]
    }
}

impl Package {
    /// Short object name of an export.
    pub fn object_name_of(&self, e: &Export) -> String {
        self.name(e.object_name)
    }
}

impl Package {
    /// Name of the outermost package of an export (first segment of its path),
    /// which is also the file that holds its separately stored bulk data.
    pub fn source_package(&self, export_index: usize) -> String {
        let p = self.object_path(export_index as i32 + 1);
        p.split('.').next().unwrap_or_default().to_string()
    }
}
