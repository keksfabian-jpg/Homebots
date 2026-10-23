//! Rig debug overlay and logs (user request 2026-10-18).
//!
//! F3: skeleton of the selected rig drawn over the game (bones parent -> child, joints coloured:
//! green = animated by a track of the playing set, yellow = reference pose only, red = collapsed
//! (hidden geometry), magenta ring = a skinned part's bone that finds no driver bone), optional
//! bone names and axes (x red, y green, z blue in the bone's space), and a panel with the
//! animation state and a bone table (local / component / world transforms, deviation from the
//! reference pose). Targets: the 1P weapon + arms, a bot by slot, or the bot in the crosshair.
//!
//! Files (logs/): "Snapshot" (Shift+F3) writes rig_<target>_<unix>.txt with every number of the
//! current pose; "Aufnahme" writes rig_<unix>_frames.csv (every bone, every frame, N frames);
//! "Rig-Prüfung" writes rig_check.txt (bone mapping, tracks, skin weights of every loaded rig).
//! The animation speed slider (0 = frozen) scales the 1P and bot animation clocks.
use crate::bots::{Bot, BotAssets};
use crate::combat::Soldier;
use crate::player::Pawn;
use crate::viewmodel::ViewModel;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

static ANIM_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

/// Factor on the animation clocks (1P weapon, bots); 0 freezes the poses.
pub fn anim_scale() -> f32 {
    f32::from_bits(ANIM_SCALE.load(Relaxed))
}

/// Mapping summary of one skinned part.
pub struct PartInfo {
    pub label: String,
    pub bones: usize,
    pub verts: usize,
    /// part bones without a driver bone (they keep the reference pose)
    pub unmapped: Vec<String>,
    pub collapsed: Vec<String>,
    /// vertices whose 4 weights do not sum to 255
    pub weight_sum_off: usize,
    /// influences pointing past the part's bone list
    pub bad_index: usize,
    /// part bones no vertex is weighted to
    pub unused: usize,
    /// part bone -> driver bone index
    pub map: Vec<Option<usize>>,
}

/// One rig's current pose and state (built by viewmodel.rs / bots.rs).
pub struct RigView {
    pub label: String,
    pub names: Vec<String>,
    pub parents: Vec<i32>,
    /// reference local pose (rotation in the code's convention, translation UE units)
    pub reference: Vec<(Quat, Vec3)>,
    pub local: Vec<(Quat, Vec3)>,
    pub comp: Vec<(Quat, Vec3)>,
    /// track index of each bone in the playing set
    pub track: Vec<Option<usize>>,
    /// joint positions, Bevy world metres
    pub world: Vec<Vec3>,
    /// bone axis tips (x, y, z), Bevy world metres
    pub axes: Vec<[Vec3; 3]>,
    pub seq: String,
    pub seq_t: f32,
    pub info: Vec<String>,
    pub parts: Vec<PartInfo>,
}

/// Local transforms from component-space ones (same parent rule as the pose composition).
pub fn local_from_comp(parents: &[i32], comp: &[(Quat, Vec3)]) -> Vec<(Quat, Vec3)> {
    comp.iter()
        .enumerate()
        .map(|(i, &(r, p))| {
            let pa = parents.get(i).copied().unwrap_or(-1);
            if i == 0 || pa < 0 || pa as usize >= i {
                (r, p)
            } else {
                let (pr, pp) = comp[pa as usize];
                let inv = pr.conjugate();
                (inv * r, inv * (p - pp))
            }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Weapon1P,
    Bot(usize),
    Aim,
}

struct FrameRec {
    w: std::io::BufWriter<std::fs::File>,
    path: String,
    left: u32,
    frame: u32,
}

#[derive(Resource)]
pub struct RigDebug {
    pub open: bool,
    pub target: Target,
    names: bool,
    axes: bool,
    only_dev: bool,
    filter: String,
    scale: f32,
    rec_frames: u32,
    rec: Option<FrameRec>,
    status: String,
    snap_req: bool,
    check_req: bool,
    env_done: bool,
}

impl Default for RigDebug {
    fn default() -> Self {
        RigDebug { open: false, target: Target::Weapon1P, names: false, axes: false, only_dev: false, filter: String::new(), scale: 1.0, rec_frames: 180, rec: None, status: String::new(), snap_req: false, check_req: false, env_done: false }
    }
}

impl RigDebug {
    pub fn request_snapshot(&mut self) {
        self.snap_req = true;
    }
    pub fn request_check(&mut self) {
        self.check_req = true;
    }
}

pub struct RigDebugPlugin;

impl Plugin for RigDebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RigDebug>().add_systems(Update, keys).add_systems(EguiPrimaryContextPass, draw);
    }
}

