//! Vehicle data for vehicles.rs: one VehicleKind per buyable USA vehicle class, read from
//! GCGameContent (class defaults over the parent classes, subobjects), GCGame (script classes,
//! sim objects, wheel classes), WeaponConfigs (the seats' guns), the AnimTree (turret / rotor
//! controls) and the vehicle audio setup (cooked into the maps).
use crate::vehicles::{FireKind, FireMode, Motion, SeatCfg, VehicleAudio, VehicleKind, VehicleSlot};
use bevy::prelude::*;
use upk::library::Library;
use upk::props::{Props, Value};
use upk::Package;

/// The buyable vehicles of the player's team (GCBattlePoints VehicleCosts, GCV_US_*).
pub const CLASSES: [&str; 5] = ["GCV_US_M1114_Content", "GCV_US_LAV_Content", "GCV_US_M1A3_Content", "GCV_US_MD600_Content", "GCV_US_AH64_Content"];

/// The KVA counterparts in GCBattlePoints VehicleCosts (GCV_CH_*: the same costs as the USA
/// classes): loaded for the KVA bots (botvehicles.rs); HB_NOKVAVEH=1 leaves them out.
pub const KVA_CLASSES: [&str; 5] = ["GCV_CH_M1114_Content", "GCV_CH_LAV_Content", "GCV_CH_T99_Content", "GCV_CH_MD600_Content", "GCV_CH_Z10_Content"];

/// Every vehicle class loaded for a match: the USA classes, then the KVA ones.
pub fn all_classes() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = CLASSES.to_vec();
    if std::env::var_os("HB_NOKVAVEH").is_none() {
        v.extend(KVA_CLASSES);
    }
    v
}

fn find(p: &Package, path: &str) -> Option<usize> {
    p.exports.iter().enumerate().position(|(i, _)| p.object_path(i as i32 + 1) == path)
}

/// Names of a class and its parents in a package (export super chain), then the first import
/// (the script class in GCGame).
fn class_chain(p: &Package, class: &str) -> (Vec<String>, Option<String>) {
    let mut out = Vec::new();
    let mut ci = p.exports.iter().position(|e| p.object_name_of(e) == class && p.export_class(e) == "Class").map(|x| x as i32 + 1).unwrap_or(0);
    for _ in 0..16 {
        if ci <= 0 {
            break;
        }
        out.push(p.object_name(ci));
        ci = p.exports[ci as usize - 1].super_index;
    }
    let import = (ci < 0).then(|| p.object_name(ci));
    (out, import)
}

/// Class defaults over the whole chain: the content classes', then the script classes' in GCGame.
fn defaults(content: &Package, gcgame: &Package, class: &str) -> (Props, Vec<String>) {
    let (mut names, import) = class_chain(content, class);
    let mut v: Vec<upk::props::Prop> = Vec::new();
    for n in &names {
        if let Some(pr) = find(content, &format!("Default__{n}")).and_then(|i| content.export_props(i)) {
            v.extend(pr.0);
        }
    }
    if let Some(script) = import {
        let (gnames, _) = class_chain(gcgame, &script);
        for n in &gnames {
            if let Some(pr) = find(gcgame, &format!("Default__{n}")).and_then(|i| gcgame.export_props(i)) {
                v.extend(pr.0);
            }
        }
        names.extend(gnames);
    }
    (Props(v), names)
}

/// A subobject (Default__<class>.<name>) over the content chain (most derived first).
fn sub(content: &Package, chain: &[String], name: &str) -> Props {
    let mut v = Vec::new();
    for c in chain {
        if let Some(pr) = find(content, &format!("Default__{c}.{name}")).and_then(|i| content.export_props(i)) {
            v.extend(pr.0);
        }
    }
    Props(v)
}

/// Sockets a content class's PostBeginPlay attaches its components to: the script's
/// `Mesh.AttachComponentToSocket(<component>, '<socket>')` (bytecode: EX_InstanceVariable 0x01
/// <component property> then EX_NameConst 0x21 <name index> <number>). AH64 / MD600 wings ->
/// Attach_Socket, GCV_SH_AAssault rocket pods -> Missile_1 / Missile_0. Keys lowercase.
/// A mirror attachment: (static mesh path#component, Scale3D, Translation, Rotation, socket
/// (bone, location, rotation)).
pub(crate) type AttachSpec = (String, [f32; 3], Vec3, [i32; 3], Option<(String, Vec3, [i32; 3])>);

/// The mirror attachments of a content class (vehload.rs load: StaticMeshComponents
/// wingleft / wingright etc.) with their sockets: the PostBeginPlay script's
/// AttachComponentToSocket, else the mesh's Attach socket.
pub(crate) fn attachment_specs(content: &Package, content_chain: &[String], mp: &Package, mi: usize) -> Vec<AttachSpec> {
    attachment_specs_sel(content, content_chain, mp, mi, false)
}

/// `attachment_specs` of the intact (`wreck` false) or the destroyed-state components (`wreck`
/// true: *wreck, e.g. wingleftwreck; their Materials (*_DEST) ride along as a third '#' field of
/// the path, `attachment_parts` passes them on as a 4th "SM|" field; destruction.rs).
pub(crate) fn attachment_specs_sel(content: &Package, content_chain: &[String], mp: &Package, mi: usize, wreck: bool) -> Vec<AttachSpec> {
    let script_socks = script_attach_sockets(content, content_chain);
    let mesh_socks = mp.mesh_sockets(mi);
    let sock_of = |comp: &str| -> Option<(String, Vec3, [i32; 3])> {
        let want = script_socks.get(&comp.to_ascii_lowercase()).map(|s| s.to_ascii_lowercase());
        mesh_socks
            .iter()
            .find(|s| match &want {
                Some(w) => s.name.eq_ignore_ascii_case(w),
                None => matches!(s.name.to_ascii_lowercase().as_str(), "attach_socket" | "attachsocket"),
            })
            .map(|s| (s.bone.clone(), Vec3::from(s.loc), s.rot))
    };
    let mut out: Vec<AttachSpec> = Vec::new();
    for c in content_chain {
        let prefix = format!("Default__{c}.");
        for (i, e) in content.exports.iter().enumerate() {
            if content.export_class(e) != "StaticMeshComponent" {
                continue;
            }
            let path = content.object_path(i as i32 + 1);
            let Some(nm) = path.strip_prefix(&prefix) else { continue };
            if nm.contains('.') || nm.to_ascii_lowercase().contains("wreck") != wreck || out.iter().any(|a| a.0.split('#').nth(1) == Some(nm)) {
                continue;
            }
            let Some(pr) = content.export_props(i) else { continue };
            if pr.bool("HiddenGame").unwrap_or(false) {
                continue;
            }
            let Some(sm) = pr.object("StaticMesh").filter(|&o| o != 0) else { continue };
            let rot = pr.rotator("Rotation").unwrap_or([0; 3]);
            let mats = if wreck { format!("#{}", pr.get("Materials").map(|v| content.array_objects(v).iter().map(|&o| if o == 0 { String::new() } else { content.object_path(o) }).collect::<Vec<_>>().join(";")).unwrap_or_default()) } else { String::new() };
            out.push((format!("{}#{nm}{mats}", content.object_path(sm)), pr.vector("Scale3D").unwrap_or([1.0; 3]), Vec3::from(pr.vector("Translation").unwrap_or([0.0; 3])), rot, sock_of(nm)));
        }
    }
    out
}

