//! First-person weapons and arms (CPU-skinned).
//!
//! Data (weaponcfg.rs): GCInfantryWeaponInfo CFG_* SKMesh (e.g. RIF_M4.SkeletalMeshes.SKW_RIF_M4_1P,
//! the weapon skeleton incl. both arms), MeshParts without bone (weapon body, e.g.
//! skw_rif_m4_1p_base, one "Root" bone) and the loadout's scope (GCWeaponScope Mesh / MeshBase,
//! placed at ScopesRelativeLocation) are rigid on the "Weapon" bone, whose reference pose is the
//! identity (bonecomp: SKW_RIF_M4_1P Weapon q (0,0,0,1) t 0). bHideFrontSight collapses the
//! vertices of the front_sight bone. CH_1P_AnimSet drives the skeleton; the arms
//! (US_1P.Meshes.SKC_MP_US_1P) follow by bone name (Default__GCChar_Player FirstPersonSkelMeshComp
//! ParentAnimComponent = WeapSkelMeshComp), both FOV 50, SDPG_Foreground.
//!
//! Bone convention: composing the mesh reference pose as stored (component = parent * local)
//! puts every bone within 1.3 units of the vertices bound to it (US_1P arms); the animation
//! tracks store the conjugate rotation (AS_RIF_M4_1P_WP frame 0 vs SKW_RIF_M4_1P), so they are
//! conjugated before use. Translations agree.
//!
//! Rendering note (bevy 0.16 on lavapipe): a newly visible material pipeline compiles
//! asynchronously, so the first-person meshes can appear some seconds after spawning there.
use crate::menu::{MatchFlow, Phase};
use crate::player::Pawn;
use crate::weaponcfg::WeaponCfg;
use bevy::prelude::*;
use std::collections::HashMap;
use upk::anim::AnimSetData;
use upk::skeletal::SkeletalMesh;

pub struct ViewModelPlugin;

impl Plugin for ViewModelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (update_viewmodel.after(crate::player::PlayerSet).after(crate::combat::CombatSet), mark_fp_muzzle));
        use bevy::core_pipeline::core_3d::Transparent3d;
        use bevy::render::{render_phase::sort_phase_system, ExtractSchedule, Render, RenderApp, RenderSet};
        if let Some(r) = app.get_sub_app_mut(RenderApp) {
            r.init_resource::<FpDraws>()
                .add_systems(ExtractSchedule, extract_fp_draws)
                .add_systems(Render, fp_sort_last.in_set(RenderSet::PhaseSort).before(sort_phase_system::<Transparent3d>));
        }
    }
}

/// First-person draw (UE3 SDPG_Foreground: the 1P weapon / arms / sight and their effects are
/// drawn after the whole world, translucency included). Inserted by world.rs spawn_mesh for every
/// mesh with the foreground flag (objects[].w = 1) and by `mark_fp_muzzle` for the 1P muzzle
/// flash. The value is the layer inside the foreground group (higher = drawn later).
///
/// Why: bevy sorts Transparent3d by the view depth of the entity's Transform translation. The 1P
/// meshes sit at the eye (depth ~0); a world translucent mesh whose translation lies BEHIND the
/// camera (road splines: the RoadSpline actor at the spline start, CPU-deformed up to ~100 m
/// along the road; Crossroads RoadSpline_5 M_Road_Translucent edge strip past the gas station)
/// sorted after the sight and painted over the holographic reticle (user 2026-10-21 21:59).
#[derive(Component, Clone, Copy)]
pub struct FirstPersonDraw(pub f32);

/// Sort distance of the foreground group: beyond every world item (bevy view z in metres; a
/// world translucent item behind the camera is at most a map size, < 2000 m). NOT from data.
const FP_SORT_BASE: f32 = 1.0e5;

/// Render world: main entities drawn in the foreground group, with their layer.
#[derive(Resource, Default)]
struct FpDraws(std::collections::HashMap<bevy::render::sync_world::MainEntity, f32>);

fn extract_fp_draws(mut fp: ResMut<FpDraws>, q: bevy::render::Extract<Query<(Entity, &FirstPersonDraw, &ViewVisibility)>>) {
    fp.0.clear();
    for (e, l, v) in &q {
        if v.get() {
            fp.0.insert(e.into(), l.0);
        }
    }
}

/// Before the transparent sort: every foreground item goes behind all world items in draw
/// order (back-to-front = ascending distance). Items of one layer keep their queue order, as
/// before (the 1P meshes all share the eye translation, so they tied already). NOT from data:
/// UE3 renders the foreground group as its own pass after the world; bevy has one pass.
fn fp_sort_last(fp: Res<FpDraws>, mut phases: ResMut<bevy::render::render_phase::ViewSortedRenderPhases<bevy::core_pipeline::core_3d::Transparent3d>>) {
    if fp.0.is_empty() {
        return;
    }
    for phase in phases.values_mut() {
        for it in phase.items.iter_mut() {
            if let Some(l) = fp.0.get(&it.entity.1) {
                it.distance = FP_SORT_BASE + l;
            }
        }
    }
}

