//! Level contents: static mesh placements and material -> diffuse texture lookup.

use crate::props::{Props, Rd, Value};
use crate::Package;

#[derive(Debug, Clone)]
pub struct RoadSplineMesh {
    pub mesh: i32,
    pub materials: Vec<i32>,
    pub location: [f32; 3],
    pub rotation: [i32; 3],
    pub start_pos: [f32; 3],
    pub start_tangent: [f32; 3],
    pub end_pos: [f32; 3],
    pub end_tangent: [f32; 3],
    pub start_scale: [f32; 2],
    pub end_scale: [f32; 2],
    pub start_roll: f32,
    pub end_roll: f32,
    pub material_scale: f32,
    /// RoadSpline.DisableCollision not set: the deformed mesh blocks (Crossroads RoadSpline_13,
    /// the road over the creek bridge west of the overpass, has DisableCollision + CollisionScaleX 0)
    pub collision: bool,
    /// collision width factor at the start / end of the segment (RoadSplineMeshComponent
    /// Start/EndCollisionScale, copies of the actors' CollisionScaleX at the joints; an unset one
    /// is the float default 0; neither set: the actor's CollisionScaleX, class default 1.0)
    pub collision_scale: [f32; 2],
}

/// A component's 2D light map (FLightMap2D of a StaticMeshComponent LOD, layout as found in
/// the cooked data, examples/lmscan + lmguids): LightMapType 2, u32, LightGuids[] (the static
/// point / spot lights baked into it), 3 x {ULightMapTexture2D, FVector ScaleVector},
/// CoordinateScale, CoordinateBias. Coefficients 0 / 1 = the HQ (directional) pair the game
/// samples with FDirectionalLightWithLightMapPolicy (LightMapTextures[0/1], LightMapScale[0/1]:
/// Angel_Geo_Communication / Maintenance), coefficient 2 = the LQ (simple) map of
/// FDirectionalLightWithSimpleLightMapTexturePolicy (LightMapTextures, LightMapScale: all other
/// light-mapped sublevels of Angel Island / Crossroads / Overpass). The textures are exports of
/// the sublevel itself (outer = the "<sublevel>_sl" package object).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightMap2D {
    /// LightMapTexture2D references per coefficient (0 = none)
    pub textures: [i32; 3],
    /// ScaleVectors = the shader's LightMapScale per coefficient
    pub scales: [[f32; 3]; 3],
    pub coord_scale: [f32; 2],
    pub coord_bias: [f32; 2],
}

impl LightMap2D {
    /// HQ pair present (FDirectionalLightWithLightMapPolicy), else the LQ map.
    pub fn directional(&self) -> bool {
        self.textures[0] != 0 && self.textures[1] != 0
    }
}

#[derive(Debug, Clone)]
pub struct Placement {
    /// StaticMesh object reference (>0 export + 1, <0 import, e.g. from the persistent level).
    pub mesh: i32,
    pub location: [f32; 3],
    /// UE rotator: pitch, yaw, roll in 65536ths of a turn.
    pub rotation: [i32; 3],
    pub scale: [f32; 3],
    /// Part of the skybox (render unlit, no fog/shadows).
    pub sky: bool,
    /// Per-section material overrides from the component (0 = use the mesh's own), LOD 0.
    pub materials: Vec<i32>,
    /// The same per mesh LOD (index = LOD that is rendered).
    pub lod_materials: Vec<Vec<i32>>,
    /// Skeletal placements whose component archetype lives in another package (an import,
    /// e.g. oilspill's DSO_Buffalo_Destroyed exported by FL-Crossroads): that import, the
    /// mesh then comes from its SkeletalMesh (mesh = 0). 0 otherwise.
    pub comp_import: i32,
    /// collision: bit 0 blocks pawns (CollideActors && BlockActors && BlockNonZeroExtent), bit 1
    /// blocks shots (CollideActors && BlockZeroExtent); missing flags are the StaticMeshActor
    /// component defaults (all true), the actor's bCollideActors also gates
    pub collide: u8,
    /// Per mesh LOD (index = LODData index of the component): the 2D light map, if baked
    /// (StaticMeshActor placements only; empty elsewhere).
    pub lightmaps: Vec<Option<LightMap2D>>,
    /// GCNudgeableCollectionActor entry (small physics prop the pawns push aside: no
    /// movement collision)
    pub nudgeable: bool,
}

#[derive(Debug, Clone)]
pub struct Sun {
    pub rotation: [i32; 3],
    pub brightness: f32,
    pub color: [u8; 4],
}

#[derive(Debug, Clone)]
pub struct SkyLight {
    pub brightness: f32,
    pub color: [u8; 4],
    pub lower_brightness: f32,
    pub lower_color: [u8; 4],
    /// Homefront: multiplier on the terrain grass colour used as lower sky colour.
    pub lower_terrain_brightness: f32,
    /// Homefront: height over which the grass-coloured lower sky fades out.
    pub terrain_falloff: f32,
    pub use_as_material_sky_light: bool,
    /// LightingChannels (LightComponent defaults + overrides): affects BSP/Static/Dynamic.
    pub affects_world: bool,
}

impl Package {
    /// Tagged properties of an export, merged over its archetype chain: UE3 serialises only
    /// the properties that differ from the archetype (e.g. a placed GCInteractiveSkelMeshActor
    /// and its components inherit mesh/materials from the prefab-like archetype in the package).
    pub fn export_props(&self, export_index: usize) -> Option<Props> {
        let mut r = self.export_reader(export_index);
        let mut own = self.read_props(&mut r)?;
        let mut arch = self.exports.get(export_index)?.archetype_index;
        for _ in 0..8 {
            if arch <= 0 || arch as usize - 1 == export_index {
                break;
            }
            let ai = arch as usize - 1;
            let mut ar = self.export_reader(ai);
            if let Some(ap) = self.read_props(&mut ar) {
                for p in ap.0 {
                    if !own.0.iter().any(|q| q.name == p.name && q.index == p.index) {
                        own.0.push(p);
                    }
                }
            }
            arch = self.exports[ai].archetype_index;
        }
        Some(own)
    }

