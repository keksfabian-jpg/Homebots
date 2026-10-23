//! Impact effects by physical material and damage type (PhysicalMaterialPropertyEx of the
//! level's PM_Library physical materials: DamageTypeInstigators[] = CollisionEffect
//! {ParticleSystems, SoundCues} for DamageTypes[]; the entry for the closest damage type class
//! of the hit's damage type wins, a material without one asks its Parent).
//!
//! Surfaces: static / skeletal meshes carry their section material's PhysMaterial (Material or
//! the MaterialInstance parent chain), terrain the PhysMaterial of the layer with the highest
//! weight at the hit, soldiers PMA_Soldier_Chest / PMA_Head like the damage grid.

use bevy::prelude::*;
use std::collections::HashMap;
use upk::Package;

/// CollisionWorld physical material id of terrain triangles (looked up from the layer weights)
pub const TERRAIN: u16 = u16::MAX;

#[derive(Default, Clone)]
pub(crate) struct PhysFx {
    parent: Option<String>,
    /// (damage type class names, particle systems, sound cues), lowercase class names
    entries: Vec<(Vec<String>, Vec<String>, Vec<String>)>,
    /// FootstepInstigators[0] sound cues
    footstep: Vec<String>,
}

#[derive(Resource, Default)]
pub struct Impacts {
    pub(crate) mats: HashMap<String, PhysFx>,
    /// damage type (lowercase) -> its class chain, itself first (GCGame.u super classes)
    chains: HashMap<String, Vec<String>>,
    /// physical material id (1-based, CollisionWorld) -> lowercase path
    pub names: Vec<String>,
    ids: HashMap<String, u16>,
    terrain: Option<(upk::terrain::TerrainLayers, [u16; 4])>,
}

impl Impacts {
    /// Read the physical materials of the level packages and the damage type classes.
    pub fn load(pkgs: &[Package], gcgame: Option<&Package>) -> Impacts {
        let mut im = Impacts::default();
        for p in pkgs {
            for (i, e) in p.exports.iter().enumerate() {
                if p.export_class(e) != "PhysicalMaterial" {
                    continue;
                }
                let path = p.object_path(i as i32 + 1).to_ascii_lowercase();
                if im.mats.contains_key(&path) {
                    continue;
                }
                let Some(pr) = p.export_props(i) else { continue };
                let mut fx = PhysFx { parent: pr.object("Parent").filter(|&o| o != 0).map(|o| p.object_path(o).to_ascii_lowercase()), entries: Vec::new(), footstep: Vec::new() };
                if let Some(pp) = pr.object("PhysicalMaterialProperty").filter(|&o| o > 0).and_then(|o| p.export_props(o as usize - 1)) {
                    if let Some(st) = p.struct_array(&pp, "FootstepInstigators").into_iter().next() {
                        let ce = p.struct_prop(&st, "CollisionEffect");
                        fx.footstep = ce.as_ref().and_then(|c| c.get("SoundCues")).map(|v| p.array_objects(v)).unwrap_or_default().into_iter().filter(|&o| o != 0).map(|o| p.object_path(o)).collect();
                    }
                    for st in p.struct_array(&pp, "DamageTypeInstigators") {
                        let dts: Vec<String> = st.get("DamageTypes").map(|v| p.array_objects(v)).unwrap_or_default().into_iter().map(|o| p.object_name(o).to_ascii_lowercase()).collect();
                        let ce = p.struct_prop(&st, "CollisionEffect");
                        let list = |n: &str| -> Vec<String> { ce.as_ref().and_then(|c| c.get(n)).map(|v| p.array_objects(v)).unwrap_or_default().into_iter().filter(|&o| o != 0).map(|o| p.object_path(o)).collect() };
                        fx.entries.push((dts, list("ParticleSystems"), list("SoundCues")));
                    }
                }
                im.mats.insert(path, fx);
            }
        }
        if let Some(g) = gcgame {
            for (i, e) in g.exports.iter().enumerate() {
                if g.export_class(e) != "Class" {
                    continue;
                }
                let name = g.object_name_of(e);
                if !name.starts_with("GCDmgType") {
                    continue;
                }
                let mut chain = Vec::new();
                let mut k = i as i32 + 1;
                for _ in 0..16 {
                    if k <= 0 {
                        break;
                    }
                    chain.push(g.object_name(k).to_ascii_lowercase());
                    k = g.exports[k as usize - 1].super_index;
                }
                im.chains.insert(name.to_ascii_lowercase(), chain);
            }
        }
        info!("impacts: {} physical materials, {} damage types", im.mats.len(), im.chains.len());
        im
    }