/// The player's muzzle flash (WeaponEffectMuzzleFlash View1P / ViewADS templates, combat.rs) is
/// a component of the 1P weapon in UE3, so it is in the foreground group too: layer -1, i.e.
/// after the world but before the sight glass / reticle (layer 0) it is seen through. Pooled
/// effects sit at the world origin (particles.rs), so without this their order against world
/// translucency was arbitrary. Templates also used as a 3P flash (bots) stay world effects.
fn mark_fp_muzzle(
    mut commands: Commands,
    data: Option<Res<crate::combat::CombatData>>,
    fx: Res<crate::particles::Particles>,
    me: Res<crate::MeshEntities>,
    marked: Query<(), With<FirstPersonDraw>>,
    mut key: Local<(usize, usize)>,
) {
    let Some(data) = data else { return };
    let k = (fx.systems.len(), me.0.len());
    if *key == k {
        return;
    }
    *key = k;
    let third: std::collections::HashSet<&str> = data.weapons.iter().filter_map(|w| w.muzzle[1].as_deref()).collect();
    for w in &data.weapons {
        for t in [&w.muzzle[0], &w.muzzle[2]].into_iter().flatten() {
            if third.contains(t.as_str()) {
                continue;
            }
            let Some(list) = fx.pools.get(t) else { continue };
            for &si in list {
                let Some(s) = fx.systems.get(si) else { continue };
                for em in &s.emitters {
                    for h in std::iter::once(&em.mesh).chain(em.trail_mesh.as_ref()) {
                        for &e in me.0.get(&h.id()).into_iter().flatten() {
                            if !marked.contains(e) {
                                commands.entity(e).try_insert(FirstPersonDraw(-1.0));
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct SkinPart {
    pub meshes: Vec<Handle<Mesh>>,
    pub mesh: SkeletalMesh,
    /// part bone -> driver skeleton bone (by name, or the "Weapon" bone for rigid parts)
    pub bone_map: Vec<Option<usize>>,
    /// reference component-space transforms (UE units): rotation, translation
    pub ref_comp: Vec<(Quat, Vec3)>,
    /// part bones whose vertices collapse onto the bone (hidden geometry)
    pub collapsed: Vec<bool>,
    pub tx: Vec<Vec3>,
    pub tz: Vec<(Vec3, u8)>,
    /// per mesh in `meshes`: its vertices in the skeletal mesh (world.rs skinned_mesh builds
    /// each section with only the vertices it uses)
    pub maps: Vec<Vec<u32>>,
}

/// Section vertex maps in the order world.rs skinned_mesh built the section meshes.
fn part_maps(mesh: &SkeletalMesh) -> Vec<Vec<u32>> {
    mesh.lod.sections.iter().filter_map(|s| crate::section_vertex_map(&mesh.lod, s)).collect()
}

/// Tread texture scroll (GCTreadedVehicle / GCVeh_RCDroneTank native, EXE 0x0087b310 /
/// 0x00bb3d20): every frame the tread MIC gets SetVectorParameterValue('TreadLocation',
/// (0, G, 0, 0)) with G = (1 - PoweredWheel.CurrentRotation / 360) * TexPan2UnrealScale
/// (constants 1.0 and 1/360 read from the EXE); the tread materials (M_US_M1A3_treads,
/// UCV_GroundDrones M_treads) sample every texture at TexCoord0 + (0, -TreadLocation.G).
/// Applied here to the section's UV0 instead (same result; the material's parameter stays at
/// its stored value 0). `deg` = the powered wheel's rotation in degrees.
pub fn scroll_tread(meshes: &mut Assets<Mesh>, parts: &[TreadPart], deg: f32, tex_pan: f32) {
    for p in parts {
        let g = (1.0 - (deg * p.dir).rem_euclid(360.0) / 360.0) * tex_pan;
        if let Some(m) = meshes.get_mut(&p.mesh) {
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, p.base.iter().map(|uv| [uv[0], uv[1] - g]).collect::<Vec<_>>());
        }
    }
}

/// A tread section mesh: its reference UV0 and the rotation sign (`tread_dir`).
#[derive(Clone)]
pub struct TreadPart {
    pub mesh: Handle<Mesh>,
    pub base: Vec<[f32; 2]>,
    pub dir: f32,
}

/// Sign of the wheel rotation for a tread section so that its bottom run stands still on the
/// ground while driving forward: SKV_US_M1A3's V grows towards the front on the bottom run,
/// SKV_UCV_GAssault's towards the back (examples/treaddir). NOT from data: the sign of the
/// game's CurrentRotation (the EXE only shows G = (1 - rot / 360) * scale); chosen per mesh so
/// the bottom run does not slide.
fn tread_dir(lod: &upk::mesh::MeshLod, s: &upk::mesh::Section) -> f32 {
    let Some(idx) = lod.indices.get(s.first_index as usize..(s.first_index + s.num_triangles * 3) as usize) else { return 1.0 };
    let ymax = idx.iter().map(|&v| lod.positions[v as usize][1]).fold(f32::MIN, f32::max);
    let ymin = idx.iter().map(|&v| lod.positions[v as usize][1]).fold(f32::MAX, f32::min);
    let lo = ymax - (ymax - ymin) * 0.1;
    let mut votes = 0i32;
    for t in idx.chunks(3) {
        for k in 0..3 {
            let (a, b) = (t[k] as usize, t[(k + 1) % 3] as usize);
            let (pa, pb) = (lod.positions[a], lod.positions[b]);
            if pa[1] < lo || pb[1] < lo {
                continue;
            }
            let (dz, dv) = (pb[2] - pa[2], lod.uvs[b][1] - lod.uvs[a][1]);
            if dz.abs() > 0.5 && dv.abs() > 1e-3 && dv.abs() < 2.0 {
                votes += if dz * dv > 0.0 { 1 } else { -1 };
            }
        }
    }
    if votes < 0 { -1.0 } else { 1.0 }
}

/// Gather the per-vertex arrays of one section mesh and write them into it.
pub fn write_skinned(meshes: &mut Assets<Mesh>, h: &Handle<Mesh>, map: Option<&Vec<u32>>, bv: &[[f32; 3]], ue: &[[f32; 3]], tx: &[[u8; 4]], tz: &[[u8; 4]]) {
    let Some(m) = meshes.get_mut(h) else { return };
    crate::perf::count("skinned_vertices_uploaded", map.map(|m| m.len()).unwrap_or(bv.len()) as u64);
    // written into the mesh's existing attribute buffers (same length every frame): four new
    // Vecs per skinned mesh and frame before (bots, drone, 1P arms / weapon). Same data.
    match map {
        Some(map) => {
            put_f3(m, Mesh::ATTRIBUTE_POSITION, map.iter().map(|&v| bv.get(v as usize).copied().unwrap_or([0.0; 3])));
            put_f3(m, crate::gameshader::ATTR_UE_POS, map.iter().map(|&v| ue.get(v as usize).copied().unwrap_or([0.0; 3])));
            put_u4(m, crate::gamemat::ATTR_TANGENT_X, map.iter().map(|&v| tx.get(v as usize).copied().unwrap_or([128, 128, 255, 255])));
            put_u4(m, crate::gamemat::ATTR_TANGENT_Z, map.iter().map(|&v| tz.get(v as usize).copied().unwrap_or([128, 128, 255, 255])));
        }
        None => {
            put_f3(m, Mesh::ATTRIBUTE_POSITION, bv.iter().copied());
            put_f3(m, crate::gameshader::ATTR_UE_POS, ue.iter().copied());
            put_u4(m, crate::gamemat::ATTR_TANGENT_X, tx.iter().copied());
            put_u4(m, crate::gamemat::ATTR_TANGENT_Z, tz.iter().copied());
        }
    }
}

/// Replace a Float32x3 attribute's contents in place (inserted when missing / of another format).
fn put_f3(m: &mut Mesh, attr: bevy::render::mesh::MeshVertexAttribute, it: impl Iterator<Item = [f32; 3]>) {
    use bevy::render::mesh::VertexAttributeValues as V;
    if let Some(V::Float32x3(v)) = m.attribute_mut(attr.id) {
        v.clear();
        v.extend(it);
        return;
    }
    m.insert_attribute(attr, V::Float32x3(it.collect()));
}

/// Replace a Unorm8x4 attribute's contents in place (inserted when missing / of another format).
fn put_u4(m: &mut Mesh, attr: bevy::render::mesh::MeshVertexAttribute, it: impl Iterator<Item = [u8; 4]>) {
    use bevy::render::mesh::VertexAttributeValues as V;
    if let Some(V::Unorm8x4(v)) = m.attribute_mut(attr.id) {
        v.clear();
        v.extend(it);
        return;
    }
    m.insert_attribute(attr, V::Unorm8x4(it.collect()));
}

/// Weapon animation state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WeaponAnim {
    Draw,
    Idle,
    Fire,
    Reload,
}

pub struct WeaponVm {
    pub cfg: WeaponCfg,
    /// weapon mesh, body parts, scope
    pub parts: Vec<SkinPart>,
    /// arms bound to this weapon's skeleton
    pub arms: SkinPart,
    pub anims: AnimSetData,
    pub skeleton: Vec<upk::skeletal::Bone>,
    pub track_of_bone: Vec<Option<usize>>,
    pub seq_index: HashMap<String, usize>,
    /// quick-throw grenade (SKW_EXP_Frag_1P) bound to this weapon's skeleton
    pub frag: Option<SkinPart>,
}

#[derive(Resource)]
pub struct ViewModel {
    pub weapons: Vec<WeaponVm>,
    pub active: usize,
    pub anim: WeaponAnim,
    pub anim_time: f32,
    /// aim-down-sights blend 0 (hip) .. 1 (aimed)
    pub ads: f32,
    pub ads_time: f32,
    /// hidden while throwing a grenade etc.
    pub lowered: bool,
    pub visible: bool,
    shown: Option<usize>,
    init: bool,
    /// quick throw: seconds since the throw started (grenade_cook then grenade_throw), and the
    /// time the grenade leaves the hand (PreThrowAnimationTime)
    pub throw_t: Option<f32>,
    pub throw_release: f32,
    /// the throw is a C4 charge: CFG_EXP_C4's CH_1P_AnimSet (AS_EXP_C4_1P_CH, merged as
    /// "c4_grenade_cook" / "c4_grenade_throw"), no frag in hand (no 1P C4 mesh in the data)
    pub throw_c4: bool,
    /// current 1P mesh FOV (horizontal UE degrees, FOVMesh .. FOVMeshZoom; combat.rs)
    pub mesh_fov: f32,
    /// what is drawn: (weapon, throw stage 0 none / 1 grenade in hand / 2 hand empty)
    shown_state: (Option<usize>, u8),
    /// additive sprint layer (ADD_sprint_in / _loop / _out): 0 off, 1 in, 2 loop, 3 out
    sprint: u8,
    sprint_t: f32,
    /// additive walk layer ADD_move_F: weight (speed / run speed) and its loop time
    move_w: f32,
    move_t: f32,
    /// cross-fade from the last drawn pose after a sprint state change (0.2 s, NOT from data)
    fade: Option<(Vec<(Quat, Vec3)>, f32)>,
    last_pose: Vec<(Quat, Vec3)>,
    /// reload = a root sequence (reload_root_A/B/C, when the set has one) followed by the
    /// branch. AT_CHC_PL_1P_Alpha's GCAnimBlendReload children by GCAnimBlendReloadIndices
    /// (GCABRI_RELOAD_DRY = 0, _TACTICAL = 1, _RANDOM_ROOT = 2..4): DRY -> reload_branch_tact,
    /// TACTICAL -> reload_branch_dry (the sequence names are swapped against their use; the M4
    /// sounds agree: reload_branch_tact carries SC_M4_ReloadBranchDry + SC_M4DrawB). The M9 set
    /// has only the branches. The root ends back at idle while the branches start mid-reload:
    /// the switch happens RELOAD_BLEND before the root's end with a cross-fade of that length
    /// (the root's pose is closest to the branch start there; the blend time is NOT from data).
    reload_chain: Vec<&'static str>,
    /// index of the chain entry drawn last frame (cross-fade on a change)
    reload_seg: usize,
    /// play rate of the chain so it lasts the weapon's ReloadAnimTime (M4 1.587 s, M9 1.6 s:
    /// the sequences at their own rates take ~2.1 s)
    reload_rate: f32,
}

fn q(r: [f32; 4]) -> Quat {
    Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize()
}

fn comp_pose(bones: &[upk::skeletal::Bone], local: impl Fn(usize) -> (Quat, Vec3)) -> Vec<(Quat, Vec3)> {
    let mut out: Vec<(Quat, Vec3)> = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        let (lr, lp) = local(i);
        if i == 0 || b.parent < 0 || b.parent as usize >= i {
            out.push((lr, lp));
        } else {
            let (pr, pp) = out[b.parent as usize];
            out.push((pr * lr, pp + pr * lp));
        }
    }
    out
}

fn unpack(v: [u8; 4]) -> Vec3 {
    Vec3::new(v[0] as f32 / 127.5 - 1.0, v[1] as f32 / 127.5 - 1.0, v[2] as f32 / 127.5 - 1.0)
}

fn pack(v: Vec3, w: u8) -> [u8; 4] {
    let c = |x: f32| ((x.clamp(-1.0, 1.0) + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
    [c(v.x), c(v.y), c(v.z), w]
}

impl SkinPart {
    /// Skinned part following the driver skeleton by bone name.
    pub fn new(meshes: Vec<Handle<Mesh>>, mesh: SkeletalMesh, driver: &[upk::skeletal::Bone], hide: &[&str]) -> SkinPart {
        let bone_map = mesh.bones.iter().map(|b| driver.iter().position(|d| d.name.eq_ignore_ascii_case(&b.name))).collect();
        let ref_comp = comp_pose(&mesh.bones, |i| (q(mesh.bones[i].rot), Vec3::from(mesh.bones[i].pos)));
        let collapsed = mesh.bones.iter().map(|b| hide.iter().any(|h| h.eq_ignore_ascii_case(&b.name))).collect();
        let tx = mesh.lod.tangent_x.iter().map(|t| unpack(*t)).collect();
        let tz = mesh.lod.tangent_z.iter().map(|t| (unpack(*t), t[3])).collect();
        let maps = part_maps(&mesh);
        SkinPart { meshes, mesh, bone_map, ref_comp, collapsed, tx, tz, maps }
    }

    /// Section meshes drawn with material slot `slot` (section MaterialIndex through
    /// LODMaterialMap), each with its reference UV0 (tread scrolling).
    pub fn slot_meshes(&self, slot: i32) -> Vec<TreadPart> {
        let mut out = Vec::new();
        let mut j = 0;
        for s in &self.mesh.lod.sections {
            if crate::section_vertex_map(&self.mesh.lod, s).is_none() {
                continue;
            }
            let k = self.mesh.lod0_material_map.get(s.material as usize).copied().unwrap_or(s.material);
            if k == slot {
                if let (Some(h), Some(map)) = (self.meshes.get(j), self.maps.get(j)) {
                    out.push(TreadPart { mesh: h.clone(), base: map.iter().map(|&v| self.mesh.lod.uvs.get(v as usize).copied().unwrap_or([0.0; 2])).collect(), dir: tread_dir(&self.mesh.lod, s) });
                }
            }
            j += 1;
        }
        out
    }

    /// Rigid part on one driver bone (identity reference), offset in that bone's space.
    pub fn rigid(meshes: Vec<Handle<Mesh>>, mesh: SkeletalMesh, driver: &[upk::skeletal::Bone], bone: &str, offset: Vec3) -> SkinPart {
        let d = driver.iter().position(|b| b.name.eq_ignore_ascii_case(bone));
        let n = mesh.bones.len();
        let tx = mesh.lod.tangent_x.iter().map(|t| unpack(*t)).collect();
        let tz = mesh.lod.tangent_z.iter().map(|t| (unpack(*t), t[3])).collect();
        let maps = part_maps(&mesh);
        SkinPart { meshes, mesh, bone_map: vec![d; n], ref_comp: vec![(Quat::IDENTITY, -offset); n], collapsed: vec![false; n], tx, tz, maps }
    }
}

impl WeaponVm {
    pub fn new(cfg: WeaponCfg, anims: AnimSetData, skeleton: Vec<upk::skeletal::Bone>, parts: Vec<SkinPart>, arms: SkinPart) -> WeaponVm {
        let track_of_bone = skeleton.iter().map(|b| anims.bones.iter().position(|t| t.eq_ignore_ascii_case(&b.name))).collect();
        let seq_index = anims.seqs.iter().enumerate().map(|(i, s)| (s.name.to_ascii_lowercase(), i)).collect();
        WeaponVm { cfg, parts, arms, anims, skeleton, track_of_bone, seq_index, frag: None }
    }

    /// Add another AnimSet's sequences (the mesh component's AnimSets list), tracks matched by
    /// bone name; sequences already present keep the weapon set's version.
    pub fn merge_set(&mut self, set: &AnimSetData) {
        for s in &set.seqs {
            if self.seq_index.contains_key(&s.name.to_ascii_lowercase()) {
                continue;
            }
            let tracks = self.anims.bones.iter().map(|b| set.bones.iter().position(|x| x.eq_ignore_ascii_case(b)).and_then(|k| s.tracks.get(k).cloned()).unwrap_or_default()).collect();
            let mut s2 = s.clone();
            s2.tracks = tracks;
            self.seq_index.insert(s.name.to_ascii_lowercase(), self.anims.seqs.len());
            self.anims.seqs.push(s2);
        }
    }

    /// Like `merge_set` with the sequence names prefixed (a second set with the same names).
    pub fn merge_set_as(&mut self, set: &AnimSetData, prefix: &str) {
        let mut renamed = set.clone();
        for s in renamed.seqs.iter_mut() {
            s.name = format!("{prefix}{}", s.name);
        }
        self.merge_set(&renamed);
    }

    pub fn seq(&self, name: &str) -> Option<&upk::anim::AnimSeq> {
        self.seq_index.get(&name.to_ascii_lowercase()).map(|&i| &self.anims.seqs[i])
    }

    /// Local (parent-space) pose of a sequence at time t (seconds of the sequence).
    fn local_pose(&self, name: &str, t: f32, hold_end: bool) -> Vec<(Quat, Vec3)> {
        let seq = self.seq(name);
        let t = seq.map(|s| if hold_end { s.length } else { t.min(s.length) }).unwrap_or(0.0);
        self.skeleton
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let reference = (q(b.rot), Vec3::from(b.pos));
                match (seq, self.track_of_bone[i]) {
                    (Some(s), Some(tr)) => match s.sample(tr, t) {
                        Some((p, r)) => (q(r).conjugate(), Vec3::from(p)),
                        None => reference,
                    },
                    _ => reference,
                }
            })
            .collect()
    }

    /// Additive sequence (bIsAdditive, delta to AdditiveRefName) at time t: per bone the delta
    /// rotation (our conjugated convention) and translation; identity where no track.
    fn additive_pose(&self, name: &str, t: f32, looped: bool) -> Option<Vec<(Quat, Vec3)>> {
        let seq = self.seq(name)?;
        let t = if looped { t % seq.length.max(1e-3) } else { t.min(seq.length) };
        Some(
            (0..self.skeleton.len())
                .map(|i| match self.track_of_bone[i].and_then(|tr| seq.sample(tr, t)) {
                    Some((p, r)) => (q(r).conjugate(), Vec3::from(p)),
                    None => (Quat::IDENTITY, Vec3::ZERO),
                })
                .collect(),
        )
    }

    /// Length (seconds, RateScale applied) of a sequence.
    pub fn duration(&self, name: &str) -> f32 {
        self.seq(name).map(|s| s.length / s.rate.max(1e-3)).unwrap_or(0.0)
    }
}

/// Cross-fade between the reload root and its branch (seconds, NOT from data).
const RELOAD_BLEND: f32 = 0.2;

/// Animation sequence names of the states (AS_*_1P_WP).
pub fn anim_name(a: WeaponAnim, ads: bool) -> (&'static str, bool) {
    match (a, ads) {
        (WeaponAnim::Draw, _) => ("Draw", false),
        // no hip idle sequence in the sets: the end of Draw is the hip pose
        (WeaponAnim::Idle, false) => ("Draw", true),
        (WeaponAnim::Idle, true) => ("ADS_idle", false),
        (WeaponAnim::Fire, false) => ("Fire1", false),
        (WeaponAnim::Fire, true) => ("ADS_fire", false),
        (WeaponAnim::Reload, _) => ("reload_root_A", false),
    }
}

impl ViewModel {
    pub fn new(weapons: Vec<WeaponVm>) -> ViewModel {
        ViewModel { weapons, active: 0, anim: WeaponAnim::Draw, anim_time: 0.0, ads: 0.0, ads_time: 0.0, lowered: false, visible: false, shown: None, init: false, throw_t: None, throw_release: 0.56, throw_c4: false, mesh_fov: 50.0, shown_state: (None, 0), sprint: 0, sprint_t: 0.0, move_w: 0.0, move_t: 0.0, fade: None, last_pose: Vec::new(), reload_chain: Vec::new(), reload_seg: 0, reload_rate: 1.0 }
    }

    pub fn weapon(&self) -> Option<&WeaponVm> {
        self.weapons.get(self.active)
    }

    pub fn play(&mut self, a: WeaponAnim) {
        // cross-fade into the reload from the pose last drawn (0.2 s, NOT from data)
        if !self.last_pose.is_empty() && a == WeaponAnim::Reload {
            self.fade = Some((self.last_pose.clone(), 0.0));
        }
        self.anim = a;
        self.anim_time = 0.0;
    }

    /// Reload: a random root of the set (which root the game picks is NOT from data) and the
    /// dry or tactical branch.
    pub fn play_reload(&mut self, dry: bool, pick: u32) {
        let mut chain = Vec::new();
        if let Some(w) = self.weapon() {
            let roots: Vec<&'static str> = ["reload_root_A", "reload_root_B", "reload_root_C"].into_iter().filter(|n| w.seq(n).is_some()).collect();
            if !roots.is_empty() {
                chain.push(roots[pick as usize % roots.len()]);
            }
            let (want, other) = if dry { ("reload_branch_tact", "reload_branch_dry") } else { ("reload_branch_dry", "reload_branch_tact") };
            if w.seq(want).is_some() {
                chain.push(want);
            } else if w.seq(other).is_some() {
                chain.push(other);
            }
        }
        self.reload_chain = chain;
        self.reload_seg = 0;
        self.reload_rate = 1.0;
        let total = self.duration(WeaponAnim::Reload);
        let target = self.weapon().map(|w| w.cfg.reload_anim_time).unwrap_or(0.0);
        self.reload_rate = if total > 0.0 && target > 0.05 { total / target } else { 1.0 };
        self.play(WeaponAnim::Reload);
    }

    /// Played length (seconds at the sequence's own rate) of chain entry k: all but the last
    /// end RELOAD_BLEND early (the cross-fade into the next).
    fn reload_len(&self, k: usize) -> f32 {
        let Some(w) = self.weapon() else { return 0.0 };
        let Some(&n) = self.reload_chain.get(k) else { return 0.0 };
        let d = w.duration(n);
        if k + 1 < self.reload_chain.len() { (d - RELOAD_BLEND).max(d * 0.5) } else { d }
    }

    /// Chain entry at time t (seconds), its sequence and the time in it (sequence units).
    fn reload_at(&self, t: f32) -> Option<(usize, &'static str, f32)> {
        let w = self.weapon()?;
        let mut t = t * self.reload_rate;
        for (k, &n) in self.reload_chain.iter().enumerate() {
            let d = self.reload_len(k);
            let rate = w.seq(n).map(|s| s.rate).unwrap_or(1.0);
            if t < d || k + 1 == self.reload_chain.len() {
                return Some((k, n, t.min(d) * rate));
            }
            t -= d;
        }
        None
    }

    /// Duration of the current state's sequence (hip or aimed variant).
    pub fn duration(&self, a: WeaponAnim) -> f32 {
        let Some(w) = self.weapon() else { return 0.0 };
        if a == WeaponAnim::Reload {
            let _ = w;
            return (0..self.reload_chain.len()).map(|k| self.reload_len(k)).sum::<f32>() / self.reload_rate.max(1e-3);
        }
        let (n, _) = anim_name(a, self.ads > 0.5);
        w.duration(n)
    }

    /// Throw sequence length (grenade_cook + grenade_throw).
    pub fn throw_len(&self) -> f32 {
        let (cook, throw) = self.throw_seqs();
        self.weapon().map(|w| w.duration(cook) + w.duration(throw)).unwrap_or(0.0)
    }

    /// The throw's sequences (C4: AS_EXP_C4_1P_CH when the weapon set has it merged).
    fn throw_seqs(&self) -> (&'static str, &'static str) {
        if self.throw_c4 && self.weapon().is_some_and(|w| w.seq("c4_grenade_throw").is_some()) {
            ("c4_grenade_cook", "c4_grenade_throw")
        } else {
            ("grenade_cook", "grenade_throw")
        }
    }

    /// Component-space pose: hip and aimed variants blended by `ads`.
    fn pose(&self) -> Vec<(Quat, Vec3)> {
        let Some(w) = self.weapon() else { return Vec::new() };
        if let Some(t) = self.throw_t {
            let (cn, tn) = self.throw_seqs();
            let cook = w.duration(cn);
            let rate = |n: &str| w.seq(n).map(|s| s.rate).unwrap_or(1.0);
            let local = if t < cook { w.local_pose(cn, t * rate(cn), false) } else { w.local_pose(tn, (t - cook) * rate(tn), false) };
            return comp_pose(&w.skeleton, |i| local[i]);
        }
        let sample = |aimed: bool| {
            if self.anim == WeaponAnim::Reload {
                if let Some((_, n, t)) = self.reload_at(self.anim_time) {
                    // the reload time itself (hold_end = true would sample the sequence end: the
                    // reload showed one frozen pose)
                    return w.local_pose(n, t, false);
                }
            }
            let (mut name, mut hold) = anim_name(self.anim, aimed);
            // sets with a hip idle loop (AS_ATW_PZF3_1P_WP "IDLE")
            if name == "Draw" && hold && w.seq("IDLE").is_some() {
                name = "IDLE";
                hold = false;
            }
            let rate = w.seq(name).map(|s| s.rate).unwrap_or(1.0);
            let looped = name == "ADS_idle" || name == "IDLE";
            let t = if looped { self.ads_time * rate % w.seq(name).map(|s| s.length.max(1e-3)).unwrap_or(1.0) } else { self.anim_time * rate };
            w.local_pose(name, t, hold)
        };
        let local = if self.ads <= 0.0 {
            sample(false)
        } else if self.ads >= 1.0 || matches!(self.anim, WeaponAnim::Reload | WeaponAnim::Draw) {
            if matches!(self.anim, WeaponAnim::Reload | WeaponAnim::Draw) { sample(false) } else { sample(true) }
        } else {
            let a = sample(false);
            let b = sample(true);
            a.iter().zip(b.iter()).map(|(x, y)| (x.0.slerp(y.0, self.ads), x.1.lerp(y.1, self.ads))).collect()
        };
        // additive layers (UE3 BlendAdditive: Rotation = Delta * Base, Translation += Delta;
        // in our conjugated convention delta * base: examples/addcheck keeps the left hand on the
        // weapon this way, base * delta moves it 4-6 units off), scaled by their weight
        let mut local = local;
        let mut add = |name: &str, t: f32, looped: bool, wgt: f32| {
            if wgt <= 0.0 {
                return;
            }
            let rate = w.seq(name).map(|s| s.rate).unwrap_or(1.0);
            if let Some(d) = w.additive_pose(name, t * rate, looped) {
                for (l, (dr, dp)) in local.iter_mut().zip(d) {
                    l.0 = Quat::IDENTITY.slerp(dr, wgt) * l.0;
                    l.1 += dp * wgt;
                }
            }
        };
        match self.sprint {
            1 => add("ADD_sprint_in", self.sprint_t, false, 1.0),
            2 => add("ADD_sprint_loop", self.sprint_t, true, 1.0),
            3 => add("ADD_sprint_out", self.sprint_t, false, 1.0),
            _ => {}
        }
        add("ADD_move_F", self.move_t, true, self.move_w * (1.0 - self.ads));
        comp_pose(&w.skeleton, |i| local[i])
    }

    /// Sprint and walk layers from the pawn's movement; true when the sprint layer jumped to
    /// another sequence (the caller cross-fades).
    fn update_layers(&mut self, sprinting: bool, speed_frac: f32, dt: f32) -> bool {
        let len = |vm: &ViewModel, n: &str| vm.weapon().map(|w| w.duration(n)).unwrap_or(0.0);
        self.sprint_t += dt;
        let before = self.sprint;
        match (self.sprint, sprinting) {
            (0 | 3, true) => {
                self.sprint = 1;
                self.sprint_t = 0.0;
            }
            (1, true) if self.sprint_t >= len(self, "ADD_sprint_in") => {
                self.sprint = 2;
                self.sprint_t = 0.0;
            }
            (1 | 2, false) => {
                self.sprint = 3;
                self.sprint_t = 0.0;
            }
            (3, false) if self.sprint_t >= len(self, "ADD_sprint_out") => self.sprint = 0,
            _ => {}
        }
        // walk layer weight follows the speed smoothly (time constant 0.125 s, NOT from data)
        let target = if self.sprint == 0 { speed_frac.clamp(0.0, 1.0) } else { 0.0 };
        self.move_w += (target - self.move_w) * (dt * 8.0).min(1.0);
        self.move_t += dt;
        before != self.sprint && !(before == 1 && self.sprint == 2) && !(before == 3 && self.sprint == 0)
    }
}

/// Lateral scale of the 1P mesh measured in trace t3 frame 2700 (1366x768): local X/Y rows of
/// the arms' LocalToWorld have length 1.4535, local Z (forward) 1.0. It is the ratio of the
/// world and the 1P-mesh (FOV 50) projections; aiming changes both (scope FOVZoom /
/// FOVMeshZoom), see `ads_lateral_scale`.
const FP_LATERAL_SCALE: f32 = 1.4535;

/// World FOV values are 4:3 horizontal angles (vertical = horizontal * 3/4 in tangent space:
/// trace vertical 52.5 deg); the lateral scale follows the tangent ratio world / mesh.
#[allow(dead_code)]
pub fn ads_lateral_scale(fov_zoom: f32, fov_mesh_zoom: f32) -> f32 {
    let hip_world = (crate::player::VFOV_DEG.to_radians() * 0.5).tan() / 0.75;
    let hip_ratio = hip_world / 25f32.to_radians().tan();
    let zoom_ratio = (fov_zoom.to_radians() * 0.5).tan() / (fov_mesh_zoom.to_radians() * 0.5).tan();
    FP_LATERAL_SCALE * zoom_ratio / hip_ratio
}

/// Lateral scale for a vertical world FOV (degrees) and a horizontal 1P mesh FOV (degrees).
pub fn lateral_scale(world_vfov: f32, mesh_fov: f32) -> f32 {
    let hip_world = (crate::player::VFOV_DEG.to_radians() * 0.5).tan() / 0.75;
    let hip_ratio = hip_world / 25f32.to_radians().tan();
    let ratio = ((world_vfov.to_radians() * 0.5).tan() / 0.75) / (mesh_fov.max(0.1).to_radians() * 0.5).tan();
    FP_LATERAL_SCALE * ratio / hip_ratio
}

/// Vertical world FOV (degrees) for a 4:3 horizontal UE FOV.
pub fn vfov_for(fov: f32) -> f32 {
    2.0 * ((fov.to_radians() * 0.5).tan() * 0.75).atan().to_degrees()
}

#[allow(clippy::too_many_arguments)]
fn update_viewmodel(
    mut commands: Commands,
    time: Res<Time>,
    vm: Option<ResMut<ViewModel>>,
    flow: Res<MatchFlow>,
    pawns: Query<&Pawn>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ents: Query<(Entity, &Mesh3d, &mut Visibility, &mut Transform)>,
    mut sfx: ResMut<crate::sfx::SfxQueue>,
    mut last_seq: Local<(String, f32)>,
    me: Res<crate::MeshEntities>,
    (mut steps, mut last_step): (ResMut<crate::impacts::PlayerSteps>, Local<(&'static str, f32)>),
) {
    let _perf = crate::perf::scope("viewmodel");
    let Some(mut vm) = vm else { return };
    let custom_on = crate::vmanim::active();
    let show = flow.phase == Phase::Playing && pawns.single().is_ok() && (!vm.lowered || custom_on);
    let want = if show { Some(vm.active) } else { None };
    if std::env::var_os("HB_VMDBG").is_some() && (time.elapsed_secs() * 0.5).fract() < time.delta_secs() * 0.5 {
        info!("VMSTATE show {show} lowered {} phase {:?} pawn {} throw {:?} anim {:?} t {:.2} active {}", vm.lowered, flow.phase, pawns.single().is_ok(), vm.throw_t, vm.anim, vm.anim_time, vm.active);
    }
    // quick throw: the weapon is hidden, the grenade in hand until it is released
    let stage = match vm.throw_t {
        // own arm animation (vmanim.rs): the arms only
        _ if custom_on => 2u8,
        Some(t) if t < vm.throw_release => 1u8,
        Some(_) => 2,
        None => 0,
    };
    if want != vm.shown || !vm.init || (want, stage) != vm.shown_state {
        let first = !vm.init;
        vm.init = true;
        vm.shown_state = (want, stage);
        let frag_ids: std::collections::HashSet<AssetId<Mesh>> = vm.weapons.iter().filter_map(|w| w.frag.as_ref()).flat_map(|p| p.meshes.iter().map(|h| h.id())).collect();
        for (e, m, mut v, _) in &mut ents {
            if frag_ids.contains(&m.0.id()) {
                *v = if want.is_some() && stage == 1 && !vm.throw_c4 { Visibility::Visible } else { Visibility::Hidden };
                if first {
                    commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                }
            }
        }
        for (wi, w) in vm.weapons.iter().enumerate() {
            let on = want == Some(wi) && stage == 0;
            let ids: std::collections::HashSet<AssetId<Mesh>> = w.parts.iter().flat_map(|p| p.meshes.iter().map(|h| h.id())).collect();
            for (e, m, mut v, _) in &mut ents {
                if ids.contains(&m.0.id()) {
                    *v = if on { Visibility::Visible } else { Visibility::Hidden };
                    if first {
                        // positions are rewritten every frame: the creation-time bounds are meaningless
                        commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                    }
                }
            }
        }
        if let Some(w) = vm.weapons.first() {
            let ids: std::collections::HashSet<AssetId<Mesh>> = w.arms.meshes.iter().map(|h| h.id()).collect();
            for (e, m, mut v, _) in &mut ents {
                if ids.contains(&m.0.id()) {
                    *v = if want.is_some() { Visibility::Visible } else { Visibility::Hidden };
                    if first {
                        commands.entity(e).insert(bevy::render::view::NoFrustumCulling);
                    }
                }
            }
        }
        let was_hidden = vm.shown.is_none();
        vm.shown = want;
        vm.visible = show;
        if show && was_hidden && flow.phase == Phase::Playing && vm.anim != WeaponAnim::Reload {
            vm.play(WeaponAnim::Draw);
        }
    }
    if !show {
        return;
    }
    let Ok(p) = pawns.single() else { return };
    // rig debug (F3) slows or freezes the animation clock
    let dt = time.delta_secs() * crate::rigdebug::anim_scale();
    vm.anim_time += dt;
    vm.ads_time += dt;
    if matches!(vm.anim, WeaponAnim::Draw | WeaponAnim::Fire | WeaponAnim::Reload) && vm.anim_time >= vm.duration(vm.anim) {
        let from_reload = vm.anim == WeaponAnim::Reload;
        vm.anim = WeaponAnim::Idle;
        vm.anim_time = 0.0;
        if from_reload && !vm.last_pose.is_empty() {
            vm.fade = Some((vm.last_pose.clone(), 0.0));
        }
    }
    if std::env::var_os("HB_VMDBG").is_some() {
        let r = if vm.anim == WeaponAnim::Reload { vm.reload_at(vm.anim_time).map(|(k, n, t)| format!("{k} {n} {t:.2}")) } else { None };
        info!("VMANIM {:?} t {:.3} dur {:.3} reload {:?} chain {:?} rate {:.2} fade {}", vm.anim, vm.anim_time, vm.duration(vm.anim), r, vm.reload_chain, vm.reload_rate, vm.fade.as_ref().map(|f| f.1).unwrap_or(-1.0));
    }
    // reload chain: cross-fade when the root hands over to the branch
    let reload_now = if vm.anim == WeaponAnim::Reload { vm.reload_at(vm.anim_time) } else { None };
    if let Some((k, _, _)) = reload_now {
        if k != vm.reload_seg {
            vm.reload_seg = k;
            if !vm.last_pose.is_empty() {
                vm.fade = Some((vm.last_pose.clone(), 0.0));
            }
        }
    }
    // AnimNotify_Sound of the playing sequence (reload, draw ...)
    if let Some(w) = vm.weapon() {
        let (mut name, hold) = anim_name(vm.anim, vm.ads > 0.5);
        let mut reload_t = None;
        if let Some((_, n, t)) = reload_now {
            name = n;
            reload_t = Some(t);
        }
        if !hold {
            if let Some(seq) = w.seq(name) {
                let t = reload_t.unwrap_or(vm.anim_time * seq.rate);
                let prev = if last_seq.0 == name && last_seq.1 <= t { last_seq.1 } else { -1.0 };
                for (nt, cue) in &seq.sounds {
                    if *nt > prev && *nt <= t {
                        sfx.play(cue, None);
                    }
                }
                *last_seq = (name.to_string(), t);
            }
        } else {
            last_seq.0.clear();
        }
    }
    // walk layer weight: horizontal speed over the run speed (GroundSpeed, player.rs)
    let hs = Vec2::new(p.vel.x, p.vel.z).length();
    let sprinting = p.sprinting && hs > 0.1 && vm.anim != WeaponAnim::Reload;
    if vm.update_layers(sprinting, if p.on_ground { hs / crate::player::GROUND_SPEED } else { 0.0 }, dt) && !vm.last_pose.is_empty() {
        vm.fade = Some((vm.last_pose.clone(), 0.0));
    }
    // the player's footsteps: AnimNotify_Footstep of the walk / sprint layer playing (AS_*_1P_WP
    // ADD_move_F at 0.01 / 0.43 of 0.83 s, ADD_sprint_loop at 0.01 / 0.35 of 0.7 s); the walk
    // layer counts while its weight is above 0.25 (NOT from data)
    let layer = if vm.sprint == 2 { Some(("ADD_sprint_loop", vm.sprint_t)) } else if vm.sprint == 0 && vm.move_w > 0.25 && p.on_ground { Some(("ADD_move_F", vm.move_t)) } else { None };
    match layer.and_then(|(n, t)| vm.weapon().and_then(|w| w.seq(n)).map(|s| (n, s, t))) {
        Some((n, s, t)) => {
            let len = s.length.max(1e-3);
            let lt = (t * s.rate) % len;
            if last_step.0 == n {
                let prev = last_step.1;
                let crossed = s.footsteps.iter().any(|&f| if lt >= prev { f > prev && f <= lt } else { f > prev || f <= lt });
                if crossed {
                    steps.0 += 1;
                }
            }
            *last_step = (n, lt);
        }
        None => *last_step = ("", 0.0),
    }
    let mut pose = vm.pose();
    if let Some((from, t)) = vm.fade.as_mut() {
        *t += dt / 0.2;
        let k = t.min(1.0);
        if from.len() == pose.len() {
            for (p2, f) in pose.iter_mut().zip(from.iter()) {
                p2.0 = f.0.slerp(p2.0, k);
                p2.1 = f.1.lerp(p2.1, k);
            }
        }
        if k >= 1.0 {
            vm.fade = None;
        }
    }
    // HB_VMDUMP=<file>: the drawn component pose once (Blender animation reference, vmanim.rs)
    if let (Ok(path), Some(w)) = (std::env::var("HB_VMDUMP"), vm.weapon()) {
        static DUMPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if vm.anim_time > 2.0 && !DUMPED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let mut js = String::from("{\"bones\":[");
            for (i, (b, (r, p))) in w.skeleton.iter().zip(pose.iter()).enumerate() {
                js.push_str(&format!("{}{{\"name\":\"{}\",\"parent\":{},\"q\":[{},{},{},{}],\"p\":[{},{},{}]}}", if i > 0 { "," } else { "" }, b.name, b.parent, r.x, r.y, r.z, r.w, p.x, p.y, p.z));
            }
            js.push_str(&format!("],\"mesh_fov\":{},\"fov\":{}}}", vm.mesh_fov, p.fov));
            let _ = std::fs::write(&path, js);
            info!("viewmodel: pose dumped to {path}");
        }
    }
    // custom arm animation (vmanim.rs: the nuke's hook-up and farewell): replaces the arms' bones,
    // the weapon is not drawn while it plays
    let custom = crate::vmanim::current();
    if let (Some(c), Some(w)) = (custom.as_ref(), vm.weapon()) {
        if std::env::var_os("HB_VMDBG").is_some() {
            let hits = w.skeleton.iter().filter(|b| c.contains_key(&b.name.to_ascii_lowercase())).count();
            info!("VMCUSTOM {} of {} driver bones, hand {:?}", hits, w.skeleton.len(), c.get("righthand"));
        }
        for (i, b) in w.skeleton.iter().enumerate() {
            if let Some(v) = c.get(&b.name.to_ascii_lowercase()) {
                if let Some(slot) = pose.get_mut(i) {
                    *slot = *v;
                }
            }
        }
    }
    vm.last_pose = pose.clone();
    let Some(w) = vm.weapon() else { return };
    // view: eye in UE units, rotation yaw (Z) then pitch (Y)
    let eye_b = p.feet + Vec3::Y * p.eye;
    let eye = Vec3::new(eye_b.x, eye_b.z, eye_b.y) * 100.0;
    let yaw = p.yaw / 65536.0 * std::f32::consts::TAU;
    let pitch = p.pitch / 65536.0 * std::f32::consts::TAU;
    let view = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(-pitch);
    // the mesh drawn with its own FOV inside the world view: lateral scale from the current
    // world FOV (p.fov) and mesh FOV (vm.mesh_fov), = ads_lateral_scale at the zoom's end
    let k = lateral_scale(p.fov, vm.mesh_fov);
    // Scopes table OffsetLocationZoomed (1P mesh space, Z forward): eye relief of the scope while
    // aiming (CFG_RIF_M4 ACOG z 12.54, Reflex 5.0, Holographic -0.23)
    let ads_off = w.cfg.scope.as_ref().map(|s| Vec3::from(s.offset_zoomed)).unwrap_or(Vec3::ZERO) * vm.ads;
    let tr = Vec3::new(eye.x, eye.z, eye.y) * 0.01;
    let drawn: Vec<&SkinPart> = match stage {
        _ if custom.is_some() => vec![&w.arms],
        0 => w.parts.iter().chain(std::iter::once(&w.arms)).collect(),
        1 if !vm.throw_c4 => w.frag.iter().chain(std::iter::once(&w.arms)).collect(),
        _ => vec![&w.arms],
    };
    for part in drawn {
        // skin matrices per part bone
        let skin: Vec<(Quat, Vec3, bool)> = part
            .mesh
            .bones
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let cur = part.bone_map[i].and_then(|d| pose.get(d).copied()).unwrap_or(part.ref_comp[i]);
                if part.collapsed[i] {
                    return (Quat::IDENTITY, cur.1, true);
                }
                let (rr, rp) = part.ref_comp[i];
                let inv = rr.conjugate();
                (cur.0 * inv, cur.1 - cur.0 * (inv * rp), false)
            })
            .collect();
        let n = part.mesh.lod.positions.len();
        let mut ue = Vec::with_capacity(n);
        let mut bv = Vec::with_capacity(n);
        let mut tx = Vec::with_capacity(n);
        let mut tz = Vec::with_capacity(n);
        for vi in 0..n {
            let v = Vec3::from(part.mesh.lod.positions[vi]);
            let (bi, wt) = part.mesh.influences[vi];
            let mut pos = Vec3::ZERO;
            let mut rot_acc = Vec3::ZERO;
            let mut rot_acc_z = Vec3::ZERO;
            for k2 in 0..4 {
                if wt[k2] == 0 {
                    continue;
                }
                let wf = wt[k2] as f32 / 255.0;
                let (r, t, collapsed) = skin.get(bi[k2] as usize).copied().unwrap_or((Quat::IDENTITY, Vec3::ZERO, false));
                pos += if collapsed { t } else { r * v + t } * wf;
                rot_acc += (r * part.tx[vi]) * wf;
                rot_acc_z += (r * part.tz[vi].0) * wf;
            }
            // 1P mesh space -> view (trace t3 frame 2700, arms draw c230 vs the camera of c238):
            // origin at the eye, local Z = forward, local X = -right * S, local Y = -up * S
            let pos = pos + ads_off;
            let lv = Vec3::new(pos.z, -pos.x * k, -pos.y * k);
            let rel = view * lv;
            ue.push((eye + rel).to_array());
            // relative to the eye (entity Transform = eye)
            bv.push([rel.x * 0.01, rel.z * 0.01, rel.y * 0.01]);
            let to_view = |d: Vec3| Vec3::new(d.z, -d.x, -d.y);
            tx.push(pack((view * to_view(rot_acc)).normalize_or_zero(), 255));
            tz.push(pack((view * to_view(rot_acc_z)).normalize_or_zero(), part.tz[vi].1));
        }
        if std::env::var_os("HB_VMDBG").is_some() && (time.elapsed_secs() * 0.5).fract() < time.delta_secs() * 0.5 {
            // view-space bounds of the part (UE units: x forward, y right, z up)
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for q in &ue {
                let r = view.conjugate() * (Vec3::from(*q) - eye);
                lo = lo.min(r);
                hi = hi.max(r);
            }
            let vis: Vec<String> = ents.iter().filter(|(_, mh, _, _)| part.meshes.iter().any(|h| h.id() == mh.0.id())).map(|(_, _, v, _)| format!("{v:?}")).collect();
            info!("vm part {} {} verts ads {:.2} k {:.2}: {:?}..{:?} vis {:?}", part.mesh.bones.first().map(|b| b.name.as_str()).unwrap_or(""), n, vm.ads, k, lo, hi, vis);
        }
        for e in crate::mesh_ents(&me, &part.meshes) {
            if let Ok((_, _, _, mut tf)) = ents.get_mut(e) {
                if tf.translation != tr {
                    tf.translation = tr;
                }
            }
        }
        for (k, h) in part.meshes.iter().enumerate() {
            write_skinned(&mut meshes, h, part.maps.get(k), &bv, &ue, &tx, &tz);
        }
    }
}

/// Muzzle position (UE world units) of the active weapon in its current pose, for effects.
pub fn muzzle_world(vm: &ViewModel, p: &Pawn) -> Option<Vec3> {
    let w = vm.weapon()?;
    let i = w.skeleton.iter().position(|b| b.name.eq_ignore_ascii_case("Muzzle"))?;
    let pose = vm.pose();
    let pos = pose.get(i)?.1;
    let eye_b = p.feet + Vec3::Y * p.eye;
    let eye = Vec3::new(eye_b.x, eye_b.z, eye_b.y) * 100.0;
    let yaw = p.yaw / 65536.0 * std::f32::consts::TAU;
    let pitch = p.pitch / 65536.0 * std::f32::consts::TAU;
    let view = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(-pitch);
    let k = lateral_scale(p.fov, vm.mesh_fov);
    Some(eye + view * Vec3::new(pos.z, -pos.x * k, -pos.y * k))
}

/// Mapping summary of a skinned part against its driver skeleton (rig debug).
pub(crate) fn part_info(part: &SkinPart, label: &str, driver: &[upk::skeletal::Bone]) -> crate::rigdebug::PartInfo {
    let n = part.mesh.bones.len();
    let mut used = vec![false; n];
    let (mut weight_sum_off, mut bad_index) = (0, 0);
    for (bi, wt) in &part.mesh.influences {
        let sum: u32 = wt.iter().map(|&w| w as u32).sum();
        if sum != 255 {
            weight_sum_off += 1;
        }
        for k in 0..4 {
            if wt[k] == 0 {
                continue;
            }
            match used.get_mut(bi[k] as usize) {
                Some(u) => *u = true,
                None => bad_index += 1,
            }
        }
    }
    let rigid = part.bone_map.iter().all(|m| *m == part.bone_map.first().copied().flatten()) && n > 0 && part.ref_comp.iter().all(|r| r.0 == Quat::IDENTITY);
    crate::rigdebug::PartInfo {
        label: format!("{label} ({}{})", part.mesh.bones.first().map(|b| b.name.as_str()).unwrap_or("?"), if rigid { ", rigid" } else { "" }),
        bones: n,
        verts: part.mesh.lod.positions.len(),
        unmapped: part.mesh.bones.iter().zip(&part.bone_map).filter(|(_, m)| m.is_none()).map(|(b, _)| b.name.clone()).collect(),
        collapsed: part.mesh.bones.iter().zip(&part.collapsed).filter(|(_, c)| **c).map(|(b, _)| b.name.clone()).collect(),
        weight_sum_off,
        bad_index,
        unused: used.iter().filter(|u| !**u).count(),
        map: part.bone_map.iter().map(|m| m.filter(|&d| d < driver.len())).collect(),
    }
}

/// All parts of a 1P weapon (weapon parts, arms, frag).
pub(crate) fn part_infos(w: &WeaponVm) -> Vec<crate::rigdebug::PartInfo> {
    let mut v: Vec<crate::rigdebug::PartInfo> = w.parts.iter().enumerate().map(|(i, p)| part_info(p, &format!("weapon part {i}"), &w.skeleton)).collect();
    v.push(part_info(&w.arms, "arms", &w.skeleton));
    if let Some(f) = &w.frag {
        v.push(part_info(f, "frag", &w.skeleton));
    }
    v
}

impl ViewModel {
    /// The sequence drawn now and its time (sequence seconds).
    fn current_seq(&self) -> (String, f32) {
        let Some(w) = self.weapon() else { return (String::new(), 0.0) };
        if let Some(t) = self.throw_t {
            let (cn, tn) = self.throw_seqs();
            let cook = w.duration(cn);
            let rate = |n: &str| w.seq(n).map(|s| s.rate).unwrap_or(1.0);
            return if t < cook { (cn.into(), t * rate(cn)) } else { (tn.into(), (t - cook) * rate(tn)) };
        }
        if self.anim == WeaponAnim::Reload {
            if let Some((_, n, t)) = self.reload_at(self.anim_time) {
                return (n.into(), t);
            }
        }
        let (mut name, mut hold) = anim_name(self.anim, self.ads >= 1.0);
        if name == "Draw" && hold && w.seq("IDLE").is_some() {
            name = "IDLE";
            hold = false;
        }
        let rate = w.seq(name).map(|s| s.rate).unwrap_or(1.0);
        let len = w.seq(name).map(|s| s.length).unwrap_or(0.0);
        let t = if hold {
            len
        } else if name == "ADS_idle" || name == "IDLE" {
            self.ads_time * rate % len.max(1e-3)
        } else {
            (self.anim_time * rate).min(len)
        };
        (name.into(), t)
    }
}

/// Rig debug view of the 1P weapon skeleton (the pose drawn last frame).
pub(crate) fn rig_view(vm: &ViewModel, p: &Pawn) -> Option<crate::rigdebug::RigView> {
    let w = vm.weapon()?;
    if vm.last_pose.len() != w.skeleton.len() || !vm.visible {
        return None;
    }
    let comp = vm.last_pose.clone();
    let parents: Vec<i32> = w.skeleton.iter().map(|b| b.parent).collect();
    let local = crate::rigdebug::local_from_comp(&parents, &comp);
    let eye_b = p.feet + Vec3::Y * p.eye;
    let eye = Vec3::new(eye_b.x, eye_b.z, eye_b.y) * 100.0;
    let yaw = p.yaw / 65536.0 * std::f32::consts::TAU;
    let pitch = p.pitch / 65536.0 * std::f32::consts::TAU;
    let view = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(-pitch);
    let k = lateral_scale(p.fov, vm.mesh_fov);
    let ads_off = w.cfg.scope.as_ref().map(|s| Vec3::from(s.offset_zoomed)).unwrap_or(Vec3::ZERO) * vm.ads;
    let map = |pos: Vec3| {
        let pos = pos + ads_off;
        let ue = eye + view * Vec3::new(pos.z, -pos.x * k, -pos.y * k);
        Vec3::new(ue.x, ue.z, ue.y) * 0.01
    };
    let world = comp.iter().map(|c| map(c.1)).collect();
    let axes = comp.iter().map(|&(r, t)| [map(t + r * Vec3::X * 2.0), map(t + r * Vec3::Y * 2.0), map(t + r * Vec3::Z * 2.0)]).collect();
    let (seq, seq_t) = vm.current_seq();
    let sl = w.seq(&seq);
    let info = vec![
        format!("weapon {} ({}), set {}", w.cfg.cfg, w.cfg.sk_mesh, w.anims.path),
        format!(
            "state {:?} t {:.3} / {:.3}  seq len {:.3} rate {:.2} frames {}",
            vm.anim,
            vm.anim_time,
            vm.duration(vm.anim),
            sl.map(|s| s.length).unwrap_or(0.0),
            sl.map(|s| s.rate).unwrap_or(0.0),
            sl.map(|s| s.frames).unwrap_or(0)
        ),
        format!("ads {:.3} (t {:.2})  mesh FOV {:.2}  lateral scale {:.4}  ads offset {:?}", vm.ads, vm.ads_time, vm.mesh_fov, k, ads_off),
        format!("sprint layer {} t {:.3}  walk layer w {:.3} t {:.3}  fade {}", vm.sprint, vm.sprint_t, vm.move_w, vm.move_t, vm.fade.as_ref().map(|f| format!("{:.2}", f.1)).unwrap_or("-".into())),
        format!("reload chain {:?} seg {} rate {:.3}  throw {:?} c4 {}", vm.reload_chain, vm.reload_seg, vm.reload_rate, vm.throw_t, vm.throw_c4),
        format!("eye (UE) {:.1} {:.1} {:.1}  yaw {:.1}° pitch {:.1}°", eye.x, eye.y, eye.z, yaw.to_degrees(), pitch.to_degrees()),
    ];
    Some(crate::rigdebug::RigView {
        label: format!("1P {}", w.cfg.cfg),
        names: w.skeleton.iter().map(|b| b.name.clone()).collect(),
        parents,
        reference: w.skeleton.iter().map(|b| (q(b.rot), Vec3::from(b.pos))).collect(),
        local,
        comp,
        track: w.track_of_bone.clone(),
        world,
        axes,
        seq,
        seq_t,
        info,
        parts: part_infos(w),
    })
}