fn keys(keys: Res<ButtonInput<KeyCode>>, mut st: ResMut<RigDebug>, flow: Res<crate::menu::MatchFlow>, time: Res<Time<Real>>, mut since: Local<f32>) {
    // HB_RIGDBG=<1p|aim|bot>: overlay on, rig check and a snapshot 5 s into the match (tests)
    if flow.phase == crate::menu::Phase::Playing && !st.env_done {
        *since += time.delta_secs();
        if let Ok(v) = std::env::var("HB_RIGDBG") {
            st.open = true;
            st.names = true;
            st.target = match v.as_str() {
                "aim" => Target::Aim,
                "bot" => Target::Bot(0),
                _ => Target::Weapon1P,
            };
            if *since > 5.0 {
                st.env_done = true;
                st.snap_req = true;
                st.check_req = true;
            }
        } else {
            st.env_done = true;
        }
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::F3) {
        if shift {
            st.snap_req = true;
            st.open = true;
        } else {
            st.open = !st.open;
        }
    }
    if keys.just_pressed(KeyCode::F4) && st.open {
        // next target: 1P -> crosshair bot -> bots by slot
        st.target = match st.target {
            Target::Weapon1P => Target::Aim,
            Target::Aim => Target::Bot(0),
            Target::Bot(i) => Target::Bot(i + 1),
        };
    }
}

fn unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn deg(q: Quat) -> Vec3 {
    let (x, y, z) = q.to_euler(EulerRot::XYZ);
    Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI)
}

fn ang(a: Quat, b: Quat) -> f32 {
    a.angle_between(b).to_degrees()
}

fn v3(v: Vec3) -> String {
    format!("{:9.3} {:9.3} {:9.3}", v.x, v.y, v.z)
}

fn q4(q: Quat) -> String {
    format!("{:8.5} {:8.5} {:8.5} {:8.5}", q.x, q.y, q.z, q.w)
}

fn part_text(o: &mut String, p: &PartInfo) {
    let _ = writeln!(
        o,
        "  part {}: {} bones, {} verts, unmapped {} {:?}, collapsed {:?}, weight sum != 255: {}, bad bone index: {}, bones without vertices: {}",
        p.label,
        p.bones,
        p.verts,
        p.unmapped.len(),
        p.unmapped,
        p.collapsed,
        p.weight_sum_off,
        p.bad_index,
        p.unused
    );
}

