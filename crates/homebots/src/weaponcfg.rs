//! Infantry weapon configuration (Weapons_FL2/WeaponConfigs.upk, GCInfantryWeaponInfo
//! MP.Infantry.CFG_*), the default loadouts (GCGame.ini [GCCustomLoadoutProfiles]
//! DefaultLoadouts), the damage grid (MP.Damage.DamageGridMP) and the frag grenade
//! (MP.Grenades.CFG_EXP_Frag + Default__GCThrowableWeaponInfo / GCProj_Grenade / GCProjectile).
use upk::props::{Props, Value};
use upk::Package;

#[derive(Clone, Debug, Default)]
pub struct Recoil {
    pub pitch_min: f32,
    pub pitch_max: f32,
    pub yaw_min: f32,
    pub yaw_max: f32,
    pub recovery: f32,
    pub speed: f32,
    pub duration: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Deviation {
    pub min: f32,
    pub max: f32,
    pub decay: f32,
    pub fire_add: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ScopeCfg {
    /// GCWeaponScope name (CFG_Scope_*)
    pub name: String,
    /// skeletal mesh object paths (package.group.name)
    pub mesh: Option<String>,
    pub base: Option<String>,
    pub hide_front_sight: bool,
    /// the weapon's Scopes entry bHideRearSightBone (M249, ACR: the rear iron sight is hidden
    /// under a scope)
    pub hide_rear_sight: bool,
    /// horizontal FOV while aiming (world / 1P mesh)
    pub fov_zoom: f32,
    pub fov_mesh_zoom: f32,
    pub offset_zoomed: [f32; 3],
    pub ads_blend: (f32, f32),
}

#[derive(Clone, Debug, Default)]
pub struct WeaponCfg {
    pub cfg: String,
    pub display: String,
    pub sk_mesh: String,
    /// third-person mesh (SKMesh3P)
    pub sk_mesh_3p: Option<String>,
    pub bot_fire_rate: f32,
    pub bot_refire_rate: f32,
    /// MeshParts without a bone (weapon body), attached to the "Weapon" bone
    pub parts: Vec<String>,
    pub anim_set: String,
    pub scopes_rel: [f32; 3],
    pub scope: Option<ScopeCfg>,
    /// ScopeRailMesh (M249: SKW_ACH_M249_Rail), drawn with a scope
    pub scope_rail: Option<String>,
    pub fire_interval: f32,
    pub clip: i32,
    pub total: i32,
    pub max: i32,
    pub reload_time: f32,
    pub range: f32,
    pub full_range: f32,
    /// damage type class name (GCDmgType_*)
    pub dmg_type: String,
    pub full_auto: bool,
    /// bDisableZoom (C4 detonator)
    pub disable_zoom: bool,
    pub equip_time: f32,
    pub fire_anim_time: f32,
    pub reload_anim_time: f32,
    /// FOVMeshZoom of weapons without a scope entry (M110 5: its scope fills the view)
    pub fov_mesh_zoom: f32,
    /// Crosshair.UnZoomed.bCrosses: the hip crosshair's four dashes
    pub crosses: bool,
    pub fov_zoom: f32,
    pub ads_up: f32,
    /// FOVMesh (1P mesh FOV at the hip, UE default 50)
    pub fov_mesh: f32,
    /// ADSBlendStartTime / EndTime (1P mesh FOV change) and WorldFOVStartTime / EndTime (world
    /// FOV change), seconds into the zoom
    pub mesh_fov_time: (f32, f32),
    pub world_fov_time: (f32, f32),
    pub ads_down: f32,
    pub recoil_hip: Recoil,
    pub recoil_zoom: Recoil,
    pub spread_stand: Deviation,
    pub spread_crouch: Deviation,
    pub move_speed: f32,
    pub ads_move: f32,
    /// EWFT_Projectile: WeaponProjectile class (GCGameContent.GCProj_*_C) and its defaults
    pub projectile: Option<ProjectileCfg>,
    /// EffectsInfo (KFX_WeaponNodes.*) and its Day muzzle flash templates (1P, 3P, ADS)
    pub effects: Option<String>,
    pub muzzle: [Option<String>; 3],
    /// Day WeaponEffectTracer of View1P / View3P
    pub tracer: [Option<TracerCfg>; 2],
    /// UIIcon (kill crawl)
    pub ui_icon: Option<String>,
    /// GCSoundInfoWeapon (A2_Weapons_C.*.SI_*)
    pub sound_info: Option<String>,
    /// HUD bullet icon (UI_PlayerHUD.Textures.HUD.*) and its size (1280x720 units)
    pub bullet_icon: Option<String>,
    pub bullet_icon_size: [f32; 2],
    /// GCWeaponInfo LockOn (LockOnConfigInfo, bEnabled): CFG_ATW_JAV has one; CFG_ATW_PROX has
    /// none in the data and gets the Javelin's (main.rs, user request, NOT from data)
    pub lock_on: Option<LockOnCfg>,
}

/// GCWeaponInfo.LockOnConfigInfo (CFG_ATW_JAV: SupportedPawnClasses GCVehicle, BigRectangle
/// -239,-115 .. 239,130.8, SmallRectangle lock +-40 / material -40,-49 .. 49,40, LockTime 1.2,
/// LockGraceTime 2, LockGraceMaxAngle 90, MaxLockDist 15000; struct defaults MinLockDist 0,
/// bAllowUnZoomedLock false, bSkipLineOfSightCheck false). 1280x720 units around the centre.
#[derive(Clone, Debug, Default)]
pub struct LockOnCfg {
    pub big: ([f32; 2], [f32; 2]),
    pub small_lock: ([f32; 2], [f32; 2]),
    pub small_mat: ([f32; 2], [f32; 2]),
    pub lock_time: f32,
    pub grace_time: f32,
    /// degrees
    pub grace_angle: f32,
    pub min_dist: f32,
    pub max_dist: f32,
    pub allow_unzoomed: bool,
    pub skip_los: bool,
    /// SupportedPawnClasses (object names)
    pub classes: Vec<String>,
    /// the config it was read from (CFG_ATW_JAV for the PROX)
    pub from: String,
}

/// LockOn of a WeaponConfigs entry (None without one or with bEnabled false).
pub fn read_lock_on(p: &Package, cfg: &str) -> Option<LockOnCfg> {
    let i = find(p, cfg, "GCInfantryWeaponInfo")?;
    let pr = p.export_props(i)?;
    let l = p.struct_prop(&pr, "LockOn")?;
    if !l.bool("bEnabled").unwrap_or(false) {
        return None;
    }
    let rect = |n: &str, a: &str, b: &str| p.struct_prop(&l, n).and_then(|r| Some((vec2(&r, a)?, vec2(&r, b)?)));
    let classes = match l.get("SupportedPawnClasses") {
        Some(Value::Raw { data, .. }) if data.len() >= 4 => {
            let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
            (0..n).filter_map(|k| data.get(4 + k * 4..8 + k * 4)).map(|b| p.object_name(i32::from_le_bytes(b.try_into().unwrap()))).collect()
        }
        _ => Vec::new(),
    };
    Some(LockOnCfg {
        big: rect("BigRectangle", "LockTopLeftCorner", "LockBottomRightCorner")?,
        small_lock: rect("SmallRectangle", "LockTopLeftCorner", "LockBottomRightCorner").unwrap_or(([-40.0, -40.0], [40.0, 40.0])),
        small_mat: rect("SmallRectangle", "MaterialTopLeftCorner", "MaterialBottomRightCorner").unwrap_or(([-40.0, -40.0], [40.0, 40.0])),
        lock_time: l.float("LockTime").unwrap_or(0.0),
        grace_time: l.float("LockGraceTime").unwrap_or(0.0),
        grace_angle: l.float("LockGraceMaxAngle").unwrap_or(0.0),
        min_dist: l.float("MinLockDist").unwrap_or(0.0),
        max_dist: l.float("MaxLockDist").unwrap_or(0.0),
        allow_unzoomed: l.bool("bAllowUnZoomedLock").unwrap_or(false),
        skip_los: l.bool("bSkipLineOfSightCheck").unwrap_or(false),
        classes,
        from: cfg.to_string(),
    })
}

fn vec2(pr: &Props, name: &str) -> Option<[f32; 2]> {
    match pr.get(name)? {
        Value::Raw { data, .. } if data.len() >= 8 => Some([f32::from_le_bytes(data[0..4].try_into().ok()?), f32::from_le_bytes(data[4..8].try_into().ok()?)]),
        _ => None,
    }
}

fn recoil(pr: &Props) -> Recoil {
    Recoil {
        pitch_min: pr.float("ViewPitchMin").unwrap_or(0.0),
        pitch_max: pr.float("ViewPitchMax").unwrap_or(0.0),
        yaw_min: pr.float("ViewYawMin").unwrap_or(0.0),
        yaw_max: pr.float("ViewYawMax").unwrap_or(0.0),
        recovery: pr.float("ViewRecoveryRate").unwrap_or(0.0),
        speed: pr.float("Speed").unwrap_or(0.0),
        duration: pr.float("Duration").unwrap_or(0.0),
    }
}

fn deviation(pr: &Props) -> Deviation {
    Deviation { min: pr.float("Min").unwrap_or(0.0), max: pr.float("Max").unwrap_or(0.0), decay: pr.float("DecayRate").unwrap_or(0.0), fire_add: pr.float("FireAdd").unwrap_or(0.0) }
}

/// Export index of an object by name, preferring the MP variant.
pub fn find(p: &Package, name: &str, class: &str) -> Option<usize> {
    let mut best = None;
    for (i, e) in p.exports.iter().enumerate() {
        if p.object_name_of(e).eq_ignore_ascii_case(name) && (class.is_empty() || p.export_class(e) == class) {
            if p.object_path(i as i32 + 1).starts_with("MP.") {
                return Some(i);
            }
            best.get_or_insert(i);
        }
    }
    best
}

fn obj_path(p: &Package, o: Option<i32>) -> Option<String> {
    o.filter(|&o| o != 0).map(|o| p.object_path(o))
}

/// UIIcon_Small (DPad icon) of a WeaponConfigs entry of any class (special weapons and
/// equipment such as CFG_EQP_FlakJacket).
pub fn small_icon(p: &Package, cfg: &str) -> Option<String> {
    let i = find(p, cfg, "")?;
    obj_path(p, p.export_props(i)?.object("UIIcon_Small"))
}

pub fn read_weapon(p: &Package, cfg: &str, scope_cfg: Option<&str>) -> Option<WeaponCfg> {
    let i = find(p, cfg, "GCInfantryWeaponInfo")?;
    let pr = p.export_props(i)?;
    let mut w = WeaponCfg { cfg: cfg.to_string(), ..Default::default() };
    w.display = match pr.get("WeaponDisplayName") {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    };
    w.sk_mesh = obj_path(p, pr.object("SKMesh"))?;
    w.sk_mesh_3p = obj_path(p, pr.object("SKMesh3P"));
    w.bot_fire_rate = pr.float("BotMaxFireRate").unwrap_or(1.0);
    w.bot_refire_rate = pr.float("BotMaxRefireRate").unwrap_or(2.0);
    for part in p.struct_array(&pr, "MeshParts") {
        let bone = part.enum_name("BoneName").unwrap_or("None");
        if bone == "None" {
            if let Some(m) = obj_path(p, part.object("Mesh")) {
                w.parts.push(m);
            }
        }
    }
    w.anim_set = obj_path(p, pr.object("CH_1P_AnimSet"))?;
    w.scopes_rel = pr.vector("ScopesRelativeLocation").unwrap_or([0.0; 3]);
    w.equip_time = pr.float("EquipTime").unwrap_or(0.5);
    w.fire_anim_time = pr.float("FireAnimTime").unwrap_or(0.2);
    w.reload_anim_time = pr.float("ReloadAnimTime").unwrap_or(1.5);
    w.fov_zoom = pr.float("FOVZoom").unwrap_or(60.0);
    w.fov_mesh_zoom = pr.float("FOVMeshZoom").unwrap_or(40.0);
    w.crosses = p.struct_prop(&pr, "Crosshair").and_then(|c| p.struct_prop(&c, "UnZoomed")).is_some_and(|u| matches!(u.get("bCrosses"), Some(upk::props::Value::Bool(true))));
    w.ads_up = pr.float("ADSBringUpTime").unwrap_or(0.2);
    w.disable_zoom = pr.bool("bDisableZoom").unwrap_or(false);
    w.fov_mesh = pr.float("FOVMesh").unwrap_or(50.0);
    w.mesh_fov_time = (pr.float("ADSBlendStartTime").unwrap_or(0.0), pr.float("ADSBlendEndTime").unwrap_or(w.ads_up));
    w.world_fov_time = (pr.float("WorldFOVStartTime").unwrap_or(0.0), pr.float("WorldFOVEndTime").unwrap_or(w.ads_up));
    w.ads_down = pr.float("ADSBringDownTime").unwrap_or(0.175);
    w.move_speed = pr.float("MovementSpeedModifier").unwrap_or(1.0);
    w.bullet_icon = obj_path(p, pr.object("BulletIcon"));
    w.effects = obj_path(p, pr.object("EffectsInfo"));
    w.ui_icon = obj_path(p, pr.object("UIIcon"));
    w.sound_info = match pr.get("SoundInfo") {
        Some(Value::Raw { data, .. }) if data.len() >= 8 => obj_path(p, Some(i32::from_le_bytes(data[4..8].try_into().unwrap()))),
        _ => None,
    };
    w.bullet_icon_size = vec2(&pr, "BulletIconSize").unwrap_or([4.0, 16.0]);
    w.lock_on = read_lock_on(p, cfg);
    w.ads_move = pr.float("ADSMovementPercent").unwrap_or(0.4);
    if let Some(rc) = p.struct_prop(&pr, "Recoil") {
        if let Some(h) = p.struct_prop(&rc, "Hip") {
            w.recoil_hip = recoil(&h);
        }
        if let Some(z) = p.struct_prop(&rc, "Zoom") {
            w.recoil_zoom = recoil(&z);
        }
    }
    if let Some(sp) = p.struct_prop(&pr, "Spread") {
        if let Some(s) = p.struct_prop(&sp, "Stand") {
            w.spread_stand = deviation(&s);
        }
        if let Some(s) = p.struct_prop(&sp, "Crouch") {
            w.spread_crouch = deviation(&s);
        }
    }
    if let Some(fm) = p.struct_array(&pr, "FireModes").into_iter().next() {
        w.fire_interval = fm.float("FireInterval").unwrap_or(0.1);
        w.clip = fm.int("AmmoClip").unwrap_or(30);
        w.total = fm.int("AmmoTotal").unwrap_or(90);
        w.max = fm.int("AmmoMax").unwrap_or(w.total);
        w.reload_time = fm.float("ReloadTime").unwrap_or(w.reload_anim_time);
        w.range = fm.float("WeaponRange").unwrap_or(3500.0);
        w.full_range = fm.float("FullDamageRange").unwrap_or(1000.0);
        w.full_auto = fm.enum_name("FireSelector") == Some("EWFS_FullAuto");
        w.dmg_type = fm.object("InstantHitDamageType").filter(|&o| o != 0).map(|o| p.object_name(o)).unwrap_or_default();
        if fm.enum_name("WeaponFireType") == Some("EWFT_Projectile") {
            w.projectile = obj_path(p, fm.object("WeaponProjectile")).map(|c| ProjectileCfg { class: c, ..Default::default() });
        }
    }
    if let Some(sc) = scope_cfg {
        // the weapon's Scopes table lists one variant per scope type; the loadout may name another
        // variant (CFG_Scope_Holographic_var3): match the type, take the meshes of the named one
        let base = |n: &str| n.to_ascii_lowercase().split("_var").next().unwrap_or("").to_string();
        let entries = p.struct_array(&pr, "Scopes");
        let pick = entries
            .iter()
            .find(|s| s.object("Scope").filter(|&o| o != 0).is_some_and(|o| p.object_name(o).eq_ignore_ascii_case(sc)))
            .or_else(|| entries.iter().find(|s| s.object("Scope").filter(|&o| o != 0).is_some_and(|o| base(&p.object_name(o)) == base(sc))));
        if let Some(s) = pick {
            let mut cfg = ScopeCfg {
                name: sc.to_string(),
                fov_zoom: s.float("FOVZoom").unwrap_or(w.fov_zoom),
                fov_mesh_zoom: s.float("FOVMeshZoom").unwrap_or(50.0),
                offset_zoomed: s.vector("OffsetLocationZoomed").unwrap_or([0.0; 3]),
                ads_blend: (s.float("ADSBlendStartTime").unwrap_or(0.1), s.float("ADSBlendEndTime").unwrap_or(0.2)),
                hide_rear_sight: s.bool("bHideRearSightBone").unwrap_or(false),
                ..Default::default()
            };
            if let Some(si) = find(p, sc, "GCWeaponScope") {
                if let Some(sp) = p.export_props(si) {
                    cfg.mesh = obj_path(p, sp.object("Mesh"));
                    cfg.base = obj_path(p, sp.object("MeshBase"));
                    cfg.hide_front_sight = sp.bool("bHideFrontSight").unwrap_or(false);
                }
            }
            w.scope = Some(cfg);
            w.scope_rail = p.struct_prop(&pr, "ScopeRailMesh").and_then(|r| obj_path(p, r.object("Mesh")));
        }
    }
    Some(w)
}

/// Default loadouts: (primary CFG, its scope CFG) per DefaultLoadouts entry, plus the
/// remaining items.
#[derive(Clone, Debug, Default)]
pub struct LoadoutCfg {
    pub primary: String,
    pub scope: Option<String>,
    pub items: Vec<String>,
}

fn coalesced(cooked: &std::path::Path) -> Option<upk::ui::Loc> {
    upk::ui::Loc::load(&cooked.join("Coalesced_int.bin")).or_else(|| {
        std::fs::read_dir(cooked).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("Coalesced_"))).and_then(|p| upk::ui::Loc::load(&p))
    })
}