/// The attachments as rigid parts on their socket's bone of the skeleton `sk` (spawned through
/// `spawn` with an "SM|" request: main.rs builds a one-bone skeletal mesh of them).
pub(crate) fn attachment_parts(specs: &[AttachSpec], sk: &[upk::skeletal::Bone], spawn: &mut dyn FnMut(&str) -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)>) -> Vec<crate::viewmodel::SkinPart> {
    specs
        .iter()
        .filter_map(|(path, scale, tr, rot, sock)| {
            let (sbone, sloc, srot) = sock.clone()?;
            let bi = sk.iter().position(|b| b.name.eq_ignore_ascii_case(&sbone))?;
            // the bone's reference in component space (mesh space of the vertices)
            let mut chain = vec![bi];
            while let Some(&c) = chain.last() {
                let p = sk[c].parent;
                if c == 0 || p < 0 || p as usize >= c {
                    break;
                }
                chain.push(p as usize);
            }
            let (mut bq, mut bp) = (Quat::IDENTITY, Vec3::ZERO);
            for &c in chain.iter().rev() {
                let b = &sk[c];
                let (lq, lp) = (Quat::from_xyzw(b.rot[0], b.rot[1], b.rot[2], b.rot[3]).normalize(), Vec3::from(b.pos));
                bp += bq * lp;
                bq *= lq;
            }
            let ue_q = |r: [i32; 3]| {
                let a = |u: i32| u as f32 / 65536.0 * std::f32::consts::TAU;
                Quat::from_rotation_z(a(r[1])) * Quat::from_rotation_y(-a(r[0])) * Quat::from_rotation_x(a(r[2]))
            };
            // raw = B * (S_rot * (C_rot * (scale * v) + C_tr) + S_loc)
            let rq = bq * ue_q(srot) * ue_q(*rot);
            let lin = bevy::math::Mat3::from_quat(rq) * bevy::math::Mat3::from_diagonal(Vec3::from(*scale));
            let t = bp + bq * (ue_q(srot) * *tr + sloc);
            let c = lin.to_cols_array();
            let nums: Vec<String> = c.iter().chain(t.to_array().iter()).chain(bq.to_array().iter()).chain(bp.to_array().iter()).map(|x| format!("{x}")).collect();
            let mut req = format!("SM|{}|{}|{}", path.split('#').next().unwrap_or(""), sk[bi].name, nums.join(","));
            if let Some(mats) = path.split('#').nth(2) {
                req.push('|');
                req.push_str(mats);
            }
            let (h, sm) = spawn(&req)?;
            Some(crate::viewmodel::SkinPart::new(h, sm, sk, &[]))
        })
        .collect()
}

/// SVehicle CollectiveMultiplier for the player helicopter: clamp01(1 - GCPlayerInput
/// HelicopterCollectiveMultiplier 0.88) (ProcessCarInput 0x7579e0, RequestMultipliers)
pub const HELI_COLLECTIVE_MULT: f32 = 0.12;

pub(crate) fn script_attach_sockets(content: &Package, chain: &[String]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for c in chain {
        let Some(fi) = find(content, &format!("{c}.PostBeginPlay")) else { continue };
        let b = content.export_bytes(&content.exports[fi]);
        let rd = |k: usize| b.get(k..k + 4).map(|x| i32::from_le_bytes([x[0], x[1], x[2], x[3]]));
        for i in 0..b.len().saturating_sub(14) {
            if b[i] != 0x01 || b[i + 5] != 0x21 {
                continue;
            }
            let (Some(obj), Some(ni), Some(num)) = (rd(i + 1), rd(i + 6), rd(i + 10)) else { continue };
            if obj <= 0 || obj as usize > content.exports.len() || content.export_class(&content.exports[obj as usize - 1]) != "ComponentProperty" {
                continue;
            }
            let Some(n) = content.names.get(ni as usize) else { continue };
            let sock = if num > 0 { format!("{}_{}", n.name, num - 1) } else { n.name.clone() };
            out.entry(content.object_name(obj).to_ascii_lowercase()).or_insert(sock);
        }
    }
    out
}

/// Object references of an object array property (raw: count, then i32 per element).
fn obj_list(v: Option<&Value>) -> Vec<i32> {
    let Some(Value::Raw { data, .. }) = v else { return Vec::new() };
    if data.len() < 4 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    (0..n).filter_map(|k| data.get(4 + k * 4..8 + k * 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))).collect()
}

/// The seats' guns: MP.Vehicle.CFG_* paths (inside WeaponConfigs) per seat, "" for none.
pub fn seat_weapons(content: &Package, class: &str) -> Vec<String> {
    let (def, _) = {
        let (names, _) = class_chain(content, class);
        let mut v = Vec::new();
        for n in &names {
            if let Some(pr) = find(content, &format!("Default__{n}")).and_then(|i| content.export_props(i)) {
                v.extend(pr.0);
            }
        }
        (Props(v), names)
    };
    content
        .struct_array(&def, "Seats")
        .iter()
        .map(|s| {
            s.object("GunClass")
                .filter(|&o| o != 0)
                .map(|o| content.object_name(o))
                .and_then(|g| find(content, &format!("Default__{g}")))
                .and_then(|i| content.export_props(i))
                .and_then(|p| p.object("VehicleWeaponInfo").filter(|&o| o != 0))
                .map(|o| content.object_path(o))
                .map(|p| p.strip_prefix("WeaponConfigs.").map(String::from).unwrap_or(p))
                .unwrap_or_default()
        })
        .collect()
}

/// The vehicle audio setup (GCVehicleAudio, cooked into the maps): engine sounds and per seat /
/// fire mode SCFireSound (with bLooping).
fn audio(lib: &Library, content: &Package, def: &Props) -> (VehicleAudio, Vec<Vec<(String, bool)>>) {
    let mut out = VehicleAudio::default();
    let mut seats = Vec::new();
    let Some(setup_path) = def.object("VehicleAudio").filter(|&o| o != 0).map(|o| content.object_path(o)) else { return (out, seats) };
    let Some(pk) = setup_path.split('.').next().map(String::from) else { return (out, seats) };
    // its own file, else every map file the package is cooked into (a2_ah64_c: the setup sits in
    // <map>_FrontlinesData, other cues in FL-<map>)
    let mut cands: Vec<(std::sync::Arc<Package>, bool)> = lib.open(&pk).map(|p| (std::sync::Arc::new(p), true)).into_iter().collect();
    cands.extend(crate::sfx::cooked_packages(lib, &pk).into_iter().map(|p| (p, false)));
    let Some((ap, own, i)) = cands.into_iter().find_map(|(ap, own)| {
        let inner = if own { setup_path.split_once('.').map(|x| x.1).unwrap_or("").to_string() } else { setup_path.clone() };
        let i = ap.exports.iter().enumerate().position(|(i, e)| ap.export_class(e) == "GCVehicleAudio" && ap.object_path(i as i32 + 1).eq_ignore_ascii_case(&inner))?;
        Some((ap, own, i))
    }) else {
        warn!("vehicle audio {setup_path}: not found");
        return (out, seats);
    };
    let Some(pr) = ap.export_props(i) else { return (out, seats) };
    let path = |o: i32| if o > 0 && own { format!("{pk}.{}", ap.object_path(o)) } else { ap.object_path(o) };
    let Some(setup) = ap.struct_array(&pr, "VehicleAudioSetups").into_iter().next() else { return (out, seats) };
    let obj = |n: &str| setup.object(n).filter(|&o| o != 0).map(path).unwrap_or_default();
    out.engine_idle = obj("SCEngineIdle");
    out.engine_shutdown = obj("SCEngineShutdown");
    out.engine_sprint = obj("SCEngineSprint");
    out.emp_start = obj("SCEMPStart");
    out.emp_loop = obj("SCEMPLoop");
    out.damage_critical = obj("SCDamageCritical");
    for seat in ap.struct_array(&setup, "Seats") {
        seats.push(ap.struct_array(&seat, "FireModes").iter().map(|fm| (fm.object("SCFireSound").filter(|&o| o != 0).map(path).unwrap_or_default(), fm.bool("bLooping").unwrap_or(false))).collect());
    }
    (out, seats)
}

