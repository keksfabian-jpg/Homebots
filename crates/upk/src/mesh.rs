//! StaticMesh decoding for Homefront (UE3 v576 / licensee 100).
//!
//! Layout as observed in Homefront's cooked packages (differs from stock UE3):
//!   NetIndex, properties, u32 (unknown, 0), FBoxSphereBounds, BodySetup,
//!   InternalVersion (18), TArray<FName>, TArray<LOD>, kDOP nodes, kDOP triangles, ...
//! Each LOD: 2 dwords (unknown), TArray<Section>, then (searched) the tangent/UV stream,
//! the position stream, vertex count, index buffer.

use crate::props::{Props, Rd};
use crate::Package;

#[derive(Debug, Clone)]
pub struct Section {
    pub material: i32,
    pub first_index: u32,
    pub num_triangles: u32,
    pub min_vertex: u32,
    pub max_vertex: u32,
}

#[derive(Debug, Clone, Default)]
pub struct MeshLod {
    pub sections: Vec<Section>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Packed TangentX / TangentZ exactly as stored (UBYTE4; the game's vertex shaders
    /// unpack them with x * 2/255 - 1; TangentZ.w is the binormal sign).
    pub tangent_x: Vec<[u8; 4]>,
    pub tangent_z: Vec<[u8; 4]>,
    /// All UV sets (set 0 equals `uvs`).
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct StaticMesh {
    pub props: Props,
    pub origin: [f32; 3],
    pub extent: [f32; 3],
    pub radius: f32,
    pub lods: Vec<MeshLod>,
    /// LOD index of `lods[0]` in the mesh (the component's per-LOD material overrides
    /// are indexed by it).
    pub lod_index: usize,
    /// Vertex data of LODs that live in the source package (bulk header per LOD, `None` if
    /// inline), with their sections and vertex counts.
    pub separate: Vec<Option<(crate::texture::BulkData, Vec<Section>, usize)>>,
}

fn half(h: u16) -> f32 {
    half::f16::from_bits(h).to_f32()
}

/// UE3 packed normal: 4 bytes, each (b / 127.5 - 1).
fn unpack_normal(v: u32) -> [f32; 3] {
    let b = v.to_le_bytes();
    let f = |x: u8| x as f32 / 127.5 - 1.0;
    [f(b[0]), f(b[1]), f(b[2])]
}

fn dw(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

impl Package {
    pub fn read_static_mesh(&self, export_index: usize) -> Option<StaticMesh> {
        self.read_static_mesh_any(export_index).filter(|sm| !sm.lods.is_empty())
    }

    /// `read_static_mesh`, but a mesh whose LODs are all separate bulk data (no inline LOD in
    /// this package) comes back with `lods` empty and `lod_index` = LOD count, for
    /// `read_static_mesh_full` to load from the source package. Found on the new maps
    /// (2026-10-22): Commercial.buildings.Statics.Office_Small_OneStory / _TwoStory (Grenzland, 27
    /// placements), FARM Warehouse_A_Foundation_1280 (38), VEH_military NK_truck (Hafen 58),
    /// STO_barriers indFence4 / indFenceBroke (Engelsinsel 37) were not drawn.
    fn read_static_mesh_any(&self, export_index: usize) -> Option<StaticMesh> {
        let mut r = self.export_reader(export_index);
        let props = self.read_props(&mut r)?;
        let _unknown = r.u32()?;
        let origin = r.vec3()?;
        let extent = r.vec3()?;
        let radius = r.f32()?;
        let _body_setup = r.i32()?;
        let _internal_version = r.i32()?;
        let n_names = r.i32()?;
        r.bytes(n_names.max(0) as usize * 8)?;
        let n_lods = r.i32()?;
        if !(1..=8).contains(&n_lods) {
            return None;
        }
        // LOD headers: index, unknown, sections, then 8 dwords:
        // NumVerts, NumTexCoords, 0, 16, 0, 0, DataSize, 0.
        let mut headers = Vec::new();
        // a LOD header that does not parse ends the list (the LODs before it are kept and the
        // vertex data is searched in the whole export below). Office_Small_TwoStory in
        // borderlands_bases: LOD 0 header (3141 verts, 4 sections) is followed by different
        // fields (2, 5856, index data) instead of the 7 dwords, so LOD 1 read garbage
        // (720906 sections) and the whole mesh failed; the same mesh reads in other sublevels.
        let mut headers_ok = true;
        for _ in 0..n_lods {
            let at = r.pos;
            let h = (|| -> Option<(Vec<Section>, usize)> {
                let _lod_index = r.u32()?;
                let _unk = r.u32()?;
                let sections = read_sections(&mut r)?;
                let nv = r.u32()? as usize;
                r.bytes(7 * 4)?;
                Some((sections, nv))
            })();
            match h {
                Some(h) => headers.push(h),
                None if !headers.is_empty() => {
                    r.pos = at;
                    headers_ok = false;
                    break;
                }
                None => return None,
            }
        }
        // After the LOD headers one bulk-data header per LOD (flags, count, size, offset)
        // followed by the LOD's vertex/index data when it is inline. Cooked maps keep the
        // lower LODs inline; LOD 0 of multi-LOD meshes is flagged StoreInSeparateFile (+LZO)
        // and lives in the source package (e.g. VEH_civilian.upk), as for texture mips.
        // Verified: Overpass trace draws Boulder8 / Boulder1 with their LOD-0 vertex counts.
        let start = r.pos;
        let mut separate = Vec::new();
        let mut inline: Vec<Option<(usize, usize)>> = Vec::new();
        let ok = headers_ok && (|| -> Option<()> {
            for _ in 0..headers.len() {
                let flags = r.u32()?;
                let count = r.u32()?;
                let size = r.u32()? as usize;
                let offset = r.u32()?;
                if flags & crate::texture::BULK_SEPARATE_FILE != 0 {
                    separate.push(Some(crate::texture::BulkData { flags, element_count: count, size_on_disk: size as u32, offset_in_file: offset, data: Vec::new() }));
                    inline.push(None);
                } else {
                    r.bytes(size)?;
                    separate.push(None);
                    inline.push(Some((r.pos - size, size)));
                }
            }
            Some(())
        })()
        .is_some();
        let mut found = None;
        if ok {
            for (li, (sections, nv)) in headers.iter().enumerate() {
                let Some((o, n)) = inline[li] else { continue };
                let tris = section_tris(sections);
                if let Some(l) = find_lod_data(&r.b[..o + n], o, *nv, tris, sections.clone()) {
                    found = Some((li, l));
                    break;
                }
            }
        }
        if found.is_none() {
            // older search over the whole export (meshes whose headers did not parse)
            for (li, (sections, nv)) in headers.iter().enumerate() {
                if let Some(l) = find_lod_data(r.b, start, *nv, section_tris(sections), sections.clone()) {
                    found = Some((li, l));
                    break;
                }
            }
        }
        let n_headers = headers.len();
        let separate: Vec<_> = if ok {
            separate.into_iter().zip(headers).map(|(b, (s, nv))| b.map(|b| (b, s, nv))).collect()
        } else {
            Vec::new()
        };
        let (lod_index, lods) = match found {
            Some((li, l)) => (li, vec![l]),
            None if separate.iter().any(|s| s.is_some()) => (n_headers, Vec::new()),
            None => return None,
        };
        Some(StaticMesh { props, origin, extent, radius, lods, lod_index, separate })
    }
}

impl Package {
    /// Which traces a StaticMesh blocks by itself (bit 0 pawns = non-zero extent, bit 1 shots =
    /// zero extent), the rule of UStaticMeshComponent::LineCheck in HOMEFRONT.exe (0x583d50):
    /// unless the trace asks for complex collision (flag 0x20000), UseSimpleBoxCollision
    /// (UStaticMesh+0x11c) for a non-zero-extent check, or UseSimpleLineCollision (+0x118) for a
    /// zero-extent one, selects the simplified collision: first Homefront's collision mesh
    /// (`static_mesh_collision_model`, kDOP pointer array +0xcc / count +0xd0, 0x583de6), else
    /// the BodySetup (+0xd8, its AggGeom, 0x5841f2); without both the check returns "no hit"
    /// (-> 0x584821). Otherwise the per-poly kDOP tree of LOD LODForCollision (+0xbc[+0x7c]) is
    /// used. Defaults from the cooked data (only the non-default value is ever serialized:
    /// UseSimpleBoxCollision=0, UseSimpleLineCollision=1): Box TRUE, Line FALSE. So a mesh with
    /// neither collision mesh nor BodySetup (grass, weeds) does not block pawns but still stops
    /// shots; rocks / walls / decks have a collision mesh and block.
    pub fn static_mesh_collision_mask(&self, export_index: usize) -> u8 {
        let mut r = self.export_reader(export_index);
        let Some(props) = self.read_props(&mut r) else { return 3 };
        let box_simple = props.bool("UseSimpleBoxCollision").unwrap_or(true);
        let line_simple = props.bool("UseSimpleLineCollision").unwrap_or(false);
        // serialized after the properties: u32, FBoxSphereBounds (28 bytes), BodySetup
        let serial = (|| -> Option<i32> {
            r.u32()?;
            r.bytes(28)?;
            r.i32()
        })()
        .unwrap_or(0);
        let tagged = props.object("BodySetup").unwrap_or(0);
        let body = [serial, tagged].into_iter().find(|&b| b != 0);
        // a BodySetup counts when its AggGeom has an element (an export we can read; an
        // unreadable or imported one is taken as having geometry)
        let simple = body.is_some_and(|b| {
            if b < 0 {
                return true;
            }
            let Some(bp) = self.export_props(b as usize - 1) else { return true };
            let Some(ag) = self.struct_prop(&bp, "AggGeom") else { return false };
            ["SphereElems", "BoxElems", "SphylElems", "ConvexElems"].iter().any(|k| !self.struct_array(&ag, k).is_empty())
        });
        // a mesh whose layout we cannot decode keeps its pawn collision (NOT from data; none of
        // them is without BodySetup on Angel Island / Crossroads / Overpass)
        let simple = simple || self.static_mesh_collision_model_count(export_index).map_or(true, |n| n > 0);
        (if !box_simple || simple { 1 } else { 0 }) | (if !line_simple || simple { 2 } else { 0 })
    }
}

impl Package {
    /// Byte offset (in the export) right after the LOD bulk data of a StaticMesh.
    pub fn static_mesh_tail(&self, export_index: usize) -> Option<usize> {
        let mut r = self.export_reader(export_index);
        self.read_props(&mut r)?;
        r.u32()?;
        r.bytes(28)?;
        r.i32()?;
        r.i32()?;
        let n_names = r.i32()?;
        r.bytes(n_names.max(0) as usize * 8)?;
        let n_lods = r.i32()?;
        if !(1..=8).contains(&n_lods) {
            return None;
        }
        for _ in 0..n_lods {
            r.u32()?;
            r.u32()?;
            read_sections(&mut r)?;
            r.bytes(8 * 4)?;
        }
        for _ in 0..n_lods {
            let flags = r.u32()?;
            r.u32()?;
            let size = r.u32()? as usize;
            r.u32()?;
            if flags & crate::texture::BULK_SEPARATE_FILE == 0 {
                r.bytes(size)?;
            }
        }
        // the tail starts with the LOD count twice (Boulder8 3 3, Farm1_Floor 1 1); some meshes
        // (DEST_tire01, milCrate03, portaPotty_NK, indFence4, ...) have a LOD layout this does
        // not decode -> None
        let n = n_lods as u32;
        (dw(r.b, r.pos)? == n && dw(r.b, r.pos + 4)? == n).then_some(r.pos)
    }

    /// Per LOD, per section: the section's EnableCollision flag (FStaticMeshElement, serialized
    /// right after the material: EnableCollision, OldEnableCollision, bEnableShadowCasting). UE3
    /// builds the per-poly collision (kDOP) only from sections with EnableCollision; a section
    /// without it (e.g. the translucent edge strips of the road splines) blocks nothing.
    pub fn static_mesh_section_collision(&self, export_index: usize) -> Option<Vec<Vec<bool>>> {
        let mut r = self.export_reader(export_index);
        self.read_props(&mut r)?;
        r.u32()?;
        r.bytes(28)?;
        r.i32()?;
        r.i32()?;
        let n_names = r.i32()?;
        r.bytes(n_names.max(0) as usize * 8)?;
        let n_lods = r.i32()?;
        if !(1..=8).contains(&n_lods) {
            return None;
        }
        let mut out = Vec::new();
        for _ in 0..n_lods {
            r.u32()?;
            r.u32()?;
            let n_sec = r.i32()?;
            if !(0..=256).contains(&n_sec) {
                return None;
            }
            let mut v = Vec::new();
            for _ in 0..n_sec {
                r.i32()?;
                let collide = r.u32()?;
                r.bytes(7 * 4)?;
                let n_frag = r.i32()?;
                r.bytes(n_frag.max(0) as usize * 8)?;
                v.push(collide != 0);
            }
            out.push(v);
            r.bytes(8 * 4)?;
        }
        Some(out)
    }

    /// The triangles pawns collide with when they come from the collision mesh (UseSimpleBox-
    /// Collision set, the default, and a decoded collision mesh; see
    /// `static_mesh_collision_mask`), UE units in mesh space. `None`: pawns use the per-poly
    /// triangles, the BodySetup, or nothing.
    pub fn static_mesh_pawn_collision(&self, export_index: usize) -> Option<Vec<[[f32; 3]; 3]>> {
        let mut r = self.export_reader(export_index);
        let props = self.read_props(&mut r)?;
        if !props.bool("UseSimpleBoxCollision").unwrap_or(true) {
            return None;
        }
        self.static_mesh_collision_model(export_index).filter(|t| !t.is_empty())
    }

    /// Number of collision models (see `static_mesh_collision_model`); `None` if the mesh's
    /// layout was not decoded.
    pub fn static_mesh_collision_model_count(&self, export_index: usize) -> Option<u32> {
        let t = self.static_mesh_tail(export_index)?;
        let n = dw(self.export_bytes(&self.exports[export_index]), t + 32)?;
        (n <= 8).then_some(n)
    }

    /// Homefront's simplified collision mesh of a StaticMesh: the first element of a second
    /// TIndirectArray<FStaticMeshRenderData> (UStaticMesh+0x48) with its own kDOP tree
    /// (pointer array UStaticMesh+0xcc, count +0xd0). HOMEFRONT.exe
    /// UStaticMeshComponent::LineCheck (0x583de6) tests that tree first whenever the
    /// UseSimple*Collision path is taken (0x583e12: tree = *(+0xcc)[0], 0x9d6230: positions from
    /// (*(+0x48))[0]+0x50, materials from its Elements +0xf8); only without it the BodySetup
    /// (+0xd8, AggGeom at +0x48, PhysMaterial +0xc4) is used, and without both nothing is hit.
    /// Serialized after the LOD bulk data and 8 dwords (ThumbnailAngle/-Distance among them):
    /// count, then per model a LOD header like the render LODs (sections, vertex count, ...),
    /// its position buffer (Stride 12, NumVertices, bulk array 12 x NumVertices), other
    /// streams, and the kDOP: nodes, then triangles (element size 8: v0, v1, v2, material, u16).
    /// Returns `None` when the mesh has no collision model (count 0, e.g. grass_Long_03), else
    /// UE-space triangles of model 0 (empty if its data could not be read).
    pub fn static_mesh_collision_model(&self, export_index: usize) -> Option<Vec<[[f32; 3]; 3]>> {
        if self.static_mesh_collision_model_count(export_index)? == 0 {
            return None;
        }
        let t = self.static_mesh_tail(export_index)?;
        let b = self.export_bytes(&self.exports[export_index]);
        let tris = (|| -> Option<Vec<[[f32; 3]; 3]>> {
            let mut r = Rd { b, pos: t + 36 };
            r.u32()?;
            r.u32()?;
            let sections = read_sections(&mut r)?;
            let nv = r.u32()?;
            let ntri = sections.iter().map(|s| s.num_triangles).sum::<u32>();
            // position buffer: Stride 12, NumVertices, then the bulk array (12, NumVertices)
            let mut o = r.pos;
            let pos_at = loop {
                if o + 16 > b.len() || o > r.pos + 256 {
                    return None;
                }
                if dw(b, o)? == 12 && dw(b, o + 4)? == nv && dw(b, o + 8)? == 12 && dw(b, o + 12)? == nv {
                    break o + 16;
                }
                o += 4;
            };
            let f = |o: usize| f32::from_bits(dw(b, o).unwrap_or(0));
            let pos: Vec<[f32; 3]> = (0..nv as usize).map(|k| [f(pos_at + 12 * k), f(pos_at + 12 * k + 4), f(pos_at + 12 * k + 8)]).collect();
            if pos_at + 12 * nv as usize > b.len() {
                return None;
            }
            // kDOP triangles: the last (8, ntri) array of the export
            let mut o = pos_at + 12 * nv as usize;
            let mut kt = None;
            while o + 8 + 8 * ntri as usize <= b.len() {
                if dw(b, o)? == 8 && dw(b, o + 4)? == ntri {
                    kt = Some(o + 8);
                }
                // byte steps: the streams before it are not all dword sized (index buffers)
                o += 1;
            }
            let kt = kt?;
            let h = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as usize;
            let mut out = Vec::new();
            for k in 0..ntri as usize {
                let e = kt + 8 * k;
                let (a, bb, c) = (h(e), h(e + 2), h(e + 4));
                if a >= pos.len() || bb >= pos.len() || c >= pos.len() {
                    return None;
                }
                out.push([pos[a], pos[bb], pos[c]]);
            }
            Some(out)
        })();
        Some(tris.unwrap_or_default())
    }
}

fn section_tris(sections: &[Section]) -> usize {
    sections.iter().map(|s| s.first_index as usize / 3 + s.num_triangles as usize).max().unwrap_or(0)
}

impl Package {
    /// Like `read_static_mesh`, but loads the most detailed LOD from the source package when
    /// the map only has it as separate bulk data (what the game renders up close).
    pub fn read_static_mesh_full(&self, export_index: usize, lib: &crate::library::Library) -> Option<StaticMesh> {
        let mut sm = self.read_static_mesh_any(export_index)?;
        for (li, sep) in sm.separate.iter().enumerate() {
            if li >= sm.lod_index {
                break;
            }
            let Some((bulk, sections, nv)) = sep else { continue };
            let Some(data) = lib.read_separate(&self.source_package(export_index), bulk) else { continue };
            if let Some(l) = find_lod_data(&data, 0, *nv, section_tris(sections), sections.clone()) {
                sm.lods = vec![l];
                sm.lod_index = li;
                break;
            }
        }
        (!sm.lods.is_empty()).then_some(sm)
    }
}

fn read_sections(r: &mut Rd) -> Option<Vec<Section>> {
    let n_sec = r.i32()?;
    if !(0..=256).contains(&n_sec) {
        return None;
    }
    let mut sections = Vec::new();
    for _ in 0..n_sec {
        let material = r.i32()?;
        let _collide = r.u32()?;
        let _old_collide = r.u32()?;
        let _shadow = r.u32()?;
        let first_index = r.u32()?;
        let num_triangles = r.u32()?;
        let min_vertex = r.u32()?;
        let max_vertex = r.u32()?;
        let _material_index = r.u32()?;
        let n_frag = r.i32()?;
        r.bytes(n_frag.max(0) as usize * 8)?;
        sections.push(Section { material, first_index, num_triangles, min_vertex, max_vertex });
    }
    Some(sections)
}

fn find_lod_data(b: &[u8], from: usize, nv: usize, tris: usize, sections: Vec<Section>) -> Option<MeshLod> {
    let mut o = from;
    while o + 24 <= b.len() {
        let h = |k: usize| dw(b, o + 4 * k).unwrap_or(u32::MAX) as usize;
        let (nt, st, n, fp, es, cnt) = (h(0), h(1), h(2), h(3), h(4), h(5));
        if nv > 0 && (1..=8).contains(&nt) && fp <= 1 && st == es && n == nv && cnt == nv && st >= 8 && st <= 64 {
            let uv_bytes = nt * if fp == 1 { 8 } else { 4 };
            let pos_at = o + 24 + st * nv;
            let ph = |k: usize| dw(b, pos_at + 4 * k).unwrap_or(u32::MAX) as usize;
            if uv_bytes + 8 <= st && ph(0) == 12 && ph(1) == nv && ph(2) == 12 && ph(3) == nv {
                return decode_streams(b, o, nt, st, fp == 1, nv, pos_at, tris, sections);
            }
        }
        o += 4;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn decode_streams(
    b: &[u8],
    uv_at: usize,
    num_tex: usize,
    stride: usize,
    full: bool,
    nv: usize,
    pos_at: usize,
    tris: usize,
    sections: Vec<Section>,
) -> Option<MeshLod> {
    let raw = b.get(uv_at + 24..uv_at + 24 + stride * nv)?;
    let uv_bytes = num_tex * if full { 8 } else { 4 };
    // Homefront vertex record: TangentX, TangentZ, [extra], UV sets at the end.
    let uv_off = stride - uv_bytes;
    let mut normals = Vec::with_capacity(nv);
    let mut tangent_x = Vec::with_capacity(nv);
    let mut tangent_z = Vec::with_capacity(nv);
    let mut uv_sets: Vec<Vec<[f32; 2]>> = vec![Vec::with_capacity(nv); num_tex.max(1)];
    for v in raw.chunks_exact(stride) {
        tangent_x.push([v[0], v[1], v[2], v[3]]);
        tangent_z.push([v[4], v[5], v[6], v[7]]);
        normals.push(unpack_normal(u32::from_le_bytes(v[4..8].try_into().unwrap())));
        for t in 0..num_tex {
            let u = &v[uv_off + t * if full { 8 } else { 4 }..];
            uv_sets[t].push(if full {
                [f32::from_le_bytes(u[0..4].try_into().unwrap()), f32::from_le_bytes(u[4..8].try_into().unwrap())]
            } else {
                [half(u16::from_le_bytes([u[0], u[1]])), half(u16::from_le_bytes([u[2], u[3]]))]
            });
        }
    }
    if num_tex == 0 {
        uv_sets[0] = vec![[0.0; 2]; nv];
    }
    let uvs = uv_sets[0].clone();
    let mut r = Rd::new(b);
    r.pos = pos_at + 16;
    let mut positions = Vec::with_capacity(nv);
    for _ in 0..nv {
        positions.push(r.vec3()?);
    }
    // Index buffer: (2, count) with count >= 3 * tris and all indices < nv.
    let mut o = r.pos;
    while o + 8 <= b.len() && o <= r.pos + 4096 {
        let (es, cnt) = (dw(b, o)?, dw(b, o + 4)? as usize);
        if es == 2 && cnt % 3 == 0 && cnt >= tris * 3 && o + 8 + cnt * 2 <= b.len() {
            let idx: Vec<u32> =
                b[o + 8..o + 8 + cnt * 2].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect();
            if idx.iter().all(|&i| (i as usize) < nv) {
                return Some(MeshLod { sections, positions, normals, uvs, tangent_x, tangent_z, uv_sets, indices: idx });
            }
        }
        o += 4;
    }
    None
}