    /// Id (1-based) of a physical material path for CollisionWorld::add_phys.
    pub fn id(&mut self, path: &str) -> u16 {
        let key = path.to_ascii_lowercase();
        if let Some(&i) = self.ids.get(&key) {
            return i;
        }
        self.names.push(key.clone());
        let i = self.names.len() as u16;
        self.ids.insert(key, i);
        i
    }

    pub fn set_terrain(&mut self, layers: upk::terrain::TerrainLayers, phys: [u16; 4]) {
        self.terrain = Some((layers, phys));
    }

    /// Terrain layer physical material at a UE position (the layer with the highest weight).
    pub fn terrain_at(&self, x: f32, y: f32) -> u16 {
        let Some((tl, phys)) = &self.terrain else { return 0 };
        let u = (x - tl.bounds[0]) / (tl.bounds[1] - tl.bounds[0]).max(1.0);
        let v = (y - tl.bounds[2]) / (tl.bounds[3] - tl.bounds[2]).max(1.0);
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return 0;
        }
        let px = ((u * tl.weight_w as f32) as usize).min(tl.weight_w.saturating_sub(1));
        let py = ((v * tl.weight_h as f32) as usize).min(tl.weight_h.saturating_sub(1));
        let base = (py * tl.weight_w + px) * 4;
        let mut best = (0u8, 0u16);
        for (ai, &ph) in phys.iter().enumerate() {
            let w = tl.weights.get(base + upk::terrain::WEIGHT_CHANNEL[ai]).copied().unwrap_or(0);
            if ph != 0 && w > best.0 {
                best = (w, ph);
            }
        }
        best.1
    }

    /// Particle systems and sound cues for a hit on `physmat` (path, any case) by `dmg`
    /// (damage type class name).
    pub fn effect(&self, physmat: &str, dmg: &str) -> Option<(&[String], &[String])> {
        self.effect_where(physmat, dmg, false)
    }

    /// `effect`, but only entries that spawn particle systems count: PM_Default_Exp has an empty
    /// entry for GCDmgType_RocketLauncher, the walk then goes on up the class chain
    /// (GCDmgType_Explosion: ATW_Air).
    pub fn effect_particles(&self, physmat: &str, dmg: &str) -> Option<&[String]> {
        self.effect_where(physmat, dmg, true).map(|e| e.0)
    }

    fn effect_where(&self, physmat: &str, dmg: &str, need_ps: bool) -> Option<(&[String], &[String])> {
        let dl = dmg.to_ascii_lowercase();
        let one = vec![dl.clone()];
        let chain = self.chains.get(&dl).unwrap_or(&one);
        let mut pm = physmat.to_ascii_lowercase();
        for _ in 0..8 {
            let m = self.mats.get(&pm)?;
            let mut best: Option<(usize, &(Vec<String>, Vec<String>, Vec<String>))> = None;
            for e in m.entries.iter().filter(|e| !need_ps || !e.1.is_empty()) {
                if let Some(rank) = chain.iter().position(|c| e.0.contains(c)) {
                    if best.is_none_or(|b| rank < b.0) {
                        best = Some((rank, e));
                    }
                }
            }
            if let Some((_, e)) = best {
                return Some((&e.1, &e.2));
            }
            pm = m.parent.clone()?;
        }
        None
    }

    /// `effect` by physical material id (TERRAIN: the layer at the UE position `at`).
    pub fn effect_id(&self, id: u16, at: Vec3, dmg: &str) -> Option<(&[String], &[String])> {
        let id = if id == TERRAIN { self.terrain_at(at.x, at.y) } else { id };
        let name = self.names.get((id as usize).checked_sub(1)?)?;
        self.effect(name, dmg)
    }

    /// The physical material path of a soldier hit (PM_Library names as in the damage grid).
    pub fn soldier_path(&self, head: bool) -> Option<&str> {
        let want = if head { "pma_head" } else { "pma_soldier_chest" };
        self.mats.keys().find(|k| k.rsplit('.').next() == Some(want)).map(|s| s.as_str())
    }

    /// Particle systems the given damage types can spawn on any material (fx pools).
    pub fn systems_for(&self, dmgs: &[String]) -> Vec<String> {
        let mut v = Vec::new();
        for k in self.mats.keys() {
            for d in dmgs {
                if let Some((ps, _)) = self.effect(k, d) {
                    v.extend(ps.iter().cloned());
                }
                if let Some(ps) = self.effect_particles(k, d) {
                    v.extend(ps.iter().cloned());
                }
            }
        }
        v.sort();
        v.dedup();
        v
    }

    /// Spawn the impact effect of a hit: particle systems along the surface normal, sounds.
    pub fn play(&self, phys: &str, dmg: &str, at: Vec3, normal: Vec3, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue) -> bool {
        let Some((ps, sc)) = self.effect(phys, dmg) else { return false };
        if std::env::var_os("HB_IMPACTDBG").is_some() {
            info!("impact {phys} {dmg} at {at:?}: {ps:?} {sc:?}");
        }
        let rot = crate::combat::dir_quat(normal.normalize_or_zero());
        for p in ps {
            fx.fire(p, at, rot);
        }
        for c in sc {
            sfx.play(c, Some(at));
        }
        true
    }

    /// Explosion of a projectile / grenade with damage type `dmg` at `at` (UE units). The surface
    /// below (`surface`: physical material id, hit point, normal) gives its entry (sounds, dirt /
    /// water / debris particles); the explosion itself comes from PM_Default_Exp (GCObjectPool_Content
    /// ProjExplosionDefaultPM), first entry along the damage type's class chain that has particle
    /// systems, when the surface entry has none. That fallback is NOT from data (the native
    /// GCProjectile explosion code is not decoded): the surface entries of PM_Dirt etc. for
    /// GCDmgType_Explosion only hold sounds, which left rockets / C4 without any explosion.
    pub fn explosion(&self, surface: Option<(u16, Vec3, Vec3)>, dmg: &str, at: Vec3, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue) -> bool {
        let mut fired: Vec<String> = Vec::new();
        let mut sound = false;
        let normal = surface.map(|s| s.2).unwrap_or(Vec3::Z);
        if let Some((id, hit, n)) = surface {
            let id = if id == TERRAIN { self.terrain_at(hit.x, hit.y) } else { id };
            if let Some((ps, sc)) = self.names.get((id as usize).wrapping_sub(1)).and_then(|name| self.effect(name, dmg)) {
                let rot = crate::combat::dir_quat(n.normalize_or_zero());
                for p in ps {
                    if fx.fire(p, hit, rot) {
                        fired.push(p.clone());
                    }
                }
                for c in sc {
                    sfx.play(c, Some(hit));
                    sound = true;
                }
            }
        }
        if let Some(d) = self.default_exp().filter(|_| fired.is_empty()) {
            let rot = crate::combat::dir_quat(normal.normalize_or_zero());
            if let Some(ps) = self.effect_particles(d, dmg) {
                for p in ps {
                    if !fired.contains(p) && fx.fire(p, at, rot) {
                        fired.push(p.clone());
                    }
                }
            }
            if !sound {
                if let Some((_, sc)) = self.effect(d, dmg) {
                    for c in sc {
                        sfx.play(c, Some(at));
                    }
                }
            }
        }
        if std::env::var_os("HB_IMPACTDBG").is_some() {
            info!("explosion {dmg} at {at:?} surface {surface:?}: {fired:?}");
        }
        !fired.is_empty()
    }

    /// Surface particles only (no sounds) of `dmg` for a hit: the Hellfire's TankShell debris
    /// fountains under rocket / C4 explosions (combat.rs, NOT from data).
    pub fn surface_debris(&self, surface: (u16, Vec3, Vec3), dmg: &str, fx: &mut crate::particles::Particles) {
        let (id, hit, n) = surface;
        let id = if id == TERRAIN { self.terrain_at(hit.x, hit.y) } else { id };
        let Some(name) = self.names.get((id as usize).wrapping_sub(1)) else { return };
        if let Some(ps) = self.effect_particles(name, dmg) {
            let rot = crate::combat::dir_quat(n.normalize_or_zero());
            for p in ps {
                fx.fire(p, hit, rot);
            }
        }
    }

    /// `play` by physical material id (terrain resolved at the hit).
    pub fn play_id(&self, id: u16, dmg: &str, at: Vec3, normal: Vec3, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue) -> bool {
        let id = if id == TERRAIN { self.terrain_at(at.x, at.y) } else { id };
        let Some(name) = self.names.get((id as usize).wrapping_sub(1)).cloned() else { return false };
        self.play(&name, dmg, at, normal, fx, sfx)
    }

    /// Effect of a shot (combat::trace_shot result) with damage type `dmg`.
    pub fn shot(&self, hit: &crate::combat::ShotHit, o: Vec3, d: Vec3, dmg: &str, soldier_center: Option<Vec3>, fx: &mut crate::particles::Particles, sfx: &mut crate::sfx::SfxQueue) {
        match hit {
            crate::combat::ShotHit::World(t, n, ph) => {
                self.play_id(*ph, dmg, o + d * *t, *n, fx, sfx);
                // the level's interactive cars (levelcars.rs)
                crate::levelcars::world_hit(o + d * *t, d, dmg);
            }
            crate::combat::ShotHit::Soldier(_, _, at) | crate::combat::ShotHit::Player(_, at) => {
                let head = soldier_center.is_some_and(|c| at.z > c.z + crate::combat::HEAD_OFFSET - crate::combat::HEAD_RADIUS);
                if let Some(p) = self.soldier_path(head).map(String::from) {
                    self.play(&p, dmg, *at, -d, fx, sfx);
                }
            }
            crate::combat::ShotHit::Nothing => {}
        }
    }

    /// Footstep cue of a physical material id (terrain resolved at `at`), Parent chain.
    pub fn footstep(&self, id: u16, at: Vec3) -> Option<&str> {
        let id = if id == TERRAIN { self.terrain_at(at.x, at.y) } else { id };
        let mut pm = self.names.get((id as usize).checked_sub(1)?)?.clone();
        for _ in 0..8 {
            let m = self.mats.get(&pm)?;
            if let Some(c) = m.footstep.first() {
                return Some(c.as_str());
            }
            pm = m.parent.clone()?;
        }
        None
    }

    /// Default explosion material (PM_Library.Default.PM_Default_Exp) path, if loaded.
    pub fn default_exp(&self) -> Option<&str> {
        self.mats.keys().find(|k| k.ends_with(".pm_default_exp")).map(|s| s.as_str())
    }

    /// All particle systems any entry can spawn (fx pools).
    pub fn all_systems(&self) -> Vec<String> {
        let mut v: Vec<String> = self.mats.values().flat_map(|m| m.entries.iter().flat_map(|e| e.1.iter().cloned())).collect();
        v.sort();
        v.dedup();
        v
    }
}