    /// Tagged properties of an export alone (no archetype merge).
    pub fn export_own_props(&self, export_index: usize) -> Option<Props> {
        let mut r = self.export_reader(export_index);
        self.read_props(&mut r)
    }

    /// Elements of an ArrayProperty of object references.
    pub fn array_objects(&self, v: &Value) -> Vec<i32> {
        let Value::Raw { data, .. } = v else { return vec![] };
        let mut r = Rd::new(data);
        let n = r.i32().unwrap_or(0).max(0);
        (0..n).filter_map(|_| r.i32()).collect()
    }

    /// Elements of an ArrayProperty of structs (each a tagged property list).
    /// Per-section material overrides of a mesh component: Materials, else Homefront's
    /// lodOverrideMaterialContainers[0].lodOverrideElementContainers[k].pOverrideMaterial
    /// (e.g. the Overpass highway signs: the text variants M_HWoverPassSign0N_C).
    pub fn component_materials(&self, cp: &Props) -> Vec<i32> {
        self.component_lod_materials(cp).into_iter().next().unwrap_or_default()
    }

    /// Per mesh LOD: Materials, gaps filled from lodOverrideMaterialContainers[LOD]
    /// (at least one entry, LOD 0). The containers are indexed by the mesh LOD that is
    /// rendered (schoolBusShort01: LOD 0 = 6 metal/glass sections, LOD 1 = one _PM section).
    pub fn component_lod_materials(&self, cp: &Props) -> Vec<Vec<i32>> {
        let base: Vec<i32> = cp.get("Materials").map(|v| self.array_objects(v)).unwrap_or_default();
        let conts = cp.get("lodOverrideMaterialContainers").map(|v| self.array_structs(v)).unwrap_or_default();
        let mut out = Vec::new();
        for c in &conts {
            let mut m = base.clone();
            let els = c.get("lodOverrideElementContainers").map(|v| self.array_structs(v)).unwrap_or_default();
            for (k, e) in els.iter().enumerate() {
                let o = e.object("pOverrideMaterial").unwrap_or(0);
                if o == 0 {
                    continue;
                }
                if m.len() <= k {
                    m.resize(k + 1, 0);
                }
                if m[k] == 0 {
                    m[k] = o;
                }
            }
            out.push(m);
        }
        if out.is_empty() {
            out.push(base);
        }
        out
    }

    pub fn array_structs(&self, v: &Value) -> Vec<Props> {
        let Value::Raw { data, .. } = v else { return vec![] };
        let mut r = Rd::new(data);
        let n = r.i32().unwrap_or(0).max(0);
        let mut out = Vec::new();
        for _ in 0..n {
            match self.read_props(&mut r) {
                Some(p) => out.push(p),
                None => break,
            }
        }
        out
    }

    /// StaticMeshComponent native data after the tagged properties: LODData[] =
    /// {ShadowMaps[] (UShadowMap2D refs), ShadowVertexBuffers[] (UShadowMap1D refs), FLightMapRef}
    /// with the light map as `LightMap2D` describes (type 0 = none). Per LOD: the 2D light map.
    /// Stops (rest None) at anything else: shadow maps, 1D (vertex) light maps (type 1) - none of
    /// them occur in Angel Island / Crossroads / Overpass (examples/lmscan: all LODs decode).
    pub fn component_lightmaps(&self, comp_export: usize) -> Vec<Option<LightMap2D>> {
        let mut out = Vec::new();
        if comp_export >= self.exports.len() {
            return out;
        }
        let mut r = self.export_reader(comp_export);
        if self.read_props(&mut r).is_none() {
            return out;
        }
        let nlod = r.i32().unwrap_or(0);
        if !(0..=16).contains(&nlod) {
            return out;
        }
        for _ in 0..nlod {
            let lm = (|| -> Option<Option<LightMap2D>> {
                let ns = r.i32()?;
                let nv = r.i32()?;
                if ns != 0 || nv != 0 {
                    return None;
                }
                match r.i32()? {
                    0 => Some(None),
                    2 => {
                        let _ = r.u32()?;
                        let ng = r.i32()?;
                        if !(0..=4096).contains(&ng) {
                            return None;
                        }
                        r.bytes(ng as usize * 16)?;
                        let mut tex = [(0i32, [0.0f32; 3]); 3];
                        for t in tex.iter_mut() {
                            *t = (r.i32()?, r.vec3()?);
                        }
                        let cs = [r.f32()?, r.f32()?];
                        let cb = [r.f32()?, r.f32()?];
                        let l = LightMap2D { textures: tex.map(|t| t.0), scales: tex.map(|t| t.1), coord_scale: cs, coord_bias: cb };
                        Some((l.directional() || l.textures[2] != 0).then_some(l))
                    }
                    _ => None,
                }
            })();
            match lm {
                Some(l) => out.push(l),
                None => break,
            }
        }
        out
    }

    /// The level's DirectionalLight (sun), if any.
    pub fn sun(&self) -> Option<Sun> {
        for (i, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != "DirectionalLight" {
                continue;
            }
            let ap = self.export_props(i)?;
            let comp = ap.object("LightComponent").filter(|&c| c > 0)?;
            let cp = self.export_props(comp as usize - 1)?;
            return Some(Sun {
                rotation: ap.rotator("Rotation").unwrap_or([0; 3]),
                brightness: cp.float("Brightness").unwrap_or(1.0),
                color: cp.color("LightColor").unwrap_or([255; 4]),
            });
        }
        None
    }

