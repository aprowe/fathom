//! The smooth interaction landscape over the colour torus, and its bake into a LUT.
//!
//! Colour is a continuous angle, so "which colours attract which" is not a matrix of
//! N×N numbers but a *surface* `A(a, b)` over the torus `[0, 2π)²`. Building it out of
//! a handful of harmonics rather than sampling noise is what makes it smooth by
//! construction: a truncated Fourier series is analytic, so the force field it induces
//! has no cliffs for the integrator to fall off, and its derivatives — which the energy
//! ledger needs exactly, not approximately — come out in closed form rather than from a
//! finite difference.
//!
//! Smoothness is therefore a single integer. Capping the harmonic order at `h` caps how
//! fast `A` can turn, so "Smoothness" in the panel is literally how many harmonics are
//! allowed.
//!
//! The landscape is split into its symmetric and antisymmetric parts:
//!
//! ```text
//! S(a,b) = (A(a,b) + A(b,a)) / 2     even — an ordinary reciprocal pair strength
//! Λ(a,b) = (A(a,b) − A(b,a)) / 2     odd  — the chase; Λ(b,a) = −Λ(a,b)
//! ```
//!
//! The split is done here, at bake time, rather than in the shader: the shader would
//! otherwise have to evaluate the landscape twice per pair, once with the arguments
//! swapped, and the result is the same every frame until someone randomises.
//!
//! A second, independent landscape `D(a,b)` rides along, giving the *preferred
//! separation* of a colour pair. That is what makes the well profile express "these two
//! should stay this far apart" instead of only "these two attract this much".

use std::f32::consts::TAU;

/// Resolution of the baked table along each colour axis.
///
/// Set by the energy ledger rather than by how the landscape looks. The force pass reads
/// `dS/dc` from this table, and an interpolated derivative is not the derivative of the
/// interpolated energy — they differ by something of the order of a cell, which becomes a
/// fixed fraction of the work the chase does and shows up as a conservation residual that
/// no timestep can reduce. Doubling this from 128 cut that residual by 4.7×; the picture
/// looked identical either way. Two megabytes is a cheap price for a claim that holds.
///
/// See `what_the_chase_leaves_behind_is_the_tables_resolution_not_the_timestep`.
pub const LUT_SIZE: usize = 256;

/// The highest harmonic order the Smoothness control will allow.
pub const MAX_HARMONICS: u32 = 6;

/// One term of the series: `c·cos(m·a − n·b) + s·sin(m·a − n·b)`.
#[derive(Clone, Copy, Debug)]
struct Term {
    m: f32,
    n: f32,
    c: f32,
    s: f32,
}

/// A value of one landscape together with its partial derivatives in both colours.
///
/// Returned as a unit because the conservation argument needs the derivative of *this*
/// evaluation, not of an analytically similar function — computing them apart is how
/// they drift.
#[derive(Clone, Copy, Debug, Default)]
struct Sample {
    v: f32,
    d1: f32,
    d2: f32,
}

/// A truncated Fourier series over the colour torus.
#[derive(Clone, Debug, Default)]
struct Field {
    terms: Vec<Term>,
    /// Divides the raw series so the field spans about [−1, 1] whatever the harmonics
    /// happen to sum to. Without it, "chase strength 1.0" would mean something different
    /// after every randomise.
    norm: f32,
}

impl Field {
    fn random(rng: &mut Lcg, harmonics: u32) -> Self {
        let h = harmonics.clamp(1, MAX_HARMONICS) as i32;
        let mut terms = Vec::new();
        for m in 0..=h {
            for n in 0..=h {
                if m == 0 && n == 0 {
                    // The constant term is a uniform offset on every pair, which is what
                    // the baseline-attraction parameter already is, and a better control
                    // than a random one.
                    continue;
                }
                // Higher harmonics get smaller amplitudes. Equal weight would make the
                // finest allowed detail dominate, so raising Smoothness would look like
                // adding noise rather than adding structure.
                let decay = 1.0 / (1 + m + n) as f32;
                terms.push(Term {
                    m: m as f32,
                    n: n as f32,
                    c: rng.signed() * decay,
                    s: rng.signed() * decay,
                });
            }
        }
        let mut field = Self { terms, norm: 1.0 };
        field.norm = field.peak().max(1e-6);
        field
    }

