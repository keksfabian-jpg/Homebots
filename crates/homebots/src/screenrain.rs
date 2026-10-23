//! Rain drops on the screen while it rains (user request 2026-10-21: "subtle so it doesn't
//! annoy me in gameplay"). NOT in the original game: Homefront has no rain, and its only
//! water-on-lens effect is the bullet-in-water splash (KFX_Impacts FX_Aftermath.AFM_Bullet_Water,
//! material Shaders_Aftermath.LensFX_Waterdroplets). From that material comes the method (the
//! scene re-sampled at a shifted screen position, no tint) and the bound for the shift; the
//! drops, their behaviour and every number below are NOT from data (screenrain.wgsl).
//!
//! One full-screen pass after Homefront's post chain (hfpost.rs, i.e. on the final image, under
//! the UI camera), skipped entirely while no drop is visible. Amount = weather rain strength
//! (weather.rs: rain 0.6, storm 1.0), reduced under a roof (rays up from the camera against
//! the collision world) and off in a closed vehicle cockpit (first-person seat with a cockpit
//! mesh), eased in / out over a few seconds.
//!
//! Switches: HB_SCREENRAIN=0 off, =1 forced on at full strength (testing, ignores weather /
//! roof / cockpit), =<0..1> forced on at that amount; unset = automatic. Options in the
//! `ScreenRainSettings` resource (`enabled`, `strength`).
use bevy::{
    core_pipeline::{
        core_3d::graph::{Core3d, Node3d},
        fullscreen_vertex_shader::fullscreen_shader_vertex_state,
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin},
        render_graph::{NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner},
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
        RenderApp,
    },
};

pub const SHADER: &str = "embedded://homebots/screenrain.wgsl";

/// Ease in / out times (s) of the amount (NOT from data)
const RISE_S: f32 = 2.5;
const FALL_S: f32 = 4.0;
/// roof check: rays straight up from the camera this long (m), at these x/z offsets (m);
/// covered fraction = rays that hit (NOT from data)
const ROOF_UP: f32 = 30.0;
const ROOF_RAYS: [(f32, f32); 3] = [(0.0, 0.0), (0.8, 0.5), (-0.8, -0.5)];
/// default drop opacity (NOT from data)
const STRENGTH: f32 = 0.85;

/// Per-view uniform: x amount, y time (s, wrapped), z strength, w unused.
#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct ScreenRain {
    pub p: Vec4,
}

#[derive(Resource)]
pub struct ScreenRainSettings {
    /// automatic drops while it rains
    pub enabled: bool,
    /// drop opacity 0..1
    pub strength: f32,
    /// HB_SCREENRAIN=1 / <amount>: forced amount, ignoring the weather
    pub force: Option<f32>,
    amount: f32,
    time: f32,
}

impl Default for ScreenRainSettings {
    fn default() -> Self {
        let mut s = ScreenRainSettings { enabled: true, strength: STRENGTH, force: None, amount: 0.0, time: 0.0 };
        if let Ok(v) = std::env::var("HB_SCREENRAIN") {
            match v.trim().to_ascii_lowercase().as_str() {
                "" => {}
                "0" | "off" => s.enabled = false,
                "1" | "on" => s.force = Some(1.0),
                x => match x.parse::<f32>() {
                    Ok(a) => s.force = Some(a.clamp(0.0, 1.0)),
                    Err(_) => warn!("HB_SCREENRAIN={v}: expected 0, 1 or an amount 0..1"),
                },
            }
            info!("screen rain: enabled {}, forced {:?}", s.enabled, s.force);
        }
        s
    }
}

pub struct ScreenRainPlugin;

impl Plugin for ScreenRainPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "screenrain.wgsl");
        app.init_resource::<ScreenRainSettings>()
            .add_plugins((ExtractComponentPlugin::<ScreenRain>::default(), UniformComponentPlugin::<ScreenRain>::default()))
            .add_systems(Update, update);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_render_graph_node::<ViewNodeRunner<ScreenRainNode>>(Core3d, ScreenRainLabel)
            .add_render_graph_edges(Core3d, (crate::hfpost::HfPostLabel, ScreenRainLabel, Node3d::EndMainPassPostProcessing));
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<ScreenRainPipeline>();
    }
}