    /// SkyLight components (ambient), excluding PS3-only ones.
    pub fn sky_lights(&self) -> Vec<SkyLight> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != "SkyLightComponent" {
                continue;
            }
            let Some(p) = self.export_props(i) else { continue };
            if matches!(p.get("bUseOnPS3Only"), Some(Value::Bool(true))) {
                continue;
            }
            // Default__LightComponent: bInitialized, BSP, Static, Dynamic, CompositeDynamic,
            // Character_RS, Character_WC = true (Engine.u); fields in the level override.
            let ch = p.get("LightingChannels").and_then(|v| self.struct_props_pub(v)).unwrap_or_default();
            let flag = |k: &str, d: bool| ch.bool(k).unwrap_or(d);
            let affects_world = flag("BSP", true) || flag("Static", true) || flag("Dynamic", true);
            out.push(SkyLight {
                brightness: p.float("Brightness").unwrap_or(1.0),
                color: p.color("LightColor").unwrap_or([255; 4]),
                lower_brightness: p.float("LowerBrightness").unwrap_or(0.0),
                lower_color: p.color("LowerColor").unwrap_or([0, 0, 0, 255]),
                lower_terrain_brightness: p.float("LowerTerrainBrightness").unwrap_or(0.0),
                terrain_falloff: p.float("TerrainLightingFalloffDistance").unwrap_or(0.0),
                use_as_material_sky_light: p.bool("bUseAsMaterialSkyLight").unwrap_or(false),
                affects_world,
            });
        }
        out
    }

    /// All StaticMeshActor placements whose mesh is stored in this package.
    pub fn static_mesh_placements(&self) -> Vec<Placement> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            let class = self.export_class(e);
            let sky = class == "GCSkyboxActor";
            // GCNudgeableCollectionActor: small physics props (tires, crates, road signs) as
            // NudgeableEntries {Location, Rotation, NudgeableInfo -> GCNudgeableInfo.StaticMesh}
            if class == "GCNudgeableCollectionActor" {
                let Some(ap) = self.export_props(i) else { continue };
                for en in ap.get("NudgeableEntries").map(|v| self.array_structs(v)).unwrap_or_default() {
                    let Some(info) = en.object("NudgeableInfo").filter(|&o| o > 0) else { continue };
                    let Some(mesh) = self.export_props(info as usize - 1).and_then(|ip| ip.object("StaticMesh")).filter(|&m| m != 0) else { continue };
                    out.push(Placement {
                        mesh,
                        location: en.vector("Location").unwrap_or([0.0; 3]),
                        rotation: en.rotator("Rotation").unwrap_or([0; 3]),
                        scale: [1.0; 3],
                        sky: false,
                        materials: Vec::new(),
                        lod_materials: Vec::new(),
                        nudgeable: true,
                    comp_import: 0,
                    collide: 3,
                    lightmaps: Vec::new(),
                    });
                }
                continue;
            }
            // GCDestructibleStaticObject (placed ones in the PersistentLevel; the others are the
            // archetypes): mesh component is BaseStaticMesh (road signs, trash cans, ...)
            let destructible = class == "GCDestructibleStaticObject";
            if destructible && !self.object_path(i as i32 + 1).starts_with("TheWorld.PersistentLevel.") {
                continue;
            }
            // GCRotatingSMActor: windmill rotors etc. (Farm 31, Lowlands 9; StaticMeshComponent +
            // RotationSpeed): placed static, the spin (RotationSpeed) is not animated
            if !sky && !destructible && class != "StaticMeshActor" && class != "InterpActor" && class != "GCStaticMeshActor" && class != "GCRotatingSMActor" {
                continue;
            }
            let Some(ap) = self.export_props(i) else { continue };
            if matches!(ap.get("bHidden"), Some(Value::Bool(true))) {
                continue;
            }
            let Some(comp) = ap.object(if destructible { "BaseStaticMesh" } else { "StaticMeshComponent" }) else { continue };
            if comp <= 0 {
                continue;
            }
            let Some(cp) = self.export_props(comp as usize - 1) else { continue };
            // PrimitiveComponent.HiddenGame: not rendered in game (e.g. KFX_LevelDesign
            // WaterCollisionPlane under the Overpass river)
            if matches!(cp.get("HiddenGame"), Some(Value::Bool(true))) {
                continue;
            }
            let Some(mesh) = cp.object("StaticMesh") else { continue };
            if mesh == 0 {
                continue;
            }
            let ds = ap.float("DrawScale").unwrap_or(1.0);
            let s3 = ap.vector("DrawScale3D").unwrap_or([1.0; 3]);
            let flag = |p: &crate::props::Props, n: &str| !matches!(p.get(n), Some(Value::Bool(false)));
            let collide = if !flag(&cp, "CollideActors") || !flag(&ap, "bCollideActors") {
                0
            } else {
                (if flag(&cp, "BlockActors") && flag(&cp, "BlockNonZeroExtent") { 1 } else { 0 }) | (if flag(&cp, "BlockZeroExtent") { 2 } else { 0 })
            };
            out.push(Placement {
                mesh,
                location: ap.vector("Location").unwrap_or([0.0; 3]),
                rotation: ap.rotator("Rotation").unwrap_or([0; 3]),
                scale: [s3[0] * ds, s3[1] * ds, s3[2] * ds],
                sky,
                materials: self.component_materials(&cp),
                lod_materials: self.component_lod_materials(&cp),
                nudgeable: false,
                comp_import: 0,
                collide,
                lightmaps: self.component_lightmaps(comp as usize - 1),
            });
        }
        out
    }

    /// Static mesh placements whose component has a Homefront MinimapRenderMode (MRM_Terrain,
    /// MRM_Impassable, MRM_Road): drawn into the HUD minimap's scene capture. HiddenGame
    /// components are included (the *_minimap sublevel's terrain plane is hidden in game).
    pub fn minimap_placements(&self) -> Vec<(Placement, String)> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            let class = self.export_class(e);
            if class != "StaticMeshActor" && class != "InterpActor" && class != "GCStaticMeshActor" {
                continue;
            }
            let Some(ap) = self.export_props(i) else { continue };
            if matches!(ap.get("bHidden"), Some(Value::Bool(true))) {
                continue;
            }
            let Some(comp) = ap.object("StaticMeshComponent").filter(|&c| c > 0) else { continue };
            let Some(cp) = self.export_props(comp as usize - 1) else { continue };
            let Some(mode) = cp.enum_name("MinimapRenderMode").map(String::from) else { continue };
            let Some(mesh) = cp.object("StaticMesh").filter(|&m| m != 0) else { continue };
            let ds = ap.float("DrawScale").unwrap_or(1.0);
            let s3 = ap.vector("DrawScale3D").unwrap_or([1.0; 3]);
            out.push((
                Placement {
                    mesh,
                    location: ap.vector("Location").unwrap_or([0.0; 3]),
                    rotation: ap.rotator("Rotation").unwrap_or([0; 3]),
                    scale: [s3[0] * ds, s3[1] * ds, s3[2] * ds],
                    sky: false,
                    materials: self.component_materials(&cp),
                    lod_materials: self.component_lod_materials(&cp),
                    nudgeable: false,
                    comp_import: 0,
                    collide: 3,
                    lightmaps: Vec::new(),
                },
                mode,
            ));
        }
        out
    }

    /// Skeletal mesh components of placed actors (GCInteractiveSkelMeshActor etc.), with
    /// the actor transform. Mesh/material references may be imports (resolved by the caller).
    pub fn skeletal_mesh_placements(&self) -> Vec<Placement> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            if !self.object_path(i as i32 + 1).starts_with("TheWorld.PersistentLevel.") {
                continue;
            }
            let class = self.export_class(e);
            if !class.contains("Actor") {
                continue;
            }
            let Some(ap) = self.export_props(i) else { continue };
            if matches!(ap.get("bHidden"), Some(Value::Bool(true))) {
                continue;
            }
            let Some(comp) = ap.object("SkeletalMeshComponent").filter(|&c| c > 0) else { continue };
            let Some(cp) = self.export_props(comp as usize - 1) else { continue };
            if matches!(cp.get("HiddenGame"), Some(Value::Bool(true))) {
                continue;
            }
            let mut comp_import = 0;
            let mesh = match cp.object("SkeletalMesh").filter(|&m| m != 0) {
                Some(m) => m,
                None => {
                    let mut arch = self.exports[comp as usize - 1].archetype_index;
                    for _ in 0..8 {
                        if arch <= 0 {
                            break;
                        }
                        arch = self.exports[arch as usize - 1].archetype_index;
                    }
                    if arch >= 0 || self.object_path(arch).contains("Default__") {
                        continue;
                    }
                    comp_import = arch;
                    0
                }
            };
            let ds = ap.float("DrawScale").unwrap_or(1.0);
            let s3 = ap.vector("DrawScale3D").unwrap_or([1.0; 3]);
            out.push(Placement {
                mesh,
                location: ap.vector("Location").unwrap_or([0.0; 3]),
                rotation: ap.rotator("Rotation").unwrap_or([0; 3]),
                scale: [s3[0] * ds, s3[1] * ds, s3[2] * ds],
                sky: false,
                materials: self.component_materials(&cp),
                lod_materials: self.component_lod_materials(&cp),
                nudgeable: false,
                comp_import,
                collide: 3,
                lightmaps: Vec::new(),
            });
        }
        out
    }

    /// RoadSplineMeshComponents (Homefront road splines): mesh deformed along a Hermite
    /// segment by FSplineMeshVertexFactory. Location/rotation are the owning RoadSpline's.
    pub fn road_spline_meshes(&self) -> Vec<RoadSplineMesh> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != "RoadSpline" {
                continue;
            }
            // placed ones only: Angel_Ter_Terrain also holds the Prefab template
            // Roads.Intersections.TJunction_1`0x (its arcs sat at the world origin)
            if !self.object_path(i as i32 + 1).starts_with("TheWorld.PersistentLevel.") {
                continue;
            }
            let Some(ap) = self.export_props(i) else { continue };
            // HiddenGame (actor and PrimitiveComponent): not drawn in game. Angel Island's 31
            // M_DirtRoad splines (RoadTexture / TerrainOverlay = EngineMaterials.DefaultDiffuse,
            // the magenta checker) are all HiddenGame + DisableCollision.
            if matches!(ap.get("HiddenGame"), Some(Value::Bool(true))) {
                continue;
            }
            let comps = ap.get("SplineMeshComps").map(|v| self.array_objects(v)).unwrap_or_default();
            for c in comps.into_iter().filter(|&c| c > 0) {
                let Some(cp) = self.export_props(c as usize - 1) else { continue };
                if matches!(cp.get("HiddenGame"), Some(Value::Bool(true))) {
                    continue;
                }
                let Some(mesh) = cp.object("StaticMesh").filter(|&m| m != 0) else { continue };
                let Some(sp) = cp.get("SplineParams").and_then(|v| self.struct_props(v)) else { continue };
                let v2 = |name: &str| -> [f32; 2] {
                    match sp.get(name) {
                        Some(Value::Raw { data, .. }) if data.len() >= 8 => {
                            [f32::from_le_bytes(data[0..4].try_into().unwrap()), f32::from_le_bytes(data[4..8].try_into().unwrap())]
                        }
                        _ => [1.0, 1.0],
                    }
                };
                out.push(RoadSplineMesh {
                    mesh,
                    materials: self.component_materials(&cp),
                    location: ap.vector("Location").unwrap_or([0.0; 3]),
                    rotation: ap.rotator("Rotation").unwrap_or([0; 3]),
                    start_pos: sp.vector("StartPos").unwrap_or([0.0; 3]),
                    start_tangent: sp.vector("StartTangent").unwrap_or([0.0; 3]),
                    end_pos: sp.vector("EndPos").unwrap_or([0.0; 3]),
                    end_tangent: sp.vector("EndTangent").unwrap_or([0.0; 3]),
                    start_scale: v2("StartScale"),
                    end_scale: v2("EndScale"),
                    start_roll: sp.float("StartRoll").unwrap_or(0.0),
                    end_roll: sp.float("EndRoll").unwrap_or(0.0),
                    material_scale: sp.float("MaterialScale").unwrap_or(1.0),
                    collision: !ap.bool("DisableCollision").unwrap_or(false),
                    collision_scale: match (cp.float("StartCollisionScale"), cp.float("EndCollisionScale")) {
                        (None, None) => [ap.float("CollisionScaleX").unwrap_or(1.0); 2],
                        (s, e) => [s.unwrap_or(0.0), e.unwrap_or(0.0)],
                    },
                });
            }
        }
        out
    }

    /// FluidSurfaceActors: (material, width, height, location, rotation, scale).
    pub fn fluid_surfaces(&self) -> Vec<(i32, f32, f32, [f32; 3], [i32; 3], [f32; 3])> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != "FluidSurfaceActor" {
                continue;
            }
            let Some(ap) = self.export_props(i) else { continue };
            let Some(c) = ap.object("FluidComponent").filter(|&c| c > 0) else { continue };
            let Some(cp) = self.export_props(c as usize - 1) else { continue };
            let Some(m) = cp.object("FluidMaterial").filter(|&m| m != 0) else { continue };
            let ds = ap.float("DrawScale").unwrap_or(1.0);
            let s3 = ap.vector("DrawScale3D").unwrap_or([1.0; 3]);
            out.push((
                m,
                cp.float("FluidWidth").unwrap_or(1000.0),
                cp.float("FluidHeight").unwrap_or(1000.0),
                ap.vector("Location").unwrap_or([0.0; 3]),
                ap.rotator("Rotation").unwrap_or([0; 3]),
                [s3[0] * ds, s3[1] * ds, s3[2] * ds],
            ));
        }
        out
    }

    /// Diffuse texture (export index) of a Material / MaterialInstanceConstant, found by
    /// walking the material graph from the DiffuseColor input to its texture samples and
    /// applying the instance's texture parameter overrides. Falls back to name heuristics.
    pub fn diffuse_texture(&self, material: i32) -> Option<usize> {
        self.diffuse_from_graph(material).or_else(|| self.diffuse_by_name(material))
    }

    fn struct_props(&self, v: &Value) -> Option<Props> {
        let Value::Raw { data, .. } = v else { return None };
        self.read_props(&mut Rd::new(data))
    }

    fn diffuse_from_graph(&self, material: i32) -> Option<usize> {
        // Collect texture-parameter overrides from the instance chain (child first wins)
        // and find the base Material.
        let mut overrides: Vec<(String, i32)> = Vec::new();
        let mut cur = material;
        let mut base = None;
        for _ in 0..8 {
            if cur <= 0 {
                break;
            }
            let idx = cur as usize - 1;
            if self.export_class(&self.exports[idx]) == "Material" {
                base = Some(idx);
                break;
            }
            let props = self.export_props(idx)?;
            if let Some(v) = props.get("TextureParameterValues") {
                for p in self.array_structs(v) {
                    if let (Some(n), Some(t)) = (p.enum_name("ParameterName"), p.object("ParameterValue")) {
                        overrides.push((n.to_string(), t));
                    }
                }
            }
            cur = props.object("Parent").unwrap_or(0);
        }
        let base = self.export_props(base?)?;
        let root = self.struct_props(base.get("DiffuseColor")?)?.object("Expression")?;
        let mut samples = Vec::new();
        self.collect_samples(root, 0, false, &mut samples);
        let is_tex = |o: i32| o > 0 && self.export_class(&self.exports[o as usize - 1]) == "Texture2D";
        // 1) a sample whose parameter the instance overrides (not only used as a mask)
        for masked_only in [false, true] {
            for (param, default, via_alpha) in &samples {
                if *via_alpha != masked_only {
                    continue;
                }
                if let Some(pn) = param {
                    if let Some((_, t)) = overrides.iter().find(|(n, _)| n == pn) {
                        if is_tex(*t) {
                            return Some(*t as usize - 1);
                        }
                    }
                }
                if let Some(t) = default {
                    if is_tex(*t) && param.as_ref().map_or(true, |pn| !overrides.iter().any(|(n, _)| n == pn)) {
                        return Some(*t as usize - 1);
                    }
                }
            }
        }
        None
    }

    /// Depth-first walk over material expressions, recording texture samples as
    /// (parameter name, default texture, reached-only-through-an-alpha/mask-input).
    fn collect_samples(&self, expr: i32, depth: usize, via_alpha: bool, out: &mut Vec<(Option<String>, Option<i32>, bool)>) {
        if expr <= 0 || depth > 24 || out.len() > 32 {
            return;
        }
        let idx = expr as usize - 1;
        let class = self.export_class(&self.exports[idx]);
        let Some(p) = self.export_props(idx) else { return };
        if class.starts_with("MaterialExpressionTextureSample") {
            out.push((p.enum_name("ParameterName").map(|s| s.to_string()), p.object("Texture"), via_alpha));
            return;
        }
        // Static switches: follow the default branch first.
        let mut inputs: Vec<(&str, i32)> = Vec::new();
        for x in &p.0 {
            if let Some(sp) = self.struct_props(&x.value) {
                if let Some(e) = sp.object("Expression") {
                    inputs.push((x.name.as_str(), e));
                }
            }
        }
        if class == "MaterialExpressionStaticSwitchParameter" && !matches!(p.get("DefaultValue"), Some(Value::Bool(true))) {
            inputs.sort_by_key(|(n, _)| if *n == "B" { 0 } else { 1 });
        }
        for (name, e) in inputs {
            let skip = matches!(name, "Coordinates" | "Coordinate" | "UVs" | "Time" | "Normal");
            if skip {
                continue;
            }
            let alpha = via_alpha || matches!(name, "Alpha" | "Mask");
            self.collect_samples(e, depth + 1, alpha, out);
        }
    }

    fn diffuse_by_name(&self, material: i32) -> Option<usize> {
        let mut cur = material;
        for _ in 0..8 {
            if cur <= 0 {
                return None;
            }
            let idx = cur as usize - 1;
            let props = self.export_props(idx)?;
            let is_tex = |o: i32| o > 0 && self.export_class(&self.exports[o as usize - 1]) == "Texture2D";
            let mut cands: Vec<(i32, String)> = Vec::new();
            if let Some(v) = props.get("TextureParameterValues") {
                for p in self.array_structs(v) {
                    let pname = p.enum_name("ParameterName").unwrap_or("").to_ascii_lowercase();
                    if let Some(t) = p.object("ParameterValue") {
                        if is_tex(t) {
                            cands.push((t, pname));
                        }
                    }
                }
            }
            if let Some(v) = props.get("ReferencedTextures") {
                for t in self.array_objects(v) {
                    if is_tex(t) {
                        cands.push((t, String::new()));
                    }
                }
            }
            let score = |(t, pname): &(i32, String)| -> i32 {
                let n = self.object_name(*t).to_ascii_lowercase();
                let mut s = 0;
                if pname.contains("diffuse") || pname.contains("color") || pname.contains("base") {
                    s += 4;
                }
                if n.ends_with("_c") || n.ends_with("_d") || n.contains("diffuse") {
                    s += 3;
                }
                if n.ends_with("_n") || n.ends_with("_s") || pname.contains("normal") || pname.contains("spec") {
                    s -= 5;
                }
                if n.contains("mask") || n.contains("noise") || n.contains("lightmap") {
                    s -= 4;
                }
                s
            };
            if let Some(best) = cands.iter().max_by_key(|c| score(c)) {
                if score(best) > -5 {
                    return Some(best.0 as usize - 1);
                }
            }
            cur = props.object("Parent").unwrap_or(0);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    Masked,
    Translucent,
    Additive,
    Modulate,
}

#[derive(Debug, Clone)]
pub struct MaterialInfo {
    pub diffuse: Option<usize>,
    pub blend: Blend,
    pub unlit: bool,
    pub two_sided: bool,
    /// Alpha threshold for masked materials (Material.OpacityMaskClipValue, engine default 1/3).
    pub mask_clip: f32,
}

impl Package {
    /// Diffuse texture plus render state, inherited along the Parent chain
    /// (instances usually only override parameters; the base Material holds BlendMode).
    pub fn material_info(&self, material: i32) -> MaterialInfo {
        let mut info = MaterialInfo { diffuse: self.diffuse_texture(material), blend: Blend::Opaque, unlit: false, two_sided: false, mask_clip: 1.0 / 3.0 };
        let mut cur = material;
        for _ in 0..8 {
            if cur <= 0 {
                break;
            }
            let Some(p) = self.export_props(cur as usize - 1) else { break };
            if let Some(b) = p.enum_name("BlendMode") {
                info.blend = match b {
                    "BLEND_Masked" => Blend::Masked,
                    "BLEND_Translucent" => Blend::Translucent,
                    "BLEND_Additive" => Blend::Additive,
                    "BLEND_Modulate" | "BLEND_ModulateAndAdd" => Blend::Modulate,
                    _ => Blend::Opaque,
                };
            }
            if matches!(p.get("bIsMasked"), Some(Value::Bool(true))) && info.blend == Blend::Opaque {
                info.blend = Blend::Masked;
            }
            if p.enum_name("LightingModel") == Some("MLM_Unlit") {
                info.unlit = true;
            }
            if matches!(p.get("TwoSided"), Some(Value::Bool(true))) {
                info.two_sided = true;
            }
            if let Some(c) = p.float("OpacityMaskClipValue") {
                info.mask_clip = c;
            }
            cur = p.object("Parent").unwrap_or(0);
        }
        info
    }
}

impl Package {
    /// Scene_Desaturation of the first PostProcessVolume's Settings, if present.
    pub fn post_process_desaturation(&self) -> Option<f32> {
        for (i, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != "PostProcessVolume" {
                continue;
            }
            let p = self.export_props(i)?;
            let st = self.struct_props_pub(p.get("Settings")?)?;
            return st.float("Scene_Desaturation");
        }
        None
    }

    /// Post-process settings of the level: the PostProcessVolume with the highest
    /// Priority (Overpass: one volume, a +-24667 uu box around the whole map),
    /// falling back to WorldInfo.DefaultPostProcessSettings. Fields the level does
    /// not set keep the PostProcessSettings struct defaults from Engine.u.
    pub fn post_process_settings(&self) -> Option<PostProcess> {
        let mut best: Option<(f32, Props)> = None;
        for (_, prio, st) in self.post_process_volume_props() {
            if best.as_ref().map_or(true, |(bp, _)| prio > *bp) {
                best = Some((prio, st));
            }
        }
        if best.is_none() {
            for (i, e) in self.exports.iter().enumerate() {
                if self.export_class(e) != "WorldInfo" {
                    continue;
                }
                let Some(p) = self.export_props(i) else { continue };
                if let Some(st) = p.get("DefaultPostProcessSettings").and_then(|v| self.struct_props_pub(v)) {
                    best = Some((f32::MIN, st));
                }
            }
        }
        let (_, s) = best?;
        Some(Self::post_process_from(&s))
    }

    /// Enabled PostProcessVolumes: (export index, Priority, Settings struct).
    fn post_process_volume_props(&self) -> Vec<(usize, f32, Props)> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != "PostProcessVolume" {
                continue;
            }
            let Some(p) = self.export_props(i) else { continue };
            if p.bool("bEnabled") == Some(false) {
                continue;
            }
            let prio = p.float("Priority").unwrap_or(0.0);
            let Some(st) = p.get("Settings").and_then(|v| self.struct_props_pub(v)) else { continue };
            out.push((i, prio, st));
        }
        out
    }

    /// Every enabled PostProcessVolume with its Priority and its brush's world bounds, so the
    /// renderer can use the one the camera is in, like UE3 (highest Priority among the volumes
    /// that encompass the view point). Bridge: PostProcessVolume_1 (Priority 1, blue tint) is the
    /// box below z -1196 (under the water), PostProcessVolume_2 (Priority 0) the rest.
    pub fn post_process_volumes(&self) -> Vec<PpVolume> {
        let brushes = self.brush_volumes("PostProcessVolume");
        let mut out = Vec::new();
        for (i, prio, st) in self.post_process_volume_props() {
            let name = self.object_path(i as i32 + 1);
            let short = name.rsplit('.').next().unwrap_or(&name).to_string();
            let Some(b) = brushes.iter().find(|b| b.export == i) else { continue };
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for q in b.polys.iter().flatten() {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            if lo[0] > hi[0] {
                continue;
            }
            out.push(PpVolume { name: short, priority: prio, min: lo, max: hi, settings: Self::post_process_from(&st) });
        }
        out
    }

    fn post_process_from(s: &Props) -> PostProcess {
        let d = PostProcess::default();
        let lc = |n: &str, dv: [f32; 4]| s.linear_color(n).unwrap_or(dv);
        let f = |n: &str, dv: f32| s.float(n).unwrap_or(dv);
        PostProcess {
            tint_shadow: lc("TintShadowColor", d.tint_shadow),
            tint_mid: lc("TintMidToneColor", d.tint_mid),
            tint_high: lc("TintHighlightColor", d.tint_high),
            bloom_scale: f("Bloom_Scale", d.bloom_scale),
            bloom_kernel: [f("Bloom_BlurKernelSizeX", d.bloom_kernel[0]), f("Bloom_BlurKernelSizeY", d.bloom_kernel[1])],
            bloom_step: [f("Bloom_BlurStepSizeX", d.bloom_step[0]), f("Bloom_BlurStepSizeY", d.bloom_step[1])],
            desaturation: f("Scene_Desaturation", d.desaturation),
            tint_shadow_cutoff: f("TintShadowCutoff", d.tint_shadow_cutoff),
            tint_highlight_cutoff: f("TintHighlightCutoff", d.tint_highlight_cutoff),
            lum_center: f("HDRLumCenter", d.lum_center),
            lum_time: f("HDRLumTime", d.lum_time),
            lum_factor: f("HDRLumFactor", d.lum_factor),
            lum_low: f("HDRLumLowClamp", d.lum_low),
            lum_high: f("HDRLumHighClamp", d.lum_high),
            gradient_scale: f("GradientScale", d.gradient_scale),
            gradient_offset: f("GradientOffset", d.gradient_offset),
            enable_bloom: s.bool("bEnableBloom").unwrap_or(d.enable_bloom),
            enable_gradient: s.bool("bEnableGradient").unwrap_or(d.enable_gradient),
        }
    }

    pub fn struct_props_pub(&self, v: &Value) -> Option<Props> {
        let Value::Raw { data, .. } = v else { return None };
        self.read_props(&mut Rd::new(data))
    }
}

