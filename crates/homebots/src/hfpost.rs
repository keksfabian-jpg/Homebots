//! Homefront's post-process chain (luminance, eye adaptation, bloom, uber blend),
//! run on Bevy's HDR main texture after the main pass. The shaders in
//! `hfpost.wgsl` transcribe the game's own; this file reproduces what the game's
//! CPU side does, as measured with apitrace (tutorial level SP-Devil, D3D9, 1366x768):
//!
//! * passes per frame: luminance 16x16 <- scene, 4x4 <- 16x16, 1x1 <- 4x4,
//!   adaptation 1x1 (ping-pong), bloom downsample into a (W/4+2)x(H/4+2) buffer,
//!   horizontal + vertical blur, uber blend into the back buffer
//! * HDRLumParameters = (HDRLumLowClamp, HDRLumHighClamp, HDRLumCenter, HDRLumFactor),
//!   LumAdaptTime = HDRLumTime, BloomScale = Bloom_Scale
//! * SceneShadowsAndDesaturation.w = 1 - Scene_Desaturation,
//!   SceneScaledLuminanceWeights = Scene_Desaturation * (0.3, 0.59, 0.11)
//! * GammaColorScaleAndInverse = (1, 1, 1, 1 / DisplayGamma), GammaOverlayColor = 0
//! * VignetteScaleBias = (GradientScale, GradientOffset)
//! * blur: gaussian taps at positions k * Bloom_BlurStepSize (filter-buffer texels),
//!   merged pairwise into bilinear samples; see `blur_kernel` for the measured kernels.
use bevy::render::diagnostic::RecordDiagnostics;
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
            binding_types::{texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
        Render, RenderApp, RenderSet,
    },
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

pub const SHADER: &str = "embedded://homebots/hfpost.wgsl";

#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct HfPost {
    pub hdr_lum: Vec4,
    pub misc: Vec4,
    pub blur_off: [Vec4; 2],
    pub blur_w: [Vec4; 2],
    pub desat: Vec4,
    pub gamma: Vec4,
    pub overlay: Vec4,
    pub cut_grad: Vec4,
    pub tint_shadow: Vec4,
    pub tint_mid: Vec4,
    pub tint_high: Vec4,
}

/// Gaussian blur kernel for a Bloom_BlurKernelSize, as captured from the game:
/// (first tap index, number of taps, sigma in tap units).
///   KernelSize 16 (StepSize 0.7): taps -5..6,  sigma 2.0645825
///   KernelSize 20 (StepSize 1.0): taps -6..7,  sigma 2.3082733
/// Both fit sigma = 0.516146 * sqrt(KernelSize); only measured sizes are accepted.
fn blur_kernel(kernel: f32) -> Option<(i32, i32, f32)> {
    if (kernel - 16.0).abs() < 1e-3 {
        Some((-5, 12, 2.0645825))
    } else if (kernel - 20.0).abs() < 1e-3 {
        Some((-6, 14, 2.3082733))
    } else {
        None
    }
}

/// Paired-sample offsets (filter-buffer texels) and weights.
fn blur_samples(kernel: f32, step: f32) -> (Vec<f32>, Vec<f32>) {
    let (k0, n, sigma) = blur_kernel(kernel).unwrap_or_else(|| {
        warn!("Bloom_BlurKernelSize {kernel} was never measured; using 16");
        blur_kernel(16.0).unwrap()
    });
    let g: Vec<f64> = (0..n).map(|i| {
        let t = (k0 + i) as f64;
        (-t * t / (2.0 * (sigma as f64).powi(2))).exp()
    }).collect();
    let sum: f64 = g.iter().sum();
    let (mut off, mut w) = (vec![], vec![]);
    for j in 0..(n / 2) as usize {
        let (wa, wb) = (g[2 * j] / sum, g[2 * j + 1] / sum);
        let (ta, tb) = ((k0 + 2 * j as i32) as f64, (k0 + 2 * j as i32 + 1) as f64);
        off.push(((ta * wa + tb * wb) / (wa + wb)) as f32 * step);
        w.push((wa + wb) as f32);
    }
    (off, w)
}

impl HfPost {
    pub fn from_level(pp: &upk::level::PostProcess, display_gamma: f32) -> Self {
        // the game uses one kernel for both axes; X and Y are equal in every level seen
        let (off, w) = blur_samples(pp.bloom_kernel[0], pp.bloom_step[0]);
        let mut o = [Vec4::ZERO; 2];
        let mut ww = [Vec4::ZERO; 2];
        for i in 0..off.len().min(8) {
            o[i / 4][i % 4] = off[i];
            ww[i / 4][i % 4] = w[i];
        }
        let d = pp.desaturation;
        let (gs, go) = if pp.enable_gradient { (pp.gradient_scale, pp.gradient_offset) } else { (0.0, 1.0) };
        HfPost {
            hdr_lum: Vec4::new(pp.lum_low, pp.lum_high, pp.lum_center, pp.lum_factor),
            misc: Vec4::new(pp.lum_time, 0.0, if pp.enable_bloom { pp.bloom_scale } else { 0.0 }, off.len().min(8) as f32),
            blur_off: o,
            blur_w: ww,
            desat: Vec4::new(1.0 - d, 0.3 * d, 0.59 * d, 0.11 * d),
            gamma: Vec4::new(1.0, 1.0, 1.0, 1.0 / display_gamma),
            overlay: Vec4::ZERO,
            cut_grad: Vec4::new(pp.tint_shadow_cutoff, pp.tint_highlight_cutoff, gs, go),
            tint_shadow: Vec4::from_array(pp.tint_shadow),
            tint_mid: Vec4::from_array(pp.tint_mid),
            tint_high: Vec4::from_array(pp.tint_high),
        }
    }
}

