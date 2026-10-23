//! Debug menu (F1, user request 2026-10-18): player cheats, bot overview and controls, match
//! controls (rounds, objectives, time scale) and the bot behaviour log. Drawn with egui on the
//! HUD camera (the menu's Camera2d, drawn last). While it is open the game keeps running but
//! takes no player input and the cursor is free.
use crate::bots::{Bot, BotDebug, BotLog};
use crate::combat::{Actor, Cheats, CombatData, DamageQueue, Hit, PlayerCombat, Soldier, Target, TEAM_KVA, TEAM_USA};
use crate::menu::{MatchFlow, Phase};
use crate::objectives::Objectives;
use crate::player::{CollisionWorld, Pawn};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .insert_resource(EguiGlobalSettings { auto_create_primary_context: false, ..default() })
            .init_resource::<DebugUi>()
            .add_plugins(crate::coldebug::ColDebugPlugin)
            .add_systems(Update, (attach_context, toggle))
            .add_systems(EguiPrimaryContextPass, draw);
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Player,
    Bots,
    Match,
    Log,
    Tools,
}

#[derive(Resource)]
struct DebugUi {
    open: bool,
    tab: Tab,
    styled: bool,
    time_scale: f32,
    fps: f32,
    /// last action, shown in the footer
    status: String,
    /// bot table: only problems (idle / no nav path)
    only_problems: bool,
}

impl Default for DebugUi {
    fn default() -> Self {
        DebugUi { open: false, tab: Tab::Player, styled: false, time_scale: 1.0, fps: 0.0, status: String::new(), only_problems: false }
    }
}

/// egui draws on the HUD camera (order 1, drawn after the 3D view and the minimap capture).
fn attach_context(mut commands: Commands, cams: Query<(Entity, &Camera), (With<Camera2d>, Without<PrimaryEguiContext>)>, has: Query<(), With<PrimaryEguiContext>>) {
    if !has.is_empty() {
        return;
    }
    if let Some((e, _)) = cams.iter().max_by_key(|(_, c)| c.order) {
        commands.entity(e).insert(PrimaryEguiContext);
    }
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut ui: ResMut<DebugUi>, mut flow: ResMut<MatchFlow>, time: Res<Time<Real>>, mut log: ResMut<BotLog>, mut tests: Local<(bool, f32, bool)>) {
    if keys.just_pressed(KeyCode::F1) {
        ui.open = !ui.open;
    }
    // test hooks: HB_DEBUGMENU=<player|bots|match|log> opens the menu once playing,
    // HB_BOTLOG=1 records the bot log to a file, HB_TEST_ROUND=<s> wins a round for USA that
    // many real seconds into the match
    if flow.phase == Phase::Playing {
        if !tests.0 {
            tests.0 = true;
            if let Ok(t) = std::env::var("HB_DEBUGMENU") {
                ui.open = true;
                ui.tab = match t.as_str() {
                    "bots" => Tab::Bots,
                    "match" => Tab::Match,
                    "log" => Tab::Log,
                    "tools" => Tab::Tools,
                    _ => Tab::Player,
                };
            }
            if std::env::var_os("HB_TEST_UAV").is_some() {
                flow.hud.uav_left = 9999.0;
            }
            if std::env::var_os("HB_BOTLOG").is_some() {
                let p = log.start_file();
                info!("bot log: {}", p.display());
            }
        }
        tests.1 += time.delta_secs();
        // HB_TEST_MATCHWIN=<s>: the second round won for USA (match end)
        if let Some(at) = std::env::var("HB_TEST_MATCHWIN").ok().and_then(|v| v.parse::<f32>().ok()) {
            if tests.1 >= at && !tests.2 {
                tests.2 = true;
                flow.hud.rounds[0] = 1;
                flow.hud.score[0] = flow.hud.score_limit.max(1);
                info!("test: match won for USA");
            }
        }
        if let Some(at) = std::env::var("HB_TEST_ROUND").ok().and_then(|v| v.parse::<f32>().ok()) {
            if tests.1 >= at && !tests.2 {
                tests.2 = true;
                flow.hud.score[0] = flow.hud.score_limit.max(1);
                info!("test: round won for USA");
            }
        }
    }
    let dt = time.delta_secs();
    if dt > 0.0 {
        ui.fps = if ui.fps == 0.0 { 1.0 / dt } else { ui.fps * 0.95 + 0.05 / dt };
    }
    flow.debug = ui.open && flow.phase == Phase::Playing;
}

