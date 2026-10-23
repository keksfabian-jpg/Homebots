//! Per-frame SpeedTree wind: one CWind per USpeedTree (as in the game), its shader table
//! written into the wind slots of every placed tree's object block (gameshader OBJ_WIND).
//! The objects buffer is patched in place on the GPU (queue.write_buffer), so the material
//! bind groups stay valid.

use crate::gameshader::{OBJ_STRIDE, OBJ_WIND};
use crate::speedwind::SpeedWind;
use bevy::{
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        renderer::RenderQueue,
        storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
        Render, RenderApp, RenderSet,
    },
};

#[derive(Resource, Default)]
pub struct TreeWind {
    pub winds: Vec<SpeedWind>,
    /// (object tag, wind index)
    pub objects: Vec<(u32, usize)>,
    pub buffer: Option<Handle<ShaderStorageBuffer>>,
}

#[derive(Resource, Clone, Default, ExtractResource)]
struct WindUpload {
    buffer: Option<Handle<ShaderStorageBuffer>>,
    writes: Vec<(u64, [[f32; 4]; 7])>,
}

pub struct TreeWindPlugin;

impl Plugin for TreeWindPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TreeWind>()
            .init_resource::<WindUpload>()
            .add_plugins(ExtractResourcePlugin::<WindUpload>::default())
            .add_systems(Update, advance);
        if let Some(r) = app.get_sub_app_mut(RenderApp) {
            r.add_systems(Render, upload.in_set(RenderSet::PrepareResources));
        }
    }
}

fn advance(time: Res<Time>, mut tw: ResMut<TreeWind>, mut up: ResMut<WindUpload>) {
    let t = time.elapsed_secs();
    for w in tw.winds.iter_mut() {
        w.frame(t);
    }
    let consts: Vec<[[f32; 4]; 7]> = tw.winds.iter().map(|w| w.constants()).collect();
    up.buffer = tw.buffer.clone();
    up.writes = tw.objects.iter().map(|&(tag, wi)| (((tag as usize * OBJ_STRIDE + OBJ_WIND) * 16) as u64, consts[wi])).collect();
}

fn upload(up: Res<WindUpload>, buffers: Res<RenderAssets<GpuShaderStorageBuffer>>, queue: Res<RenderQueue>) {
    let Some(h) = &up.buffer else { return };
    let Some(b) = buffers.get(h) else { return };
    for (off, c) in &up.writes {
        if off + 112 <= b.buffer.size() {
            queue.write_buffer(&b.buffer, *off, bytemuck::cast_slice(c));
        }
    }
}