    /// Largest absolute value on a scan of the torus, used to normalise.
    fn peak(&self) -> f32 {
        let mut peak = 0.0f32;
        // Twice the bake resolution, so the normaliser does not systematically miss a
        // maximum that falls between baked samples and let the table exceed 1.
        let steps = LUT_SIZE * 2;
        for iy in 0..steps {
            let b = iy as f32 / steps as f32 * TAU;
            for ix in 0..steps {
                let a = ix as f32 / steps as f32 * TAU;
                peak = peak.max(self.eval(a, b).v.abs());
            }
        }
        peak
    }

    fn eval(&self, a: f32, b: f32) -> Sample {
        let mut out = Sample::default();
        for t in &self.terms {
            let phase = t.m * a - t.n * b;
            let (sin, cos) = phase.sin_cos();
            out.v += t.c * cos + t.s * sin;
            let dphase = -t.c * sin + t.s * cos;
            out.d1 += t.m * dphase;
            out.d2 += -t.n * dphase;
        }
        let inv = 1.0 / self.norm;
        Sample { v: out.v * inv, d1: out.d1 * inv, d2: out.d2 * inv }
    }
}

/// One baked table entry: everything a pair of colours needs, in two `vec4`s.
///
/// The layout is the shader's, not Rust's convenience: `strength` is what the force pass
/// samples on every neighbour of every particle, so its four numbers are the four it
/// wants together in one fetch.
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Entry {
    /// `[S, Λ, ∂S/∂a, ∂Λ/∂a]`.
    pub strength: [f32; 4],
    /// `[D, ∂D/∂a, 0, 0]` — preferred separation as a fraction of the cutoff.
    pub distance: [f32; 4],
}

/// A generated interaction landscape, and the table the GPU reads it from.
#[derive(Clone, Debug)]
pub struct Landscape {
    pub seed: u64,
    pub harmonics: u32,
    strength: Field,
    distance: Field,
}

impl Landscape {
    pub fn new(seed: u64, harmonics: u32) -> Self {
        let mut rng = Lcg::new(seed);
        // Two draws from one stream, so the same seed always gives the same pair of
        // fields and a preset is one number.
        let strength = Field::random(&mut rng, harmonics);
        let distance = Field::random(&mut rng, harmonics);
        Self { seed, harmonics, strength, distance }
    }

    /// The raw interaction coefficient before the even/odd split: what a reader
    /// recognises as the interaction matrix, positive for attraction.
    pub fn a(&self, a: f32, b: f32) -> f32 {
        self.strength.eval(a, b).v
    }

    /// Symmetric part and its derivative in the first colour.
    pub fn s(&self, a: f32, b: f32) -> (f32, f32) {
        let f = self.strength.eval(a, b);
        let r = self.strength.eval(b, a);
        ((f.v + r.v) * 0.5, (f.d1 + r.d2) * 0.5)
    }

    /// Antisymmetric part — the chase — and its derivative in the first colour.
    pub fn lambda(&self, a: f32, b: f32) -> (f32, f32) {
        let f = self.strength.eval(a, b);
        let r = self.strength.eval(b, a);
        ((f.v - r.v) * 0.5, (f.d1 - r.d2) * 0.5)
    }

    /// Preferred separation of the pair as a fraction of the span, in `[0, 1]`, and its
    /// derivative in the first colour.
    ///
    /// Symmetrised: how far apart two particles want to be is a property of the pair,
    /// and an asymmetric answer would mean each one pulling toward a different distance
    /// with no distance satisfying either.
    pub fn distance(&self, a: f32, b: f32) -> (f32, f32) {
        let f = self.distance.eval(a, b);
        let r = self.distance.eval(b, a);
        let sym = (f.v + r.v) * 0.5;
        let dsym = (f.d1 + r.d2) * 0.5;
        (0.5 + 0.5 * sym, 0.5 * dsym)
    }

