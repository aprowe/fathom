// The two instruments, drawn into the corners of the view.
//
// The panel is egui in the same window and has no way to hand a chart back to it, so
// these are drawn where the data already lives: on the GPU, from the same landscape
// buffer and the same `pair_terms` the force pass runs. That is not a workaround. Any
// chart drawn from a second copy of the maths is a chart that can be right while the
// simulation is wrong, which is the one failure mode an instrument must not have.
//
//   bottom left   the interaction landscape A(a,b) over the colour torus
//   bottom right  the radial force profile for the probe pair of colours
//
// The probe colours are two sliders rather than a click target: sweeping a slider walks
// you along the landscape continuously, which is how you find the interesting region,
// and clicking only ever lands you on one point of it.

struct VsOut {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) & 1) * 4.0 - 1.0;
    var out: VsOut;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}

const MARGIN: f32 = 14.0;
const PAD: f32 = 1.5;

// Blue for repulsion, warm for attraction, near-black where the pair does nothing — so
// the eye reads the zero contour, which is the line structure forms along.
fn diverging(v: f32) -> vec3<f32> {
    let x = clamp(v, -1.0, 1.0);
    let cold = vec3<f32>(0.25, 0.55, 1.0);
    let zero = vec3<f32>(0.06, 0.07, 0.11);
    let warm = vec3<f32>(1.0, 0.62, 0.24);
    return select(mix(zero, cold, -x), mix(zero, warm, x), x > 0.0);
}

fn panel_size(viewport: vec2<f32>) -> f32 {
    return clamp(min(viewport.x, viewport.y) * 0.26, 96.0, 190.0);
}

// The outward radial force of the probe pair at separation r, as the force pass computes
// it — landscape sample included.
fn probe_force(r: f32) -> f32 {
    let land = landscape_at(u.probe_a, u.probe_b);
    let t = pair_terms(r, land.s, land.l, land.ds, land.dl, land.d01, land.dd01);
    return t.f_out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (u.overlay == 0u) {
        discard;
        return vec4<f32>(0.0);
    }
    let px = in.position.xy;
    let vp = u.viewport;
    let size = panel_size(vp);

    // ---- the landscape, bottom left -------------------------------------------------
    let map_min = vec2<f32>(MARGIN, vp.y - MARGIN - size);
    let map_local = px - map_min;
    if (all(map_local >= vec2<f32>(0.0)) && all(map_local < vec2<f32>(size))) {
        let edge = min(min(map_local.x, map_local.y), min(size - map_local.x, size - map_local.y));
        if (edge < PAD) {
            return vec4<f32>(0.35, 0.38, 0.45, 0.55);
        }
        // x is the first colour, y the second, rising upward.
        let a = map_local.x / size * TAU;
        let b = (1.0 - map_local.y / size) * TAU;
        let land = landscape_at(a, b);
        var rgb = diverging(land.s + land.l);

        // Where the probe sits. Crosshairs rather than a dot: on a surface this busy a
        // dot is invisible, and the lines also read off each colour on its own axis.
        let probe = vec2<f32>(
            fract(u.probe_a / TAU) * size,
            (1.0 - fract(u.probe_b / TAU)) * size,
        );
        let cross = min(abs(map_local.x - probe.x), abs(map_local.y - probe.y));
        if (cross < 0.9) {
            rgb = mix(rgb, vec3<f32>(1.0), 0.75);
        }
        // The diagonal a = b: the chase is exactly zero along it, because Λ is odd.
        if (abs(map_local.x - (size - map_local.y)) < 0.8) {
            rgb = mix(rgb, vec3<f32>(0.55, 0.58, 0.62), 0.4);
        }
        return vec4<f32>(rgb, 0.92);
    }

    // ---- the radial profile, bottom right -------------------------------------------
    let plot = vec2<f32>(size * 1.45, size * 0.78);
    let plot_min = vec2<f32>(vp.x - MARGIN - plot.x, vp.y - MARGIN - plot.y);
    let local = px - plot_min;
    if (all(local >= vec2<f32>(0.0)) && all(local < plot)) {
        let edge = min(min(local.x, local.y), min(plot.x - local.x, plot.y - local.y));
        if (edge < PAD) {
            return vec4<f32>(0.35, 0.38, 0.45, 0.55);
        }
        var rgb = vec3<f32>(0.05, 0.055, 0.075);

        // Force is unbounded at the core, so the vertical axis is squashed rather than
        // clipped: the core spike stays on the chart instead of running off the top and
        // taking the shape of the well with it.
        let full = max((abs(u.s0) + u.amp_sym) * 2.0 / max(u.r_cut, 1e-4), 1e-4);
        let r = local.x / plot.x * u.r_cut;
        let dr = u.r_cut / plot.x;
        let y_of = local.y / plot.y;

        // Zero line, and the two lengths worth marking.
        if (abs(y_of - 0.5) < 0.004) {
            rgb = vec3<f32>(0.28, 0.30, 0.36);
        }
        if (abs(r - u.r_core) < dr * 0.7) {
            rgb = mix(rgb, vec3<f32>(0.9, 0.35, 0.35), 0.5);
        }
        if (u.profile == 1u) {
            let land = landscape_at(u.probe_a, u.probe_b);
            let rstar = u.rstar_min + u.rstar_span * land.d01;
            if (abs(r - rstar) < dr * 0.7) {
                rgb = mix(rgb, vec3<f32>(0.4, 0.85, 0.55), 0.55);
            }
        }

        // Fill the pixel if the curve passes anywhere through the column it covers. A
        // point sample would break the line into dashes wherever the profile is steep,
        // which is exactly where its shape matters.
        let lo = tanh(probe_force(max(r - dr * 0.5, 1e-5)) / full);
        let hi = tanh(probe_force(r + dr * 0.5) / full);
        let top = 0.5 - max(lo, hi) * 0.48;
        let bottom = 0.5 - min(lo, hi) * 0.48;
        if (y_of > top - 0.006 && y_of < bottom + 0.006) {
            // Warm above the line, blue below: attraction and repulsion read the same
            // way here as they do on the landscape beside it.
            let attracting = 0.5 - (top + bottom) * 0.5;
            rgb = select(vec3<f32>(0.35, 0.65, 1.0), vec3<f32>(1.0, 0.72, 0.35), attracting > 0.0);
        }

        // The pair being probed, as two swatches in the corner.
        if (local.x < 22.0 && local.y < 11.0) {
            rgb = hue(select(u.probe_b, u.probe_a, local.y < 5.5));
        }
        return vec4<f32>(rgb, 0.92);
    }

    discard;
    return vec4<f32>(0.0);
}
