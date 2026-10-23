//! UE3 ShaderCache package (RefShaderCache-PC-D3D-SM3.upk) as cooked by Homefront.
//!
//! Layout as observed in the file:
//! * shaders: FName ShaderType (8) | FGuid Id (16) | i32 SkipOffset | u8 Platform |
//!   u8 Frequency | TArray<u8> Code | parameter bindings ... (up to SkipOffset)
//! * material shader maps: see `ShaderCache::record` (shader lists precede the MaterialId and
//!   FriendlyName they belong to; the entry ends at its SkipOffset)
//! Records are found by MaterialId, which materials store in their FMaterialResource.

use crate::Package;
use std::collections::HashMap;

pub struct ShaderEntry {
    pub type_name: String,
    pub code_offset: usize,
    pub code_len: usize,
}

pub struct ShaderCache {
    pub data: Vec<u8>,
    pub names: Vec<String>,
    pub shaders: HashMap<[u8; 16], ShaderEntry>,
    /// MaterialId -> offsets of "MaterialId + FriendlyName" occurrences
    records: HashMap<[u8; 16], Vec<usize>>,
    /// BaseMaterialId -> material ids having an entry with that base (built on first use)
    by_base: std::sync::OnceLock<HashMap<[u8; 16], Vec<[u8; 16]>>>,
}

#[derive(Debug, Clone)]
pub struct MeshMap {
    pub vertex_factory: String,
    /// (shader type, shader id)
    pub shaders: Vec<(String, [u8; 16])>,
}

#[derive(Debug, Clone)]
pub struct MaterialRecord {
    pub friendly_name: String,
    pub maps: Vec<MeshMap>,
    /// FStaticParameterSet after the friendly name: static switches the shaders were compiled
    /// with: (name, value, override, ExpressionGUID)
    pub switches: Vec<(String, bool, bool, [u8; 16])>,
    /// FStaticParameterSet.BaseMaterialId
    pub base_id: [u8; 16],
}

impl ShaderCache {
    pub fn load(path: &std::path::Path) -> std::io::Result<ShaderCache> {
        let pkg = Package::open(path).map_err(|e| std::io::Error::other(format!("{e}")))?;
        let names: Vec<String> = pkg.names.iter().map(|n| n.name.clone()).collect();
        let data = std::fs::read(path)?;
        let mut sc = ShaderCache { data, names, shaders: HashMap::new(), records: HashMap::new(), by_base: std::sync::OnceLock::new() };
        sc.index_shaders();
        sc.index_records();
        Ok(sc)
    }

