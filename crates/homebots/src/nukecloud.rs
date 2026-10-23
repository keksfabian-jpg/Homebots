//! Volumetric nuclear cloud for nuke.rs (NOT from data; the game has no nuke): a ray-marched
//! material on a box around ground zero (nukecloud.wgsl), clipped by the scene depth of the
//! scene copy (scenecopy.rs), with a tileable 64^3 Perlin-Worley noise texture built at start.
use bevy::{
    pbr::{MaterialPipeline, MaterialPipelineKey, NotShadowCaster, NotShadowReceiver},
    prelude::*,
    render::{
        mesh::MeshVertexBufferLayoutRef,
        render_asset::RenderAssetUsages,
        render_resource::{AsBindGroup, CompareFunction, Extent3d, Face, RenderPipelineDescriptor, ShaderRef, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat},
    },
};

pub const SHADER: &str = "embedded://homebots/nukecloud.wgsl";

#[derive(Clone, Copy, Default, ShaderType, Debug)]
pub struct CloudParams {
    pub a: Vec4,
    pub b: Vec4,
    pub c: Vec4,
    pub d: Vec4,
    pub e: Vec4,
    pub f: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct NukeCloudMat {
    #[uniform(0)]
    pub p: CloudParams,
    #[texture(1)]
    pub scene: Handle<Image>,
    #[texture(2, dimension = "3d")]
    #[sampler(3)]
    pub noise: Handle<Image>,
}

impl Material for NukeCloudMat {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
    /// Back faces without a depth test: the shader clips the ray with the scene depth itself,
    /// so the cloud also draws while the camera is inside the box.
    fn specialize(_p: &MaterialPipeline<Self>, d: &mut RenderPipelineDescriptor, _l: &MeshVertexBufferLayoutRef, _k: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        d.primitive.cull_mode = Some(Face::Front);
        if let Some(ds) = d.depth_stencil.as_mut() {
            ds.depth_compare = CompareFunction::Always;
            ds.depth_write_enabled = false;
        }
        Ok(())
    }
}

/// Set by nuke.rs every frame: shown, parameters (see nukecloud.wgsl CloudParams).
#[derive(Resource, Default)]
pub struct NukeCloud {
    pub on: bool,
    pub p: CloudParams,
    mat: Option<Handle<NukeCloudMat>>,
}

#[derive(Component)]
struct CloudBox;

pub struct NukeCloudPlugin;

impl Plugin for NukeCloudPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "nukecloud.wgsl");
        app.add_plugins(MaterialPlugin::<NukeCloudMat> { prepass_enabled: false, shadows_enabled: false, ..default() })
            .init_resource::<NukeCloud>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, apply.before(bevy::render::view::VisibilitySystems::CheckVisibility));
        if std::env::var_os("HB_CLOUDDBG").is_some() {
            if let Some(ra) = app.get_sub_app_mut(bevy::render::RenderApp) {
                ra.add_systems(bevy::render::Render, render_diag.in_set(bevy::render::RenderSet::Render));
            }
        }
    }
}

fn render_diag(mats: Res<bevy::render::render_asset::RenderAssets<bevy::pbr::PreparedMaterial<NukeCloudMat>>>, imgs: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>, cache: Res<bevy::render::render_resource::PipelineCache>, mut last: Local<Option<std::time::Instant>>) {
    use bevy::render::render_resource::CachedPipelineState as S;
    let mut n = [0usize; 4];
    for p in cache.pipelines() {
        match &p.state {
            S::Queued => n[0] += 1,
            S::Creating(_) => n[1] += 1,
            S::Ok(_) => n[2] += 1,
            S::Err(e) => {
                n[3] += 1;
                if last.is_none() {
                    warn!("cloud render dbg: pipeline error {e:?}");
                }
            }
        }
    }
    if last.is_some_and(|l| l.elapsed().as_secs_f32() < 2.0) {
        return;
    }
    *last = Some(std::time::Instant::now());
    info!("cloud render dbg: prepared materials {}, gpu images {}, pipelines queued/creating/ok/err {:?}", mats.iter().count(), imgs.iter().count(), n);
}

/// Tileable value noise (period `per` lattice cells over the texture).
fn value_noise(p: Vec3, per: i32, seed: u32) -> f32 {
    let h = |x: i32, y: i32, z: i32| -> f32 {
        let (x, y, z) = (x.rem_euclid(per) as u32, y.rem_euclid(per) as u32, z.rem_euclid(per) as u32);
        let mut v = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ z.wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x1656_67b1);
        v ^= v >> 15;
        v = v.wrapping_mul(0x2c1b_3c6d);
        v ^= v >> 12;
        v = v.wrapping_mul(0x297a_2d39);
        v ^= v >> 15;
        (v & 0xffff) as f32 / 65535.0
    };
    let i = p.floor();
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let (ix, iy, iz) = (i.x as i32, i.y as i32, i.z as i32);
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    l(
        l(l(h(ix, iy, iz), h(ix + 1, iy, iz), u.x), l(h(ix, iy + 1, iz), h(ix + 1, iy + 1, iz), u.x), u.y),
        l(l(h(ix, iy, iz + 1), h(ix + 1, iy, iz + 1), u.x), l(h(ix, iy + 1, iz + 1), h(ix + 1, iy + 1, iz + 1), u.x), u.y),
        u.z,
    )
}