/// The AnimTree's skeletal controls: ControlName -> bone (SkelControlLists BoneName with their
/// ControlHead / NextControl chains), and the rotor swap controls.
fn anim_tree(pkgs: &[Package], tree: &str, skeleton: &[upk::skeletal::Bone]) -> (std::collections::HashMap<String, usize>, Vec<(Vec<(usize, f32, bool)>, Vec3)>) {
    let mut names = std::collections::HashMap::new();
    let mut rotors = Vec::new();
    let bone = |n: &str| skeleton.iter().position(|b| b.name.eq_ignore_ascii_case(n));
    let Some((p, ti)) = pkgs.iter().find_map(|p| {
        let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "AnimTree" && {
            let op = p.object_path(i as i32 + 1);
            op.eq_ignore_ascii_case(tree) || tree.split_once('.').is_some_and(|(_, rest)| op.eq_ignore_ascii_case(rest))
        })?;
        Some((p, i))
    }) else {
        warn!("vehicle: anim tree {tree} not found");
        return (names, rotors);
    };
    let Some(tp) = p.export_props(ti) else { return (names, rotors) };
    for l in p.struct_array(&tp, "SkelControlLists") {
        let Some(b) = l.enum_name("BoneName").and_then(bone) else { continue };
        let mut c = l.object("ControlHead").unwrap_or(0);
        for _ in 0..16 {
            if c <= 0 {
                break;
            }
            let Some(cp) = p.export_props(c as usize - 1) else { break };
            if let Some(n) = cp.enum_name("ControlName") {
                names.insert(n.to_ascii_lowercase(), b);
            }
            if p.export_class(&p.exports[c as usize - 1]) == "GCSkelControl_HeliRotorSwap" {
                // the spinning state (highest RotorRPS)
                let defs = p.struct_array(&cp, "RotorDefinitions");
                if let Some(d) = defs.iter().max_by(|a, b| a.float("RotorRPS").unwrap_or(0.0).total_cmp(&b.float("RotorRPS").unwrap_or(0.0))) {
                    let bones: Vec<(usize, f32, bool)> = p.struct_array(d, "AffectedBones").iter().filter_map(|x| Some((bone(x.enum_name("BoneName")?)?, x.float("BoneScale").unwrap_or(1.0), x.bool("BoneRotates").unwrap_or(false)))).collect();
                    let rps = Vec3::from(cp.vector("vMaxRotationsPerSecond").unwrap_or([0.0; 3]));
                    if !rotors.iter().any(|(b, _): &(Vec<(usize, f32, bool)>, Vec3)| b == &bones) {
                        rotors.push((bones, rps));
                    }
                }
            }
            c = cp.object("NextControl").unwrap_or(0);
        }
    }
    (names, rotors)
}