/// GCBattlePoints.ini SpecialAbilityCosts: class -> (Cost, PurchaseAmount, StingerName cue),
/// plus GCGame.ini [GCGame.GCWeap_EQP_FlakJacket] MaxHealthScale.
/// A float of the Coalesced ini ("5.f" style values accepted).
pub fn ini_f32(cooked: &std::path::Path, key: &str) -> Option<f32> {
    let loc = coalesced(cooked)?;
    let v = loc.ini.get(&key.to_ascii_lowercase())?.first()?.clone();
    v.trim().trim_end_matches(';').trim_end_matches('f').trim_end_matches('.').parse().ok().or_else(|| v.trim().trim_end_matches(";").trim_end_matches('f').parse().ok())
}

pub fn special_costs(cooked: &std::path::Path) -> (std::collections::HashMap<String, (i32, i32, Option<String>)>, f32) {
    let mut out = std::collections::HashMap::new();
    let Some(loc) = coalesced(cooked) else { return (out, 1.5) };
    for s in loc.ini.get("gcbattlepoints.default.specialabilitycosts").cloned().unwrap_or_default() {
        let Some(c) = crate::spawnsel::ue_field(&s, "AbilityClassName") else { continue };
        let num = |k: &str| crate::spawnsel::ue_field(&s, k).map(|v| v.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect::<String>()).and_then(|v| v.parse::<i32>().ok());
        let stinger = crate::spawnsel::ue_field(&s, "StingerName").map(|v| v.trim_matches('"').to_string());
        out.insert(c.trim_matches('"').to_ascii_lowercase(), (num("Cost").unwrap_or(0), num("PurchaseAmount").unwrap_or(1), stinger));
    }
    for s in loc.ini.get("gcbattlepoints.default.vehiclecosts").cloned().unwrap_or_default() {
        let Some(c) = crate::spawnsel::ue_field(&s, "VehicleClassName") else { continue };
        let cost = crate::spawnsel::ue_field(&s, "BPCost").and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
        out.insert(c.trim_matches('"').to_ascii_lowercase(), (cost, 1, None));
    }
    let flak = loc.ini.get("gcgame.gcgame.gcweap_eqp_flakjacket.maxhealthscale").and_then(|v| v.first()).and_then(|v| v.parse().ok()).unwrap_or(1.5);
    (out, flak)
}

