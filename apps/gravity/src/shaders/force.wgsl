// Pass 1: acceleration on every body from every other body.
//
// Brute force, O(n^2), but tiled: each workgroup pulls 256 bodies into workgroup
// storage and every thread in the group reads them from there, so the whole tile costs
// one global read per thread instead of 256. That is what makes an exact n-body
// simulation of tens of thousands of bodies run at frame rate.
//
// This pass is deliberately the only place that knows how forces are found. Swapping in
// a Barnes-Hut or grid approximation means replacing this file, and nothing else.

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read_write> bodies: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> vel: array<vec2<f32>>;
@group(0) @binding(3) var<storage, read_write> accel: array<vec2<f32>>;

const TILE: u32 = 256u;

var<workgroup> tile: array<vec4<f32>, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let count = max(u.n, 1u);
    let i = gid.x;
    // Threads past the end still run the loop so the barriers below stay uniform; only
    // the final store is guarded.
    let me = bodies[min(i, count - 1u)];

    var acc = vec2<f32>(0.0, 0.0);
    let eps2 = u.softening * u.softening;
    let tiles = (count + TILE - 1u) / TILE;

    for (var t: u32 = 0u; t < tiles; t = t + 1u) {
        let src = t * TILE + lid.x;
        let loaded = bodies[min(src, count - 1u)];
        // Out-of-range slots get zero mass, so they contribute nothing.
        tile[lid.x] = select(vec4<f32>(0.0, 0.0, 0.0, 0.0), loaded, src < count);
        workgroupBarrier();

        for (var k: u32 = 0u; k < TILE; k = k + 1u) {
            let other = tile[k];
            let d = other.xy - me.xy;
            // Plummer softening: the +eps2 keeps a close pass from producing an
            // infinite impulse, which would fling the pair off screen.
            let r2 = dot(d, d) + eps2;
            let inv_r = inverseSqrt(r2);
            acc = acc + d * (other.z * inv_r * inv_r * inv_r);
        }
        workgroupBarrier();
    }

    if (i < count) {
        accel[i] = acc * u.g;
    }
}