/// Load one vehicle class. `rockets`: CombatData.vehicle_rockets (projectile modes with their
/// weapon entries); `spawn` places a skinned instance of a mesh path.
#[allow(clippy::too_many_arguments)]
pub fn load(
    lib: &Library,
    pkgs: &[Package],
    content: &Package,
    gcgame: &Package,
    wc: &Package,
    class: &str,
    rockets: &std::collections::HashMap<String, Vec<(crate::weaponcfg::RocketMode, usize)>>,
    spawn: &mut dyn FnMut(&str) -> Option<(Vec<Handle<Mesh>>, upk::skeletal::SkeletalMesh)>,
) -> Option<VehicleKind> {
    let (def, classes) = defaults(content, gcgame, class);
    let content_chain: Vec<String> = classes.iter().filter(|c| find(content, &format!("Default__{c}")).is_some()).cloned().collect();
    let name = match def.get("VehicleNameString") {
        Some(Value::Str(s)) => s.clone(),
        _ => class.to_string(),
    };
    let has = |n: &str| classes.iter().any(|c| c.eq_ignore_ascii_case(n));
    let motion = if has("GCVAttackHeli") || def.bool("bCanFly").unwrap_or(false) {
        Motion::Heli
    } else if has("GCVTank") || has("GCVAmphibious") {
        Motion::Tracked
    } else {
        Motion::Car
    };
    let mesh_comp = sub(content, &content_chain, "SVehicleMesh");
    let mesh_path = content.object_path(mesh_comp.object("SkeletalMesh").filter(|&o| o != 0)?);
    let tree = mesh_comp.object("AnimTreeTemplate").filter(|&o| o != 0).map(|o| content.object_path(o)).unwrap_or_default();
    let cyl = sub(content, &content_chain, "CollisionCylinder");
    let health = match def.get("Health") {
        Some(Value::Int(h)) => *h as f32,
        _ => 1000.0,
    };
    let max_speed = if motion == Motion::Heli { def.float("AirSpeed").unwrap_or(800.0) } else { def.float("MaxSpeed").or_else(|| def.float("GroundSpeed")).unwrap_or(800.0) };
    let sprint = def.float("MaxSprintSpeed").map(|s| (s, def.float("SprintTimeMax").unwrap_or(2.0), def.float("SprintRechargeRate").unwrap_or(0.5)));
    // sim object: its own values over its class defaults (GCGame: GCVSimCar_M1114,
    // GCVSimTank_SD, GCVSimTankLAV, GCVSimChopper_*)
    let sim = {
        let mut v = Vec::new();
        if let Some(o) = def.object("PlayerSimObj").filter(|&o| o > 0).or_else(|| def.object("SimObj").filter(|&o| o > 0)) {
            if let Some(pr) = content.export_props(o as usize - 1) {
                v.extend(pr.0);
            }
            let cls = content.export_class(&content.exports[o as usize - 1]).to_string();
            let (gn, _) = class_chain(gcgame, &cls);
            for n in gn {
                if let Some(pr) = find(gcgame, &format!("Default__{n}")).and_then(|i| gcgame.export_props(i)) {
                    v.extend(pr.0);
                }
            }
        }
        Props(v)
    };
    let raw_curve = |n: &str| match sim.get(n) {
        Some(Value::Raw { data, .. }) => crate::vehicles::parse_curve(data),
        _ => Vec::new(),
    };
    // wheels (Wheels[]: GCVWheel_* subobjects, BoneName / SteerFactor / WheelRadius over the
    // wheel class default)
    let mut wheel_defs: Vec<(String, f32, f32)> = Vec::new();
    for o in obj_list(def.get("Wheels")).into_iter().filter(|&o| o > 0) {
        let Some(wp) = content.export_props(o as usize - 1) else { continue };
        let wcls = content.export_class(&content.exports[o as usize - 1]).to_string();
        let wdef = find(gcgame, &format!("Default__{wcls}")).and_then(|i| gcgame.export_props(i)).or_else(|| find(content, &format!("Default__{wcls}")).and_then(|i| content.export_props(i)));
        let f = |n: &str, d: f32| wp.float(n).or_else(|| wdef.as_ref().and_then(|w| w.float(n))).unwrap_or(d);
        let Some(b) = wp.enum_name("BoneName").or_else(|| wdef.as_ref().and_then(|w| w.enum_name("BoneName"))).map(String::from) else { continue };
        wheel_defs.push((b, f("SteerFactor", 0.0), f("WheelRadius", 26.0)));
    }
    // mesh, sockets (in the standalone package or cooked into a map)
    let (pk, rest) = mesh_path.split_once('.')?;
    let (mp, mi) = pkgs.iter().find_map(|p| {
        let i = p.exports.iter().enumerate().position(|(i, e)| {
            p.export_class(e) == "SkeletalMesh" && {
                let op = p.object_path(i as i32 + 1);
                op.eq_ignore_ascii_case(&mesh_path) || op.eq_ignore_ascii_case(rest)
            }
        })?;
        Some((p, i))
    })?;
    let _ = pk;
    // GCTreadedVehicle (GCVTank): tread material slots, TexPan2UnrealScale (Default__GCTreadedVehicle
    // 2.0), LeftPoweredWheel's WheelRadius (M1A3 LMWheel 25)
    let tread = (has("GCTreadedVehicle")).then(|| {
        let idx = |n: &str| match def.get(n) {
            Some(Value::Int(i)) => *i,
            _ => -1,
        };
        let radius = def
            .object("LeftPoweredWheel")
            .filter(|&o| o > 0)
            .and_then(|o| {
                let wp = content.export_props(o as usize - 1)?;
                let wcls = content.export_class(&content.exports[o as usize - 1]).to_string();
                wp.float("WheelRadius").or_else(|| find(gcgame, &format!("Default__{wcls}")).and_then(|i| gcgame.export_props(i)).and_then(|w| w.float("WheelRadius")))
            })
            .unwrap_or(wheel_radius_default(&wheel_defs));
        crate::vehicles::Tread { slots: [idx("LeftTreadMaterialIndex"), idx("RightTreadMaterialIndex")], tex_pan: def.float("TexPan2UnrealScale").unwrap_or(0.0), radius }
    });
    // m_CockpitMesh / m_CockpitMeshSecondary components: SkeletalMesh, Translation
    let cockpit_comp = [sub(content, &content_chain, "CockpitMesh"), sub(content, &content_chain, "CockpitMeshSecondary")];
    let cockpit_paths: [Option<String>; 2] = [0, 1].map(|i| cockpit_comp[i].object("SkeletalMesh").filter(|&o| o != 0).map(|o| content.object_path(o)));
    let cockpit_tr: [Vec3; 2] = [0, 1].map(|i| Vec3::from(cockpit_comp[i].vector("Translation").unwrap_or([0.0; 3])));
    // mirror attachments: the content class's StaticMeshComponents (AH64 / MD600 wingleft /
    // wingright: stub wings, pods, engine side panels; LAV attachleft / attachright, M1A3
    // Mirror_Left / Mirror_Right: suspension, fenders), one half modelled, the other side with
    // Scale3D -1 on one axis. They sit in the frame of the mesh's Attach socket (AH64 / MD600
    // Attach_Socket yaw 0, LAV AttachSocket / M1A3 Attach_Socket yaw 16384), checked against
    // the hull geometry (examples/skobj + smverts). Without them the open bays and the damage
    // geometry under them showed ("looks destroyed", user 2026-10-20). Not: *wreck (the
    // destroyed state), HiddenGame components (M1114 ragdollcollision).
    let attach_specs = attachment_specs(content, &content_chain, mp, mi);
    info!("vehicle {class} attachments {:?}", attach_specs.iter().map(|a| (&a.0, &a.4)).collect::<Vec<_>>());
    let mut slots = Vec::new();
    let mut skeleton = Vec::new();
    let mut lowest = 0.0f32;
    let mut top = 0.0f32;
    let mut tread_low: Option<f32> = None;
    let mut extent = Vec2::ZERO;
    let us = class.starts_with("GCV_US_");
    // over-armour (GCVehicle ArmorInfo, a GCOverArmorInfo archetype): its bones are shown and
    // hidden by the damage state (vehicles.rs part_hidden), not by the *_DEST rule below
    let armor_raw = def.object("ArmorInfo").filter(|&o| o != 0).map(|o| over_armor_raw(pkgs, &content.object_path(o))).unwrap_or_default();
    for _ in 0..2 {
        let (h, m) = spawn(&mesh_path)?;
        skeleton = m.bones.clone();
        // hide the other team's variant (CH_VARIANT / CH_*_ACC / OA_CH_* for the USA class) and
        // the destroyed pieces (*_DEST, not *_PRE_DEST): whole subtrees
        let hide_root = |n: &str| {
            let n = n.to_ascii_uppercase();
            let other = if us { n.starts_with("CH_") || n.starts_with("OA_CH_") } else { n.starts_with("US_") || n.starts_with("OA_US_") };
            let dest = ((n.contains("_DEST") && !n.contains("PRE_DEST")) || n.starts_with("DEST_")) && !armor_raw.iter().any(|a| a.name.eq_ignore_ascii_case(&n));
            other || dest
        };
        let mut hide: Vec<String> = Vec::new();
        for (i, b) in m.bones.iter().enumerate() {
            let mut k = i;
            for _ in 0..64 {
                if hide_root(&m.bones[k].name) {
                    hide.push(b.name.clone());
                    break;
                }
                if k == 0 || m.bones[k].parent < 0 {
                    break;
                }
                k = m.bones[k].parent as usize;
            }
        }
        // lowest vertex (mesh space: up = -y) and the body's extent (x = left, z = forward)
        lowest = m.lod.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        top = -m.lod.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        extent = m.lod.positions.iter().fold(Vec2::ZERO, |a, p| Vec2::new(a.x.max(p[2].abs()), a.y.max(p[0].abs())));
        let hide_refs: Vec<&str> = hide.iter().map(|s| s.as_str()).collect();
        let sk = m.bones.clone();
        // only slots whose material reads TreadLocation (GCVAmphibious LAV: GCTreadedVehicle
        // with wheels, its slot 0 is the hull)
        let scrolls: Vec<bool> = tread.as_ref().map(|t| t.slots.iter().map(|&k| m.materials.get(k as usize).is_some_and(|&r| tread_param(pkgs, mp, r))).collect()).unwrap_or_default();
        // the treads' lowest vertex: the track rests on the ground
        if let Some(t) = &tread {
            for sec in &m.lod.sections {
                let k = m.lod0_material_map.get(sec.material as usize).copied().unwrap_or(sec.material);
                if (0..2).any(|i| scrolls[i] && t.slots[i] == k) {
                    let idx = &m.lod.indices[sec.first_index as usize..(sec.first_index + sec.num_triangles * 3) as usize];
                    let low = idx.iter().map(|&v| m.lod.positions[v as usize][1]).fold(f32::MIN, f32::max);
                    tread_low = Some(tread_low.map_or(low, |x: f32| x.max(low)));
                }
            }
        }
        let part = crate::viewmodel::SkinPart::new(h, m, &sk, &hide_refs);
        let tread_uv = match &tread {
            Some(t) => [0, 1].map(|i| if scrolls[i] { part.slot_meshes(t.slots[i]) } else { Vec::new() }),
            None => [Vec::new(), Vec::new()],
        };
        let cockpit = [0usize, 1].map(|i| {
            let path = cockpit_paths[i].as_ref()?;
            let (h, m) = spawn(path)?;
            Some(crate::viewmodel::SkinPart::new(h, m, &sk, &[]))
        });
        // the mirror attachments as rigid parts on the Attach socket's bone (spawned through
        // `spawn` with an "SM|" request: main.rs builds a one-bone skeletal mesh of them)
        let attach = attachment_parts(&attach_specs, &sk, spawn);
        slots.push(VehicleSlot { part, tread_uv, cockpit, attach });
    }
    let ref_pose = crate::drone::ref_pose(&skeleton);
    let damage = damage_cfg(content, &content_chain, &def, armor_raw, slots.first().map(|s| &s.part.mesh), mp, mi);
    info!(
        "vehicle {class} damage: part health {}, smoke < {}, critical < {} ({}), parts {:?}, {} event refs",
        damage.part_health,
        damage.smoke_threshold,
        damage.critical_threshold,
        damage.critical_sound,
        damage.parts.iter().map(|p| (&p.name, p.outer, p.pass, p.lo.round(), p.hi.round())).collect::<Vec<_>>(),
        damage.refs.len()
    );
    let bone = |n: &str| skeleton.iter().position(|b| b.name.eq_ignore_ascii_case(n));
    let mut sockets = std::collections::HashMap::new();
    let mut socket_fx = Vec::new();
    for s in mp.mesh_sockets(mi) {
        for e in s.effects.iter().filter(|e| e.class == "ParticleSystem" && e.view != "VIEW_1P" && e.start_tag != "None") {
            socket_fx.push((e.start_tag.to_ascii_lowercase(), s.name.to_ascii_lowercase(), e.reference.clone()));
        }
        if let Some(bi) = bone(&s.bone) {
            sockets.insert(s.name.to_ascii_lowercase(), (bi, Vec3::from(s.loc)));
        }
    }
    // the cockpit meshes' sockets (CameraTag of the 1P view) on the outer skeleton's bones
    let cockpit_sockets: [std::collections::HashMap<String, (usize, Vec3)>; 2] = [0, 1].map(|i| {
        let mut out = std::collections::HashMap::new();
        let Some(path) = cockpit_paths[i].as_ref() else { return out };
        let rest = path.split_once('.').map(|x| x.1).unwrap_or(path);
        let Some((cp, ci)) = pkgs.iter().find_map(|p| {
            let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "SkeletalMesh" && {
                let op = p.object_path(i as i32 + 1);
                op.eq_ignore_ascii_case(path) || op.eq_ignore_ascii_case(rest)
            })?;
            Some((p, i))
        }) else {
            return out;
        };
        for s in cp.mesh_sockets(ci) {
            if let Some(bi) = bone(&s.bone) {
                out.insert(s.name.to_ascii_lowercase(), (bi, Vec3::from(s.loc)));
            }
        }
        out
    });
    info!("vehicle {class} cockpit meshes {cockpit_paths:?} translation {cockpit_tr:?}, sockets {:?}", cockpit_sockets.iter().map(|m| m.len()).collect::<Vec<_>>());
    let (controls, rotors) = if tree.is_empty() { Default::default() } else { anim_tree(pkgs, &tree, &skeleton) };
    // wheels: the Wheels[] bones; tanks drive all WHL_* bones of the treads (the powered wheels
    // LMWheel / RMWheel stand for them)
    let wheel_radius = wheel_defs.first().map(|w| w.2).unwrap_or(26.0);
    let ue_pos = |k: usize| {
        let p = ref_pose[k].1;
        Vec3::new(p.z, -p.x, -p.y)
    };
    let mut wheels: Vec<(usize, f32, f32)> = Vec::new();
    if motion == Motion::Tracked {
        for (k, b) in skeleton.iter().enumerate() {
            let n = b.name.to_ascii_uppercase();
            if n.starts_with("WHL_L") || n.starts_with("WHL_R") {
                wheels.push((k, 0.0, if n.starts_with("WHL_L") { -1.0 } else { 1.0 }));
            }
        }
    } else {
        for (b, sf, _) in &wheel_defs {
            if let Some(k) = bone(b) {
                wheels.push((k, *sf, if ue_pos(k).y < 0.0 { -1.0 } else { 1.0 }));
            }
        }
    }
    let wp: Vec<Vec3> = wheels.iter().map(|w| ue_pos(w.0)).collect();
    let (probes, ride_height, wheel_base, track_half) = if motion != Motion::Heli && !wp.is_empty() {
        let front = wp.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        let rear = wp.iter().map(|p| p.x).fold(f32::MAX, f32::min);
        let half = wp.iter().map(|p| p.y.abs()).fold(0.0f32, f32::max);
        let centre_z = wp.iter().map(|p| p.z).sum::<f32>() / wp.len() as f32;
        // treaded: on the tread's lowest vertex (the mean of all WHL_* bones includes the raised
        // sprocket / idler wheels and buried the track 7.5 units)
        (vec![Vec2::new(front, -half), Vec2::new(front, half), Vec2::new(rear, -half), Vec2::new(rear, half)], tread_low.unwrap_or(-centre_z + wheel_radius), front - rear, half)
    } else {
        // helicopters: on the lowest vertex
        (vec![Vec2::ZERO], lowest.max(10.0), 200.0, 100.0)
    };
    let (half_len, half_width) = if motion == Motion::Heli {
        // the fuselage (the mesh extent includes the rotor discs): NOT from data, a third of the
        // collision cylinder's radius sideways, 0.8 of it along the body
        let r = cyl.float("CollisionRadius").unwrap_or(290.0);
        (r * 0.8, r * 0.33)
    } else {
        (extent.x, extent.y)
    };
    // seats
    let (aud, seat_sounds) = audio(lib, content, &def);
    let guns = seat_weapons(content, class);
    let mut seats = Vec::new();
    for (si, s) in content.struct_array(&def, "Seats").iter().enumerate() {
        let curve_pts = |n: &str| match s.get(n) {
            Some(Value::Raw { data, .. }) => crate::vehicles::curve_vec_x(data),
            _ => Vec::new(),
        };
        let curve_x = |n: &str| curve_pts(n).first().map(|p| p.1);
        let tc = content.name_array(s.get("TurretControls"));
        let ctl = |n: &str| controls.get(&n.to_ascii_lowercase()).copied().or_else(|| bone(n));
        let turret = match (tc.first().and_then(|n| ctl(n)), tc.get(1).and_then(|n| ctl(n))) {
            (Some(a), Some(b)) => Some((a, b)),
            (Some(a), None) => Some((a, a)),
            _ => None,
        };
        let cfg = guns.get(si).cloned().unwrap_or_default();
        let mut modes = Vec::new();
        let mut fov_zoom = 0.0;
        if !cfg.is_empty() {
            let gp = wc.exports.iter().enumerate().position(|(i, e)| wc.export_class(e) == "GCVehicleWeaponInfo" && wc.object_path(i as i32 + 1) == cfg).and_then(|gi| wc.export_props(gi)).unwrap_or_default();
            fov_zoom = gp.float("FOVZoom").unwrap_or(0.0);
            let vfms = wc.struct_array(&gp, "VehicleFireModes");
            let seat_modes = content.struct_array(s, "SeatFireModes");
            for (m, fm) in wc.struct_array(&gp, "FireModes").iter().enumerate() {
                let sockets: Vec<String> = seat_modes.get(m).map(|x| content.name_array(x.get("Sockets"))).unwrap_or_default();
                let flash_tags: Vec<String> = seat_modes.get(m).map(|x| content.name_array(x.get("FlashTags"))).unwrap_or_default();
                let (sound, looping) = seat_sounds.get(si).and_then(|v| v.get(m)).cloned().unwrap_or_default();
                let kind = match fm.enum_name("WeaponFireType") {
                    Some("EWFT_InstantHit") => FireKind::Instant(crate::vehicles::GunCfg {
                        fire_interval: fm.float("FireInterval").unwrap_or(0.1),
                        range: fm.float("WeaponRange").unwrap_or(16384.0),
                        spread: gp.float("SpreadMin").or_else(|| fm.float("FireSpread")).unwrap_or(1.0),
                        dmg_type: fm.object("InstantHitDamageType").filter(|&o| o != 0).map(|o| wc.object_name(o)).unwrap_or_default(),
                        overheat_max: fm.float("OverheatMaxTime").unwrap_or(4.0),
                        overheat_add: fm.float("OverheatAddTime").unwrap_or(0.12),
                        overheat_recover: fm.float("OverheatMinRecoverTime").unwrap_or(1.5),
                        sound: sound.clone(),
                        sound_looping: looping,
                        engine_idle: String::new(),
                        engine_shutdown: String::new(),
                        engine_sprint: String::new(),
                        emp_start: String::new(),
                        emp_loop: String::new(),
                        full_range: fm.float("FullDamageRange").unwrap_or(0.0),
                        splash: fm.object("InstantHitDamageType").filter(|&o| o != 0).and_then(|o| splash_of(gcgame, &wc.object_name(o))),
                        tracer: vfms.get(m).and_then(|vf| wc.struct_prop(vf, "Tracer")).and_then(|t| {
                            let template = t.object("ParticleSystem").filter(|&o| o != 0).map(|o| wc.object_path(o))?;
                            Some(crate::weaponcfg::TracerCfg {
                                template,
                                size: t.vector("Size").unwrap_or([10.0, 150.0, 0.0]),
                                color: t.vector("TracerColor").unwrap_or([1.0; 3]),
                                offset: t.vector("Offset").unwrap_or([0.0; 3]),
                                speed: t.float("Speed").unwrap_or(7000.0),
                                frequency: match t.get("Frequency") {
                                    Some(Value::Byte(b)) => (*b).max(1) as u32,
                                    Some(Value::Int(i)) => (*i).max(1) as u32,
                                    _ => 1,
                                },
                            })
                        }),
                    }),
                    Some("EWFT_Projectile") => {
                        let Some((rm, wi)) = rockets.get(&cfg.to_ascii_lowercase()).and_then(|v| v.iter().find(|(r, _)| r.mode == m)).cloned() else { continue };
                        FireKind::Projectile(rm, wi)
                    }
                    _ => continue,
                };
                modes.push(FireMode { kind, sockets, sound, flash_tags });
            }
        }
        let cam_tag = s.enum_name("CameraTag").filter(|t| *t != "None").unwrap_or("").to_string();
        let v3 = |n: &str| Vec3::from(s.vector(n).unwrap_or([0.0; 3]));
        let view_pitch = (curve_x("ViewMinLimit").unwrap_or(-30.0), curve_x("ViewMaxLimit").unwrap_or(45.0));
        let rate = |m: &str, f: &str| s.float(m).unwrap_or(100.0) * s.float(f).unwrap_or(1.0);
        let exit = format!("Seat{si}StartPosition");
        let flag = |n: &str| s.bool(n).unwrap_or(false);
        let behind_pitch = s.rotator("BehindCameraRotation").map(|r| r[0] as f32 / 65536.0 * std::f32::consts::TAU).unwrap_or(0.0);
        seats.push(SeatCfg {
            camera_tag3p: s.enum_name("CameraTag3p").filter(|t| *t != "None").unwrap_or("").to_string(),
            behind_pitch,
            behind_sprint: v3("BehindCameraMaxOffsetSprint"),
            fixed1p: flag("bCameraFixedToSocket1P"),
            fixed3p: flag("bCameraFixedToSocket3P"),
            views: match s.enum_name("ESeatAllowedViews") {
                Some("SEATVIEW_FIRSTONLY") => 0,
                Some("SEATVIEW_THIRDONLY") => 1,
                _ => 2,
            },
            cockpit1p: if flag("bUseCockpitMesh1p") { 1 } else if flag("bUseSecondaryCockpitMesh1p") { 2 } else { 0 },
            hide_outer_1p: flag("bDisable3pMeshIn1p"),
            no_roll_3p: flag("bCameraDoesNotRoll3P"),
            turret_parent: s.int("TurretParentSeatIndex").filter(|&i| i >= 0 && i as usize != si).map(|i| i as usize),
            camera_tag: cam_tag,
            camera_offset: v3("CameraOffset"),
            behind: v3("BehindCameraMaxOffset"),
            behind_min: v3("BehindCameraMinOffset"),
            view_pitch,
            turret,
            turret_pitch: {
                let (lo, hi) = (curve_pts("TurretControllerMinLimit"), curve_pts("TurretControllerMaxLimit"));
                (if lo.is_empty() { vec![(0.0, view_pitch.0)] } else { lo }, if hi.is_empty() { vec![(0.0, view_pitch.1)] } else { hi })
            },
            turret_rate: (rate("fTurretYawMaxSpeed", "fTurretYawSpeedModifier"), rate("fTurretPitchMaxSpeed", "fTurretPitchSpeedModifier")),
            weapon: if modes.is_empty() { String::new() } else { cfg },
            modes,
            fov_zoom,
            exit,
        });
    }
    // m_DefaultPhysicalMaterial (PM_Library.Armor.Vehicle.PMA_Heavy): the armour's column in the
    // damage grid
    let armor = def.object("m_DefaultPhysicalMaterial").filter(|&o| o != 0).map(|o| content.object_name(o)).unwrap_or_else(|| "PMA_Medium".into());
    if let Some(t) = &tread {
        info!("vehicle {class} treads: material slots {:?}, TexPan2UnrealScale {}, powered wheel radius {}, sections {:?}", t.slots, t.tex_pan, t.radius, slots.first().map(|s: &VehicleSlot| [s.tread_uv[0].len(), s.tread_uv[1].len()]));
    }
    info!("vehicle {class} socket effects: {:?}", socket_fx.iter().filter(|e| e.0.contains("fire")).collect::<Vec<_>>());
    let heli = (motion == Motion::Heli).then(|| {
        let f = |n: &str, d: f32| sim.float(n).unwrap_or(d);
        let sin = |deg: f32| deg.to_radians().sin();
        crate::vehicles::HeliSim {
            passive_lift: f("PassiveLiftFactor", 50000.0),
            active_lift: f("ActiveLiftFactor", 38000.0),
            up_damping: f("UpDamping", 0.5),
            turn_torque: f("TurnTorqueFactor", 20000.0),
            turn_damping: f("TurnDamping", 20.0),
            max_yaw_rate: f("MaxYawRate", 3.14),
            pitch_torque: f("PitchTorqueFactor", 500.0),
            pitch_torque_max: f("PitchTorqueMax", 25000.0),
            pitch_damping: f("PitchDamping", 40.0),
            roll_upright: f("RollUprightFactor", 1500.0),
            pitch_upright: f("PitchUprightFactor", 500.0),
            roll_strafe: f("RollTorqueStrafeFactor", 350.0),
            roll_torque_max: f("RollTorqueMax", 25000.0),
            roll_damping: f("RollDamping", 50.0),
            collective_comp: f("CollectiveCompensationForce", 20000.0),
            air_resistance: f("AirResistanceMultiplier", 0.001),
            reasonably_upright: sin(f("fReasonablyUpright", 58.0)),
            very_upright: sin(f("fAHVeryUpright", 30.0)),
            max_pitch: sin(f("fAHMaxPitch", 45.0)),
            collective_roll: f("fCollectiveRollMultiplier", 1.0),
            falling_mult: f("fUpwardForceFallingMultiplier", 2.5),
            inertia: Vec3::from(sim.vector("InertiaTensorOverride").unwrap_or([20000.0; 3])),
            // the StayUpright constraint exists only with SimObj.bAlwaysUpright (SVehicle
            // SetSimObj 0x5307c0 / physics init 0x53124f); the player MD600 / AH64 sims leave it
            // false (only GCVehicleSimChopper_SD_EMP sets it): angles in degrees (InitConstraint
            // 0x414b80 converts with pi / 180)
            upright_pitch: if sim.bool("bAlwaysUpright").unwrap_or(false) { (f("StayUprightPitchResistAngle", 0.0).to_radians(), f("StayUprightPitchStiffness", 0.0)) } else { (0.0, 0.0) },
            upright_roll: if sim.bool("bAlwaysUpright").unwrap_or(false) { (f("StayUprightRollResistAngle", 0.0).to_radians(), f("StayUprightRollStiffness", 0.0)) } else { (0.0, 0.0) },
            // roll / pitch input that pushes further out stops past lerp(sin fAHMin<Roll|Pitch>,
            // sin fAHMax<Roll|Pitch>, CollectiveMultiplier) (vehicles.rs heli_step)
            // (CollectiveMultiplier = 1 - HelicopterCollectiveMultiplier 0.88 = 0.12)
            roll_limit: sin(f("fAHMinRoll", 30.0)) + (sin(f("fAHMaxRoll", 45.0)) - sin(f("fAHMinRoll", 30.0))) * HELI_COLLECTIVE_MULT,
            pitch_limit: sin(f("fAHMinPitch", 40.0)) + (sin(f("fAHMaxPitch", 45.0)) - sin(f("fAHMinPitch", 40.0))) * HELI_COLLECTIVE_MULT,
        }
    });
    if let Some(h) = &heli {
        info!("vehicle {class} heli sim: {h:?}");
    }
    let kind = VehicleKind {
        top,
        cockpit_tr,
        cockpit_sockets,
        heli,
        socket_fx,
        damage: {
            let mut d = damage;
            if d.critical_sound.is_empty() {
                d.critical_sound = aud.damage_critical.clone();
            }
            d
        },
        tread,
        armor,
        class: class.to_string(),
        name,
        classes,
        motion,
        skeleton,
        ref_pose,
        sockets,
        wheels,
        wheel_radius,
        ride_height,
        probes,
        wheel_base,
        track_half,
        half_len,
        half_width,
        torque_curve: raw_curve("TorqueVSpeedCurve"),
        steer_curve: raw_curve("MaxSteerAngleCurve"),
        gravity_factor: sim.float("GravityFactorEngineOn").unwrap_or(1.0),
        max_speed,
        sprint,
        tread_steer: sim.float("m_fTreadMaxSpeedSteering").unwrap_or(150.0),
        yaw_rate: sim.float("MaxYawRate").unwrap_or(3.14),
        max_pitch: sim.float("fAHMaxPitch").unwrap_or(30.0),
        seats,
        slots,
        audio: aud,
        rotors,
        health,
    };
    info!(
        "vehicle {} {} ({:?}): {mesh_path}, {} bones, {} sockets, wheels {} r {} ride {:.1} base {:.1} track {:.1} body {:.0}x{:.0}, speed {} sprint {:?}, tread steer {}, yaw rate {} pitch {}, rotors {}, health {} armour {}",
        kind.class,
        kind.name,
        kind.motion,
        kind.skeleton.len(),
        kind.sockets.len(),
        kind.wheels.len(),
        kind.wheel_radius,
        kind.ride_height,
        kind.wheel_base,
        kind.track_half,
        kind.half_len,
        kind.half_width,
        kind.max_speed,
        kind.sprint,
        kind.tread_steer,
        kind.yaw_rate,
        kind.max_pitch,
        kind.rotors.len(),
        kind.health,
        kind.armor
    );
    for (si, s) in kind.seats.iter().enumerate() {
        info!(
            "vehicle {} seat {si}: camera {} offset {:?} behind {:?}/{:?} view {:?} turret {:?} pitch {:?} rate {:?} gun {} modes {:?}",
            kind.class,
            s.camera_tag,
            s.camera_offset,
            s.behind_min,
            s.behind,
            s.view_pitch,
            s.turret.map(|(a, b)| (kind.skeleton[a].name.clone(), kind.skeleton[b].name.clone())),
            s.turret_pitch,
            s.turret_rate,
            s.weapon,
            s.modes.iter().map(|m| (match &m.kind { FireKind::Instant(g) => format!("instant {} {}", g.fire_interval, g.dmg_type), FireKind::Projectile(r, w) => format!("projectile {} clip {} burst {} w{w}", r.missile.class, r.clip, r.burst) }, m.sockets.clone(), m.sound.clone())).collect::<Vec<_>>()
        );
    }
    info!("vehicle {} audio {:?}", kind.class, kind.audio);
    Some(kind)
}

