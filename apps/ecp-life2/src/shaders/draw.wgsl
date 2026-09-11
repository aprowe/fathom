// Drawing the particles.
//
// One instance per particle, six vertices each, sized in device pixels so a particle
// stays legible however far the camera is zoomed out. Colour is the particle's actual
// colour by default: it is a continuous angle, so it maps onto a hue wheel without any
// choice having to be made, and a structure that has sorted itself by colour then reads
// as a structure that has sorted itself by hue.

@group(0) @binding(1) var<storage, read> pos: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> vel: array<vec4<f32>>;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: vec2<f32>,
    @location(1) tint: vec3<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) inst: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );

    let p = pos[inst];
    let state = vel[inst];
    let clip = (p.xy - u.center) * u.scale;
    let corner = corners[vi];
    let offset = corner * u.point_size * 2.0 / u.viewport;

    var out: VsOut;
    out.position = vec4<f32>(clip + offset, 0.0, 1.0);
    out.offset = corner;

    if (u.color_mode == 0u) {
        out.tint = hue(p.z);
    } else if (u.color_mode == 1u) {
        // Speed, scaled so an ordinary bound particle sits mid-ramp.
        out.tint = ramp(length(state.xy) * 12.0);
    } else if (u.color_mode == 2u) {
        // The colour gradient: where the system is storing tension. Bright means this
        // particle disagrees loudly with its neighbours and has fuel to burn.
        out.tint = ramp(abs(state.z) * 0.4);
    } else {
        out.tint = vec3<f32>(0.62, 0.78, 1.0);
    }
    return out;
}

fn ramp(t: f32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    let cold = vec3<f32>(0.16, 0.35, 0.95);
    let mid = vec3<f32>(0.45, 0.85, 1.0);
    let hot = vec3<f32>(1.0, 0.86, 0.62);
    return select(mix(mid, hot, (x - 0.5) * 2.0), mix(cold, mid, x * 2.0), x < 0.5);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let d2 = dot(in.offset, in.offset);
    if (d2 > 1.0) {
        discard;
    }
    // A soft core rather than a hard disc, so overlapping particles sum into a glow
    // instead of a field of visible circles.
    let falloff = exp(-d2 * 3.5) * 0.30;
    return vec4<f32>(in.tint * falloff, falloff);
}