pub struct HfPostPlugin;

impl Plugin for HfPostPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "hfpost.wgsl");
        app.add_plugins((ExtractComponentPlugin::<HfPost>::default(), UniformComponentPlugin::<HfPost>::default()));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(Render, prepare_textures.in_set(RenderSet::PrepareResources))
            .add_render_graph_node::<ViewNodeRunner<HfPostNode>>(Core3d, HfPostLabel)
            .add_render_graph_edges(Core3d, (Node3d::Tonemapping, HfPostLabel, Node3d::EndMainPassPostProcessing));
    }
    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<HfPostPipelines>().init_resource::<HfPostTextures>();
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct HfPostLabel;

struct Tex {
    _t: Texture,
    v: TextureView,
}

fn tex(dev: &RenderDevice, w: u32, h: u32, format: TextureFormat, label: &'static str) -> Tex {
    let t = dev.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let v = t.create_view(&TextureViewDescriptor::default());
    Tex { _t: t, v }
}

struct Set {
    size: (u32, u32),
    lum16: Tex,
    lum4: Tex,
    lum1: Tex,
    adapt: [Tex; 2],
    bloom: [Tex; 2],
}

#[derive(Resource, Default)]
struct HfPostTextures(Mutex<Option<Set>>);

fn prepare_textures(dev: Res<RenderDevice>, res: Res<HfPostTextures>, views: Query<&ViewTarget, With<HfPost>>) {
    let Some(vt) = views.iter().next() else { return };
    let s = vt.main_texture().size();
    let size = (s.width, s.height);
    let mut g = res.0.lock().unwrap();
    if g.as_ref().map(|x| x.size) == Some(size) {
        return;
    }
    // filter buffer: (W/4 + 2) x (H/4 + 2), e.g. 343x194 for 1366x768
    let (bw, bh) = (size.0 / 4 + 2, size.1 / 4 + 2);
    *g = Some(Set {
        size,
        lum16: tex(&dev, 16, 16, TextureFormat::R32Float, "hf_lum16"),
        lum4: tex(&dev, 4, 4, TextureFormat::R32Float, "hf_lum4"),
        lum1: tex(&dev, 1, 1, TextureFormat::R32Float, "hf_lum1"),
        adapt: [tex(&dev, 1, 1, TextureFormat::R32Float, "hf_adapt0"), tex(&dev, 1, 1, TextureFormat::R32Float, "hf_adapt1")],
        bloom: [tex(&dev, bw, bh, TextureFormat::Rgba16Float, "hf_bloom0"), tex(&dev, bw, bh, TextureFormat::Rgba16Float, "hf_bloom1")],
    });
}

#[derive(Resource)]
struct HfPostPipelines {
    layout: BindGroupLayout,
    lum1: CachedRenderPipelineId,
    lum2: CachedRenderPipelineId,
    lum3: CachedRenderPipelineId,
    adapt: CachedRenderPipelineId,
    bloom_down: CachedRenderPipelineId,
    blur_h: CachedRenderPipelineId,
    blur_v: CachedRenderPipelineId,
    uber: CachedRenderPipelineId,
}

impl FromWorld for HfPostPipelines {
    fn from_world(world: &mut World) -> Self {
        let dev = world.resource::<RenderDevice>();
        let unf = || texture_2d(TextureSampleType::Float { filterable: false });
        let layout = dev.create_bind_group_layout(
            "hf_post_layout",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (unf(), unf(), unf(), uniform_buffer::<HfPost>(true))),
        );
        let shader = world.load_asset(SHADER);
        let cache = world.resource_mut::<PipelineCache>();
        let mk = |entry: &'static str, format: TextureFormat| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(format!("hf_{entry}").into()),
                layout: vec![layout.clone()],
                vertex: fullscreen_shader_vertex_state(),
                fragment: Some(FragmentState {
                    shader: shader.clone(),
                    shader_defs: vec![],
                    entry_point: entry.into(),
                    targets: vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })],
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                push_constant_ranges: vec![],
                zero_initialize_workgroup_memory: false,
            })
        };
        let r32 = TextureFormat::R32Float;
        let f16 = TextureFormat::Rgba16Float;
        let me = HfPostPipelines {
            layout: layout.clone(),
            lum1: mk("lum1", r32),
            lum2: mk("lum2", r32),
            lum3: mk("lum3", r32),
            adapt: mk("adapt", r32),
            bloom_down: mk("bloom_down", f16),
            blur_h: mk("blur_h", f16),
            blur_v: mk("blur_v", f16),
            uber: mk("uber", ViewTarget::TEXTURE_FORMAT_HDR),
        };
        me
    }
}