pub fn default_loadouts(cooked: &std::path::Path) -> Vec<LoadoutCfg> {
    let Some(loc) = coalesced(cooked) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for l in loc.ini.get("gcgame.gcgame.gccustomloadoutprofiles.defaultloadouts").cloned().unwrap_or_default() {
        let mut lo = LoadoutCfg::default();
        if let Some(groups) = crate::spawnsel::ue_field(&l, "InventoryGroups") {
            for (k, g) in crate::spawnsel::ue_list(groups).iter().enumerate() {
                let Some(p) = crate::spawnsel::ue_field(g, "PrimaryItem") else { continue };
                if k == 0 {
                    lo.primary = p.to_string();
                    lo.scope = crate::spawnsel::ue_field(g, "SecondaryItems").and_then(|s| crate::spawnsel::ue_list(s).into_iter().find(|x| x.to_ascii_lowercase().starts_with("cfg_scope")));
                } else {
                    lo.items.push(p.to_string());
                }
            }
        }
        if !lo.primary.is_empty() {
            out.push(lo);
        }
    }
    out
}

/// MP damage grid: damage type x physical material -> (near, far) damage.
#[derive(Clone, Debug, Default)]
pub struct DamageGrid {
    pub types: Vec<String>,
    pub mats: Vec<String>,
    pub cells: Vec<(u16, u16)>,
    /// damage type class (lower case) without a row -> the nearest parent class with one
    /// (GCDmgType_LAV_Seat0_Bullet -> GCDmgType_Veh_30mm, GCDmgType_AH64D_Seat1_Bullet ->
    /// GCDmgType_Veh_30mm): without it those guns did no damage at all (user 2026-10-20)
    pub alias: std::collections::HashMap<String, String>,
}