/// Largest wheel radius of the Wheels[] entries (fallback for the powered wheel).
fn wheel_radius_default(wheels: &[(String, f32, f32)]) -> f32 {
    wheels.iter().map(|w| w.2).fold(0.0, f32::max).max(1.0)
}

/// Whether a material (instance chain up to its Material) has a VectorParameter TreadLocation
/// in its expressions.
pub(crate) fn tread_param(pkgs: &[Package], pkg: &Package, mat_ref: i32) -> bool {
    // resolve an import by its path in the loaded packages
    let (mut p, mut cur) = (pkg, mat_ref);
    if cur < 0 {
        let path = pkg.object_path(cur);
        let Some(hit) = pkgs.iter().find_map(|q| q.exports.iter().enumerate().position(|(i, e)| q.export_class(e).contains("Material") && q.object_path(i as i32 + 1).eq_ignore_ascii_case(&path)).map(|i| (q, i as i32 + 1))) else { return false };
        (p, cur) = hit;
    }
    for _ in 0..8 {
        if cur <= 0 {
            return false;
        }
        let e = &p.exports[cur as usize - 1];
        let Some(pr) = p.export_own_props(cur as usize - 1) else { return false };
        if p.export_class(e) == "Material" {
            return obj_list(pr.get("Expressions")).into_iter().filter(|&o| o > 0).any(|o| {
                p.export_class(&p.exports[o as usize - 1]) == "MaterialExpressionVectorParameter" && p.export_own_props(o as usize - 1).is_some_and(|x| x.enum_name("ParameterName") == Some("TreadLocation"))
            });
        }
        let parent = pr.object("Parent").unwrap_or(0);
        if parent < 0 {
            return tread_param(pkgs, p, parent);
        }
        cur = parent;
    }
    false
}

