// The game blends its UI in gamma space (non-sRGB back buffer). The scene post (hfpost uber)
// therefore writes gamma values, the UI draws on top of them, and this pass converts the
// result to linear for Bevy's sRGB swap chain.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var src: texture_2d<f32>;

fn lin_from_srgb(v: vec3<f32>) -> vec3<f32> {
    return select(pow((v + 0.055) / 1.055, vec3<f32>(2.4)), v / 12.92, v <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(floor(in.position.xy)), 0);
    return vec4<f32>(lin_from_srgb(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}