#[derive(Default)]
struct HfPostNode {
    frame: AtomicUsize,
}

impl ViewNode for HfPostNode {
    type ViewQuery = (&'static ViewTarget, &'static HfPost, &'static DynamicUniformIndex<HfPost>);

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        ctx: &mut RenderContext,
        (target, _settings, index): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let pipes = world.resource::<HfPostPipelines>();
        let cache = world.resource::<PipelineCache>();
        let ids = [pipes.lum1, pipes.lum2, pipes.lum3, pipes.adapt, pipes.bloom_down, pipes.blur_h, pipes.blur_v, pipes.uber];
        let mut p = vec![];
        for id in ids {
            let Some(x) = cache.get_render_pipeline(id) else { return Ok(()) };
            p.push(x);
        }
        let uniforms = world.resource::<ComponentUniforms<HfPost>>();
        let Some(ubind) = uniforms.uniforms().binding() else { return Ok(()) };
        let texs = world.resource::<HfPostTextures>();
        let guard = texs.0.lock().unwrap();
        let Some(t) = guard.as_ref() else { return Ok(()) };

        let parity = self.frame.fetch_add(1, Ordering::Relaxed) & 1;
        let (prev, cur) = (&t.adapt[parity].v, &t.adapt[1 - parity].v);

        let post = target.post_process_write();
        let scene = post.source;
        let dev = ctx.render_device().clone();
        let off = index.index();
        let diag = ctx.diagnostic_recorder();

        let pass = |ctx: &mut RenderContext, pipe: &RenderPipeline, a: &TextureView, b: &TextureView, c: &TextureView, out: &TextureView, label: &str| {
            let bg = dev.create_bind_group(label, &pipes.layout, &BindGroupEntries::sequential((a, b, c, ubind.clone())));
            let mut rp = ctx.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: out,
                    resolve_target: None,
                    ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            // GPU time per pass in the profiler (perf.rs, RenderDiagnosticsPlugin)
            let span = diag.pass_span(&mut rp, label.to_string());
            rp.set_render_pipeline(pipe);
            rp.set_bind_group(0, &bg, &[off]);
            rp.draw(0..3, 0..1);
            span.end(&mut rp);
        };

        pass(ctx, p[0], scene, &t.lum1.v, &t.lum1.v, &t.lum16.v, "hf_lum1");
        pass(ctx, p[1], &t.lum16.v, &t.lum1.v, &t.lum1.v, &t.lum4.v, "hf_lum2");
        pass(ctx, p[2], &t.lum4.v, &t.lum16.v, &t.lum16.v, &t.lum1.v, "hf_lum3");
        pass(ctx, p[3], &t.lum1.v, prev, prev, cur, "hf_adapt");
        pass(ctx, p[4], scene, cur, &t.bloom[1].v, &t.bloom[0].v, "hf_bloom_down");
        pass(ctx, p[5], &t.bloom[0].v, cur, cur, &t.bloom[1].v, "hf_blur_h");
        pass(ctx, p[6], &t.bloom[1].v, cur, cur, &t.bloom[0].v, "hf_blur_v");
        pass(ctx, p[7], scene, cur, &t.bloom[0].v, post.destination, "hf_uber");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::blur_samples;
    /// Captured: SP-Devil PostProcessVolume_2 (kernel 16, step 0.7) and _3 (kernel 20, step 1.0),
    /// offsets converted from UV (x 343 filter-buffer texels).
    #[test]
    fn blur_matches_capture() {
        let cases: [(f32, f32, &[f32], &[f32]); 2] = [
            (16.0, 0.7, &[-2.98069, -1.65021, -0.32950, 0.98904, 2.31387, 3.65106],
             &[0.04004301, 0.1889133, 0.3666555, 0.2939765, 0.09723116, 0.01318059]),
            (20.0, 1.0, &[-5.26265, -3.34144, -1.43008, 0.47656, 2.38480, 4.30057, 6.22795],
             &[0.02250566, 0.1130965, 0.2768669, 0.3311085, 0.193556, 0.05521011, 0.007656251]),
        ];
        for (k, s, off, w) in cases {
            let (o, ww) = blur_samples(k, s);
            assert_eq!(o.len(), off.len());
            for i in 0..o.len() {
                assert!((o[i] - off[i]).abs() < 2e-4, "{k}: off {i} {} vs {}", o[i], off[i]);
                assert!((ww[i] - w[i]).abs() < 1e-6, "{k}: w {i} {} vs {}", ww[i], w[i]);
            }
        }
    }
}