/// PhysMaterial of a material reference (Material.PhysMaterial, else the MaterialInstance's
/// Parent chain), resolved across the loaded packages.
pub fn material_phys(sp: &crate::world::Spawner, pi: usize, mat: i32) -> Option<String> {
    let (mut p, mut i) = sp.resolve(pi, mat)?;
    for _ in 0..8 {
        let pr = sp.pkgs[p].export_props(i)?;
        if let Some(pm) = pr.object("PhysMaterial").filter(|&o| o != 0) {
            return Some(sp.pkgs[p].object_path(pm));
        }
        let parent = pr.object("Parent").filter(|&o| o != 0)?;
        (p, i) = sp.resolve(p, parent)?;
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn lookup() {
        let root = std::path::Path::new("/mnt/user-data/uploads/HOMEFRONT/GCGame/CookedPC");
        let (Ok(m), Ok(g)) = (upk::Package::open(root.join("Maps/MP/Crossroads/FL-Crossroads.map")), upk::Package::open(root.join("GCGame.u"))) else { return };
        let im = super::Impacts::load(std::slice::from_ref(&m), Some(&g));
        for (pm, d) in [("PM_Library.Env.Rock.PM_Rock", "GCDmgType_RIF_M4"), ("PM_Library.Env.Wood.PM_Grass", "GCDmgType_PST_M9"), ("PM_Library.Armor.Soldier.PMA_Soldier_Chest", "GCDmgType_RIF_M4"), ("PM_Library.Armor.Soldier.PMA_Head", "GCDmgType_SNI_M110"), ("PM_Library.Env.Dirt.PM_Mud", "GCDmgType_EXP_Frag"), ("PM_Library.Default.PM_Default_Exp", "GCDmgType_EXP_C4"), ("PM_Library.Env.Rock.PM_Concrete", "GCDmgType_M1114_Seat1_Bullet")] {
            println!("{pm} {d}: {:?}", im.effect(pm, d));
        }
        println!("soldier {:?} default exp {:?}", im.soldier_path(false), im.default_exp());
    }
}

/// Footsteps of the local player counted by the viewmodel (AnimNotify_Footstep of its walk /
/// sprint layer).
#[derive(Resource, Default)]
pub struct PlayerSteps(pub u32);

/// The local player's own steps: their cue at this factor (user request 2026-10-18 "much too
/// loud"; NOT from data).
const PLAYER_STEP_VOLUME: f32 = 0.35;

/// Footsteps (FootstepInstigators of the physical material under the feet): one step per half
/// locomotion cycle of the 3P body (root motion per cycle of run_front_ads_us / mp_sprint_F,
/// AnimLib::root_cycle) - the footstep notifies of the sequences are not read (NOT from data:
/// the step timing by distance). The player hears his own steps 2D, bots' steps are placed.
pub fn footsteps(
    impacts: Option<Res<Impacts>>,
    assets: Option<Res<crate::bots::BotAssets>>,
    col: Res<crate::player::CollisionWorld>,
    pawns: Query<&crate::player::Pawn>,
    soldiers: Query<&crate::combat::Soldier>,
    mut sfx: ResMut<crate::sfx::SfxQueue>,
    time: Res<Time>,
    mut acc: Local<(f32, std::collections::HashMap<u32, (f32, Vec3)>)>,
    mut buf: Local<Vec<u32>>,
    (player_steps, mut steps_seen): (Res<PlayerSteps>, Local<u32>),
) {
    let (Some(im), Some(assets)) = (impacts, assets) else { return };
    let run = assets.lib.root_cycle("run_front_ads_us").map(|c| c.0 * 0.5).unwrap_or(70.0);
    let sprint = assets.lib.root_cycle("mp_sprint_F").map(|c| c.0 * 0.5).unwrap_or(220.0);
    let dt = time.delta_secs().min(0.1);
    let step = |feet: Vec3, d: f32, at: Option<Vec3>, buf: &mut Vec<u32>, sfx: &mut crate::sfx::SfxQueue| {
        let _ = d;
        let vol = if at.is_none() { PLAYER_STEP_VOLUME } else { 1.0 };
        let o = feet + Vec3::Y * 0.3;
        if let Some((_, _, tri)) = col.raycast_hit(o, Vec3::NEG_Y, 0.8, buf, crate::player::COL_PLAYER) {
            let ue = Vec3::new(feet.x, feet.z, feet.y) * 100.0;
            if let Some(c) = im.footstep(col.phys_of(tri), ue) {
                if std::env::var_os("HB_IMPACTDBG").is_some() {
                    info!("footstep {c} at {at:?}");
                }
                sfx.play_volume(c, at, vol);
            }
        }
    };
    // the player: one step per AnimNotify_Footstep of the first-person layers (viewmodel.rs)
    let n = player_steps.0.wrapping_sub(*steps_seen);
    *steps_seen = player_steps.0;
    if let Ok(p) = pawns.single() {
        if n > 0 && p.on_ground && !p.dead && !p.in_vehicle && !p.remote {
            step(p.feet, 0.0, None, &mut buf, &mut sfx);
        }
    }
    let _ = &mut acc.0;
    // bots within 25 m of the player
    let me = pawns.single().ok().map(|p| p.feet);
    let mut seen = Vec::new();
    for (k, s) in soldiers.iter().enumerate() {
        if !s.alive || me.is_none_or(|m| m.distance(s.feet) > 25.0) {
            continue;
        }
        let key = k as u32;
        seen.push(key);
        let e = acc.1.entry(key).or_insert((0.0, s.feet));
        let moved = Vec2::new(s.feet.x - e.1.x, s.feet.z - e.1.z).length() * 100.0;
        e.1 = s.feet;
        if moved > 0.2 && moved < 200.0 {
            e.0 += moved;
            let len = if moved / dt.max(1e-3) > 400.0 { sprint } else { run };
            if e.0 >= len {
                e.0 = 0.0;
                let ue = Vec3::new(s.feet.x, s.feet.z, s.feet.y) * 100.0;
                step(s.feet, moved, Some(ue), &mut buf, &mut sfx);
            }
        }
    }
    acc.1.retain(|k, _| seen.contains(k));
}