    /// Bake the whole torus into the table the GPU samples.
    pub fn table(&self) -> Table {
        Table { entries: self.bake() }
    }

    /// Bake the whole torus into the table the GPU samples.
    pub fn bake(&self) -> Vec<Entry> {
        let mut out = vec![Entry::default(); LUT_SIZE * LUT_SIZE];
        for iy in 0..LUT_SIZE {
            let b = iy as f32 / LUT_SIZE as f32 * TAU;
            for ix in 0..LUT_SIZE {
                let a = ix as f32 / LUT_SIZE as f32 * TAU;
                let (s, ds) = self.s(a, b);
                let (l, dl) = self.lambda(a, b);
                let (d, dd) = self.distance(a, b);
                out[iy * LUT_SIZE + ix] =
                    Entry { strength: [s, l, ds, dl], distance: [d, dd, 0.0, 0.0] };
            }
        }
        out
    }
}

/// What a lookup of the baked table gives back, before any strength control is applied.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sampled {
    pub s: f32,
    pub l: f32,
    pub ds: f32,
    pub dl: f32,
    pub d01: f32,
    pub dd01: f32,
}

/// The baked table, read the way the shader reads it.
///
/// This exists so the CPU side can ask what the landscape is *as the simulation sees
/// it*, not as it was generated. The two are not the same: the shader runs on a
/// bilinear interpolation of the table, and between samples that differs from the
/// analytic series it was baked from. Checking the simulation's forces against the
/// analytic field would charge them for an error they did not make, and — worse — would
/// hide a real one behind it.
pub struct Table {
    entries: Vec<Entry>,
}

impl Table {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Bilinear, wrapping in both colours. The exact counterpart of `landscape_at` in
    /// `shaders/lut.wgsl`; if one changes, so must the other.
    pub fn sample(&self, a: f32, b: f32) -> Sampled {
        let size = LUT_SIZE as f32;
        let fa = (a / TAU).rem_euclid(1.0) * size;
        let fb = (b / TAU).rem_euclid(1.0) * size;
        let (ia, ib) = (fa as usize % LUT_SIZE, fb as usize % LUT_SIZE);
        let (ja, jb) = ((ia + 1) % LUT_SIZE, (ib + 1) % LUT_SIZE);
        let (ta, tb) = (fa - fa.floor(), fb - fb.floor());

        let mix = |lo: f32, hi: f32, t: f32| lo + (hi - lo) * t;
        let bilinear = |pick: fn(&Entry) -> f32| {
            let at = |x: usize, y: usize| pick(&self.entries[y * LUT_SIZE + x]);
            mix(
                mix(at(ia, ib), at(ja, ib), ta),
                mix(at(ia, jb), at(ja, jb), ta),
                tb,
            )
        };

        Sampled {
            s: bilinear(|e| e.strength[0]),
            l: bilinear(|e| e.strength[1]),
            ds: bilinear(|e| e.strength[2]),
            dl: bilinear(|e| e.strength[3]),
            d01: bilinear(|e| e.distance[0]),
            dd01: bilinear(|e| e.distance[1]),
        }
    }
}

