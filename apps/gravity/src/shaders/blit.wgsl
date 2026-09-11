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

fn to_pixels(world: vec2<f32>) -> vec2<f32> {
    let clip = (world - u.center) * u.scale;
    return vec2<f32>(clip.x + 1.0, 1.0 - clip.y) * 0.5 * u.viewport;
}

// The body being aimed, drawn over the frame: an outline the size it will be, and the
// line the drag has pulled out, which is its velocity. Drawn here as distance fields
// rather than as geometry because there is exactly one of it and this pass already
// covers every pixel.
fn launch_overlay(px: vec2<f32>) -> vec4<f32> {
    if (u.launch_on < 0.5) {
        return vec4<f32>(0.0);
    }
    let a = to_pixels(u.launch.xy);
    let b = to_pixels(u.launch.zw);
    let radius = u.point_size * radius_for(u.launch_mass);

    // Ring: a hairline at the body's radius.
    let ring = abs(length(px - a) - radius);
    // Segment a-b.
    let ab = b - a;
    let t = clamp(dot(px - a, ab) / max(dot(ab, ab), 1e-3), 0.0, 1.0);
    let seg = length(px - (a + ab * t));
    // Only the part of the line outside the ring, so the two do not overlap.
    let outside = step(radius, length(px - a));

    let line = (1.0 - smoothstep(0.6, 1.6, seg)) * outside;
    let edge = 1.0 - smoothstep(0.6, 1.6, ring);
    let strength = max(line * 0.7, edge);
    let warm = vec3<f32>(1.0, 0.79, 0.54);
    return vec4<f32>(warm * strength, strength);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let hdr = textureSample(accum, accum_sampler, in.uv).rgb * u.brightness;
    let mapped = vec3<f32>(1.0) - exp(-hdr);
    // A near-black ground rather than pure black: it keeps the sim area visibly
    // distinct from the surrounding interface on both targets.
    let background = vec3<f32>(0.024, 0.027, 0.043);
    let overlay = launch_overlay(in.uv * u.viewport);
    return vec4<f32>(mix(background + mapped, overlay.rgb, overlay.a), 1.0);
}