/// Splash of an instant-hit damage type: SplashDamageClass, SplashDamageRadius,
/// SplashDamageRadiusFull from its class defaults (child first; GCGame script classes).
fn splash_of(gcgame: &Package, dmg: &str) -> Option<(String, f32, f32)> {
    let chain = crate::weaponcfg::class_defaults(gcgame, dmg);
    let class = chain.iter().find_map(|p| p.object("SplashDamageClass").filter(|&o| o != 0)).map(|o| gcgame.object_name(o))?;
    let radius = chain.iter().find_map(|p| p.float("SplashDamageRadius")).unwrap_or(0.0);
    let full = chain.iter().find_map(|p| p.float("SplashDamageRadiusFull")).unwrap_or(0.0);
    (radius > 0.0).then_some((class, radius, full))
}

/// One DestructibleBoneInfo as stored (names; bones resolved in damage_cfg).
#[derive(Default, Clone)]
pub(crate) struct ArmorRaw {
    name: String,
    outer: String,
    pass: f32,
    ignore: bool,
    events: Vec<String>,
    events_hidden: Vec<String>,
    sound: String,
}

/// The DestructibleBones of a GCOverArmorInfo archetype (package path, e.g.
/// SD_LAV25.VehicleSetup.OverArmor_US / US_M1A3.VehicleSetup.OverArmor), in the vehicle
/// packages or cooked into a map.
fn over_armor_raw(pkgs: &[Package], path: &str) -> Vec<ArmorRaw> {
    let rest = path.split_once('.').map(|x| x.1).unwrap_or(path);
    let Some((p, i)) = pkgs.iter().find_map(|p| {
        let i = p.exports.iter().enumerate().position(|(i, e)| p.export_class(e) == "GCOverArmorInfo" && {
            let op = p.object_path(i as i32 + 1);
            op.eq_ignore_ascii_case(path) || op.eq_ignore_ascii_case(rest)
        })?;
        Some((p, i))
    }) else {
        warn!("vehicle: over-armour {path} not found");
        return Vec::new();
    };
    let Some(pr) = p.export_props(i) else { return Vec::new() };
    let names = |v: Option<&Value>| -> Vec<String> {
        let Some(Value::Raw { data, .. }) = v else { return Vec::new() };
        let n = data.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).unwrap_or(0);
        (0..n).filter_map(|k| data.get(4 + k * 8..8 + k * 8)).map(|b| p.names.get(u32::from_le_bytes(b.try_into().unwrap()) as usize).map(|x| x.name.to_ascii_lowercase()).unwrap_or_default()).collect()
    };
    p.struct_array(&pr, "DestructibleBones")
        .iter()
        .map(|b| ArmorRaw {
            name: b.enum_name("BoneName").unwrap_or("").to_string(),
            outer: b.enum_name("Outer").filter(|o| *o != "None").unwrap_or("").to_string(),
            pass: b.float("DamagePassThrough").unwrap_or(0.0),
            ignore: b.bool("bIgnoreDamage").unwrap_or(false),
            events: names(b.get("VehicleEvents")),
            events_hidden: names(b.get("VehicleEventsOnHidden")),
            sound: b.object("DestructionSound").filter(|&o| o != 0).map(|o| p.object_path(o)).unwrap_or_default(),
        })
        .collect()
}