/// Colours of the menu (dark, one accent).
const ACCENT: egui::Color32 = egui::Color32::from_rgb(64, 156, 255);
const USA_C: egui::Color32 = egui::Color32::from_rgb(110, 170, 255);
const KVA_C: egui::Color32 = egui::Color32::from_rgb(255, 105, 105);
const WARN_C: egui::Color32 = egui::Color32::from_rgb(255, 180, 70);
const OK_C: egui::Color32 = egui::Color32::from_rgb(120, 220, 140);
const DIM_C: egui::Color32 = egui::Color32::from_rgb(150, 156, 166);

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.window_fill = egui::Color32::from_rgba_unmultiplied(22, 24, 30, 242);
    v.panel_fill = egui::Color32::from_rgb(22, 24, 30);
    v.extreme_bg_color = egui::Color32::from_rgb(14, 15, 19);
    v.faint_bg_color = egui::Color32::from_rgb(30, 33, 41);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.window_corner_radius = egui::CornerRadius::same(12);
    v.window_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(52, 56, 68));
    v.window_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(140) };
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive, &mut v.widgets.open] {
        w.corner_radius = egui::CornerRadius::same(7);
    }
    v.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(40, 44, 54);
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 44, 54);
    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(54, 60, 74);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(54, 60, 74);
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.bg_fill = ACCENT;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.window_margin = egui::Margin::same(16);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(21.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
    });
}

/// A titled card (section) in the menu.
fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(30, 33, 41))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).strong().color(egui::Color32::from_rgb(220, 224, 232)));
            ui.add_space(2.0);
            add(ui)
        })
        .inner
}

fn big_button(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(text).size(15.0)).min_size(egui::vec2(0.0, 30.0))).clicked()
}