impl DamageGrid {
    pub fn load(p: &Package) -> Option<DamageGrid> {
        let i = p.exports.iter().position(|e| p.object_name_of(e) == "DamageGridMP" && p.export_class(e) == "GCDamageGrid")?;
        let pr = p.export_own_props(i)?;
        let names = |n: &str| -> Vec<String> {
            match pr.get(n) {
                Some(Value::Raw { data, .. }) if data.len() >= 4 => {
                    let c = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
                    (0..c)
                        .filter_map(|k| {
                            let o = 4 + k * 8;
                            let b = data.get(o..o + 8)?;
                            Some(p.name(upk::FName { index: i32::from_le_bytes(b[0..4].try_into().unwrap()), number: i32::from_le_bytes(b[4..8].try_into().unwrap()) }))
                        })
                        .collect()
                }
                _ => Vec::new(),
            }
        };
        let types = names("DamageTypes");
        let mats = names("PhysicalMaterials");
        let cells = p
            .struct_array(&pr, "Data")
            .iter()
            .map(|c| {
                let v = c.int("Data").unwrap_or(0) as u32;
                ((v >> 16) as u16, (v & 0xffff) as u16)
            })
            .collect();
        Some(DamageGrid { types, mats, cells, alias: Default::default() })
    }

    /// Fills `alias` from the damage type classes of the script packages (super chains).
    pub fn add_aliases(&mut self, pkgs: &[&Package]) {
        for p in pkgs {
            for (i, e) in p.exports.iter().enumerate() {
                if p.export_class(e) != "Class" {
                    continue;
                }
                let name = p.object_name(i as i32 + 1);
                if !name.contains("DmgType") || self.types.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                let (mut cp, mut ci): (&Package, i32) = (p, e.super_index);
                for _ in 0..16 {
                    if ci == 0 {
                        break;
                    }
                    let n = cp.object_name(ci);
                    if let Some(t) = self.types.iter().find(|t| t.eq_ignore_ascii_case(&n)) {
                        self.alias.insert(name.to_ascii_lowercase(), t.clone());
                        break;
                    }
                    if ci > 0 {
                        ci = cp.exports[ci as usize - 1].super_index;
                        continue;
                    }
                    // parent in another script package: by name there
                    match pkgs.iter().find_map(|q| q.exports.iter().position(|x| q.object_name_of(x) == n && q.export_class(x) == "Class").map(|k| (*q, k))) {
                        Some((q, k)) => {
                            cp = q;
                            ci = q.exports[k].super_index;
                        }
                        None => break,
                    }
                }
            }
        }
    }

    /// The grid row of a damage type (its own or the nearest parent's).
    pub fn row_name<'a>(&'a self, dtype: &'a str) -> &'a str {
        if self.types.iter().any(|x| x.eq_ignore_ascii_case(dtype)) {
            return dtype;
        }
        self.alias.get(&dtype.to_ascii_lowercase()).map(|s| s.as_str()).unwrap_or(dtype)
    }

    /// Damage of a hit at `dist` (UE units): near value up to full_range, far value at range
    /// (linear in between; the grid's two halves).
    pub fn damage(&self, dtype: &str, mat: &str, dist: f32, full_range: f32, range: f32) -> f32 {
        let dtype = self.row_name(dtype);
        let Some(t) = self.types.iter().position(|x| x.eq_ignore_ascii_case(dtype)) else { return 0.0 };
        let Some(m) = self.mats.iter().position(|x| x.eq_ignore_ascii_case(mat)) else { return 0.0 };
        let Some(&(near, far)) = self.cells.get(t * self.mats.len() + m) else { return 0.0 };
        if dist <= full_range || range <= full_range {
            return near as f32;
        }
        let f = ((dist - full_range) / (range - full_range)).clamp(0.0, 1.0);
        near as f32 + (far as f32 - near as f32) * f
    }
}

/// Frag grenade (MP.Grenades.CFG_EXP_Frag): class defaults of GCThrowableWeaponInfo and the
/// projectile chain GCProj_EXP_Frag_C -> GCProj_HandGrenade -> GCProj_Grenade -> GCProjectile.
#[derive(Clone, Debug)]
pub struct GrenadeCfg {
    pub fuse: f32,
    pub pre_throw: f32,
    pub throw_anim: f32,
    pub ammo: i32,
    pub speed: f32,
    pub toss_z: f32,
    pub radius: f32,
    pub dampen: f32,
    pub dampen_parallel: f32,
    pub dmg_type: String,
    /// Default__GCProj_EXP_Frag_C ExplosionSound / GrenadeBounceSound
    pub explosion_sound: Option<String>,
    pub bounce_sound: Option<String>,
}

impl Default for GrenadeCfg {
    fn default() -> Self {
        GrenadeCfg { fuse: 3.0, pre_throw: 0.56, throw_anim: 0.5, ammo: 2, speed: 1500.0, toss_z: 100.0, radius: 400.0, dampen: 0.2, dampen_parallel: 0.45, dmg_type: "GCDmgType_EXP_Frag".into(), explosion_sound: None, bounce_sound: None }
    }
}

pub fn read_grenade(gcgame: &Package, content: Option<&Package>) -> GrenadeCfg {
    let mut g = GrenadeCfg::default();
    let dflt = |name: &str| gcgame.exports.iter().position(|e| gcgame.object_name_of(e) == name).and_then(|i| gcgame.export_props(i));
    if let Some(pr) = dflt("Default__GCThrowableWeaponInfo") {
        g.fuse = pr.float("GrenadeFuseTime").unwrap_or(g.fuse);
        g.pre_throw = pr.float("PreThrowAnimationTime").unwrap_or(g.pre_throw);
        g.throw_anim = pr.float("ThrowAnimationTime").unwrap_or(g.throw_anim);
        g.ammo = pr.int("Ammo").unwrap_or(g.ammo);
    }
    if let Some(c) = content {
        if let Some(pr) = c.exports.iter().position(|e| c.object_name_of(e) == "Default__GCProj_EXP_Frag_C").and_then(|i| c.export_props(i)) {
            g.explosion_sound = obj_path(c, pr.object("ExplosionSound"));
            g.bounce_sound = obj_path(c, pr.object("GrenadeBounceSound"));
        }
    }
    if let Some(pr) = dflt("Default__GCProj_Grenade") {
        g.speed = pr.float("Speed").unwrap_or(g.speed);
        g.toss_z = pr.float("TossZ").unwrap_or(g.toss_z);
        g.radius = pr.float("DamageRadius").unwrap_or(g.radius);
        g.dampen = pr.float("m_fDampenFactor").unwrap_or(g.dampen);
        g.dampen_parallel = pr.float("m_fDampenFactorParallel").unwrap_or(g.dampen_parallel);
    }
    g
}

#[cfg(test)]
mod tests {
    #[test]
    fn m4_config() {
        let root = std::path::Path::new("/mnt/user-data/uploads/HOMEFRONT/GCGame/CookedPC");
        let Ok(p) = upk::Package::open(root.join("Weapons_FL2/WeaponConfigs.upk")) else { return };
        let w = super::read_weapon(&p, "CFG_RIF_M4", Some("CFG_Scope_Holographic_var1")).unwrap();
        println!("{w:#?}");
        assert_eq!(w.clip, 30);
        let lo = super::default_loadouts(root);
        println!("{lo:#?}");
        for l in &lo {
            if let Some(w) = super::read_weapon(&p, &l.primary, l.scope.as_deref()) {
                println!("{}: mesh {} parts {:?} scope {:?}", w.cfg, w.sk_mesh, w.parts, w.scope.as_ref().map(|s| (&s.mesh, &s.base)));
            }
        }
        let g = super::DamageGrid::load(&p).unwrap();
        println!("M4 chest near {}", g.damage("GCDmgType_RIF_M4", "PMA_Soldier_Chest", 0.0, 1000.0, 3500.0));
    }
}