/// Closed cockpit: the player's seat is in first person and shows a cockpit mesh.
fn in_closed_cockpit(assets: Option<&crate::vehicles::VehicleAssets>, vehicles: &Query<&crate::vehicles::Vehicle>) -> bool {
    let Some(assets) = assets else { return false };
    vehicles.iter().any(|v| {
        let Some(seat) = v.player_seat else { return false };
        let Some(k) = assets.kinds.get(v.kind) else { return false };
        let Some(sc) = k.seats.get(seat) else { return false };
        let cp = sc.cockpit1p as usize;
        crate::vehicles::seat_first_person(sc, v, seat) && cp > 0 && k.slots.get(v.slot).is_some_and(|s| s.cockpit.get(cp - 1).is_some_and(|c| c.is_some()))
    })
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    time: Res<Time>,
    mut s: ResMut<ScreenRainSettings>,
    weather: Option<Res<crate::weather::Weather>>,
    col: Option<Res<crate::player::CollisionWorld>>,
    assets: Option<Res<crate::vehicles::VehicleAssets>>,
    vehicles: Query<&crate::vehicles::Vehicle>,
    mut cams: Query<(Entity, &GlobalTransform, Option<&mut ScreenRain>), (With<Camera3d>, With<crate::hfpost::HfPost>, Without<crate::minimap::MinimapCam>)>,
    mut buf: Local<Vec<u32>>,
) {
    let dt = time.delta_secs().min(0.25);
    let Some((e, gt, cur)) = cams.iter_mut().next() else { return };
    let target = if !s.enabled {
        0.0
    } else if let Some(f) = s.force {
        f
    } else {
        let rain = weather.as_ref().map(|w| w.rain_amount()).unwrap_or(0.0);
        if rain <= 0.01 || in_closed_cockpit(assets.as_deref(), &vehicles) {
            0.0
        } else {
            let p = gt.translation();
            let covered = col.as_ref().map(|c| ROOF_RAYS.iter().filter(|(x, z)| c.raycast(p + Vec3::new(*x, 0.0, *z), Vec3::Y, ROOF_UP, &mut buf).is_some()).count() as f32 / ROOF_RAYS.len() as f32).unwrap_or(0.0);
            rain * (1.0 - covered)
        }
    };
    let tau = if target > s.amount { RISE_S } else { FALL_S };
    s.amount += (target - s.amount) * (1.0 - (-dt / tau).exp());
    if target == 0.0 && s.amount < 0.002 {
        s.amount = 0.0;
    }
    s.time = (s.time + dt) % 3600.0;
    let u = ScreenRain { p: Vec4::new(s.amount, s.time, s.strength.clamp(0.0, 1.0), 0.0) };
    match cur {
        Some(mut c) => {
            if c.p != u.p {
                *c = u;
            }
        }
        None => {
            commands.entity(e).insert(u);
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct ScreenRainLabel;

#[derive(Resource)]
struct ScreenRainPipeline {
    layout: BindGroupLayout,
    sampler: Sampler,
    id: CachedRenderPipelineId,
}

impl FromWorld for ScreenRainPipeline {
    fn from_world(world: &mut World) -> Self {
        let dev = world.resource::<RenderDevice>();
        let layout = dev.create_bind_group_layout(
            "screenrain_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering), uniform_buffer::<ScreenRain>(true)),
            ),
        );
        let sampler = dev.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..default()
        });
        let shader = world.load_asset(SHADER);
        let id = world.resource_mut::<PipelineCache>().queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("screenrain".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader_vertex_state(),
            fragment: Some(FragmentState {
                shader,
                shader_defs: vec![],
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState { format: ViewTarget::TEXTURE_FORMAT_HDR, blend: None, write_mask: ColorWrites::ALL })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });
        ScreenRainPipeline { layout, sampler, id }
    }
}

#[derive(Default)]
struct ScreenRainNode;

impl ViewNode for ScreenRainNode {
    type ViewQuery = (&'static ViewTarget, &'static ScreenRain, &'static DynamicUniformIndex<ScreenRain>);

    fn run(&self, _graph: &mut RenderGraphContext, ctx: &mut RenderContext, (target, rain, index): QueryItem<Self::ViewQuery>, world: &World) -> Result<(), NodeRunError> {
        // no drops: no pass at all
        if rain.p.x <= 0.0 || rain.p.z <= 0.0 || crate::diag("HB_NOSCREENRAIN") {
            return Ok(());
        }
        let pipe = world.resource::<ScreenRainPipeline>();
        let Some(rp) = world.resource::<PipelineCache>().get_render_pipeline(pipe.id) else { return Ok(()) };
        let Some(ubind) = world.resource::<ComponentUniforms<ScreenRain>>().uniforms().binding() else { return Ok(()) };
        let post = target.post_process_write();
        let bg = ctx.render_device().create_bind_group("screenrain", &pipe.layout, &BindGroupEntries::sequential((post.source, &pipe.sampler, ubind)));
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("screenrain"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, resolve_target: None, ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_render_pipeline(rp);
        pass.set_bind_group(0, &bg, &[index.index()]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}
