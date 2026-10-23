// Bink YUV -> RGB, the game's movie pixel shader (trace t3, CreatePixelShader call 20448,
// bound with tex0 = Y, tex1 = Cr, tex2 = Cb, consts c0 = (1,0,0,1)):
//   R = 1.16412354 Y + 1.59579468 Cr - 0.87065506
//   G = 1.16412354 Y - 0.813476563 Cr - 0.391448975 Cb + 0.529705048
//   B = 1.16412354 Y + 2.01782227 Cb - 1.08166885
// written as is (no blending) into the gamma-space target.
#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(2) @binding(0) var ty: texture_2d<f32>;
@group(2) @binding(1) var s: sampler;
@group(2) @binding(2) var tcr: texture_2d<f32>;
@group(2) @binding(3) var tcb: texture_2d<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let y = textureSample(ty, s, in.uv).x;
    let cr = textureSample(tcr, s, in.uv).x;
    let cb = textureSample(tcb, s, in.uv).x;
    let r = 1.16412354 * y + 1.59579468 * cr - 0.87065506;
    let g = 1.16412354 * y - 0.813476563 * cr - 0.391448975 * cb + 0.529705048;
    let b = 1.16412354 * y + 2.01782227 * cb - 1.08166885;
    return vec4<f32>(r, g, b, 1.0);
}
