//! SkeletalMesh decoding for Homefront (UE3 v576 / licensee 100), reference pose only.
//!
//! Layout as observed (VEH_Jiggle.cars.SkeletalMeshes.DEST_Semi_Cab, Overpass_gasstation):
//! properties | FBoxSphereBounds (28) | TArray<Material> | Origin (vec3) | RotOrigin (rotator) |
//! TArray<FMeshBone> (52 bytes: FName, flags, quat, pos, NumChildren, Parent, colour) |
//! SkeletalDepth | TArray<FStaticLODModel>; per LOD:
//!   TArray<Section> (10 bytes: u16 material, u16 chunk, u32 base index, u16 triangles) |
//!   index buffer (i32 element size, i32 count, data) | u32 | TArray<u16> active bones | u32 |
//!   chunks ... | NumVertices ... | GPU skin vertex buffer as a bulk array
//!   (i32 stride, i32 count, i32 stride, i32 count, data).
//! GPU skin vertex (stride 32 + 4 * NumTexCoords): position (vec3), TangentX, TangentZ
//! (packed UBYTE4), 4 bytes, influence bones, influence weights, half2 UV per set.
//! Positions are in mesh space for the reference pose.

use crate::mesh::{MeshLod, Section};
use crate::props::Props;
use crate::Package;

#[derive(Debug, Clone)]
pub struct SkeletalMesh {
    pub props: Props,
    pub materials: Vec<i32>,
    pub origin: [f32; 3],
    pub rot_origin: [i32; 3],
    pub lod: MeshLod,
    pub bones: Vec<Bone>,
    /// per GPU vertex: mesh bone indices and weights (0..255), from the chunk BoneMaps
    pub influences: Vec<([u16; 4], [u8; 4])>,
    /// LODInfo[0].LODMaterialMap: section MaterialIndex -> `materials` index. Applied at LOD 0
    /// too: SKV_US_M1A3 [0,5,2,1,2,3,4,6] puts the class's tread slots 2/3 (GCTreadedVehicle
    /// Left/RightTreadMaterialIndex) on the two V-tiled tread strips (sections 4/5);
    /// SKW_ACH_Acog_base [1,0] puts the body material on the 528-triangle body.
    pub lod0_material_map: Vec<i32>,
}

impl SkeletalMesh {
    /// Material reference of a LOD-0 section (through LODMaterialMap).
    pub fn section_material(&self, section_material: i32) -> i32 {
        let k = self.lod0_material_map.get(section_material as usize).copied().unwrap_or(section_material);
        self.materials.get(k as usize).copied().unwrap_or(0)
    }
}

/// FMeshBone: reference pose relative to the parent (UE3 convention: the stored orientation is
/// the inverse rotation for non-root bones, see anim.rs).
#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub rot: [f32; 4],
    pub pos: [f32; 3],
    pub parent: i32,
}

/// An entry of a GC SkeletalMeshSocket's References array.
#[derive(Debug, Clone)]
pub struct SocketEffect {
    pub start_tag: String,
    pub end_tag: String,
    /// object path (KFX_VHC.FX_LAV.FX_LAV_MF) and its class (ParticleSystem, lens flares, ...)
    pub reference: String,
    pub class: String,
    /// VIEW_All / VIEW_1P / VIEW_3P
    pub view: String,
    pub end_time: f32,
}