/// GCWeaponEffectsInfo WeaponEffectTracer: ParticleSystem with the instance parameters the
/// tracer code sets (TracerSize, TracerColor, TracerSpeed, TracerOffset), every Frequency-th shot.
#[derive(Clone, Debug, Default)]
pub struct TracerCfg {
    pub template: String,
    pub size: [f32; 3],
    pub color: [f32; 3],
    pub offset: [f32; 3],
    pub speed: f32,
    pub frequency: u32,
}

/// Day tracers (View1P, View3P) of a KFX_WeaponNodes effects object.
pub fn tracers(nodes: &Package, effects: &str) -> [Option<TracerCfg>; 2] {
    let mut out: [Option<TracerCfg>; 2] = Default::default();
    let rest = effects.split_once('.').map(|x| x.1).unwrap_or(effects);
    let Some(i) = nodes.exports.iter().enumerate().position(|(i, _)| nodes.object_path(i as i32 + 1).eq_ignore_ascii_case(rest)) else { return out };
    let Some(pr) = nodes.export_own_props(i) else { return out };
    for (k, view) in ["View1P", "View3P"].iter().enumerate() {
        let Some(t) = nodes.struct_prop(&pr, view).and_then(|v| nodes.struct_prop(&v, "Day")).and_then(|d| nodes.struct_prop(&d, "Tracer")) else { continue };
        let Some(template) = obj_path(nodes, t.object("ParticleSystem")) else { continue };
        out[k] = Some(TracerCfg {
            template,
            size: t.vector("Size").unwrap_or([5.0, 12.0, 0.0]),
            color: t.vector("TracerColor").unwrap_or([1.0; 3]),
            offset: t.vector("Offset").unwrap_or([0.0; 3]),
            speed: t.float("Speed").unwrap_or(10000.0),
            frequency: match t.get("Frequency") {
                Some(Value::Byte(b)) => (*b).max(1) as u32,
                Some(Value::Int(i)) => (*i).max(1) as u32,
                _ => 1,
            },
        });
    }
    out
}

/// Day muzzle flash templates (View1P, View3P, ViewADS) of a KFX_WeaponNodes effects object.
pub fn muzzle_flashes(nodes: &Package, effects: &str) -> [Option<String>; 3] {
    let mut out: [Option<String>; 3] = Default::default();
    let rest = effects.split_once('.').map(|x| x.1).unwrap_or(effects);
    let Some(i) = nodes.exports.iter().enumerate().position(|(i, _)| nodes.object_path(i as i32 + 1).eq_ignore_ascii_case(rest)) else { return out };
    let Some(pr) = nodes.export_own_props(i) else { return out };
    for (k, view) in ["View1P", "View3P", "ViewADS"].iter().enumerate() {
        let Some(v) = nodes.struct_prop(&pr, view) else { continue };
        let Some(d) = nodes.struct_prop(&v, "Day") else { continue };
        if let Some(Value::Raw { data, .. }) = d.get("MuzzleFlash") {
            if data.len() >= 8 {
                let o = i32::from_le_bytes(data[4..8].try_into().unwrap());
                out[k] = obj_path(nodes, Some(o));
            }
        }
    }
    out
}

#[derive(Clone, Debug, Default)]
pub struct ProjectileCfg {
    pub class: String,
    pub speed: f32,
    pub radius: f32,
    pub radius_full: f32,
    pub dmg_type: String,
    pub explosion_sound: Option<String>,
    pub flight_sound: Option<String>,
    pub flight_fx: Option<String>,
    /// explosion effect instead of the impact table (cluster bomblets:
    /// BombletExplosionTemplate)
    pub explosion_fx: Option<String>,
    /// GCProj_Rocket seeking: bSeekTarget, Seek_DelayTime, Seek_VInterpToValue,
    /// Seek_LoseTargetMaxDot (GCProj_ATW_PROX_C: false / 0.15 / 1.5 / 0.45 over
    /// GCProj_MissileTrackingTopDown)
    pub seek: bool,
    pub seek_delay: f32,
    pub seek_interp: f32,
    pub seek_lose_dot: f32,
    /// GCProjectile.ProximityVsAnyVehicleRange (GCProj_ATW_PROX_C 50): detonates this close to
    /// a vehicle
    pub prox_vehicle_range: f32,
}

/// Fill a projectile's values from its class defaults (Default__<class> in GCGameContent).
pub fn read_projectile(content: &Package, pc: &mut ProjectileCfg) {
    let name = pc.class.rsplit('.').next().unwrap_or("").to_string();
    let chain = class_defaults(content, &name);
    if chain.is_empty() {
        return;
    }
    // the class default with what its parent classes in the same package set (GCProj_GAntiTankDumbfire_Seat0_Missile_C
    // -> GCProj_MissileFireAndForget_Content: Speed, DamageRadius)
    let pr = upk::props::Props(chain.into_iter().flat_map(|p| p.0).collect());
    pc.speed = pr.float("Speed").unwrap_or(1500.0);
    // Projectile.MaxSpeed caps the launch speed (GCProj_Airstrike_Hellfire: Speed 10000, MaxSpeed 7000)
    if let Some(m) = pr.float("MaxSpeed").filter(|&m| m > 0.0) {
        pc.speed = pc.speed.min(m);
    }
    pc.radius = pr.float("DamageRadius").unwrap_or(250.0);
    pc.radius_full = pr.float("DamageRadiusFull").unwrap_or(0.0);
    pc.dmg_type = pr.object("MyDamageType").filter(|&o| o != 0).map(|o| content.object_name(o)).unwrap_or_default();
    pc.explosion_sound = obj_path(content, pr.object("ExplosionSound"));
    pc.flight_sound = obj_path(content, pr.object("AmbientSound"));
    pc.flight_fx = obj_path(content, pr.object("ProjFlightTemplate"));
    // ProjExplosionTemplate (GCProj_ATW_PROX_C: KFX_VHC.FX_MD600.FX_MD600_InAirExplosion,
    // GCProj_EXP_WPhos_C: Grenade_WPH_MP); the payload bombs use theirs as the release effect
    pc.explosion_fx = obj_path(content, pr.object("ProjExplosionTemplate"));
    // seeking / proximity: the content chain first, then the GCGame parents (imports) are not
    // in `chain`; GCProj_ATW_PROX_C sets bSeekTarget false and Seek_LoseTargetMaxDot itself,
    // Seek_DelayTime / Seek_VInterpToValue come from GCProj_MissileTrackingTopDown (GCGame.u,
    // read in read_projectile_script)
    pc.seek = pr.bool("bSeekTarget").unwrap_or(pc.seek);
    if let Some(v) = pr.float("Seek_DelayTime") {
        pc.seek_delay = v;
    }
    if let Some(v) = pr.float("Seek_VInterpToValue") {
        pc.seek_interp = v;
    }
    if let Some(v) = pr.float("Seek_LoseTargetMaxDot") {
        pc.seek_lose_dot = v;
    }
    pc.prox_vehicle_range = pr.float("ProximityVsAnyVehicleRange").unwrap_or(0.0);
}

