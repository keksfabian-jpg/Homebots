// Homefront's canvas tile pixel shader (FSimpleElementPixelShader, trace frame 400 PS 20658):
//   c = tex * vertexcolor; rgb = pow(max(saturate(c.rgb), 1e-6), Gamma) with Gamma = 1/2.2; a = c.a
// written to the (gamma-space) back buffer, alpha test GREATER 0, blend SRCALPHA/INVSRCALPHA.
// The UI camera draws into a gamma-space HDR target (see menu.rs, UiGammaNode).
#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var samp: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var c = textureSample(tex, samp, in.uv);
#ifdef VERTEX_COLORS
    c = c * in.color;
#endif
    if (c.a <= 0.0) {
        discard;
    }
    let g = exp2(log2(max(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1e-6))) * 0.4545454);
    return vec4<f32>(g, c.a);
}
