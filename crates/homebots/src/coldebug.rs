//! Debug overlays for collision (F1 menu, tab "Werkzeuge", user wish 2026-10-21 "Tool in mein
//! Debugmenü, um Wireframe und Kollisionsboxen zu sehen"):
//! - Spieler-Kollision: the triangles pawns move against (player::CollisionWorld, COL_PLAYER),
//!   coloured by where they come from (player::SRC_*),
//! - Schuss-Kollision: the triangles shots / line checks hit (COL_TRACE),
//! - Wireframe: the edges of the rendered meshes (Mesh3d entities) near the player,
//! - label at the crosshair: owner (mesh path / volume name) and source of the first collision
//!   triangle hit by the view ray.
//! Drawn as line-list meshes (unlit StandardMaterial, vertex colours) rebuilt when the player
//! moved more than 1 m or a setting changed, within a radius around the player. NOT from data
//! (a dev tool). The UE3 wireframe view mode (wgpu POLYGON_MODE_LINE) is not used: the device
//! feature would have to be requested at start-up and is not guaranteed on every adapter.
//! The switches start off at every launch (only the radius persists in homebots_settings.ini). HB_DEBUG_COLLISION=1 switches the
//! pawn overlay and the label on at start (=all: also shots + wireframe), for test recordings.
use crate::player::{self, CollisionWorld, Pawn};
use bevy::asset::RenderAssetUsages;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{PrimitiveTopology, VertexAttributeValues};
use bevy::render::primitives::Aabb;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

pub struct ColDebugPlugin;

impl Plugin for ColDebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ColDebug>().add_systems(Update, rebuild).add_systems(EguiPrimaryContextPass, label_ui);
    }
}

#[derive(Resource)]
pub struct ColDebug {
    pub pawn: bool,
    pub shots: bool,
    pub terrain: bool,
    pub veh_vols: bool,
    pub wire: bool,
    pub label: bool,
    pub radius: f32,
    loaded: bool,
    /// settings changed since the last rebuild
    dirty: bool,
    /// player position at the last rebuild
    at: Option<Vec3>,
    /// triangle count of the collision world at the last rebuild (level change)
    n_tris: usize,
    /// line entities (pawn/shots, wireframe)
    ents: Vec<Entity>,
    mat: Option<Handle<StandardMaterial>>,
    /// crosshair text
    hit: String,
    pub stats: String,
}

impl Default for ColDebug {
    fn default() -> Self {
        ColDebug { pawn: false, shots: false, terrain: false, veh_vols: false, wire: false, label: false, radius: 30.0, loaded: false, dirty: true, at: None, n_tris: 0, ents: Vec::new(), mat: None, hit: String::new(), stats: String::new() }
    }
}

const KEYS: [&str; 7] = ["Debug:ColPawn", "Debug:ColShots", "Debug:ColTerrain", "Debug:ColVehVol", "Debug:Wire", "Debug:ColLabel", "Debug:ColRadius"];

impl ColDebug {
    fn load(&mut self) {
        self.loaded = true;
        let g = |k: &str, d: f32| {
            let v = crate::settings::get(k);
            if v == 0.0 && crate::settings::get("Debug:ColSaved") == 0.0 {
                d
            } else {
                v
            }
        };
        // every overlay starts off (user 2026-10-22 19:43: "the wireframe was on by default but
        // should be off"; the vehicle-volume overlay defaulted to on and all switches were kept
        // in homebots_settings.ini across starts). Only the radius is kept.
        self.pawn = false;
        self.shots = false;
        self.terrain = false;
        self.veh_vols = false;
        self.wire = false;
        self.label = false;
        self.radius = g(KEYS[6], 30.0).clamp(5.0, 120.0);
        if let Ok(v) = std::env::var("HB_DEBUG_COLLISION") {
            self.pawn = true;
            self.label = true;
            if v == "all" {
                self.shots = true;
                self.wire = true;
            }
        }
    }