/// The seeking defaults of the projectile's GCGame parent classes (GCProj_MissileTrackingTopDown:
/// bSeekTarget true, Seek_DelayTime 0.15, Seek_VInterpToValue 1.5, Seek_LoseTargetMaxDot -2),
/// child first; call before read_projectile (its own values win).
pub fn read_projectile_script(content: &Package, gcgame: &Package, pc: &mut ProjectileCfg) {
    let name = pc.class.rsplit('.').next().unwrap_or("").to_string();
    // the content class's first parent outside the package (an import: GCGame class)
    let mut ci = content.exports.iter().position(|e| content.object_name_of(e) == name && content.export_class(e) == "Class").map(|x| x as i32 + 1).unwrap_or(0);
    for _ in 0..12 {
        if ci <= 0 {
            break;
        }
        ci = content.exports[ci as usize - 1].super_index;
    }
    if ci >= 0 {
        return;
    }
    let mut cls = content.object_name(ci);
    let mut chain = Vec::new();
    for _ in 0..12 {
        let Some(k) = gcgame.exports.iter().position(|e| gcgame.object_name_of(e) == format!("Default__{cls}")) else { break };
        if let Some(p) = gcgame.export_props(k) {
            chain.push(p);
        }
        let Some(c) = gcgame.exports.iter().position(|e| gcgame.object_name_of(e) == cls && gcgame.export_class(e) == "Class") else { break };
        let s = gcgame.exports[c].super_index;
        if s <= 0 {
            break;
        }
        cls = gcgame.object_name(s);
    }
    // parent first, child last: the child's value wins
    for p in chain.iter().rev() {
        if let Some(v) = p.bool("bSeekTarget") {
            pc.seek = v;
        }
        if let Some(v) = p.float("Seek_DelayTime") {
            pc.seek_delay = v;
        }
        if let Some(v) = p.float("Seek_VInterpToValue") {
            pc.seek_interp = v;
        }
        if let Some(v) = p.float("Seek_LoseTargetMaxDot") {
            pc.seek_lose_dot = v;
        }
    }
}

/// Class default of `class` followed by the defaults of its parent classes that live in the same
/// package (export super_index), child first.
pub fn class_defaults(p: &Package, class: &str) -> Vec<upk::props::Props> {
    let mut out = Vec::new();
    let mut ci = p.exports.iter().position(|e| p.object_name_of(e) == class && p.export_class(e) == "Class").map(|x| x as i32 + 1).unwrap_or(0);
    let mut name = class.to_string();
    for _ in 0..12 {
        let Some(pr) = p.exports.iter().position(|e| p.object_name_of(e) == format!("Default__{name}")).and_then(|i| p.export_props(i)) else { break };
        out.push(pr);
        if ci <= 0 {
            break;
        }
        ci = p.exports[ci as usize - 1].super_index;
        if ci <= 0 {
            break;
        }
        name = p.object_name(ci);
    }
    out
}

/// Hellfire airstrike (CFG_BMB_Hellfire -> GCWeap_BMB_Hellfire_C VehicleClass GCV_AirstrikeUAV_C):
/// an unmanned UAV the player flies (AirSpeed) for its fuel time (UCV_PredatorDrone
/// VehicleSetup.Fuel MaxFuel) with the gun GCVW_Airstrike_Hellfire_Content ->
/// MP.Vehicle.CFG_Airstrike_Hellfire (FireModes[0] AmmoClip, ReloadTime, WeaponProjectile).
#[derive(Clone, Debug, Default)]
pub struct HellfireCfg {
    pub missile: ProjectileCfg,
    pub clip: i32,
    pub reload: f32,
    pub fuel: f32,
    pub air_speed: f32,
    pub ui_icon: Option<String>,
    /// CFG_Airstrike_Hellfire bThermalVisionWhenZoomed: the feed is thermal vision
    /// (GCObjectPool CharacterThermalMaterial on characters, DefaultPPThermalMaterial post effect)
    pub thermal: bool,
    /// bombs (cluster, white phosphorus, sensor fused): what the main projectile releases
    pub payload: Option<PayloadCfg>,
    /// GCV_AirstrikeUAV: the UAV circles a centre CenterPointDistanceToInstigator in front of the
    /// player (CenterMode ECCM_InFrontOfInstigator) at FlyAltitude above it, FlyRadius away,
    /// FlySpeed: (altitude, radius, speed, centre distance)
    pub orbit: [f32; 4],
}

/// GCProj_Airstrike_Cluster / _Phosphorus / _SensorFused: at PayloadReleaseDistance from the target
/// the projectile bursts (ProjExplosionTemplate, ExplosionSound) into BombCount bomblets.
#[derive(Clone, Debug, Default)]
pub struct PayloadCfg {
    /// 1 cluster (StrikeWidth), 2 white phosphorus (StrikeRadius, SubmunitionClass), 3 sensor
    /// fused (StrikeRadius, AirburstHeight / AirburstRandRadius, StrikeDuration)
    pub kind: u8,
    pub count: i32,
    pub spread: f32,
    pub release_dist: f32,
    pub release_fx: Option<String>,
    pub bomblet: ProjectileCfg,
    pub bomblet_speed: f32,
    pub airburst_height: f32,
    pub airburst_rand: f32,
    pub duration: f32,
}

fn read_payload(content: &Package, class: &str) -> Option<PayloadCfg> {
    let name = class.rsplit('.').next().unwrap_or("");
    let pr = content.exports.iter().position(|e| content.object_name_of(e) == format!("Default__{name}")).and_then(|i| content.export_props(i))?;
    let count = pr.int("BombCount")?;
    let mut p = PayloadCfg {
        count,
        release_dist: pr.float("PayloadReleaseDistance").unwrap_or(2000.0),
        release_fx: obj_path(content, pr.object("ProjExplosionTemplate")),
        bomblet_speed: pr.float("BombletSpeed").unwrap_or(pr.float("Speed").unwrap_or(6000.0)),
        airburst_height: pr.float("AirburstHeight").unwrap_or(0.0),
        airburst_rand: pr.float("AirburstRandRadius").unwrap_or(0.0),
        duration: pr.float("StrikeDuration").unwrap_or(0.0),
        ..Default::default()
    };
    if let Some(w) = pr.float("StrikeWidth") {
        // cluster: the bomblets are not a class, their values sit on the bomb
        p.kind = 1;
        p.spread = w;
        p.bomblet = ProjectileCfg {
            class: format!("{name}.Bomblet"),
            speed: p.bomblet_speed,
            radius: pr.float("BombletDamageRadius").unwrap_or(700.0),
            dmg_type: pr.object("BombletDamageType").filter(|&o| o != 0).map(|o| content.object_name(o)).unwrap_or_default(),
            explosion_sound: obj_path(content, pr.object("BombletExplosionSound")),
            explosion_fx: obj_path(content, pr.object("BombletExplosionTemplate")),
            ..Default::default()
        };
    } else {
        p.kind = if p.airburst_height > 0.0 { 3 } else { 2 };
        p.spread = pr.float("StrikeRadius").unwrap_or(2000.0);
        let sub = pr.object("SubmunitionClass").filter(|&o| o != 0).map(|o| content.object_name(o))?;
        p.bomblet.class = sub;
        read_projectile(content, &mut p.bomblet);
        p.bomblet.speed = p.bomblet_speed;
    }
    Some(p)
}

/// The airstrike killstreaks: (cost key, config). CFG_BMB_Hellfire / _Cluster / _Phosphorus /
/// _SensorFused -> the UAV GCV_AirstrikeUAV(_<kind>)_C with the gun CFG_Airstrike_<kind>.
pub fn read_strikes(wc: &Package, content: Option<&Package>, drone: Option<&Package>) -> Vec<(String, HellfireCfg)> {
    let mut out = Vec::new();
    for (key, cfg) in [("cfg_bmb_hellfire", "CFG_Airstrike_Hellfire"), ("cfg_bmb_cluster", "CFG_Airstrike_Cluster"), ("cfg_bmb_phosphorus", "CFG_Airstrike_Phosphorus"), ("cfg_bmb_sensorfused", "CFG_Airstrike_SensorFused")] {
        if let Some(h) = read_strike(wc, content, drone, cfg) {
            out.push((key.to_string(), h));
        }
    }
    out
}

