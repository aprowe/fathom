// Reading the baked interaction landscape.
//
// Included by every shader that needs to know what two colours do to each other: the
// force pass and the overlay that draws the landscape. One copy, so the picture and the
// physics cannot disagree about what the landscape is.
//
// The table is a plain storage buffer rather than a texture. A texture would bring a
// sampler, a filterable-format question that differs per backend, and an address mode
// that has to be talked into being periodic; here the wrap is one `%` and the filtering
// is four lines, both exactly under our control.

// Two vec4s per texel: [S, Λ, dS/da, dΛ/da] then [D, dD/da, _, _].
@group(0) @binding(4) var<storage, read> lut: array<vec4<f32>>;

const LUT_SIZE: u32 = 256u;

struct Landscape {
    s: f32,
    l: f32,
    ds: f32,
    dl: f32,
    d01: f32,
    dd01: f32,
}

fn lut_texel(ix: u32, iy: u32, half: u32) -> vec4<f32> {
    return lut[(iy * LUT_SIZE + ix) * 2u + half];
}

// Bilinear, wrapping in both colours. Colour is an angle: the table's last column is
// adjacent to its first, and a sample that clamped there instead of wrapping would put a
// seam across the landscape at colour zero that the particles would visibly avoid.
fn landscape_at(a: f32, b: f32) -> Landscape {
    let size = f32(LUT_SIZE);
    let fa = fract(a / TAU) * size;
    let fb = fract(b / TAU) * size;
    let ia = u32(fa);
    let ib = u32(fb);
    let ja = (ia + 1u) % LUT_SIZE;
    let jb = (ib + 1u) % LUT_SIZE;
    let ta = fa - floor(fa);
    let tb = fb - floor(fb);

    let s00 = lut_texel(ia, ib, 0u);
    let s10 = lut_texel(ja, ib, 0u);
    let s01 = lut_texel(ia, jb, 0u);
    let s11 = lut_texel(ja, jb, 0u);
    let strength = mix(mix(s00, s10, ta), mix(s01, s11, ta), tb);

    let d00 = lut_texel(ia, ib, 1u);
    let d10 = lut_texel(ja, ib, 1u);
    let d01v = lut_texel(ia, jb, 1u);
    let d11 = lut_texel(ja, jb, 1u);
    let distance = mix(mix(d00, d10, ta), mix(d01v, d11, ta), tb);

    var out: Landscape;
    // The strength controls are applied here, once, so every consumer of the landscape
    // sees the same scaled values. Baseline attraction is added to S alone: it is what
    // keeps a pair bound whatever their colours do, and the chase must not inherit it.
    out.s = u.s0 + u.amp_sym * strength.x;
    out.l = u.amp_chase * strength.y;
    out.ds = u.amp_sym * strength.z;
    out.dl = u.amp_chase * strength.w;
    out.d01 = distance.x;
    out.dd01 = distance.y;
    return out;
}
