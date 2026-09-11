// The integrator, and with it the energy ledger.
//
// The chase does net work. Every joule of it is paid for, locally and in the same step,
// out of the energy stored in this particle's colour disagreement with its neighbours:
//
//   P = F_chase · v                 power the chase delivered to this particle
//   ċ = −P / W                      so that W·ċ = −P, exactly
//
// The rate is solved, never chosen. Offering it as a slider would be offering a knob
// that breaks conservation whenever it is touched. What *is* free is how hard the chase
// pushes in the first place: scale that and P scales, the rate adapts, and the balance
// still closes because P is measured from whatever force was actually applied.
//
// Note there is no epsilon in that denominator. The obvious way to write this is
// α = P/(W² + ε) with ċ = −α·W, which is well behaved as W → 0 and looks harmless. It is
// not: it pays back W²/(W² + ε) of the work and quietly keeps the rest, and that shortfall
// is per unit *time* rather than per step, so it does not shrink as the timestep does. It
// is invisible while the chase is weak and becomes the dominant error as soon as the chase
// matters — measured at six per cent of the total energy once the chase was scaled
// correctly, on a system whose integrator error was otherwise converging cleanly.
//
// Dividing exactly, and handling the degenerate case as a case, pays the whole bill
// whenever there is a gradient to pay it with.

@group(0) @binding(1) var<storage, read_write> pos: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> vel: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> acc: array<vec4<f32>>;

/// Below this gradient there is nothing to push colour along, and the work goes unpaid.
///
/// A particle with no colour disagreement anywhere near it has no reservoir to draw on.
/// That is a real state of the system rather than a numerical inconvenience, and the
/// honest response is to let the debt stand, not to invent a gradient to settle it with.
const W_FLOOR: f32 = 1e-6;

/// The most colour one step may move, as a fraction of the circle.
///
/// The exact rate P/W is unbounded as W approaches the floor. This bounds the step
/// without touching the arithmetic in the ordinary case: at any sane setting it never
/// binds, and when it does it is a cap on a single step rather than a tax on every one.
const MAX_TURN: f32 = 0.05;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u.n) {
        return;
    }

    var p = pos[i];
    let state = vel[i];
    var v = state.xy;
    let w = state.z;
    let total = acc[i].xy;
    let chase = acc[i].zw;

    // Velocity damping: the cold sink, and the first thing here that removes energy
    // rather than moving it. Velocity only — a sink that drained the colour store would
    // erase the mismatch the whole system runs on.
    //
    // Integrated exactly rather than applied as a force. This system wants to be strongly
    // overdamped, which is where the explicit form `v -= γ·v·dt` blows up: it needs
    // γ·dt < 1, and the useful settings here are the ones where it is not. The decay
    // factor has no such limit, and reduces to the plain kick when damping is off.
    let decayed = exp(-u.damping * u.dt);
    let v_next = (v + total * u.dt) * decayed;

    // The work the chase does over this step is F·(v_before + v_after)/2·dt, and that
    // average is the midpoint velocity. Using v_before alone leaves an O(dt) residue in
    // the ledger that reads as a slow leak.
    let v_mid = 0.5 * (v + v_next);
    let power = dot(chase, v_mid);

    // W·ċ·dt = −P·dt exactly, whenever there is a W to divide by.
    var turn = 0.0;
    if (abs(w) > W_FLOOR) {
        turn = -(power / w) * u.dt;
    }
    let cap = MAX_TURN * TAU;
    let c = p.z + clamp(turn, -cap, cap);

    v = v_next;
    let moved = p.xy + v * u.dt;

    // Fold back onto the torus. Colour is wrapped for the same reason: every use of it
    // goes through a periodic function, so this is storage hygiene and never changes the
    // dynamics — but a colour left to wander accumulates float error until it does.
    pos[i] = vec4<f32>(moved - round(moved), fract(c / TAU) * TAU, p.w);
    vel[i] = vec4<f32>(v, w, turn);
}