    fn save(&self) {
        let b = |x: bool| if x { 1.0 } else { 0.0 };
        for (k, v) in KEYS.iter().zip([b(self.pawn), b(self.shots), b(self.terrain), b(self.veh_vols), b(self.wire), b(self.label), self.radius]) {
            crate::settings::set(k, v);
        }
        crate::settings::set("Debug:ColSaved", 1.0);
        crate::settings::save();
    }
}

/// Colour of a pawn-collision triangle by source (legend in `debug_ui`).
fn src_color(src: u8) -> [f32; 4] {
    match src {
        player::SRC_COLMESH => [0.15, 1.0, 0.3, 1.0],
        player::SRC_BODYSETUP => [1.0, 0.85, 0.1, 1.0],
        player::SRC_UNDECODED => [1.0, 0.2, 1.0, 1.0],
        player::SRC_RENDER => [0.2, 0.8, 1.0, 1.0],
        player::SRC_BLOCKVOL => [1.0, 0.15, 0.15, 1.0],
        player::SRC_SKEL => [0.55, 0.55, 1.0, 1.0],
        player::SRC_TERRAIN => [0.45, 0.45, 0.45, 1.0],
        player::SRC_VEHVOL => [0.75, 0.45, 0.15, 1.0],
        _ => [0.92, 0.92, 0.92, 1.0],
    }
}

const SHOT_C: [f32; 4] = [1.0, 0.5, 0.0, 1.0];
const WIRE_C: [f32; 4] = [0.85, 0.85, 0.85, 1.0];
/// at most this many line segments per overlay (frame time on the iGPU)
const MAX_LINES: usize = 400_000;

fn src_name(src: u8) -> &'static str {
    match src {
        player::SRC_COLMESH => "Kollisions-Mesh (aus Daten)",
        player::SRC_BODYSETUP => "Render-Dreiecke statt BodySetup (NICHT aus Daten)",
        player::SRC_UNDECODED => "Render-Dreiecke, LOD-Layout nicht dekodiert (NICHT aus Daten)",
        player::SRC_RENDER => "Render-Dreiecke (Mesh-Regel)",
        player::SRC_BLOCKVOL => "BlockingVolume",
        player::SRC_SKEL => "Skeletal-Mesh",
        player::SRC_TERRAIN => "Terrain",
        player::SRC_VEHVOL => "GCVehicleBlockingVolume (nur Fahrzeuge)",
        _ => "Straße / sonstiges",
    }
}

/// The F1 menu card (debug.rs, tab "Werkzeuge").
pub fn debug_ui(ui: &mut egui::Ui, d: &mut ColDebug) {
    let before = (d.pawn, d.shots, d.terrain, d.veh_vols, d.wire, d.label, d.radius);
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.pawn, "Spieler-Kollision");
        ui.checkbox(&mut d.shots, "Schuss-Kollision");
        ui.checkbox(&mut d.wire, "Wireframe (Render)");
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.terrain, "Terrain mitzeichnen");
        ui.checkbox(&mut d.veh_vols, "Fahrzeug-Volumen");
        ui.checkbox(&mut d.label, "Name am Fadenkreuz");
    });
    ui.add(egui::Slider::new(&mut d.radius, 5.0..=120.0).text("Radius (m)").step_by(5.0));
    ui.horizontal_wrapped(|ui| {
        for s in [player::SRC_COLMESH, player::SRC_BODYSETUP, player::SRC_UNDECODED, player::SRC_RENDER, player::SRC_BLOCKVOL, player::SRC_VEHVOL, player::SRC_SKEL, player::SRC_OTHER] {
            let c = src_color(s);
            ui.label(egui::RichText::new(format!("■ {}", src_name(s))).small().color(egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)));
        }
        ui.label(egui::RichText::new("■ Schuss-Kollision").small().color(egui::Color32::from_rgb(255, 128, 0)));
    });
    if !d.stats.is_empty() {
        ui.label(egui::RichText::new(&d.stats).small().color(egui::Color32::from_rgb(150, 156, 166)));
    }
    if before != (d.pawn, d.shots, d.terrain, d.veh_vols, d.wire, d.label, d.radius) {
        d.dirty = true;
        d.save();
    }
}

