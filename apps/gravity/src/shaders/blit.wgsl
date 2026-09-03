// Final pass: tone-map the HDR accumulation buffer onto the surface.
//
// Additive blending of tens of thousands of points produces values far above 1.0 in the
// dense cores. Clipping those to white loses all the structure, so the buffer is
// rgba16float and gets an exponential curve here — bright regions compress instead of
// flattening, which is what keeps a galaxy core from becoming a white blob.

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var accum: texture_2d<f32>;
@group(1) @binding(1) var accum_sampler: sampler;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    var out: VsOut;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let hdr = textureSample(accum, accum_sampler, in.uv).rgb * u.brightness;
    let mapped = vec3<f32>(1.0) - exp(-hdr);
    // A near-black ground rather than pure black: it keeps the sim area visibly
    // distinct from the surrounding interface on both targets.
    let background = vec3<f32>(0.024, 0.027, 0.043);
    return vec4<f32>(background + mapped, 1.0);
}
