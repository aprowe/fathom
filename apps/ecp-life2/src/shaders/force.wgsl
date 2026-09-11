// The force pass: everything one particle's neighbours do to it.
//
// Three accumulators come out of here, and they have to stay apart:
//
//   force  — everything, what the integrator moves the particle with;
//   chase  — the non-reciprocal part alone, because the power it delivers is what the
//            colour update has to pay for, and it cannot be recovered from the total;
//   w      — dE/dc for this particle, the colour gradient it relaxes along.
//
// The chase comes out same-direction on both members of a pair automatically: evaluating
// the same expression from the neighbour's side flips both the sign of Λ and the sign of
// the separation unit vector. That is asserted in the tests rather than assumed here.

@group(0) @binding(1) var<storage, read_write> pos: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> vel: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> acc: array<vec4<f32>>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u.n) {
        return;
    }

    let me = pos[i];
    let a = me.z;
    let cells = max(u.cells, 1u);
    let home = cell_of(me.xy);
    let cx = home % cells;
    let cy = home / cells;

    var force = vec2<f32>(0.0, 0.0);
    var chase = vec2<f32>(0.0, 0.0);
    var w = 0.0;

    // The cell side is at least the cutoff, so nothing outside the 3×3 block can be in
    // range and this loop is exact, not an approximation.
    for (var dy: i32 = -1; dy <= 1; dy = dy + 1) {
        for (var dx: i32 = -1; dx <= 1; dx = dx + 1) {
            // Signed arithmetic on the way round: `cx - 1` in u32 underflows to four
            // billion, and `% cells` only undoes that when cells divides 2^32.
            let nx = u32((i32(cx) + dx + i32(cells)) % i32(cells));
            let ny = u32((i32(cy) + dy + i32(cells)) % i32(cells));
            let cell = ny * cells + nx;
            let start = atomicLoad(&grid[offsets_at(cell)]);
            let n_here = atomicLoad(&grid[counts_at(cell)]);

            for (var k: u32 = 0u; k < n_here; k = k + 1u) {
                let j = sorted[start + k];
                if (j == i) {
                    continue;
                }
                let other = pos[j];
                // Points from j to i, so a positive f_out pushes them apart.
                let delta = separation(other.xy, me.xy);
                let r2 = dot(delta, delta);
                if (r2 >= u.r_cut * u.r_cut || r2 < 1e-12) {
                    continue;
                }
                let r = sqrt(r2);
                let dir = delta / r;

                let land = landscape_at(a, other.z);
                let t = pair_terms(r, land.s, land.l, land.ds, land.dl, land.d01, land.dd01);

                force = force + dir * t.f_out;
                chase = chase + dir * t.chase;
                w = w + t.w;
            }
        }
    }

    force = force + chase;
    acc[i] = vec4<f32>(force, chase);
    // The colour gradient rides in the spare lanes of the velocity, which the force pass
    // never reads from any other particle — so writing it here races with nothing.
    vel[i] = vec4<f32>(vel[i].xy, w, 0.0);
}