    fn u32(&self, o: usize) -> u32 {
        self.data.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).unwrap_or(u32::MAX)
    }

    fn fname(&self, o: usize) -> Option<&str> {
        let i = self.u32(o) as usize;
        if i < self.names.len() && self.u32(o + 4) == 0 {
            Some(&self.names[i])
        } else {
            None
        }
    }

    fn index_shaders(&mut self) {
        let d = &self.data;
        let mut found = Vec::new();
        for c in 34..d.len().saturating_sub(4) {
            // version token 0xFFFE0300 / 0xFFFF0300
            if d[c] != 0x00 || d[c + 1] != 0x03 || d[c + 3] != 0xFF || (d[c + 2] != 0xFE && d[c + 2] != 0xFF) {
                continue;
            }
            let len = u32::from_le_bytes(d[c - 4..c].try_into().unwrap()) as usize;
            if len == 0 || len > 400_000 || c + len > d.len() {
                continue;
            }
            let ti = u32::from_le_bytes(d[c - 34..c - 30].try_into().unwrap()) as usize;
            let tn = u32::from_le_bytes(d[c - 30..c - 26].try_into().unwrap());
            if ti >= self.names.len() || tn != 0 {
                continue;
            }
            let mut g = [0u8; 16];
            g.copy_from_slice(&d[c - 26..c - 10]);
            found.push((g, ShaderEntry { type_name: self.names[ti].clone(), code_offset: c, code_len: len }));
        }
        for (g, e) in found {
            self.shaders.insert(g, e);
        }
    }

    pub fn code(&self, id: &[u8; 16]) -> Option<&[u8]> {
        let e = self.shaders.get(id)?;
        self.data.get(e.code_offset..e.code_offset + e.code_len)
    }

    fn index_records(&mut self) {
        let d = &self.data;
        let mut out: HashMap<[u8; 16], Vec<usize>> = HashMap::new();
        for o in 0..d.len().saturating_sub(24) {
            let l = u32::from_le_bytes(d[o + 16..o + 20].try_into().unwrap()) as usize;
            if !(2..300).contains(&l) || o + 20 + l > d.len() || d[o + 20 + l - 1] != 0 || !d[o + 20].is_ascii_alphabetic() {
                continue;
            }
            if !d[o + 20..o + 20 + l - 1].iter().all(|c| (32..127).contains(c)) {
                continue;
            }
            let mut g = [0u8; 16];
            g.copy_from_slice(&d[o..o + 16]);
            if g.iter().filter(|&&x| x == 0).count() <= 4 {
                out.entry(g).or_default().push(o);
            }
        }
        self.records = out;
    }

    /// All record MaterialIds (GUID followed by an FString friendly name).
    pub fn record_ids(&self) -> std::collections::HashSet<[u8; 16]> {
        self.records.keys().copied().collect()
    }

    /// Is `id` the MaterialId of a shader map record (followed by a friendly name)?
    pub fn is_record(&self, id: &[u8; 16]) -> bool {
        self.records.contains_key(id)
    }

    /// Shader map record for a material resource id.
    ///
    /// Cache layout (TMap<FStaticParameterSet, FMaterialShaderMap>), per entry:
    /// key FStaticParameterSet | i32 SkipOffset | shaders: i32 N, N x (FName, FGuid, FName) |
    /// mesh maps: i32 M, M x (i32 N, N x (FName, FGuid, FName), FName VertexFactory) |
    /// FGuid MaterialId | FString FriendlyName | FStaticParameterSet ... | (SkipOffset)
    /// i.e. the friendly name comes *after* the shader lists it belongs to.
    pub fn record(&self, material_id: &[u8; 16]) -> Option<MaterialRecord> {
        self.records_all(material_id).into_iter().next()
    }

    /// All shader map entries for a material id. One material id can have several entries
    /// (one per static parameter set: the TMap key is the FStaticParameterSet).
    pub fn records_all(&self, material_id: &[u8; 16]) -> Vec<MaterialRecord> {
        let d = &self.data;
        let mut out = Vec::new();
        for &o in self.records.get(material_id).map(|v| v.as_slice()).unwrap_or(&[]) {
            let l = self.u32(o + 16) as usize;
            let friendly_name = String::from_utf8_lossy(&d[o + 20..o + 20 + l - 1]).to_string();
            if let Some(maps) = self.maps_before(o) {
                let switches = self.switches_at(o + 20 + l).unwrap_or_default();
                let mut base_id = [0u8; 16];
                if let Some(b) = d.get(o + 20 + l..o + 36 + l) {
                    base_id.copy_from_slice(b);
                }
                out.push(MaterialRecord { friendly_name, maps, switches, base_id });
            }
        }
        out
    }

    /// The shader map entry the game uses for a static parameter set: same base material id
    /// and the same switch values (by ExpressionGUID). Verified with the Overpass water trace.
    pub fn record_for_static_set(&self, base_id: &[u8; 16], switches: &[([u8; 16], bool)]) -> Option<MaterialRecord> {
        let idx = self.by_base.get_or_init(|| {
            let mut m: HashMap<[u8; 16], Vec<[u8; 16]>> = HashMap::new();
            for (id, pos) in &self.records {
                for &o in pos {
                    let l = self.u32(o + 16) as usize;
                    if let Some(b) = self.data.get(o + 20 + l..o + 36 + l) {
                        let v = m.entry(b.try_into().unwrap()).or_default();
                        if !v.contains(id) {
                            v.push(*id);
                        }
                    }
                }
            }
            m
        });
        let mut found = None;
        for id in idx.get(base_id).map(|v| v.as_slice()).unwrap_or(&[]) {
            for r in self.records_all(id) {
                if &r.base_id != base_id || r.switches.len() != switches.len() {
                    continue;
                }
                let same = switches.iter().all(|(g, v)| r.switches.iter().any(|s| &s.3 == g && s.1 == *v));
                if same {
                    found = Some(r);
                }
            }
        }
        found
    }

    /// FStaticParameterSet: FGuid BaseMaterialId, TArray<FStaticSwitchParameter>
    /// (FName ParameterName, UBOOL Value, UBOOL bOverride, FGuid ExpressionGUID).
    pub fn switches_at(&self, p: usize) -> Option<Vec<(String, bool, bool, [u8; 16])>> {
        let n = self.u32(p + 16) as usize;
        if n > 1024 {
            return None;
        }
        let mut q = p + 20;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let name = self.fname(q)?.to_string();
            let v = self.u32(q + 8);
            let ov = self.u32(q + 12);
            if v > 1 || ov > 1 {
                return None;
            }
            let mut g = [0u8; 16];
            g.copy_from_slice(self.data.get(q + 16..q + 32)?);
            out.push((name, v != 0, ov != 0, g));
            q += 32;
        }
        Some(out)
    }

    /// Records (friendly name, vertex factory) whose shader lists reference shader `id`.
    /// The owning record's MaterialId + FriendlyName follow its lists, so scan forward.
    /// Index of all shader-list entries (FName, FGuid, FName): guid -> positions of the guid.
    pub fn entry_index(&self) -> HashMap<[u8; 16], Vec<usize>> {
        let d = &self.data;
        let mut out: HashMap<[u8; 16], Vec<usize>> = HashMap::new();
        let mut o = 0;
        while o + 32 <= d.len() {
            if d[o + 4..o + 8] == [0, 0, 0, 0] && d[o..o + 8] == d[o + 24..o + 32] && self.fname(o).is_some() {
                let mut g = [0u8; 16];
                g.copy_from_slice(&d[o + 8..o + 24]);
                if self.shaders.contains_key(&g) {
                    out.entry(g).or_default().push(o + 8);
                    o += 32;
                    continue;
                }
            }
            o += 1;
        }
        out
    }

    pub fn owners(&self, idx: &HashMap<[u8; 16], Vec<usize>>, id: &[u8; 16]) -> Vec<(String, String)> {
        let d = &self.data;
        let mut out = Vec::new();
        for &q in idx.get(id).map(|v| v.as_slice()).unwrap_or(&[]) {
            // vertex factory: first FName after the list that names one
            let mut r = q + 24;
            let mut vf = String::new();
            while r < d.len() {
                match self.fname(r) {
                    Some(n) if n.contains("VertexFactory") => {
                        vf = n.to_string();
                        break;
                    }
                    Some(_) => r += 32,
                    None => break,
                }
            }
            let mut s = q + 24;
            while s + 24 < d.len() && s < q + 2_000_000 {
                let l = self.u32(s + 16) as usize;
                if (4..300).contains(&l)
                    && s + 20 + l <= d.len()
                    && d[s + 20 + l - 1] == 0
                    && d[s + 20].is_ascii_alphabetic()
                    && d[s + 20..s + 20 + l - 1].iter().all(|c| (32..127).contains(c))
                {
                    out.push((String::from_utf8_lossy(&d[s + 20..s + 20 + l - 1]).to_string(), vf));
                    break;
                }
                s += 1;
            }
        }
        out
    }

    /// Find the SkipOffset that closes the entry whose MaterialId is at `id_at`, and parse the
    /// shader lists between it and `id_at`. The parse must land exactly on `id_at`.
    fn maps_before(&self, id_at: usize) -> Option<Vec<MeshMap>> {
        let d = &self.data;
        let lo = id_at.saturating_sub(4_000_000);
        let mut p = id_at.checked_sub(8)?;
        while p > lo {
            let v = self.u32(p) as usize;
            if v > id_at + 20 && v < id_at + 20_000 {
                if let Some(m) = self.parse_lists(p + 4, id_at) {
                    return Some(m);
                }
            }
            p -= 1;
        }
        let _ = d;
        None
    }

    fn parse_entries(&self, mut q: usize, end: usize) -> Option<(Vec<(String, [u8; 16])>, usize)> {
        let n = self.u32(q) as usize;
        if n > 4096 || q + 4 + n * 32 > end {
            return None;
        }
        q += 4;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            let t1 = self.fname(q)?;
            if self.fname(q + 24)? != t1 {
                return None;
            }
            let mut g = [0u8; 16];
            g.copy_from_slice(&self.data[q + 8..q + 24]);
            v.push((t1.to_string(), g));
            q += 32;
        }
        Some((v, q))
    }

    fn parse_lists(&self, q: usize, end: usize) -> Option<Vec<MeshMap>> {
        let (global, mut q) = self.parse_entries(q, end)?;
        let m = self.u32(q) as usize;
        if m > 256 {
            return None;
        }
        q += 4;
        let mut maps = Vec::new();
        if !global.is_empty() {
            maps.push(MeshMap { vertex_factory: String::new(), shaders: global });
        }
        for _ in 0..m {
            let (ents, q2) = self.parse_entries(q, end)?;
            let vf = self.fname(q2)?;
            if !vf.contains("VertexFactory") {
                return None;
            }
            maps.push(MeshMap { vertex_factory: vf.to_string(), shaders: ents });
            q = q2 + 8;
        }
        (q == end).then_some(maps)
    }
}


impl MaterialRecord {
    pub fn shader(&self, vf: &str, type_name: &str) -> Option<[u8; 16]> {
        self.maps.iter().filter(|m| m.vertex_factory == vf).flat_map(|m| m.shaders.iter()).find(|(t, _)| t == type_name).map(|x| x.1)
    }
}
