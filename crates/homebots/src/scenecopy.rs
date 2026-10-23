//! UE3's SceneColorTexture for translucent materials: after the opaque pass the scene colour
//! is copied with the scene depth in alpha (the game keeps depth in scene colour alpha and
//! decodes it with MinZ_MaxZRatio: z = x / (alpha - y)).
//!
//! Here alpha = reverse-Z depth * DEPTH_SCALE (Bevy: depth = near / view distance), so
//! MinZ_MaxZRatio = (100 * near * DEPTH_SCALE, 0) gives the view distance in UE units.
use bevy::render::diagnostic::RecordDiagnostics;

use bevy::{
    core_pipeline::{
        core_3d::graph::{Core3d, Node3d},
        fullscreen_vertex_shader::fullscreen_shader_vertex_state,
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_graph::{NodeRunError, RenderGraphApp, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner},
        render_resource::{binding_types::texture_2d, binding_types::texture_depth_2d, *},
        renderer::{RenderContext, RenderDevice},
        texture::GpuImage,
        view::{ViewDepthTexture, ViewTarget},
        RenderApp,
    },
};

pub const DEPTH_SCALE: f32 = 1000.0;
/// Fixed copy size (like UE3's scene colour buffer, which can be larger than the view; the
/// game's ScreenPositionScaleBias maps the view into it).
pub const SIZE: [u32; 2] = [3840, 2160];
/// Camera near plane in metres (PerspectiveProjection default, used by main.rs).
pub const NEAR: f32 = 0.1;

#[derive(Resource, Clone, ExtractResource)]
pub struct SceneCopy {
    pub image: Handle<Image>,
    /// Scene colour after translucency, before post-processing: what the game's canvas
    /// materials sample (trace frame 400: the UI blur tiles read texture 0x2fabe540, the
    /// same the post chain reads, and apply pow(1/2.2) themselves).
    pub final_image: Handle<Image>,
}

const SHADER: &str = r#"
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
@group(0) @binding(0) var color: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;
@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.position.xy);
    let c = textureLoad(color, p, 0);
    let d = textureLoad(depth, p, 0);
    return vec4<f32>(c.rgb, d * DEPTH_SCALE);
}
"#;

pub struct SceneCopyPlugin;

impl Plugin for SceneCopyPlugin {
    fn build(&self, app: &mut App) {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let image = images.add(make_image(UVec2::new(SIZE[0], SIZE[1])));
        let final_image = images.add(make_image(UVec2::new(SIZE[0], SIZE[1])));
        app.insert_resource(SceneCopy { image, final_image }).add_plugins(ExtractResourcePlugin::<SceneCopy>::default());
        let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
        let src = SHADER.replace("DEPTH_SCALE", &format!("{DEPTH_SCALE:?}"));
        let _ = shaders.insert(SHADER_HANDLE.id(), Shader::from_wgsl(src, "hf_scene_copy.wgsl"));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_render_graph_node::<ViewNodeRunner<SceneCopyNode>>(Core3d, SceneCopyLabel)
            .add_render_graph_edges(Core3d, (Node3d::MainOpaquePass, SceneCopyLabel, Node3d::MainTransmissivePass))
            .add_render_graph_node::<ViewNodeRunner<FinalCopyNode>>(Core3d, FinalCopyLabel)
            .add_render_graph_edges(Core3d, (Node3d::MainTransparentPass, FinalCopyLabel, Node3d::EndMainPass));
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<SceneCopyPipeline>();
    }
}

const SHADER_HANDLE: Handle<Shader> = bevy::asset::weak_handle!("6f0b3a52-6a1c-4a07-9a3e-5b1c2a8f0c11");

fn make_image(size: UVec2) -> Image {
    let mut img = Image::new_fill(
        Extent3d { width: size.x.max(1), height: size.y.max(1), depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0u8; 8],
        TextureFormat::Rgba16Float,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_DST;
    img.sampler = bevy::image::ImageSampler::nearest();
    img
}

#[derive(Resource)]
struct SceneCopyPipeline {
    layout: BindGroupLayout,
    pipeline: CachedRenderPipelineId,
}

impl FromWorld for SceneCopyPipeline {
    fn from_world(world: &mut World) -> Self {
        let dev = world.resource::<RenderDevice>();
        let layout = dev.create_bind_group_layout(
            "hf_scene_copy_layout",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_2d(TextureSampleType::Float { filterable: false }), texture_depth_2d())),
        );
        let cache = world.resource_mut::<PipelineCache>();
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("hf_scene_copy".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader_vertex_state(),
            fragment: Some(FragmentState {
                shader: SHADER_HANDLE,
                shader_defs: vec![],
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState { format: TextureFormat::Rgba16Float, blend: None, write_mask: ColorWrites::ALL })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        });
        SceneCopyPipeline { layout, pipeline }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct SceneCopyLabel;

#[derive(Default)]
struct SceneCopyNode;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct FinalCopyLabel;

#[derive(Default)]
struct FinalCopyNode;

impl ViewNode for FinalCopyNode {
    type ViewQuery = (&'static ViewTarget, &'static ViewDepthTexture, Has<crate::minimap::MinimapCam>);

    fn run(&self, _graph: &mut RenderGraphContext, ctx: &mut RenderContext, (target, depth, minimap): QueryItem<Self::ViewQuery>, world: &World) -> Result<(), NodeRunError> {
        let Some(copy) = world.get_resource::<SceneCopy>() else { return Ok(()) };
        if minimap {
            return Ok(());
        }
        copy_into(ctx, target, depth, world, &copy.final_image, "hf_final_copy");
        Ok(())
    }
}

fn copy_into(ctx: &mut RenderContext, target: &ViewTarget, depth: &ViewDepthTexture, world: &World, image: &Handle<Image>, label: &'static str) {
    let pipe = world.resource::<SceneCopyPipeline>();
    let Some(p) = world.resource::<PipelineCache>().get_render_pipeline(pipe.pipeline) else { return };
    let Some(out) = world.resource::<RenderAssets<GpuImage>>().get(image) else { return };
    let sz = target.main_texture().size();
    if sz.width > out.size.width || sz.height > out.size.height {
        return;
    }
    let bg = ctx.render_device().create_bind_group(label, &pipe.layout, &BindGroupEntries::sequential((target.main_texture_view(), depth.view())));
    let diag = ctx.diagnostic_recorder();
    let mut rp = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment { view: &out.texture_view, resolve_target: None, ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store } })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    let span = diag.pass_span(&mut rp, label);
    rp.set_viewport(0.0, 0.0, sz.width as f32, sz.height as f32, 0.0, 1.0);
    rp.set_render_pipeline(p);
    rp.set_bind_group(0, &bg, &[]);
    rp.draw(0..3, 0..1);
    span.end(&mut rp);
}

impl ViewNode for SceneCopyNode {
    type ViewQuery = (&'static ViewTarget, &'static ViewDepthTexture, Has<crate::minimap::MinimapCam>);

    fn run(&self, _graph: &mut RenderGraphContext, ctx: &mut RenderContext, (target, depth, minimap): QueryItem<Self::ViewQuery>, world: &World) -> Result<(), NodeRunError> {
        let Some(copy) = world.get_resource::<SceneCopy>() else { return Ok(()) };
        if minimap || crate::diag("HB_NOSCENECOPY") {
            return Ok(());
        }
        copy_into(ctx, target, depth, world, &copy.image, "hf_scene_copy");
        Ok(())
    }
}