/// A PostProcessVolume: its Priority, world-space bounds of its brush and its settings.
#[derive(Debug, Clone)]
pub struct PpVolume {
    pub name: String,
    pub priority: f32,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub settings: PostProcess,
}

/// Homefront's PostProcessSettings (the subset the uber post-process uses).
#[derive(Debug, Clone)]
pub struct PostProcess {
    pub tint_shadow: [f32; 4],
    pub tint_mid: [f32; 4],
    pub tint_high: [f32; 4],
    pub bloom_scale: f32,
    pub bloom_kernel: [f32; 2],
    pub bloom_step: [f32; 2],
    pub desaturation: f32,
    pub tint_shadow_cutoff: f32,
    pub tint_highlight_cutoff: f32,
    pub lum_center: f32,
    pub lum_time: f32,
    pub lum_factor: f32,
    pub lum_low: f32,
    pub lum_high: f32,
    pub gradient_scale: f32,
    pub gradient_offset: f32,
    pub enable_bloom: bool,
    pub enable_gradient: bool,
}

impl Default for PostProcess {
    /// Struct defaults of Engine.u PostProcessVolume.PostProcessSettings.
    fn default() -> Self {
        Self {
            tint_shadow: [0.0, 0.0, 0.0, 1.0],
            tint_mid: [1.0, 1.0, 1.0, 1.0],
            tint_high: [1.0, 1.0, 1.0, 1.0],
            bloom_scale: 1.0,
            bloom_kernel: [16.0, 16.0],
            bloom_step: [1.0, 1.0],
            desaturation: 0.0,
            tint_shadow_cutoff: 0.0,
            tint_highlight_cutoff: 1.0,
            lum_center: 0.35,
            lum_time: 1.0,
            lum_factor: 1.0,
            lum_low: 0.0,
            lum_high: 10.0,
            gradient_scale: 1.0,
            gradient_offset: 0.5,
            enable_bloom: true,
            enable_gradient: true,
        }
    }
}

