// Shared declarations, textually included by every other shader in this app.
//
// WGSL has no `#include`, so `sim.rs` pastes this in front of each file. It holds the
// uniform block — bound at the same slot by all three bind group layouts — and the pair
// physics, which is the one piece of maths that absolutely must read the same to the
// force pass, to the energy check, and to the force-curve overlay. Three copies of it
// would be three chances to draw a curve the simulation is not running.

const TAU: f32 = 6.283185307179586;

struct Uniforms {
    // Camera: clip = (world - center) * scale
    center: vec2<f32>,
    scale: vec2<f32>,
    viewport: vec2<f32>,
    dt: f32,
    time: f32,

    // Physics. All lengths are absolute, already resolved from their fraction-of-cutoff
    // controls on the CPU, so the shader never has to know which knob a length came from.
    r_cut: f32,
    r_core: f32,
    k_rep: f32,
    lam: f32,
    chase_gain: f32,
    damping: f32,
    s0: f32,
    amp_sym: f32,
    amp_chase: f32,
    well_sigma: f32,
    rstar_min: f32,
    rstar_span: f32,

    point_size: f32,
    brightness: f32,
    probe_a: f32,
    probe_b: f32,
    cell_size: f32,

    n: u32,
    cells: u32,
    profile: u32,
    color_mode: u32,
    overlay: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

// What one neighbour contributes to one particle.
struct PairTerms {
    // Outward radial force along the separation, i.e. -dU/dr. Negative is attractive.
    // Equal and opposite on the two particles: this is the reciprocal half.
    f_out: f32,
    // d(pair energy)/d(c_i). The colour gradient the ledger spends against.
    w: f32,
    // Chase magnitude along the separation. Applied in the SAME direction to both
    // particles, which is what makes it the only term that does net work.
    chase: f32,
    // Pair energy, for the conservation check.
    energy: f32,
}

// Everything one pair of particles does to each other, given their separation and the
// four landscape values for their colours.
//
// `s`, `l` and their derivatives arrive already scaled by the strength controls, so the
// caller owns "how hard" and this function owns "in what shape".
fn pair_terms(r: f32, s: f32, l: f32, ds: f32, dl: f32, d01: f32, dd01: f32) -> PairTerms {
    var out: PairTerms;
    out.f_out = 0.0;
    out.w = 0.0;
    out.chase = 0.0;
    out.energy = 0.0;

    let big_r = u.r_cut;
    if (r >= big_r) {
        return out;
    }
    // Every envelope in the system is built from this one taper, so all of them reach
    // zero together at the cutoff. A neighbour crossing the radius must not change the
    // stored energy discontinuously, or the ledger breaks on a technicality no amount of
    // timestep reduction can fix.
    let q = 1.0 - r / big_r;

    // Soft repulsive core, not a collision. A hard velocity flip is a discontinuity in
    // an otherwise smooth system and makes the colour bookkeeping ill-defined at the
    // instant it happens. Dividing by r_core² keeps the contact force roughly fixed as
    // the core radius is moved, so the stiffness slider means one thing.
    if (r < u.r_core) {
        let over = u.r_core - r;
        let k = u.k_rep / max(u.r_core * u.r_core, 1e-8);
        out.energy = out.energy + k * over * over;
        out.f_out = out.f_out + 2.0 * k * over;
    }

    // The radial interaction profile: how the pair strength is spread over distance.
    var chase_env: f32;
    if (u.profile == 0u) {
        // Classic. U = S·V(r) with V monotonic, so the only place the force cancels is
        // inside the core: every pair wants the same separation, whatever its colours.
        let v = -q * q;
        let dv = 2.0 * q / big_r;
        out.energy = out.energy + s * v;
        out.f_out = out.f_out - s * dv;
        out.w = out.w + ds * v;
        chase_env = q;
    } else {
        // Well. A genuine minimum at a colour-dependent distance, so colour says how far
        // apart two particles want to be and the force changes sign across it. This is
        // the profile that gives the landscape somewhere to put repulsion.
        let rstar = u.rstar_min + u.rstar_span * d01;
        let sig = max(u.well_sigma, 1e-4);
        let z = (r - rstar) / sig;
        let bell = exp(-0.5 * z * z);
        // The taper holds flat over the inner half of the range and only then eases to
        // zero. A taper that sloped everywhere — the plain q² the other terms use — would
        // drag the minimum inward of the distance the pair actually asked for, so the
        // preferred separation would not be the separation the pair settles at, and the
        // control would be lying about what it does.
        let hold = 0.5 * big_r;
        let t = clamp((big_r - r) / (big_r - hold), 0.0, 1.0);
        let taper = t * t * (3.0 - 2.0 * t);
        let dtaper = -6.0 * t * (1.0 - t) / (big_r - hold);
        let g = bell * taper;
        let dg_dr = bell * (-(r - rstar) / (sig * sig) * taper + dtaper);
        let dg_drstar = bell * ((r - rstar) / (sig * sig)) * taper;
        out.energy = out.energy - s * g;
        out.f_out = out.f_out + s * dg_dr;
        // Two terms, because colour moves the well as well as deepening it. Dropping the
        // second is a silent conservation leak that grows with rstar_span.
        out.w = out.w - (ds * g + s * dg_drstar * u.rstar_span * dd01);
        // The chase must not outrange the bond. A Gaussian well dies within a few sigma
        // while the linear taper reaches the cutoff, and pairing them leaves an annulus
        // where the chase pushes with nothing holding on: unopposed thrust, paid out of
        // the colour store, driving the pair apart until the gradient vanishes.
        chase_env = g;
    }

    // The colour-mismatch store: the reservoir the chase burns. Note that it has a
    // spatial gradient as well as a colour one — this is the channel by which a particle
    // can relax by moving away from a neighbour it disagrees with rather than by
    // changing colour, and omitting it breaks the invariant.
    let w_env = q * q;
    let dw_env = -2.0 * q / big_r;
    out.energy = out.energy + u.lam * l * l * w_env;
    out.f_out = out.f_out - u.lam * l * l * dw_env;
    out.w = out.w + u.lam * 2.0 * l * dl * w_env;

    // Divided by the cutoff, like every other force here.
    //
    // The reciprocal terms are all gradients of an energy, so differentiating their
    // envelopes hands each of them a 1/R. The chase is declared directly as a force and
    // never picked one up, which left it a factor of R weaker than everything it is
    // supposed to compete with — five per cent of the total at a cutoff of 0.02, and a
    // different five per cent at every other cutoff, so the whole feel of the system
    // shifted whenever the range was moved. With the 1/R it scales in step, and the gain
    // means one thing at any cutoff.
    out.chase = u.chase_gain * l * chase_env / big_r;
    return out;
}

// Minimum-image separation on the unit torus.
//
// The world is exactly one unit across, which is why this is a single `round`: on any
// other world size it would need a division first.
fn separation(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let d = b - a;
    return d - round(d);
}

fn hue(h: f32) -> vec3<f32> {
    let x = fract(h / TAU) * 6.0;
    let r = clamp(abs(x - 3.0) - 1.0, 0.0, 1.0);
    let g = clamp(2.0 - abs(x - 2.0), 0.0, 1.0);
    let b = clamp(2.0 - abs(x - 4.0), 0.0, 1.0);
    return vec3<f32>(r, g, b);
}
