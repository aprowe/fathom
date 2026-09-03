// Pass 2: move the bodies.
//
// Kick-drift leapfrog. It is symplectic, which for an orbit sim means energy wobbles
// instead of steadily climbing — orbits stay orbits over long runs, where plain Euler
// would spiral everything outward.
//
// The interactive gravity well is applied here rather than in the force pass: it is a
// single external body, so it costs one extra term per body instead of an extra tile.

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read_write> bodies: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> vel: array<vec2<f32>>;
@group(0) @binding(3) var<storage, read_write> accel: array<vec2<f32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u.n) {
        return;
    }

    var body = bodies[i];
    var v = vel[i];
    var a = accel[i];

    if (u.well.w > 0.5) {
        let d = u.well.xy - body.xy;
        let r2 = dot(d, d) + 0.02;
        let inv_r = inverseSqrt(r2);
        a = a + d * (u.well.z * inv_r * inv_r * inv_r);
    }

    v = v + a * u.dt;
    body = vec4<f32>(body.xy + v * u.dt, body.z, body.w);

    vel[i] = v;
    bodies[i] = body;
}