impl Package {
    pub fn read_skeletal_mesh(&self, export_index: usize) -> Option<SkeletalMesh> {
        let mut r = self.export_reader(export_index);
        let props = self.read_props(&mut r)?;
        r.bytes(28)?;
        let nm = r.i32()?;
        if !(0..256).contains(&nm) {
            return None;
        }
        let materials: Vec<i32> = (0..nm).map(|_| r.i32()).collect::<Option<_>>()?;
        let origin = r.vec3()?;
        let rot_origin = [r.i32()?, r.i32()?, r.i32()?];
        let nb = r.i32()?;
        if !(0..4096).contains(&nb) {
            return None;
        }
        let mut bones = Vec::with_capacity(nb as usize);
        for _ in 0..nb {
            let b = r.bytes(52)?;
            let f = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            let i = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            bones.push(Bone {
                name: self.name(crate::FName { index: i(0), number: i(4) }),
                rot: [f(12), f(16), f(20), f(24)],
                pos: [f(28), f(32), f(36)],
                parent: i(44),
            });
        }
        let _depth = r.i32()?;
        let nl = r.i32()?;
        if nl < 1 {
            return None;
        }
        // LOD 0
        let ns = r.i32()?;
        if !(0..512).contains(&ns) {
            return None;
        }
        let mut sections = Vec::new();
        for _ in 0..ns {
            let material = r.u16()? as i32;
            let _chunk = r.u16()?;
            let first_index = r.u32()?;
            let num_triangles = r.u16()? as u32;
            sections.push(Section { material, first_index, num_triangles, min_vertex: 0, max_vertex: 0 });
        }
        let es = r.i32()?;
        let ni = r.i32()?;
        if !(es == 2 || es == 4) || ni < 0 {
            return None;
        }
        let data = r.bytes(es as usize * ni as usize)?;
        let indices: Vec<u32> = if es == 2 {
            data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect()
        } else {
            data.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
        };
        let max_index = indices.iter().copied().max().unwrap_or(0) as usize;
        // ShadowIndices (u16[]), ActiveBoneIndices (u16[]), ShadowTriangleDoubleSided (u8[]),
        // chunks: BaseVertexIndex, rigid vertices (38 bytes), soft vertices (45 bytes),
        // BoneMap (u16[]), NumRigid, NumSoft, MaxBoneInfluences (Weapons_FL2 / US_1P meshes)
        let mut bone_maps: Vec<(usize, usize, Vec<u16>)> = Vec::new();
        let after_indices = r.pos;
        let chunks_ok = (|| -> Option<()> {
            let n = r.i32()?;
            r.bytes(n.max(0) as usize * 2)?;
            let n = r.i32()?;
            r.bytes(n.max(0) as usize * 2)?;
            let n = r.i32()?;
            r.bytes(n.max(0) as usize)?;
            let nch = r.i32()?;
            if !(0..1024).contains(&nch) {
                return None;
            }
            for _ in 0..nch {
                let base = r.i32()? as usize;
                let nr = r.i32()?;
                r.bytes(nr.max(0) as usize * 38)?;
                let ns = r.i32()?;
                r.bytes(ns.max(0) as usize * 45)?;
                let nm = r.i32()?;
                let bm: Vec<u16> = (0..nm.max(0)).map(|_| r.u16()).collect::<Option<_>>()?;
                let (nr2, ns2, _max) = (r.i32()?, r.i32()?, r.i32()?);
                if nr2 != nr || ns2 != ns {
                    return None;
                }
                bone_maps.push((base, (nr + ns) as usize, bm));
            }
            Some(())
        })()
        .is_some();
        // GPU skin vertex buffer: (stride, count, stride, count) with count > max index.
        // A chunk layout we do not parse (FL-Crossroads Buffalo_Destroyed) can run past the
        // vertex buffer: then search again from the end of the index buffer, rigid (bone 0)
        // skinning (the partial chunk parse is kept otherwise: the weapon / arms meshes need it)
        let b = r.b;
        let mut o = r.pos;
        let mut retried = chunks_ok;
        let rd = |o: usize| b.get(o..o + 4).map(|s| i32::from_le_bytes(s.try_into().unwrap()));
        let (stride, count, start) = loop {
            if o + 16 > b.len() {
                if retried {
                    return None;
                }
                retried = true;
                bone_maps.clear();
                o = after_indices;
                continue;
            }
            let (s, n, s2, n2) = (rd(o)?, rd(o + 4)?, rd(o + 8)?, rd(o + 12)?);
            if s == s2 && n == n2 && (36..=64).contains(&s) && (s - 32) % 4 == 0 && n as usize > max_index && o + 16 + (s * n) as usize <= b.len() {
                break (s as usize, n as usize, o + 16);
            }
            o += 1;
        };
        let num_uv = (stride - 32) / 4;
        let mut influences = vec![([0u16; 4], [255u8, 0, 0, 0]); count];
        let mut lod = MeshLod { sections, indices, ..Default::default() };
        lod.uv_sets = vec![Vec::with_capacity(count); num_uv];
        for k in 0..count {
            let v = &b[start + k * stride..start + (k + 1) * stride];
            let f = |o: usize| f32::from_le_bytes(v[o..o + 4].try_into().unwrap());
            lod.positions.push([f(0), f(4), f(8)]);
            let tx = [v[12], v[13], v[14], v[15]];
            let tz = [v[16], v[17], v[18], v[19]];
            lod.tangent_x.push(tx);
            lod.tangent_z.push(tz);
            lod.normals.push([tz[0] as f32 / 127.5 - 1.0, tz[1] as f32 / 127.5 - 1.0, tz[2] as f32 / 127.5 - 1.0]);
            if let Some((_, _, bm)) = bone_maps.iter().find(|(base, n, _)| k >= *base && k < base + n) {
                let ib = [v[24], v[25], v[26], v[27]];
                let w = [v[28], v[29], v[30], v[31]];
                influences[k] = (ib.map(|i| bm.get(i as usize).copied().unwrap_or(0)), w);
            }
            for (u, set) in lod.uv_sets.iter_mut().enumerate() {
                let o = 32 + u * 4;
                let h = |o: usize| half::f16::from_bits(u16::from_le_bytes([v[o], v[o + 1]])).to_f32();
                set.push([h(o), h(o + 2)]);
            }
        }
        lod.uvs = lod.uv_sets.first().cloned().unwrap_or_default();
        let lod0_material_map = self
            .struct_array(&props, "LODInfo")
            .first()
            .and_then(|l| match l.get("LODMaterialMap") {
                Some(crate::props::Value::Raw { data, .. }) if data.len() >= 4 => Some(data[4..].chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()),
                _ => None,
            })
            .unwrap_or_default();
        Some(SkeletalMesh { props, materials, origin, rot_origin, lod, bones, influences, lod0_material_map })
    }
}