/// The full text snapshot of a rig.
pub fn snapshot_text(v: &RigView) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "HomeBots rig snapshot: {}", v.label);
    let _ = writeln!(o, "sequence {} t {:.4}", v.seq, v.seq_t);
    for l in &v.info {
        let _ = writeln!(o, "{l}");
    }
    for p in &v.parts {
        part_text(&mut o, p);
    }
    let _ = writeln!(o, "\nlocal = parent space (UE units, code rotation convention), comp = component space, world = Bevy metres");
    let _ = writeln!(
        o,
        "{:>3} {:<28} {:>3} {:>4} | {:^35} | {:^29} | {:^29} | {:>7} {:>7} | {:^35} | {:^29} | {:^29}",
        "idx", "bone", "par", "trk", "local q (x y z w)", "local pos", "local euler XYZ deg", "dRot", "dPos", "comp q", "comp pos", "world"
    );
    for i in 0..v.names.len() {
        let (lr, lp) = v.local[i];
        let (rr, rp) = v.reference[i];
        let (cr, cp) = v.comp[i];
        let _ = writeln!(
            o,
            "{i:>3} {:<28} {:>3} {:>4} | {} | {} | {} | {:7.2} {:7.3} | {} | {} | {}",
            v.names[i],
            v.parents[i],
            v.track[i].map(|t| t.to_string()).unwrap_or("-".into()),
            q4(lr),
            v3(lp),
            v3(deg(lr)),
            ang(lr, rr),
            lp.distance(rp),
            q4(cr),
            v3(cp),
            v3(v.world[i])
        );
    }
    let _ = writeln!(o, "\nreference (bind) pose:");
    for i in 0..v.names.len() {
        let (rr, rp) = v.reference[i];
        let _ = writeln!(o, "{i:>3} {:<28} q {} pos {}", v.names[i], q4(rr), v3(rp));
    }
    for p in &v.parts {
        let _ = writeln!(o, "\npart {} bone map (part bone -> driver bone):", p.label);
        let _ = writeln!(o, "  {:?}", p.map);
    }
    o
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut contexts: EguiContexts,
    mut st: ResMut<RigDebug>,
    vm: Option<Res<ViewModel>>,
    assets: Option<Res<BotAssets>>,
    pawns: Query<&Pawn>,
    bots: Query<(&Bot, &Soldier)>,
    cams: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::minimap::MinimapCam>)>,
    time: Res<Time>,
) -> Result {
    if !st.open {
        return Ok(());
    }
    let st = &mut *st;
    ANIM_SCALE.store(st.scale.to_bits(), Relaxed);
    let cam = cams.iter().filter(|(c, _)| c.is_active).min_by_key(|(c, _)| c.order);
    // the rig
    let view: Option<RigView> = match st.target {
        Target::Weapon1P => vm.as_ref().zip(pawns.single().ok()).and_then(|(vm, p)| crate::viewmodel::rig_view(vm, p)),
        Target::Bot(i) => assets.as_ref().and_then(|a| {
            let mut list: Vec<(&Bot, &Soldier)> = bots.iter().collect();
            list.sort_by_key(|(b, _)| b.slot);
            if list.is_empty() {
                return None;
            }
            let (b, s) = list[i % list.len()];
            Some(crate::bots::rig_view(a, b, s))
        }),
        Target::Aim => assets.as_ref().and_then(|a| {
            let (_, gt) = cam?;
            let (o, f) = (gt.translation(), gt.forward().as_vec3());
            bots.iter()
                .filter(|(_, s)| s.alive)
                .map(|(b, s)| {
                    let d = s.feet + Vec3::Y * 1.0 - o;
                    (b, s, d.normalize_or_zero().dot(f))
                })
                .filter(|x| x.2 > 0.9)
                .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
                .map(|(b, s, _)| crate::bots::rig_view(a, b, s))
        }),
    };
    let ctx = contexts.ctx_mut()?;
    // skeleton over the game
    if let (Some(v), Some((c, gt))) = (view.as_ref(), cam) {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("hb_rig_paint")));
        let proj = |p: Vec3| c.world_to_viewport(gt, p).ok().map(|s| egui::pos2(s.x, s.y));
        let pts: Vec<Option<egui::Pos2>> = v.world.iter().map(|&p| proj(p)).collect();
        let collapsed: std::collections::HashSet<&str> = v.parts.iter().flat_map(|p| p.collapsed.iter().map(|s| s.as_str())).collect();
        let unmapped: std::collections::HashSet<&str> = v.parts.iter().flat_map(|p| p.unmapped.iter().map(|s| s.as_str())).collect();
        let filt = st.filter.to_ascii_lowercase();
        for i in 0..v.names.len() {
            let Some(a) = pts[i] else { continue };
            let pa = v.parents[i];
            if i > 0 && pa >= 0 && (pa as usize) < i {
                if let Some(b) = pts[pa as usize] {
                    painter.line_segment([a, b], egui::Stroke::new(1.5_f32, egui::Color32::from_rgba_unmultiplied(230, 230, 230, 200)));
                }
            }
            let name = v.names[i].as_str();
            let col = if collapsed.contains(name) {
                egui::Color32::from_rgb(255, 70, 70)
            } else if v.track[i].is_some() {
                egui::Color32::from_rgb(90, 230, 120)
            } else {
                egui::Color32::from_rgb(255, 210, 60)
            };
            painter.circle_filled(a, 2.5, col);
            if unmapped.contains(name) {
                painter.circle_stroke(a, 5.0, egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 60, 255)));
            }
            if st.axes {
                for (k, ac) in [egui::Color32::RED, egui::Color32::GREEN, egui::Color32::from_rgb(80, 140, 255)].into_iter().enumerate() {
                    if let Some(t) = proj(v.axes[i][k]) {
                        painter.line_segment([a, t], egui::Stroke::new(1.0_f32, ac));
                    }
                }
            }
            if st.names && (filt.is_empty() || name.to_ascii_lowercase().contains(&filt)) {
                painter.text(a + egui::vec2(4.0, -4.0), egui::Align2::LEFT_BOTTOM, format!("{i} {name}"), egui::FontId::monospace(10.0), egui::Color32::from_rgb(220, 224, 232));
            }
        }
    }
    // snapshot / frame recording
    if st.snap_req {
        st.snap_req = false;
        match view.as_ref() {
            Some(v) => {
                let _ = std::fs::create_dir_all("logs");
                let tag: String = v.label.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
                let path = format!("logs/rig_{tag}_{}.txt", unix());
                st.status = match std::fs::write(&path, snapshot_text(v)) {
                    Ok(_) => format!("Snapshot: {path}"),
                    Err(e) => format!("Snapshot fehlgeschlagen: {e}"),
                };
                info!("rig: {}", st.status);
            }
            None => st.status = "kein Rig für dieses Ziel".into(),
        }
    }
    if let (Some(r), Some(v)) = (st.rec.as_mut(), view.as_ref()) {
        use std::io::Write;
        for i in 0..v.names.len() {
            let (lr, lp) = v.local[i];
            let (cr, cp) = v.comp[i];
            let w = v.world[i];
            let _ = writeln!(
                r.w,
                "{},{:.4},{:.5},{},{},{:.4},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.4},{:.4},{:.4},{:.6},{:.6},{:.6},{:.6},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}",
                r.frame,
                time.elapsed_secs(),
                time.delta_secs(),
                v.label,
                v.seq,
                v.seq_t,
                i,
                v.names[i],
                v.track[i].map(|t| t as i64).unwrap_or(-1),
                lr.x,
                lr.y,
                lr.z,
                lr.w,
                lp.x,
                lp.y,
                lp.z,
                cr.x,
                cr.y,
                cr.z,
                cr.w,
                cp.x,
                cp.y,
                cp.z,
                w.x,
                w.y,
                w.z
            );
        }
        r.frame += 1;
        r.left = r.left.saturating_sub(1);
        if r.left == 0 {
            let _ = r.w.flush();
            st.status = format!("Aufnahme gespeichert: {} ({} Frames)", r.path, r.frame);
            info!("rig: {}", st.status);
            st.rec = None;
        }
    }
    // panel
    let mut start_rec = false;
    let mut check = false;
    egui::Window::new(egui::RichText::new("Rig").strong())
        .id(egui::Id::new("hb_rig"))
        .default_pos(egui::pos2(16.0, 300.0))
        .default_size(egui::vec2(760.0, 460.0))
        .resizable(true)
        .collapsible(true)
        .frame(egui::Frame::window(&ctx.style()).fill(egui::Color32::from_rgba_unmultiplied(16, 18, 24, 220)))
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let nb = bots.iter().count();
                let label = match st.target {
                    Target::Weapon1P => "1P Waffe + Arme".to_string(),
                    Target::Aim => "Bot im Fadenkreuz".to_string(),
                    Target::Bot(i) => format!("Bot #{}", if nb > 0 { i % nb } else { 0 }),
                };
                egui::ComboBox::from_id_salt("rig_target").selected_text(label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.target, Target::Weapon1P, "1P Waffe + Arme");
                    ui.selectable_value(&mut st.target, Target::Aim, "Bot im Fadenkreuz");
                    let mut list: Vec<(&Bot, &Soldier)> = bots.iter().collect();
                    list.sort_by_key(|(b, _)| b.slot);
                    for (k, (_, s)) in list.iter().enumerate() {
                        ui.selectable_value(&mut st.target, Target::Bot(k), format!("#{k} {} (Team {})", s.name, s.team));
                    }
                });
                ui.checkbox(&mut st.names, "Namen");
                ui.checkbox(&mut st.axes, "Achsen");
                ui.checkbox(&mut st.only_dev, "nur Δ>0,5°");
                ui.label("Filter");
                ui.add(egui::TextEdit::singleline(&mut st.filter).desired_width(90.0));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Anim-Tempo");
                ui.add(egui::Slider::new(&mut st.scale, 0.0..=1.0).step_by(0.05));
                if ui.button(if st.scale == 0.0 { "Weiter" } else { "Einfrieren" }).clicked() {
                    st.scale = if st.scale == 0.0 { 1.0 } else { 0.0 };
                }
                if ui.button("Snapshot in Datei").clicked() {
                    st.snap_req = true;
                }
                if st.rec.is_some() {
                    ui.label(egui::RichText::new(format!("REC {} Frames übrig", st.rec.as_ref().map(|r| r.left).unwrap_or(0))).color(egui::Color32::from_rgb(255, 90, 90)));
                } else {
                    ui.add(egui::DragValue::new(&mut st.rec_frames).range(10..=3000).suffix(" Frames"));
                    if ui.button("Aufnahme").clicked() {
                        start_rec = true;
                    }
                }
                if ui.button("Rig-Prüfung in Datei").clicked() {
                    check = true;
                }
            });
            let Some(v) = view.as_ref() else {
                ui.label("kein Rig für dieses Ziel (1P nur im Match sichtbar, Bots nur mit lebenden Bots)");
                return;
            };
            ui.label(egui::RichText::new(format!("{}  ·  {} Bones  ·  Sequenz {} t {:.3}", v.label, v.names.len(), v.seq, v.seq_t)).strong());
            for l in &v.info {
                ui.label(egui::RichText::new(l).monospace().size(12.0));
            }
            for p in &v.parts {
                let warn = !p.unmapped.is_empty() || p.weight_sum_off > 0 || p.bad_index > 0;
                ui.label(
                    egui::RichText::new(format!(
                        "Teil {}: {} Bones, {} Verts, ohne Treiber-Bone {}, kollabiert {}, Gewichtssumme≠255 {}, falscher Index {}",
                        p.label,
                        p.bones,
                        p.verts,
                        p.unmapped.len(),
                        p.collapsed.len(),
                        p.weight_sum_off,
                        p.bad_index
                    ))
                    .monospace()
                    .size(12.0)
                    .color(if warn { egui::Color32::from_rgb(255, 180, 70) } else { egui::Color32::from_rgb(150, 156, 166) }),
                );
            }
            ui.separator();
            let filt = st.filter.to_ascii_lowercase();
            egui::ScrollArea::both().auto_shrink([false, false]).max_height(320.0).show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.label(egui::RichText::new(format!("{:>3} {:<24} {:>3} {:>3} | {:^26} | {:^26} | {:>6} {:>7} | {:^26}", "#", "Bone", "par", "trk", "lokal Euler XYZ °", "lokal Pos (uu)", "ΔRot°", "ΔPos", "Welt (m)")).monospace().size(11.5).strong());
                for i in 0..v.names.len() {
                    if !filt.is_empty() && !v.names[i].to_ascii_lowercase().contains(&filt) {
                        continue;
                    }
                    let (lr, lp) = v.local[i];
                    let (rr, rp) = v.reference[i];
                    let dr = ang(lr, rr);
                    if st.only_dev && dr < 0.5 {
                        continue;
                    }
                    let e = deg(lr);
                    let w = v.world[i];
                    let col = if v.track[i].is_some() { egui::Color32::from_rgb(200, 230, 205) } else { egui::Color32::from_rgb(230, 215, 160) };
                    ui.label(
                        egui::RichText::new(format!(
                            "{i:>3} {:<24} {:>3} {:>3} | {:8.2} {:8.2} {:8.2} | {:8.2} {:8.2} {:8.2} | {:6.2} {:7.3} | {:8.3} {:8.3} {:8.3}",
                            v.names[i].chars().take(24).collect::<String>(),
                            v.parents[i],
                            v.track[i].map(|t| t.to_string()).unwrap_or("-".into()),
                            e.x,
                            e.y,
                            e.z,
                            lp.x,
                            lp.y,
                            lp.z,
                            dr,
                            lp.distance(rp),
                            w.x,
                            w.y,
                            w.z
                        ))
                        .monospace()
                        .size(11.5)
                        .color(col),
                    );
                }
            });
            if !st.status.is_empty() {
                ui.label(egui::RichText::new(&st.status).small().color(egui::Color32::from_rgb(150, 156, 166)));
            }
            ui.label(egui::RichText::new("F3 an/aus · Shift+F3 Snapshot · F4 nächstes Ziel · Maus: F1").small().color(egui::Color32::from_rgb(150, 156, 166)));
        });
    if start_rec {
        let _ = std::fs::create_dir_all("logs");
        let path = format!("logs/rig_{}_frames.csv", unix());
        match std::fs::File::create(&path) {
            Ok(f) => {
                use std::io::Write;
                let mut w = std::io::BufWriter::new(f);
                let _ = writeln!(w, "frame,time,dt,target,seq,seq_t,bone_idx,bone,track,lqx,lqy,lqz,lqw,lpx,lpy,lpz,cqx,cqy,cqz,cqw,cpx,cpy,cpz,wx,wy,wz");
                st.rec = Some(FrameRec { w, path: path.clone(), left: st.rec_frames, frame: 0 });
                st.status = format!("Aufnahme läuft: {path}");
            }
            Err(e) => st.status = format!("Aufnahme nicht möglich: {e}"),
        }
    }
    if check || std::mem::take(&mut st.check_req) {
        let text = rig_check(vm.as_deref(), assets.as_deref());
        let _ = std::fs::create_dir_all("logs");
        st.status = match std::fs::write("logs/rig_check.txt", text) {
            Ok(_) => "Rig-Prüfung: logs/rig_check.txt".into(),
            Err(e) => format!("Rig-Prüfung fehlgeschlagen: {e}"),
        };
        info!("rig: {}", st.status);
    }
    Ok(())
}

