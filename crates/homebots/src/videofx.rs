//! Remote video feed look of the drones (MAT_RD.postFX.M_VideoFX, see videofx.wgsl): grey,
//! chromatic offsets, scan bands, grid, noise, scratches and the occasional static burst.
//!
//! Data: the ground drone's cockpit package Interior_UCV_GAntiTank carries
//! postFX.M_VideoFX_INST (Desaturation 0.8, fx 1) on the translucent screen of its cockpit mesh;
//! the material's textures are FX_PostProcess.Textures.* and MAT_RD.SpecialCase.UI.lineGradient03.
//! Hellfire UAV: CFG_Airstrike_Hellfire bThermalVisionWhenZoomed -> thermal vision, whose post
//! effect is GCObjectPool_Content DefaultPPThermalMaterial = MAT_RD.postFX.M_Thermal01, an
//! instance of M_VideoFX with fx 1 and Desaturation 1.
use bevy::{
    core_pipeline::{core_3d::graph::{Core3d, Node3d}, fullscreen_vertex_shader::fullscreen_shader_vertex_state},
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_graph::{NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner},
        render_resource::{binding_types::{sampler, texture_2d, uniform_buffer_sized}, *},
        renderer::{RenderContext, RenderDevice},
        texture::GpuImage,
        view::ViewTarget,
        RenderApp,
    },
};

pub const SHADER: &str = "embedded://homebots/videofx.wgsl";

#[derive(Resource, Default, Clone, ExtractResource)]
pub struct VideoFx {
    pub on: bool,
    pub desat: f32,
    pub time: f32,
    /// thermal vision (Hellfire: CFG_Airstrike_Hellfire bThermalVisionWhenZoomed): characters
    /// drawn with CharacterThermalMaterial (bots.rs thermal_bodies)
    pub thermal: bool,
    /// thermal goggles (GCObjectPool_Content DefaultPPGogglesMaterial MAT_RD.postFX.M_Thermal_Goggles
    /// = M_VideoFX_Goggles: TintColor 0.75 1 0.75, the ThermalGoggles_corner01 frame)
    pub goggles: bool,
    /// VideoTexturesCombined, VideoNoise, interference, ScreenScratches, lineGradient03,
    /// UI_PlayerHUD.Textures.HUD.ThermalGoggles_corner01
    pub tex: Option<[Handle<Image>; 6]>,
}

pub struct VideoFxPlugin;

impl Plugin for VideoFxPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "videofx.wgsl");
        app.init_resource::<VideoFx>().add_plugins(ExtractResourcePlugin::<VideoFx>::default()).add_systems(Update, (load_textures, update));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_render_graph_node::<ViewNodeRunner<VideoFxNode>>(Core3d, VideoFxLabel)
            .add_render_graph_edges(Core3d, (Node3d::Tonemapping, VideoFxLabel, crate::hfpost::HfPostLabel));
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<VideoFxPipeline>();
    }
}

/// The textures of M_VideoFX, loaded once from the game.
fn load_textures(mut fx: ResMut<VideoFx>, root: Option<Res<crate::sfx::GameRoot>>, mut images: ResMut<Assets<Image>>, mut tried: Local<bool>) {
    if *tried {
        return;
    }
    let Some(root) = root else { return };
    *tried = true;
    let Ok(lib) = upk::library::Library::scan(&root.0) else { return };
    let load = |pkg: &str, path: &str, images: &mut Assets<Image>| -> Option<Handle<Image>> {
        let p = lib.open(pkg)?;
        let i = p.exports.iter().enumerate().position(|(i, _)| p.object_path(i as i32 + 1) == path)?;
        let img = crate::gamemat::load_texture(&p, &lib, i as i32 + 1, [upk::matcompile::Address::Wrap; 2], 1024)?;
        Some(images.add(img))
    };
    let list = [
        ("FX_PostProcess", "Textures.VideoTexturesCombined"),
        ("FX_PostProcess", "Textures.VideoNoise"),
        ("FX_PostProcess", "Textures.interference"),
        ("FX_PostProcess", "Textures.ScreenScratches"),
        ("MAT_RD", "SpecialCase.UI.lineGradient03"),
        ("UI_PlayerHUD", "Textures.HUD.ThermalGoggles_corner01"),
    ];
    let mut out = Vec::new();
    for (pkg, path) in list {
        match load(pkg, path, &mut images) {
            Some(h) => out.push(h),
            None => {
                warn!("videofx: {pkg}.{path} not found, no drone feed effect");
                return;
            }
        }
    }
    fx.tex = out.try_into().ok();
    info!("videofx: M_VideoFX textures loaded");
}