/// SkeletalMeshSocket (SocketName on BoneName with a relative transform, UE units / rotator).
#[derive(Debug, Clone)]
pub struct Socket {
    pub name: String,
    pub bone: String,
    pub loc: [f32; 3],
    pub rot: [i32; 3],
    pub effects: Vec<SocketEffect>,
}

impl Package {
    /// Sockets of a SkeletalMesh export (its "Sockets" array of SkeletalMeshSocket objects).
    pub fn mesh_sockets(&self, export_index: usize) -> Vec<Socket> {
        let mut out = Vec::new();
        let Some(pr) = self.export_props(export_index) else { return out };
        let Some(crate::props::Value::Raw { data, .. }) = pr.get("Sockets") else { return out };
        if data.len() < 4 {
            return out;
        }
        let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        for k in 0..n {
            let Some(b) = data.get(4 + k * 4..8 + k * 4) else { break };
            let o = i32::from_le_bytes(b.try_into().unwrap());
            if o <= 0 {
                continue;
            }
            let Some(sp) = self.export_props(o as usize - 1) else { continue };
            // GC socket References: effects started by vehicle event tags (StartTag, e.g. the
            // seat fire modes' FlashTags "Seat0FireMode0Fire")
            let effects = self
                .struct_array(&sp, "References")
                .iter()
                .filter_map(|r| {
                    let o = r.object("Reference").filter(|&o| o != 0)?;
                    Some(SocketEffect {
                        start_tag: r.enum_name("StartTag").unwrap_or("None").to_string(),
                        end_tag: r.enum_name("EndTag").unwrap_or("None").to_string(),
                        reference: self.object_path(o),
                        class: if o < 0 { self.import_class(o) } else { self.export_class(&self.exports[o as usize - 1]) },
                        view: r.enum_name("ValidView").unwrap_or("VIEW_All").to_string(),
                        end_time: r.float("EndTime").unwrap_or(0.0),
                    })
                })
                .collect();
            out.push(Socket {
                name: sp.enum_name("SocketName").unwrap_or("").to_string(),
                bone: sp.enum_name("BoneName").unwrap_or("").to_string(),
                loc: sp.vector("RelativeLocation").unwrap_or([0.0; 3]),
                rot: sp.rotator("RelativeRotation").unwrap_or([0; 3]),
                effects,
            });
        }
        out
    }
}