/// Brush volume (Volume actor with a Polys export under its Model): world-space polygons.
#[derive(Debug, Clone)]
pub struct BrushVolume {
    pub export: usize,
    pub name: String,
    pub class: String,
    pub location: [f32; 3],
    /// polygons in world space (UE units)
    pub polys: Vec<Vec<[f32; 3]>>,
}

impl BrushVolume {
    /// Point inside the closed brush (ray parity along +X over the fan-triangulated polygons).
    pub fn contains(&self, p: [f32; 3]) -> bool {
        let mut n = 0;
        for poly in &self.polys {
            for k in 1..poly.len().saturating_sub(1) {
                let (a, b, c) = (poly[0], poly[k], poly[k + 1]);
                // intersect ray p + t*(1,0,0), t > 0 with triangle (in the YZ projection)
                let (ay, az, by, bz, cy, cz) = (a[1] - p[1], a[2] - p[2], b[1] - p[1], b[2] - p[2], c[1] - p[1], c[2] - p[2]);
                let d = (by - ay) * (cz - az) - (cy - ay) * (bz - az);
                if d.abs() < 1e-9 {
                    continue;
                }
                let u = (-ay * (cz - az) + az * (cy - ay)) / d;
                let v = (ay * (bz - az) - az * (by - ay)) / d;
                // barycentric weights of (0,0) in the YZ triangle
                let (wb, wc) = (u, v);
                let wa = 1.0 - wb - wc;
                if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                    continue;
                }
                let x = wa * a[0] + wb * b[0] + wc * c[0];
                if x > p[0] {
                    n += 1;
                }
            }
        }
        n % 2 == 1
    }
}