pub fn read_hellfire(wc: &Package, content: Option<&Package>, drone: Option<&Package>) -> Option<HellfireCfg> {
    read_strike(wc, content, drone, "CFG_Airstrike_Hellfire")
}

fn read_strike(wc: &Package, content: Option<&Package>, drone: Option<&Package>, cfg: &str) -> Option<HellfireCfg> {
    let i = find(wc, cfg, "GCVehicleWeaponInfo")?;
    let pr = wc.export_props(i)?;
    let fm = wc.struct_array(&pr, "FireModes").into_iter().next()?;
    let mut h = HellfireCfg {
        clip: fm.int("AmmoClip").unwrap_or(2),
        reload: fm.float("ReloadTime").unwrap_or(1.0),
        fuel: 30.0,
        air_speed: 1000.0,
        ui_icon: obj_path(wc, pr.object("UIIcon_Small")),
        thermal: pr.bool("bThermalVisionWhenZoomed").unwrap_or(false),
        ..Default::default()
    };
    h.missile.class = obj_path(wc, fm.object("WeaponProjectile"))?;
    if let Some(c) = content {
        read_projectile(c, &mut h.missile);
        h.payload = read_payload(c, &h.missile.class);
        if let Some(u) = c.exports.iter().position(|e| c.object_name_of(e) == "Default__GCV_AirstrikeUAV_C").and_then(|k| c.export_props(k)) {
            h.air_speed = u.float("AirSpeed").unwrap_or(h.air_speed);
        }
    }
    if let Some(d) = drone {
        if let Some(f) = d.exports.iter().position(|e| d.export_class(e) == "GCVehicleFuelInfo").and_then(|k| d.export_props(k)) {
            h.fuel = f.float("MaxFuel").unwrap_or(h.fuel);
        }
    }
    Some(h)
}

/// GCWeap_EQP_Thermal MaxThermalDuration (seconds the goggles last) and the
/// GCWeap_EQP_Thermal_C ThermalOnSC / ThermalOffSC cues.
pub fn thermal_goggles(gcgame: &Package, content: Option<&Package>) -> (f32, String, String) {
    let secs = gcgame.exports.iter().position(|e| gcgame.object_name_of(e) == "Default__GCWeap_EQP_Thermal").and_then(|i| gcgame.export_props(i)).and_then(|p| p.int("MaxThermalDuration")).unwrap_or(180) as f32;
    let (mut on, mut off) = (String::new(), String::new());
    if let Some(c) = content {
        if let Some(pr) = c.exports.iter().position(|e| c.object_name_of(e) == "Default__GCWeap_EQP_Thermal_C").and_then(|i| c.export_props(i)) {
            on = obj_path(c, pr.object("ThermalOnSC")).unwrap_or_default();
            off = obj_path(c, pr.object("ThermalOffSC")).unwrap_or_default();
        }
    }
    (secs, on, off)
}

/// GCWeap_EQP_UAV MaxSweepDuration (seconds the enemies show on the minimap).
pub fn uav_duration(gcgame: &Package) -> f32 {
    gcgame.exports.iter().position(|e| gcgame.object_name_of(e) == "Default__GCWeap_EQP_UAV").and_then(|i| gcgame.export_props(i)).and_then(|p| p.int("MaxSweepDuration")).unwrap_or(30) as f32
}

/// Tactical grenade (loadout item 2, key F = GCInput Grenade2): MP.Grenades.CFG_EXP_EMP /
/// _Flashbang / _C4 (GCThrowableWeaponInfo over its class default) and the projectile class
/// default chain (GCGameContent Default__GCProj_EXP_*_C -> GCGame Default__GCProj_*).
#[derive(Clone, Debug, Default)]
pub struct TacticalCfg {
    pub cfg: String,
    /// 1 flash bang, 2 EMP, 3 C4
    pub kind: u8,
    pub fuse: f32,
    pub pre_throw: f32,
    pub throw_anim: f32,
    pub ammo: i32,
    pub speed: f32,
    pub toss_z: f32,
    pub radius: f32,
    pub radius_full: f32,
    pub dmg_type: String,
    pub detonate_on_impact: bool,
    pub bounce: bool,
    pub emp_radius: f32,
    pub emp_duration: f32,
    pub tinnitus_radius: f32,
    pub explosion_sound: Option<String>,
    pub bounce_sound: Option<String>,
    pub trail: Option<String>,
    /// C4: C4DetonatatorWeapon (CFG_EXP_C4Detonator, a GCInfantryWeaponInfo drawn once a charge
    /// is out) and the throw's CH_1P_AnimSet (AS_EXP_C4_1P_CH: its own grenade_cook / _throw)
    pub detonator: Option<String>,
    pub anim_set: Option<String>,
}

/// CFG name of a throwable's C4DetonatatorWeapon (MP.Grenades.<cfg>).
pub fn c4_detonator(wc: &Package, cfg: &str) -> Option<String> {
    let wi = wc.exports.iter().enumerate().position(|(i, e)| wc.export_class(e) == "GCThrowableWeaponInfo" && wc.object_path(i as i32 + 1).eq_ignore_ascii_case(&format!("MP.Grenades.{cfg}")))?;
    let o = wc.export_props(wi)?.object("C4DetonatatorWeapon").filter(|&o| o != 0)?;
    Some(wc.object_name(o))
}

pub fn read_tactical(wc: &Package, gcgame: &Package, content: &Package, cfg: &str) -> Option<TacticalCfg> {
    let lower = cfg.to_ascii_lowercase();
    let kind = if lower.contains("flashbang") {
        1
    } else if lower.contains("emp") {
        2
    } else if lower.contains("c4") {
        3
    } else {
        return None;
    };
    let mut t = TacticalCfg { cfg: cfg.to_string(), kind, ..Default::default() };
    let dflt = |p: &Package, name: &str| p.exports.iter().position(|e| p.object_name_of(e) == name).and_then(|i| p.export_props(i));
    if let Some(pr) = dflt(gcgame, "Default__GCThrowableWeaponInfo") {
        t.fuse = pr.float("GrenadeFuseTime").unwrap_or(3.0);
        t.pre_throw = pr.float("PreThrowAnimationTime").unwrap_or(0.56);
        t.throw_anim = pr.float("ThrowAnimationTime").unwrap_or(0.5);
        t.ammo = pr.int("Ammo").unwrap_or(2);
    }
    let wi = wc.exports.iter().enumerate().position(|(i, e)| wc.export_class(e) == "GCThrowableWeaponInfo" && wc.object_path(i as i32 + 1).eq_ignore_ascii_case(&format!("MP.Grenades.{cfg}")))?;
    let wp = wc.export_props(wi)?;
    t.fuse = wp.float("GrenadeFuseTime").unwrap_or(t.fuse);
    t.pre_throw = wp.float("PreThrowAnimationTime").unwrap_or(t.pre_throw);
    t.throw_anim = wp.float("ThrowAnimationTime").unwrap_or(t.throw_anim);
    t.ammo = wp.int("Ammo").unwrap_or(t.ammo);
    t.detonator = wp.object("C4DetonatatorWeapon").filter(|&o| o != 0).map(|o| wc.object_name(o));
    t.anim_set = obj_path(wc, wp.object("CH_1P_AnimSet"));
    let class = wc.object_name(wp.object("ProjectileClass")?);
    // projectile defaults: the content class, then its GCGame parents (archetype imports)
    let mut chain: Vec<(bool, upk::props::Props)> = Vec::new();
    let mut name = format!("Default__{class}");
    let mut pkg_is_content = true;
    for _ in 0..6 {
        let p = if pkg_is_content { content } else { gcgame };
        let Some(i) = p.exports.iter().position(|e| p.object_name_of(e) == name) else { break };
        let Some(pr) = p.export_own_props(i) else { break };
        chain.push((pkg_is_content, pr));
        let arch = p.exports[i].archetype_index;
        if arch == 0 {
            break;
        }
        name = p.object_name(arch);
        if arch < 0 {
            pkg_is_content = false;
        }
    }
    let pkg_of = |c: bool| if c { content } else { gcgame };
    let f = |n: &str, d: f32| chain.iter().find_map(|(_, p)| p.float(n)).unwrap_or(d);
    let b = |n: &str, d: bool| chain.iter().find_map(|(_, p)| p.bool(n)).unwrap_or(d);
    let o = |n: &str| chain.iter().find_map(|(c, p)| p.object(n).filter(|&x| x != 0).map(|x| pkg_of(*c).object_path(x)));
    t.speed = f("Speed", 1500.0);
    t.toss_z = f("TossZ", 100.0);
    t.radius = f("DamageRadius", 400.0);
    t.radius_full = f("DamageRadiusFull", 0.0);
    t.detonate_on_impact = b("bDetonateOnImpact", false);
    t.bounce = b("bBounce", false);
    t.emp_radius = f("EMPDamageRadius", 0.0);
    t.emp_duration = f("EMPBlastDuration", 0.0);
    t.tinnitus_radius = f("TinnitusSoundRadius", 0.0);
    t.dmg_type = chain.iter().find_map(|(c, p)| p.object("MyDamageType").filter(|&x| x != 0).map(|x| pkg_of(*c).object_name(x))).unwrap_or_default();
    t.explosion_sound = o("ExplosionSound");
    t.bounce_sound = o("GrenadeBounceSound");
    t.trail = o("ProjFlightTemplate");
    Some(t)
}

