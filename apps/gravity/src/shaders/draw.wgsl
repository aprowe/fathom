// Pass 3: draw the bodies as soft additive points.
//
// One instance per body, six vertices each, sized in device pixels so a point stays the
// same size on screen no matter how far the camera is zoomed out. Additive blending is
// what makes density read as brightness: where a thousand bodies overlap, the core of a
// galaxy goes white on its own, without anyone computing a density field.

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> bodies: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> vel: array<vec2<f32>>;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: vec2<f32>,
    @location(1) tint: vec3<f32>,
}

// Cool where slow, hot where fast — the usual astro-image reading, and it makes the
// difference between a settled disc and an infalling stream obvious at a glance.
fn ramp(t: f32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    let cold = vec3<f32>(0.16, 0.35, 0.95);
    let mid = vec3<f32>(0.45, 0.85, 1.0);
    let hot = vec3<f32>(1.0, 0.86, 0.62);
    return select(
        mix(mid, hot, (x - 0.5) * 2.0),
        mix(cold, mid, x * 2.0),
        x < 0.5,
    );
}

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @builtin(instance_index) inst: u32,
) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );

    let body = bodies[inst];
    let clip = (body.xy - u.center) * u.scale;
    let corner = corners[vi];
    // point_size is a radius in device pixels; clip space spans the viewport in 2 units.
    let offset = corner * u.point_size * 2.0 / u.viewport;

    var out: VsOut;
    out.position = vec4<f32>(clip + offset, 0.0, 1.0);
    out.offset = corner;

    let speed = length(vel[inst]);
    if (u.color_mode == 0u) {
        // Scaled so a circular orbit at the scene radius lands mid-ramp.
        out.tint = ramp(speed * 0.55);
    } else if (u.color_mode == 1u) {
        // Mass is tiny per body, so scale it into a visible range.
        out.tint = ramp(clamp(body.z * f32(u.n) * 1.4, 0.0, 1.0));
    } else {
        out.tint = vec3<f32>(0.62, 0.78, 1.0);
    }
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let d2 = dot(in.offset, in.offset);
    if (d2 > 1.0) {
        discard;
    }
    // A gaussian-ish core rather than a hard disc: overlapping points then sum into a
    // smooth glow instead of a field of visible circles.
    let falloff = exp(-d2 * 3.5) * 0.22;
    return vec4<f32>(in.tint * falloff, falloff);
}