/// A small linear congruential generator: reproducible across targets, no wasm caveats,
/// and short enough to read. Same one the gravity example uses.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }

    fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u32 << 24) as f32
    }

    /// Uniform in `[-1, 1)`.
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn landscape() -> Landscape {
        Landscape::new(17, 3)
    }

    #[test]
    fn the_chase_is_antisymmetric_and_the_pair_strength_is_not() {
        let l = landscape();
        for &(a, b) in &[(0.4f32, 2.1f32), (5.9, 0.2), (1.0, 1.0), (3.3, 4.8)] {
            assert!((l.lambda(a, b).0 + l.lambda(b, a).0).abs() < 1e-5, "Λ not odd at {a},{b}");
            assert!((l.s(a, b).0 - l.s(b, a).0).abs() < 1e-5, "S not even at {a},{b}");
        }
        // An odd function is zero on the diagonal, so a landscape whose chase vanished
        // everywhere would pass the test above. Check it is actually doing something.
        assert!(l.lambda(0.4, 2.1).0.abs() > 1e-3, "the chase is identically zero");
    }

    #[test]
    fn the_split_reassembles_into_the_landscape_it_came_from() {
        let l = landscape();
        for &(a, b) in &[(0.4f32, 2.1f32), (5.9, 0.2), (2.7, 3.9)] {
            let rebuilt = l.s(a, b).0 + l.lambda(a, b).0;
            assert!((rebuilt - l.a(a, b)).abs() < 1e-5, "S + Λ ≠ A at {a},{b}");
        }
    }

    /// The energy ledger balances only if the derivatives are the exact derivatives of
    /// the values sitting beside them in the same table. Nothing else in the system can
    /// notice if they are merely close.
    #[test]
    fn the_baked_derivatives_are_the_derivatives_of_the_baked_values() {
        let l = landscape();
        let h = 1e-3;
        for &(a, b) in &[(0.4f32, 2.1f32), (5.9, 0.2), (2.7, 3.9), (1.5, 1.5)] {
            let fd = |f: fn(&Landscape, f32, f32) -> (f32, f32)| {
                (f(&l, a + h, b).0 - f(&l, a - h, b).0) / (2.0 * h)
            };
            assert!((l.s(a, b).1 - fd(Landscape::s)).abs() < 1e-3, "∂S/∂a wrong at {a},{b}");
            assert!(
                (l.lambda(a, b).1 - fd(Landscape::lambda)).abs() < 1e-3,
                "∂Λ/∂a wrong at {a},{b}"
            );
            assert!(
                (l.distance(a, b).1 - fd(Landscape::distance)).abs() < 1e-3,
                "∂D/∂a wrong at {a},{b}"
            );
        }
    }

    #[test]
    fn the_landscape_is_periodic_in_both_colours() {
        let l = landscape();
        let (a, b) = (0.9f32, 4.2f32);
        assert!((l.a(a, b) - l.a(a + TAU, b)).abs() < 1e-4);
        assert!((l.a(a, b) - l.a(a, b + TAU)).abs() < 1e-4);
    }

    #[test]
    fn normalisation_keeps_every_landscape_on_the_same_scale() {
        for seed in 1..12u64 {
            let l = Landscape::new(seed, 4);
            let peak = l.bake().iter().fold(0.0f32, |m, e| {
                m.max(e.strength[0].abs()).max(e.strength[1].abs())
            });
            assert!(peak <= 1.0 + 1e-3, "seed {seed} exceeds unit scale: {peak}");
            assert!(peak > 0.2, "seed {seed} is nearly flat: {peak}");
        }
    }

    #[test]
    fn preferred_separation_stays_inside_the_span_it_indexes() {
        let l = landscape();
        for e in l.bake() {
            assert!((0.0..=1.0).contains(&e.distance[0]), "D out of range: {}", e.distance[0]);
        }
    }

    #[test]
    fn smoothness_bounds_how_fast_the_landscape_can_turn() {
        let gradient = |h: u32| {
            let l = Landscape::new(5, h);
            let table = l.bake();
            let mut worst = 0.0f32;
            for iy in 0..LUT_SIZE {
                for ix in 0..LUT_SIZE {
                    let here = table[iy * LUT_SIZE + ix].strength[0];
                    let next = table[iy * LUT_SIZE + (ix + 1) % LUT_SIZE].strength[0];
                    worst = worst.max((next - here).abs());
                }
            }
            worst
        };
        assert!(gradient(1) < gradient(MAX_HARMONICS), "smoothness does not bind");
    }

    #[test]
    fn the_same_seed_gives_the_same_landscape() {
        let a = Landscape::new(42, 3).bake();
        let b = Landscape::new(42, 3).bake();
        let c = Landscape::new(43, 3).bake();
        assert_eq!(bytemuck::cast_slice::<Entry, u8>(&a), bytemuck::cast_slice::<Entry, u8>(&b));
        assert_ne!(bytemuck::cast_slice::<Entry, u8>(&a), bytemuck::cast_slice::<Entry, u8>(&c));
    }
}