/// Display names of the special abilities (SpecialAbilityCosts keys, lowercase): loadout items
/// by GCLoadoutItemInfos.LoadoutItemInfos ItemClassName -> strItemDisplayName, vehicles by
/// GCGameContent.<class>.VehicleNameString (+ the team: GCV_US_* USA, GCV_CH_* KVA), in the
/// game's language.
pub fn special_names(cooked: &std::path::Path, lang: &str, keys: &[String]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Some(loc) = upk::ui::Loc::load(&cooked.join(format!("Coalesced_{lang}.bin"))).or_else(|| coalesced(cooked)) else { return out };
    if let Ok(p) = upk::Package::open(cooked.join("Game").join("GCLoadoutItemInfos.upk")) {
        if let Some(i) = p.exports.iter().position(|e| p.object_name_of(e) == "LoadoutItemInfos") {
            if let Some(pr) = p.export_props(i) {
                for it in p.struct_array(&pr, "ItemInfos") {
                    let (Some(upk::props::Value::Name(c)), Some(upk::props::Value::Str(n))) = (it.get("ItemClassName"), it.get("strItemDisplayName")) else { continue };
                    let name = loc.resolve(n);
                    if !name.is_empty() && !name.starts_with('<') {
                        out.insert(c.to_ascii_lowercase(), name);
                    }
                }
            }
        }
    }
    for k in keys {
        if k.starts_with("gcv_") {
            if let Some(n) = loc.get(&format!("gcgamecontent.{k}.vehiclenamestring")) {
                let team = if k.starts_with("gcv_us_") { " (USA)" } else if k.starts_with("gcv_ch_") { " (KVA)" } else { "" };
                out.insert(k.clone(), format!("{n}{team}"));
            }
        }
    }
    out
}

/// One fire mode of a rocket-firing vehicle weapon (MP.Vehicle.CFG_GAntiTank FireModes[]):
/// WeaponProjectile, AmmoClip, FireBurstCount, FireInterval, ReloadTime, FireSpread (radians).
#[derive(Clone, Debug, Default)]
pub struct RocketMode {
    pub missile: ProjectileCfg,
    pub clip: i32,
    pub burst: i32,
    pub interval: f32,
    pub reload: f32,
    pub spread: f32,
    pub lock_on: bool,
    /// index in the weapon's FireModes
    pub mode: usize,
}

/// Fire modes of a vehicle weapon whose shots are projectiles (EWFT_Projectile).
pub fn read_rocket_modes(wc: &Package, content: Option<&Package>, cfg: &str) -> Vec<RocketMode> {
    let Some(pr) = find(wc, cfg, "GCVehicleWeaponInfo").and_then(|i| wc.export_props(i)) else { return Vec::new() };
    let vfm = wc.struct_array(&pr, "VehicleFireModes");
    wc.struct_array(&pr, "FireModes")
        .into_iter()
        .enumerate()
        .filter(|(_, fm)| fm.enum_name("WeaponFireType") == Some("EWFT_Projectile"))
        .filter_map(|(k, fm)| {
            let mut m = RocketMode {
                clip: fm.int("AmmoClip").unwrap_or(1),
                burst: match fm.get("FireBurstCount") {
                    Some(upk::props::Value::Byte(b)) => *b as i32,
                    _ => 1,
                },
                interval: fm.float("FireInterval").unwrap_or(0.25),
                reload: fm.float("ReloadTime").unwrap_or(3.0),
                spread: fm.float("FireSpread").unwrap_or(0.0),
                lock_on: vfm.get(k).and_then(|v| v.bool("bUseLockOn")).unwrap_or(false),
                mode: k,
                ..Default::default()
            };
            m.missile.class = obj_path(wc, fm.object("WeaponProjectile"))?;
            if let Some(c) = content {
                read_projectile(c, &mut m.missile);
            }
            if m.missile.dmg_type.is_empty() {
                m.missile.dmg_type = fm.object("DamageType").filter(|&o| o != 0).map(|o| wc.object_name(o)).unwrap_or_default();
            }
            Some(m)
        })
        .collect()
}

/// A drone killstreak's vehicle (GCWeap_DRO_<X>_C m_DroneVehClass, the USA class) and its gun's
/// weapon info (Seats[0].GunClass -> Default__<GunClass> VehicleWeaponInfo, path inside
/// WeaponConfigs, e.g. "MP.Vehicle.CFG_GAntiTank"). `key` is the cost key (cfg_dro_gantitank).
pub fn drone_classes(content: &Package, key: &str) -> Option<(String, Option<String>)> {
    let weap = format!("Default__GCWeap_DRO_{}_C", key.strip_prefix("cfg_dro_")?);
    let pr = content.exports.iter().position(|e| content.object_name_of(e).eq_ignore_ascii_case(&weap)).and_then(|i| content.export_props(i))?;
    let class = content.object_name(pr.object("m_DroneVehClass").filter(|&o| o != 0)?);
    let def = upk::props::Props(class_defaults(content, &class).into_iter().flat_map(|p| p.0).collect());
    let gun = content
        .struct_array(&def, "Seats")
        .into_iter()
        .next()
        .and_then(|s| s.object("GunClass").filter(|&o| o != 0))
        .map(|o| content.object_name(o))
        .and_then(|g| content.exports.iter().position(|e| content.object_name_of(e) == format!("Default__{g}")))
        .and_then(|i| content.export_props(i))
        .and_then(|p| p.object("VehicleWeaponInfo").filter(|&o| o != 0))
        .map(|o| content.object_path(o))
        .map(|p| p.strip_prefix("WeaponConfigs.").map(String::from).unwrap_or(p));
    Some((class, gun))
}