impl Package {
    /// Volumes of a class with their brush polygons (UPolys: owner ref, then FPoly records:
    /// Base, Normal, TextureU, TextureV, TArray<FVector> Vertices, 36 bytes of flags/refs;
    /// layout checked on Crossroads GameMode_FrontlineVolume_5.Model_7.Polys_14).
    pub fn brush_volumes(&self, class: &str) -> Vec<BrushVolume> {
        let mut out = Vec::new();
        for (i, e) in self.exports.iter().enumerate() {
            if self.export_class(e) != class {
                continue;
            }
            let path = self.object_path(i as i32 + 1);
            let Some(props) = self.export_props(i) else { continue };
            let loc = match props.get("Location") {
                Some(crate::props::Value::Vector(v)) => *v,
                _ => [0.0; 3],
            };
            let rot = match props.get("Rotation") {
                Some(crate::props::Value::Rotator(r)) => *r,
                _ => [0; 3],
            };
            let scale = match props.get("DrawScale3D") {
                Some(crate::props::Value::Vector(v)) => *v,
                _ => [1.0; 3],
            };
            let ds = match props.get("DrawScale") {
                Some(crate::props::Value::Float(f)) => *f,
                _ => 1.0,
            };
            let pre = match props.get("PrePivot") {
                Some(crate::props::Value::Vector(v)) => *v,
                _ => [0.0; 3],
            };
            let prefix = format!("{path}.");
            let Some(pi) = self.exports.iter().enumerate().position(|(j, x)| self.export_class(x) == "Polys" && self.object_path(j as i32 + 1).starts_with(&prefix)) else { continue };
            let b = self.export_bytes(&self.exports[pi]);
            let rd_i = |o: usize| b.get(o..o + 4).map(|x| i32::from_le_bytes(x.try_into().unwrap()));
            let rd_f = |o: usize| b.get(o..o + 4).map(|x| f32::from_le_bytes(x.try_into().unwrap()));
            // header: None name (8 bytes) + 4, DbNum, DbMax, owner ref
            let Some(n) = rd_i(12) else { continue };
            let mut p = 24usize;
            let rad = |u: i32| u as f32 / 65536.0 * std::f32::consts::TAU;
            let (sp, cp) = rad(rot[0]).sin_cos();
            let (sy, cy) = rad(rot[1]).sin_cos();
            let (sr, cr) = rad(rot[2]).sin_cos();
            // UE3 FRotationMatrix
            let ax = [cp * cy, cp * sy, sp];
            let ay = [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp];
            let az = [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp];
            let mut polys = Vec::new();
            for _ in 0..n.max(0) {
                let Some(nv) = rd_i(p + 48) else { break };
                if !(0..=256).contains(&nv) {
                    break;
                }
                let mut v = Vec::new();
                for k in 0..nv as usize {
                    let o = p + 52 + k * 12;
                    let (Some(x), Some(y), Some(z)) = (rd_f(o), rd_f(o + 4), rd_f(o + 8)) else { break };
                    let l = [(x - pre[0]) * scale[0] * ds, (y - pre[1]) * scale[1] * ds, (z - pre[2]) * scale[2] * ds];
                    v.push([
                        loc[0] + l[0] * ax[0] + l[1] * ay[0] + l[2] * az[0],
                        loc[1] + l[0] * ax[1] + l[1] * ay[1] + l[2] * az[1],
                        loc[2] + l[0] * ax[2] + l[1] * ay[2] + l[2] * az[2],
                    ]);
                }
                polys.push(v);
                p += 52 + nv as usize * 12 + 36;
            }
            out.push(BrushVolume { export: i, name: self.object_name_of(e).to_string(), class: class.to_string(), location: loc, polys });
        }
        out
    }
}