/// Damage stages of a class: GCVehicle InitialOverArmorHealth / DamageSmokeThreshhold /
/// DamageCriticalThreshold / DamageCriticalSound (AudioComponent's SoundCue), the over-armour
/// parts with their bones, subtrees and vertex bounds (vehicle space, reference pose), the
/// mesh's socket References of the damage events.
fn damage_cfg(content: &Package, chain: &[String], def: &Props, raw: Vec<ArmorRaw>, mesh: Option<&upk::skeletal::SkeletalMesh>, mp: &Package, mi: usize) -> crate::vehicles::DamageCfg {
    let critical_sound = chain
        .iter()
        .find_map(|c| find(content, &format!("Default__{c}")).and_then(|i| content.export_props(i)).and_then(|p| p.object("DamageCriticalSound")).filter(|&o| o > 0))
        .and_then(|o| content.export_props(o as usize - 1))
        .and_then(|p| p.object("SoundCue").filter(|&o| o != 0))
        .map(|o| content.object_path(o))
        .unwrap_or_default();
    let mut d = crate::vehicles::DamageCfg {
        part_health: def.int("InitialOverArmorHealth").unwrap_or(150) as f32,
        smoke_threshold: def.float("DamageSmokeThreshhold").unwrap_or(0.666),
        critical_threshold: def.float("DamageCriticalThreshold").unwrap_or(0.333),
        critical_sound,
        ..Default::default()
    };
    let Some(m) = mesh else { return d };
    let bone = |n: &str| m.bones.iter().position(|b| b.name.eq_ignore_ascii_case(n));
    let raw: Vec<(ArmorRaw, usize)> = raw.into_iter().filter_map(|r| bone(&r.name).map(|b| (r, b))).collect();
    let part_bones: Vec<usize> = raw.iter().map(|r| r.1).collect();
    // owning part of every bone: the nearest part bone up the parent chain
    let owner: Vec<Option<usize>> = (0..m.bones.len())
        .map(|i| {
            let mut k = i;
            for _ in 0..64 {
                if let Some(pi) = part_bones.iter().position(|&b| b == k) {
                    return Some(pi);
                }
                if k == 0 || m.bones[k].parent < 0 || m.bones[k].parent as usize == k {
                    return None;
                }
                k = m.bones[k].parent as usize;
            }
            None
        })
        .collect();
    let mut lo = vec![Vec3::splat(f32::MAX); raw.len()];
    let mut hi = vec![Vec3::splat(f32::MIN); raw.len()];
    for (vi, (b, w)) in m.influences.iter().enumerate() {
        let kk = (0..4).max_by_key(|&k| w[k]).unwrap_or(0);
        let Some(Some(pi)) = owner.get(b[kk] as usize) else { continue };
        let Some(p) = m.lod.positions.get(vi) else { continue };
        let u = Vec3::new(p[2], -p[0], -p[1]);
        lo[*pi] = lo[*pi].min(u);
        hi[*pi] = hi[*pi].max(u);
    }
    for (pi, (r, b)) in raw.iter().enumerate() {
        if lo[pi].x > hi[pi].x {
            // no vertices: nothing to hit or hide
            lo[pi] = Vec3::splat(1.0e6);
            hi[pi] = Vec3::splat(1.0e6);
        }
        d.parts.push(crate::vehicles::ArmorPart {
            bone: *b,
            name: r.name.clone(),
            outer: raw.iter().position(|o| !r.outer.is_empty() && o.0.name.eq_ignore_ascii_case(&r.outer)),
            pass: r.pass,
            ignore: r.ignore,
            events: r.events.clone(),
            events_hidden: r.events_hidden.clone(),
            sound: r.sound.clone(),
            subtree: (0..m.bones.len()).filter(|&i| owner[i] == Some(pi)).collect(),
            lo: lo[pi],
            hi: hi[pi],
        });
    }
    // socket References of the damage events
    let mut evs: Vec<String> = vec!["damagesmoke".into()];
    for p in &d.parts {
        evs.extend(p.events.iter().cloned());
        evs.extend(p.events_hidden.iter().cloned());
    }
    for s in mp.mesh_sockets(mi) {
        for e in s.effects.iter().filter(|e| e.view != "VIEW_1P" && (e.class == "ParticleSystem" || e.class == "SoundCue")) {
            let start = e.start_tag.to_ascii_lowercase();
            if evs.contains(&start) {
                d.refs.push(crate::vehicles::DamageRef { start, end: e.end_tag.to_ascii_lowercase(), socket: s.name.to_ascii_lowercase(), path: e.reference.clone(), sound: e.class == "SoundCue" });
            }
        }
    }
    d
}