/// On for the Hellfire UAV and the thermal goggles. The ground and air drones show their feed in
/// colour (user's gameplay video: no grey feed; M_VideoFX_INST sits on the cockpit glass mesh,
/// which is not drawn).
fn update(mut fx: ResMut<VideoFx>, time: Res<Time>, drone: Res<crate::drone::Drone>, strike: Res<crate::airstrike::Airstrike>, data: Option<Res<crate::combat::CombatData>>) {
    fx.time = time.elapsed_secs_wrapped();
    let goggles = crate::combat::GOGGLES.load(std::sync::atomic::Ordering::Relaxed) && drone.phase != crate::drone::DronePhase::Active && !strike.active;
    // HB_THERMALTEST=1: thermal bodies always on (tests)
    let thermal = std::env::var_os("HB_THERMALTEST").is_some() || goggles || strike.active && strike.zoomed && data.as_ref().and_then(|d| d.strikes.get(strike.kind)).is_some_and(|s| s.cfg.thermal);
    fx.goggles = goggles;
    let (on, desat) = if goggles {
        // M_Thermal_Goggles Desaturation 1
        (true, 1.0)
    } else if strike.active {
        // M_Thermal01 (without thermal vision the parent's default, also 1)
        (true, 1.0)
    } else {
        (false, 0.0)
    };
    if fx.on != on || fx.desat != desat || fx.thermal != thermal {
        fx.on = on;
        fx.desat = desat;
        fx.thermal = thermal;
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct VideoFxLabel;

#[derive(Resource)]
struct VideoFxPipeline {
    layout: BindGroupLayout,
    clamp: Sampler,
    wrap: Sampler,
    /// ThermalGoggles_corner01 AddressX / AddressY TA_Mirror
    mirror: Sampler,
    pipeline: CachedRenderPipelineId,
}

impl FromWorld for VideoFxPipeline {
    fn from_world(world: &mut World) -> Self {
        let dev = world.resource::<RenderDevice>();
        let tex = || texture_2d(TextureSampleType::Float { filterable: true });
        let layout = dev.create_bind_group_layout(
            "videofx_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (tex(), sampler(SamplerBindingType::Filtering), sampler(SamplerBindingType::Filtering), tex(), tex(), tex(), tex(), tex(), uniform_buffer_sized(false, std::num::NonZeroU64::new(32)), tex(), sampler(SamplerBindingType::Filtering)),
            ),
        );
        let mk_sampler = |mode: AddressMode| {
            dev.create_sampler(&SamplerDescriptor {
                address_mode_u: mode,
                address_mode_v: mode,
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mipmap_filter: FilterMode::Linear,
                ..default()
            })
        };
        let clamp = mk_sampler(AddressMode::ClampToEdge);
        let wrap = mk_sampler(AddressMode::Repeat);
        let mirror = mk_sampler(AddressMode::MirrorRepeat);
        let shader = world.load_asset(SHADER);
        let pipeline = world.resource_mut::<PipelineCache>().queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("videofx".into()),
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
        VideoFxPipeline { layout, clamp, wrap, mirror, pipeline }
    }
}

#[derive(Default)]
struct VideoFxNode;

impl ViewNode for VideoFxNode {
    type ViewQuery = (&'static ViewTarget, Has<crate::minimap::MinimapCam>);

    fn run(&self, _graph: &mut RenderGraphContext, ctx: &mut RenderContext, (target, minimap): QueryItem<Self::ViewQuery>, world: &World) -> Result<(), NodeRunError> {
        if minimap {
            return Ok(());
        }
        let Some(fx) = world.get_resource::<VideoFx>() else { return Ok(()) };
        if !fx.on {
            return Ok(());
        }
        let Some(handles) = fx.tex.as_ref() else { return Ok(()) };
        let images = world.resource::<RenderAssets<GpuImage>>();
        let mut views = Vec::new();
        for h in handles {
            let Some(g) = images.get(h) else { return Ok(()) };
            views.push(g.texture_view.clone());
        }
        let pipe = world.resource::<VideoFxPipeline>();
        let Some(p) = world.resource::<PipelineCache>().get_render_pipeline(pipe.pipeline) else { return Ok(()) };
        let dev = ctx.render_device().clone();
        // M_Thermal_Goggles TintColor (0.75, 1, 0.75); M_VideoFX has none
        let tint = if fx.goggles { [0.75, 1.0, 0.75] } else { [1.0, 1.0, 1.0] };
        let params: [f32; 8] = [fx.time, fx.desat, 1.0, if fx.goggles { 1.0 } else { 0.0 }, tint[0], tint[1], tint[2], 0.0];
        let buf = dev.create_buffer_with_data(&BufferInitDescriptor { label: Some("videofx_params"), contents: bytemuck::cast_slice(&params), usage: BufferUsages::UNIFORM });
        let post = target.post_process_write();
        let bg = dev.create_bind_group(
            "videofx_bg",
            &pipe.layout,
            &BindGroupEntries::sequential((post.source, &pipe.clamp, &pipe.wrap, &views[0], &views[1], &views[2], &views[3], &views[4], buf.as_entire_binding(), &views[5], &pipe.mirror)),
        );
        let mut rp = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("videofx"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, resolve_target: None, ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_render_pipeline(p);
        rp.set_bind_group(0, &bg, &[]);
        rp.draw(0..3, 0..1);
        Ok(())
    }
}