/// Static check of every loaded rig: bone mapping, animation tracks, skin weights.
pub fn rig_check(vm: Option<&ViewModel>, assets: Option<&BotAssets>) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "HomeBots rig check ({})", unix());
    if let Some(vm) = vm {
        for (i, w) in vm.weapons.iter().enumerate() {
            let _ = writeln!(o, "\n== 1P weapon {i}: {} ({}), skeleton {} bones, anim set {} ({} tracks, {} sequences)", w.cfg.cfg, w.cfg.sk_mesh, w.skeleton.len(), w.anims.path, w.anims.bones.len(), w.anims.seqs.len());
            let no_track: Vec<&str> = w.skeleton.iter().zip(&w.track_of_bone).filter(|(_, t)| t.is_none()).map(|(b, _)| b.name.as_str()).collect();
            let _ = writeln!(o, "  skeleton bones without a track ({}): {:?}", no_track.len(), no_track);
            let orphan: Vec<&String> = w.anims.bones.iter().filter(|t| !w.skeleton.iter().any(|b| b.name.eq_ignore_ascii_case(t))).collect();
            let _ = writeln!(o, "  tracks without a skeleton bone ({}): {:?}", orphan.len(), orphan);
            for s in &w.anims.seqs {
                let _ = writeln!(o, "  seq {:<28} len {:7.3} s  frames {:4}  rate {:5.2}  sounds {}  footsteps {}", s.name, s.length, s.frames, s.rate, s.sounds.len(), s.footsteps.len());
            }
            for p in crate::viewmodel::part_infos(w) {
                part_text(&mut o, &p);
            }
        }
    } else {
        let _ = writeln!(o, "\n(no 1P weapons loaded)");
    }
    if let Some(a) = assets {
        let _ = writeln!(o, "\n== bot animation sets");
        for s in &a.lib.sets {
            let _ = writeln!(o, "  {} : {} tracks, {} sequences", s.path, s.bones.len(), s.seqs.len());
        }
        for (ri, r) in a.rigs.iter().enumerate() {
            let _ = writeln!(o, "\n== bot rig {ri}: {} bones", r.skeleton.len());
            for (si, s) in a.lib.sets.iter().enumerate() {
                let missing = crate::bots::rig_missing_tracks(r, si);
                let _ = writeln!(o, "  set {}: bones without a track {}: {:?}", s.path, missing.len(), missing);
            }
        }
        for (si, s) in a.slots.iter().enumerate() {
            let _ = writeln!(o, "\n== bot slot {si}: team {} rig {} weapon cfg {}", s.team, s.rig, s.weapon_cfg);
            let rig = &a.rigs[s.rig];
            part_text(&mut o, &crate::viewmodel::part_info(&s.body, "body", &rig.skeleton));
            if let Some(w) = &s.weapon {
                part_text(&mut o, &crate::viewmodel::part_info(w, "weapon", &rig.skeleton));
            }
        }
    } else {
        let _ = writeln!(o, "\n(no bots loaded)");
    }
    o
}