fn toggle_row(ui: &mut egui::Ui, on: &mut bool, text: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.checkbox(on, egui::RichText::new(text));
        ui.label(egui::RichText::new(hint).small().color(DIM_C));
    });
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut contexts: EguiContexts,
    mut st: ResMut<DebugUi>,
    mut flow: ResMut<MatchFlow>,
    (mut pc, mut cheats, data): (ResMut<PlayerCombat>, ResMut<Cheats>, Option<Res<CombatData>>),
    (mut bot_dbg, mut log): (ResMut<BotDebug>, ResMut<BotLog>),
    mut obj: ResMut<Objectives>,
    mut pawns: Query<&mut Pawn>,
    bots: Query<(Entity, &Bot, &Soldier)>,
    mut queue: ResMut<DamageQueue>,
    col: Res<CollisionWorld>,
    mut vtime: ResMut<Time<Virtual>>,
    mut buf: Local<Vec<u32>>,
    (mut prof, mut rig, rtime): (ResMut<crate::perf::Profiler>, ResMut<crate::rigdebug::RigDebug>, Res<Time<Real>>),
    vassets: Option<Res<crate::vehicles::VehicleAssets>>,
    (mut weather, mut coldbg): (ResMut<crate::weather::Weather>, ResMut<crate::coldebug::ColDebug>),
) -> Result {
    if !st.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    if !st.styled {
        style(ctx);
        st.styled = true;
    }
    let playing = flow.phase == Phase::Playing;
    let now = vtime.elapsed_secs();
    let mut status: Option<String> = None;
    egui::Window::new(egui::RichText::new("HomeBots  ·  Debug").heading().color(egui::Color32::WHITE))
        .id(egui::Id::new("hb_debug"))
        .default_pos(egui::pos2(24.0, 24.0))
        .default_size(egui::vec2(640.0, 640.0))
        .min_width(560.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            // header: state chips
            ui.horizontal(|ui| {
                let chip = |ui: &mut egui::Ui, text: String, c: egui::Color32| {
                    egui::Frame::new().fill(c.gamma_multiply(0.18)).corner_radius(egui::CornerRadius::same(20)).inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
                        ui.label(egui::RichText::new(text).color(c).small().strong());
                    });
                };
                chip(ui, format!("{:.0} FPS", st.fps), if st.fps >= 50.0 { OK_C } else if st.fps >= 30.0 { WARN_C } else { KVA_C });
                chip(ui, format!("{} KP", flow.hud.kp), ACCENT);
                chip(ui, format!("Runde {}:{}", flow.hud.rounds[0], flow.hud.rounds[1]), DIM_C);
                chip(ui, format!("Front {}", obj.active_line), DIM_C);
                if cheats.god {
                    chip(ui, "GOD".into(), OK_C);
                }
                if bot_dbg.frozen {
                    chip(ui, "KI EINGEFROREN".into(), WARN_C);
                }
                if !playing {
                    chip(ui, "kein Match".into(), KVA_C);
                }
            });
            ui.add_space(4.0);
            // tabs
            ui.horizontal(|ui| {
                for (t, name) in [(Tab::Player, "Spieler"), (Tab::Bots, "Bots"), (Tab::Match, "Match"), (Tab::Log, "Bot-Log"), (Tab::Tools, "Werkzeuge")] {
                    let sel = st.tab == t;
                    let txt = egui::RichText::new(name).size(15.5).color(if sel { egui::Color32::WHITE } else { DIM_C });
                    if ui.add(egui::Button::new(txt).fill(if sel { ACCENT } else { egui::Color32::TRANSPARENT }).min_size(egui::vec2(96.0, 32.0))).clicked() {
                        st.tab = t;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match st.tab {
                Tab::Player => {
                    card(ui, "Kill-Punkte", |ui| {
                        ui.horizontal(|ui| {
                            for k in [1000, 5000] {
                                if big_button(ui, &format!("+{k} KP")) {
                                    flow.hud.kp += k;
                                    status = Some(format!("+{k} KP"));
                                }
                            }
                            if big_button(ui, "KP auf 0") {
                                flow.hud.kp = 0;
                                status = Some("KP auf 0".into());
                            }
                        });
                    });
                    card(ui, "Killstreaks (gratis)", |ui| {
                        ui.label(egui::RichText::new("wird beim Schließen des Menüs ausgelöst (Drohne / Hellfire übernehmen die Steuerung); grau = in HomeBots noch nicht umgesetzt").small().color(DIM_C));
                        if let Some(d) = data.as_ref() {
                            let mut items: Vec<(&String, i32)> = d.costs.iter().map(|(k, v)| (k, v.0)).collect();
                            items.sort_by_key(|x| (x.1, x.0.clone()));
                            ui.horizontal_wrapped(|ui| {
                                for (cfg, cost) in items {
                                    let name = d.special_names.get(cfg).cloned().unwrap_or_else(|| cfg.trim_start_matches("cfg_").to_uppercase());
                                    // what combat.rs can buy: flak jacket, UAV, ground assault drone, Hellfire, launchers
                                    // vehicles (GCBattlePoints VehicleCosts): placed at the player
                                    if cfg.starts_with("gcv_") {
                                        let kind = vassets.as_ref().and_then(|a| a.kind_of(cfg).map(|k| a.kinds[k].name.clone()));
                                        let r = ui.add_enabled(kind.is_some() && flow.phase == crate::menu::Phase::Playing, egui::Button::new(egui::RichText::new(format!("{}  ({cost} KP)", kind.clone().unwrap_or_else(|| cfg.to_uppercase()))).size(15.0)).min_size(egui::vec2(0.0, 30.0)));
                                        if r.clicked() {
                                            cheats.vehicle = Some(cfg.clone());
                                            status = Some(format!("{} beim Schließen", kind.unwrap_or_default()));
                                        }
                                        continue;
                                    }
                                    let works = matches!(cfg.as_str(), "cfg_eqp_flakjacket" | "cfg_eqp_uav" | "cfg_dro_gassault" | "cfg_dro_gantitank" | "cfg_dro_aassault" | "cfg_dro_arecon" | "cfg_eqp_resupply" | "cfg_eqp_thermal") || d.strikes.iter().any(|s| &s.key == cfg) || d.weapons.iter().any(|w| w.cfg.eq_ignore_ascii_case(cfg));
                                    let r = ui.add_enabled(works, egui::Button::new(egui::RichText::new(format!("{name}  ({cost} KP)")).size(15.0)).min_size(egui::vec2(0.0, 30.0)));
                                    if r.clicked() {
                                        cheats.grant.push(cfg.clone());
                                        status = Some(format!("{name} beim Schließen"));
                                    }
                                }
                                // the nuke (nuke.rs; no KP cost, the game has no such killstreak)
                                let r = ui.add(egui::Button::new(egui::RichText::new(format!("{}  (gratis)", crate::nuke::NAME)).size(15.0)).min_size(egui::vec2(0.0, 30.0)));
                                if r.clicked() {
                                    cheats.grant.push(crate::nuke::ITEM.to_ascii_lowercase());
                                    status = Some(format!("{} beim Schließen", crate::nuke::NAME));
                                }
                            });
                        }
                    });
                    card(ui, "Cheats", |ui| {
                        toggle_row(ui, &mut cheats.god, "Unverwundbar", "kein Schaden von Gegnern");
                        toggle_row(ui, &mut cheats.inf_ammo, "Unendlich Munition", "Magazin, Granaten, Taktisch");
                    });
                    card(ui, "Auffüllen", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if big_button(ui, "Gesundheit") {
                                pc.health = pc.health_max;
                                status = Some("Gesundheit voll".into());
                            }
                            if big_button(ui, "Munition") {
                                if let Some(w) = data.as_ref().and_then(|d| d.weapons.get(pc.weapon)) {
                                    pc.clip = w.clip;
                                    pc.reserve = w.total;
                                }
                                status = Some("Munition voll".into());
                            }
                            if big_button(ui, "Granaten") {
                                if let Some(d) = data.as_ref() {
                                    pc.grenades = d.grenade.ammo;
                                    let tac = d.loadouts.get(flow.loadout).and_then(|l| l.items.get(2)).and_then(|i| d.tactical.get(&i.to_ascii_lowercase())).map(|t| t.ammo).unwrap_or(0);
                                    pc.tactical = tac;
                                }
                                status = Some("Granaten voll".into());
                            }
                            if big_button(ui, "UAV starten") {
                                flow.hud.uav_left = data.as_ref().map(|d| d.uav_secs).unwrap_or(30.0);
                                status = Some("UAV aktiv".into());
                            }
                        });
                    });
                    card(ui, "Teleport", |ui| {
                        ui.horizontal(|ui| {
                            let targets: Vec<(usize, Vec3)> = obj.list.iter().filter(|o| o.line == obj.active_line).map(|o| (o.letter, Vec3::new(o.location[0], o.location[2], o.location[1]) * 0.01)).collect();
                            for (letter, at) in targets {
                                if big_button(ui, &format!("Zu {}", ["A", "B", "C", "D"][letter.min(3)])) {
                                    if let Ok(mut p) = pawns.single_mut() {
                                        teleport(&mut p, &col, at, &mut buf);
                                        status = Some(format!("Teleport zu {}", ["A", "B", "C", "D"][letter.min(3)]));
                                    }
                                }
                            }
                        });
                    });
                    card(ui, "Sonstiges", |ui| {
                        if ui.add(egui::Button::new(egui::RichText::new("Selbst töten").color(KVA_C)).min_size(egui::vec2(0.0, 30.0))).clicked() {
                            if let Ok(p) = pawns.single() {
                                let at = Vec3::new(p.feet.x, p.feet.z, p.feet.y) * 100.0;
                                queue.0.push(Hit { target: Target::Player, amount: 100_000.0, attacker: Actor::World, attacker_team: 2, headshot: false, explosive: false, from: at, icon: None, dmg_type: None });
                            }
                        }
                    });
                }
                Tab::Bots => {
                    card(ui, "Aktionen", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let mut kill = |team: Option<u8>, label: &str| {
                                let mut n = 0;
                                for (e, _, s) in &bots {
                                    if s.alive && team.is_none_or(|t| s.team == t) {
                                        queue.0.push(Hit { target: Target::Bot(e), amount: 100_000.0, attacker: Actor::World, attacker_team: 2, headshot: false, explosive: false, from: s.center_ue(), icon: None, dmg_type: None });
                                        n += 1;
                                    }
                                }
                                status = Some(format!("{label}: {n} getötet"));
                            };
                            if big_button(ui, "Alle Gegner töten") {
                                kill(Some(crate::combat::player_team() ^ 1), "Gegner");
                            }
                            if big_button(ui, "Alle Teamkameraden töten") {
                                kill(Some(crate::combat::player_team()), "Team");
                            }
                            if big_button(ui, "Alle Bots töten") {
                                kill(None, "Bots");
                            }
                        });
                        toggle_row(ui, &mut bot_dbg.frozen, "KI einfrieren", "Bots denken und laufen nicht");
                        toggle_row(ui, &mut bot_dbg.passive, "Bots schießen nicht", "Wahrnehmung und Bewegung laufen weiter");
                    });
                    let mut rows: Vec<crate::bots::BotRow> = bots.iter().map(|(e, b, s)| b.row(e, s)).collect();
                    rows.sort_by(|a, b| a.team.cmp(&b.team).then(a.name.len().cmp(&b.name.len())).then(a.name.cmp(&b.name)));
                    let alive = |t: u8| rows.iter().filter(|r| r.team == t && r.alive).count();
                    let total = |t: u8| rows.iter().filter(|r| r.team == t).count();
                    let idle = rows.iter().filter(|r| r.alive && r.idle > 3.0).count();
                    let no_nav = rows.iter().filter(|r| r.alive && !r.nav_ok).count();
                    card(ui, "Übersicht", |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("USA {}/{}", alive(TEAM_USA), total(TEAM_USA))).color(USA_C).strong());
                            ui.label(egui::RichText::new(format!("KVA {}/{}", alive(TEAM_KVA), total(TEAM_KVA))).color(KVA_C).strong());
                            ui.label(egui::RichText::new(format!("{idle} hängen > 3 s")).color(if idle > 0 { WARN_C } else { DIM_C }));
                            ui.label(egui::RichText::new(format!("{no_nav} ohne Nav-Pfad")).color(if no_nav > 0 { KVA_C } else { DIM_C }));
                        });
                        ui.checkbox(&mut st.only_problems, "Nur Auffällige (hängt, kein Nav-Pfad)");
                    });
                    card(ui, "Bots", |ui| {
                        egui::Grid::new("bots").striped(true).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
                            for h in ["Name", "HP", "Ziel", "Entf.", "Hängt", "Resets", "Nav", "Kampf", ""] {
                                ui.label(egui::RichText::new(h).small().color(DIM_C));
                            }
                            ui.end_row();
                            for r in rows.iter().filter(|r| !st.only_problems || (r.alive && (r.idle > 3.0 || !r.nav_ok))) {
                                let tc = if r.team == TEAM_USA { USA_C } else { KVA_C };
                                ui.label(egui::RichText::new(&r.name).color(tc).strong());
                                if r.alive {
                                    ui.label(format!("{:.0}", r.health));
                                } else {
                                    ui.label(egui::RichText::new("tot").color(DIM_C));
                                }
                                ui.label(&r.goal);
                                ui.label(format!("{:.0} m", r.goal_dist));
                                ui.label(egui::RichText::new(format!("{:.1} s", r.idle)).color(if r.idle > 3.0 { WARN_C } else { DIM_C }));
                                ui.label(format!("{}", r.resets));
                                ui.label(egui::RichText::new(if r.nav_ok { "ok" } else { "keiner" }).color(if r.nav_ok { OK_C } else { KVA_C }));
                                ui.label(if r.engaging { "ja" } else { "" });
                                if ui.small_button("hin").on_hover_text("Zum Bot teleportieren").clicked() {
                                    if let Ok(mut p) = pawns.single_mut() {
                                        teleport(&mut p, &col, r.feet + Vec3::new(1.5, 0.0, 0.0), &mut buf);
                                        status = Some(format!("Teleport zu {}", r.name));
                                    }
                                }
                                ui.end_row();
                            }
                        });
                    });
                }
                Tab::Match => {
                    card(ui, "Stand", |ui| {
                        ui.label(format!("Punkte USA {} : {} KVA (Ziel {})   ·   Runden {} : {}   ·   Restzeit {:.0} s", flow.hud.score[0], flow.hud.score[1], flow.hud.score_limit, flow.hud.rounds[0], flow.hud.rounds[1], flow.hud.time_left));
                    });
                    card(ui, "Runde", |ui| {
                        ui.horizontal(|ui| {
                            if big_button(ui, "Runde gewinnen (USA)") {
                                flow.hud.score[0] = flow.hud.score_limit.max(1);
                                status = Some("Runde für USA".into());
                            }
                            if big_button(ui, "Runde verlieren (KVA)") {
                                flow.hud.score[1] = flow.hud.score_limit.max(1);
                                status = Some("Runde für KVA".into());
                            }
                        });
                        ui.label(egui::RichText::new("Die Front verschiebt sich, Spawns und Bot-Ziele folgen.").small().color(DIM_C));
                    });
                    card(ui, "Einsatzziele der aktiven Front", |ui| {
                        let active = obj.active_line;
                        let mut set: Option<(usize, Option<u8>)> = None;
                        egui::Grid::new("objs").spacing(egui::vec2(10.0, 6.0)).show(ui, |ui| {
                            for (i, o) in obj.list.iter().enumerate().filter(|(_, o)| o.line == active) {
                                let (txt, c) = match o.owner {
                                    Some(0) => ("USA", USA_C),
                                    Some(_) => ("KVA", KVA_C),
                                    None => ("neutral", DIM_C),
                                };
                                ui.label(egui::RichText::new(["A", "B", "C", "D"][o.letter.min(3)]).strong());
                                ui.label(egui::RichText::new(txt).color(c));
                                if ui.button("→ USA").clicked() {
                                    set = Some((i, Some(TEAM_USA)));
                                }
                                if ui.button("→ KVA").clicked() {
                                    set = Some((i, Some(TEAM_KVA)));
                                }
                                if ui.button("neutral").clicked() {
                                    set = Some((i, None));
                                }
                                ui.end_row();
                            }
                        });
                        if let Some((i, owner)) = set {
                            let o = &mut obj.list[i];
                            o.owner = owner;
                            o.progress = 0.0;
                            o.capturing_team = None;
                            status = Some("Einsatzziel gesetzt".into());
                        }
                    });
                    card(ui, "Zeit", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Spielgeschwindigkeit");
                            if ui.add(egui::Slider::new(&mut st.time_scale, 0.1..=4.0).step_by(0.05).suffix("×")).changed() {
                                vtime.set_relative_speed(st.time_scale);
                            }
                            if ui.button("1×").clicked() {
                                st.time_scale = 1.0;
                                vtime.set_relative_speed(1.0);
                            }
                        });
                    });
                }
                Tab::Tools => {
                    card(ui, "Performance (F2)", |ui| {
                        toggle_row(ui, &mut prof.overlay, "Overlay anzeigen", "FPS, Frame-Graph, CPU-Systeme, GPU-Pässe");
                        ui.horizontal_wrapped(|ui| {
                            if prof.recording().is_some() {
                                if big_button(ui, "Aufnahme stoppen + auswerten") {
                                    crate::perf::finish(&mut prof);
                                }
                            } else {
                                for (label, secs) in [("Aufnahme 60 s", 60.0), ("Aufnahme 180 s", 180.0), ("bis Stopp", 0.0)] {
                                    if big_button(ui, label) {
                                        prof.start(rtime.elapsed_secs_f64(), secs, format!("F1 {label}"));
                                    }
                                }
                            }
                        });
                        if !prof.status.is_empty() {
                            ui.label(egui::RichText::new(&prof.status).small().color(DIM_C));
                        }
                        ui.label(egui::RichText::new("Dateien: logs/perf_<zeit>.jsonl (pro Frame) und logs/perf_<zeit>_summary.txt").small().color(DIM_C));
                    });
                    card(ui, "Kollision / Wireframe", |ui| crate::coldebug::debug_ui(ui, &mut coldbg));
                    card(ui, "Tageszeit / Wetter", |ui| crate::weather::debug_ui(ui, &mut weather));
                    card(ui, "Rig (F3)", |ui| {
                        toggle_row(ui, &mut rig.open, "Rig-Overlay anzeigen", "Skelett, Bone-Tabelle, Animationszustand");
                        ui.horizontal_wrapped(|ui| {
                            if big_button(ui, "Snapshot in Datei") {
                                rig.open = true;
                                rig.request_snapshot();
                            }
                            if big_button(ui, "Rig-Prüfung in Datei") {
                                rig.open = true;
                                rig.request_check();
                            }
                        });
                        ui.label(egui::RichText::new("Ziel, Anim-Tempo und Frame-Aufnahme im Rig-Fenster; F4 wechselt das Ziel").small().color(DIM_C));
                    });
                }
                Tab::Log => {
                    card(ui, "Aufzeichnung", |ui| {
                        toggle_row(ui, &mut log.enabled, "Bot-Ereignisse aufzeichnen", "Ziele, Steckenbleiben, Stehen, Respawn");
                        ui.horizontal(|ui| {
                            if log.file.is_none() {
                                if big_button(ui, "In Datei schreiben") {
                                    let p = log.start_file();
                                    log.event(now, "log file started".into());
                                    status = Some(format!("Log: {}", p.display()));
                                }
                            } else if big_button(ui, "Datei schließen") {
                                log.event(now, "log file closed".into());
                                log.stop_file();
                                status = Some("Log-Datei geschlossen".into());
                            }
                            if big_button(ui, "Leeren") {
                                log.events.clear();
                            }
                        });
                        if let Some(f) = &log.file {
                            let full = std::env::current_dir().map(|d| d.join(f)).unwrap_or(f.clone());
                            ui.label(egui::RichText::new(format!("Datei: {}  (Schnappschuss aller Bots alle 2 s)", full.display())).small().color(OK_C));
                        }
                    });
                    card(ui, "Ereignisse", |ui| {
                        egui::ScrollArea::vertical().max_height(360.0).stick_to_bottom(true).auto_shrink([false, true]).show(ui, |ui| {
                            if log.events.is_empty() {
                                ui.label(egui::RichText::new(if log.enabled { "noch keine Ereignisse" } else { "Aufzeichnung ist aus" }).color(DIM_C));
                            }
                            for e in &log.events {
                                let c = if e.contains("stuck") || e.contains("NO nav") {
                                    KVA_C
                                } else if e.contains("idle") {
                                    WARN_C
                                } else {
                                    egui::Color32::from_rgb(200, 204, 212)
                                };
                                ui.label(egui::RichText::new(e).monospace().color(c));
                            }
                        });
                    });
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("F1 schließt das Menü · das Spiel läuft weiter").small().color(DIM_C));
                if !st.status.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(&st.status).small().color(OK_C));
                    });
                }
            });
        });
    if let Some(s) = status {
        info!("debug menu: {s}");
        st.status = s;
    }
    Ok(())
}

/// Put the player on the floor at a point (Bevy metres).
fn teleport(p: &mut Pawn, col: &CollisionWorld, at: Vec3, buf: &mut Vec<u32>) {
    let feet = match col.floor(at + Vec3::Y * 3.0, 8.0, buf) {
        Some((y, _)) => Vec3::new(at.x, y, at.z),
        None => at,
    };
    p.feet = feet + Vec3::Y * 0.05;
    p.vel = Vec3::ZERO;
}