/// Tileable Worley (F1) distance, `per` cells.
fn worley(p: Vec3, per: i32, seed: u32) -> f32 {
    let i = p.floor();
    let mut best = 9.0f32;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = i + Vec3::new(dx as f32, dy as f32, dz as f32);
                let (cx, cy, cz) = ((c.x as i32).rem_euclid(per), (c.y as i32).rem_euclid(per), (c.z as i32).rem_euclid(per));
                let mut v = (cx as u32).wrapping_mul(73_856_093) ^ (cy as u32).wrapping_mul(19_349_663) ^ (cz as u32).wrapping_mul(83_492_791) ^ seed;
                let mut r = || {
                    v ^= v << 13;
                    v ^= v >> 17;
                    v ^= v << 5;
                    (v & 0xffff) as f32 / 65535.0
                };
                let fp = c + Vec3::new(r(), r(), r());
                best = best.min(fp.distance(p));
            }
        }
    }
    best
}

/// 64^3 Perlin-Worley: fbm of value noise eroded by inverted Worley (billowy cauliflower).
fn noise_image() -> Image {
    const N: usize = 64;
    let mut data = vec![0u8; N * N * N];
    data.chunks_mut(N * N).enumerate().for_each(|(z, slice)| {
        for y in 0..N {
            for x in 0..N {
                let p = Vec3::new(x as f32, y as f32, z as f32) / N as f32;
                let mut f = 0.0;
                let mut a = 0.5;
                let mut per = 4;
                for o in 0..4 {
                    f += a * value_noise(p * per as f32, per, 7 + o);
                    a *= 0.5;
                    per *= 2;
                }
                let w = 1.0 - (worley(p * 6.0, 6, 11) / 0.9).min(1.0);
                let w2 = 1.0 - (worley(p * 12.0, 12, 13) / 0.9).min(1.0);
                let ww = w * 0.65 + w2 * 0.35;
                // remap the fbm by the worley billows
                let v = (f * 0.55 + ww * 0.6 - 0.12).clamp(0.0, 1.0);
                slice[y * N + x] = (v * 255.0) as u8;
            }
        }
    });
    let mut img = Image::new(Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: N as u32 }, TextureDimension::D3, data, TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        address_mode_w: bevy::image::ImageAddressMode::Repeat,
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        ..default()
    });
    img
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<NukeCloudMat>>, mut images: ResMut<Assets<Image>>, copy: Res<crate::scenecopy::SceneCopy>, mut nc: ResMut<NukeCloud>) {
    let t0 = std::time::Instant::now();
    let noise = images.add(noise_image());
    info!("nuke cloud: noise texture in {:.2} s", t0.elapsed().as_secs_f32());
    let mat = mats.add(NukeCloudMat { p: CloudParams::default(), scene: copy.image.clone(), noise });
    nc.mat = Some(mat.clone());
    commands.spawn((Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))), MeshMaterial3d(mat), Transform::default(), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, bevy::render::view::NoFrustumCulling, CloudBox));
}

fn apply(nc: Res<NukeCloud>, mut mats: ResMut<Assets<NukeCloudMat>>, mut q: Query<(&mut Transform, &mut Visibility, &ViewVisibility), With<CloudBox>>, time: Res<Time>) {
    if std::env::var_os("HB_CLOUDDBG").is_some() && (time.elapsed_secs() * 2.0).fract() < (time.delta_secs() * 2.0) {
        info!("cloud dbg: {} boxes, on {}, {:?}", q.iter().count(), nc.on, q.iter().next().map(|(t, v, vv)| (t.translation, t.scale, *v, vv.get())));
    }
    let Ok((mut tf, mut v, _)) = q.single_mut() else { return };
    if !nc.on {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
        return;
    }
    *v = Visibility::Visible;
    // box: ground zero, half size xz, from 10 m below the ground to the top
    let gz = nc.p.a.truncate();
    let (half, top) = (nc.p.f.x, nc.p.f.y);
    tf.translation = gz + Vec3::Y * (top - 10.0) * 0.5;
    tf.scale = Vec3::new(half, (top + 10.0) * 0.5, half);
    // HB_CLOUDDBG=2: the material written once only (test: re-specialisation every frame)
    static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if std::env::var("HB_CLOUDDBG").is_ok_and(|v| v == "2") && ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Some(m) = nc.mat.as_ref().and_then(|h| mats.get_mut(h)) {
        m.p = nc.p;
    }
}