fn line_mesh(pos: Vec<[f32; 3]>, col: Vec<[f32; 4]>) -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::RENDER_WORLD);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m
}

#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    mut d: ResMut<ColDebug>,
    col: Res<CollisionWorld>,
    pawns: Query<&Pawn>,
    objs: Query<(&Mesh3d, &GlobalTransform, Option<&Aabb>, &InheritedVisibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut buf: Local<Vec<u32>>,
) {
    if !d.loaded {
        d.load();
    }
    let Ok(p) = pawns.single() else {
        if !d.ents.is_empty() {
            for e in d.ents.drain(..) {
                commands.entity(e).despawn();
            }
            d.at = None;
        }
        return;
    };
    let eye = p.feet + Vec3::Y * p.eye;
    // crosshair label: first collision triangle along the view ray (pawn or shot collision)
    if d.label {
        let dir = p.dir_ue();
        let dir = Vec3::new(dir.x, dir.z, dir.y).normalize_or_zero();
        d.hit = match col.raycast_hit(eye, dir, 200.0, &mut buf, player::COL_PLAYER | player::COL_TRACE) {
            Some((t, _, i)) => {
                let (m, s, o) = col.tri_info(i);
                let who = match (m & player::COL_PLAYER != 0, m & player::COL_TRACE != 0) {
                    (true, true) => "Spieler+Schüsse",
                    (true, false) => "nur Spieler",
                    (false, true) => "nur Schüsse",
                    _ => "aus",
                };
                format!("{:.1} m  {}\n{}  [{}]", t, if o.is_empty() { "?" } else { o }, src_name(s), who)
            }
            None => "kein Treffer (200 m)".into(),
        };
    }
    let any = d.pawn || d.shots || d.wire;
    let moved = d.at.is_none_or(|a| a.distance(p.feet) > 1.0);
    if !d.dirty && !moved && col.tris.len() == d.n_tris {
        return;
    }
    if !d.dirty && !any {
        return;
    }
    d.dirty = false;
    d.at = Some(p.feet);
    d.n_tris = col.tris.len();
    for e in d.ents.drain(..).collect::<Vec<_>>() {
        commands.entity(e).despawn();
    }
    if !any {
        d.stats.clear();
        return;
    }
    let mat = d.mat.get_or_insert_with(|| mats.add(StandardMaterial { base_color: Color::WHITE, unlit: true, fog_enabled: false, ..default() })).clone();
    let r = d.radius;
    let c = p.feet;
    // lines are pulled a little towards the eye so they win the depth test against their surface
    let lift = |v: Vec3| v + (eye - v).normalize_or_zero() * (0.03 + 0.004 * eye.distance(v)).min(0.25);
    let near = |t: &[Vec3; 3]| t.iter().any(|v| Vec2::new(v.x - c.x, v.z - c.z).length() < r);
    let mut stats = Vec::new();
    let push_tri =|pos: &mut Vec<[f32; 3]>, cl: &mut Vec<[f32; 4]>, t: &[Vec3; 3], color: [f32; 4]| {
        for k in 0..3 {
            pos.push(lift(t[k]).to_array());
            pos.push(lift(t[(k + 1) % 3]).to_array());
            cl.push(color);
            cl.push(color);
        }
    };
    for (on, bit) in [(d.pawn, player::COL_PLAYER), (d.shots, player::COL_TRACE)] {
        if !on {
            continue;
        }
        col.tris_in(c - Vec3::splat(r), c + Vec3::splat(r), &mut buf);
        let (mut pos, mut cl) = (Vec::new(), Vec::new());
        let mut n = 0;
        for &i in buf.iter() {
            let (m, s, _) = col.tri_info(i);
            if m & bit == 0 || (s == player::SRC_TERRAIN && !d.terrain) {
                continue;
            }
            let t = &col.tris[i as usize];
            if !near(t) || pos.len() / 2 >= MAX_LINES {
                continue;
            }
            push_tri(&mut pos, &mut cl, t, if bit == player::COL_PLAYER { src_color(s) } else { SHOT_C });
            n += 1;
        }
        if bit == player::COL_PLAYER && d.veh_vols {
            for (t, s, _) in &col.extra {
                if near(t) {
                    push_tri(&mut pos, &mut cl, t, src_color(*s));
                }
            }
        }
        stats.push(format!("{} {n} Dreiecke", if bit == player::COL_PLAYER { "Spieler" } else { "Schüsse" }));
        if !pos.is_empty() {
            let e = commands.spawn((Mesh3d(meshes.add(line_mesh(pos, cl))), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NotShadowCaster, Name::new("coldebug"))).id();
            d.ents.push(e);
        }
    }
    if d.wire {
        let (mut pos, mut cl) = (Vec::new(), Vec::new());
        let mut n = 0;
        for (m3, gt, aabb, vis) in &objs {
            if !vis.get() || pos.len() / 2 >= MAX_LINES {
                continue;
            }
            // bounds test (world centre / radius of the Aabb when there is one)
            let (ctr, rad) = match aabb {
                Some(a) => (gt.transform_point(Vec3::from(a.center)), (gt.affine().matrix3 * Vec3::from(a.half_extents)).length()),
                None => (gt.translation(), 0.0),
            };
            if Vec2::new(ctr.x - c.x, ctr.z - c.z).length() > r + rad {
                continue;
            }
            let Some(mesh) = meshes.get(&m3.0) else { continue };
            if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
                continue;
            }
            let Some(VertexAttributeValues::Float32x3(vp)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
            let w: Vec<Vec3> = vp.iter().map(|v| gt.transform_point(Vec3::from(*v))).collect();
            let idx: Vec<usize> = match mesh.indices() {
                Some(i) => i.iter().collect(),
                None => (0..w.len()).collect(),
            };
            for tri in idx.chunks_exact(3) {
                let (Some(&a), Some(&b), Some(&cc)) = (w.get(tri[0]), w.get(tri[1]), w.get(tri[2])) else { continue };
                let t = [a, b, cc];
                if !near(&t) {
                    continue;
                }
                push_tri(&mut pos, &mut cl, &t, WIRE_C);
                n += 1;
                if pos.len() / 2 >= MAX_LINES {
                    break;
                }
            }
        }
        stats.push(format!("Wireframe {n} Dreiecke"));
        if !pos.is_empty() {
            let e = commands.spawn((Mesh3d(meshes.add(line_mesh(pos, cl))), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NotShadowCaster, Name::new("coldebug wire"))).id();
            d.ents.push(e);
        }
    }
    d.stats = format!("{} (Radius {:.0} m, neu bei 1 m Bewegung)", stats.join(", "), r);
}

/// Crosshair label (also with the F1 menu closed).
fn label_ui(mut contexts: EguiContexts, d: Res<ColDebug>, pawns: Query<(), With<Pawn>>) -> Result {
    if !d.label || d.hit.is_empty() || pawns.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("hb_coldebug_label")).fixed_pos(egui::pos2(screen.center().x + 14.0, screen.center().y + 14.0)).interactable(false).show(ctx, |ui| {
        egui::Frame::new().fill(egui::Color32::from_black_alpha(170)).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.label(egui::RichText::new(&d.hit).monospace().size(12.0).color(egui::Color32::from_rgb(235, 238, 245)));
        });
    });
    Ok(())
}
