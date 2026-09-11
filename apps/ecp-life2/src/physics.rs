//! The parameters the physics runs on, and a CPU statement of the system's energy.
//!
//! [`Settings`] exists so the sliders are turned into absolute lengths and strengths in
//! exactly one place. The panel offers the core radius, the well width and the preferred
//! separation as fractions of the cutoff, because that is how anyone actually thinks
//! about them — "a third of the way out" rather than "0.004" — and the shader only ever
//! sees the resolved numbers.
//!
//! [`total_energy`] is the other half of the conservation test. The shader computes
//! *forces*; this computes the *energy* those forces are supposed to be the gradient of.
//! Deliberately written from the physics rather than transcribed from the WGSL: two
//! independent statements of the same system, where one being wrong shows up as drift in
//! a quantity that should not move.

use crate::landscape::Table;
use crate::scenes::{Particle, Velocity};

/// Which radial shape the pair interaction takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// `S(a,b)·V(r)` with `V` monotonic. Binds reliably and conserves exactly, but every
    /// pair wants the same separation and no relationship can be repulsive at range.
    Classic,
    /// A genuine minimum at a colour-dependent distance. Colour then says how far apart
    /// two particles want to be, and the landscape gains the repulsive regions the
    /// classic profile cannot express.
    Well,
}

impl Profile {
    pub const LABELS: &'static [&'static str] = &["Classic", "Well"];

    pub fn from_index(i: u32) -> Profile {
        if i == 0 { Profile::Classic } else { Profile::Well }
    }

    pub fn index(self) -> u32 {
        match self {
            Profile::Classic => 0,
            Profile::Well => 1,
        }
    }
}

/// Physics settings with every length already resolved into world units.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub r_cut: f32,
    pub r_core: f32,
    pub k_rep: f32,
    pub lam: f32,
    pub chase_gain: f32,
    pub damping: f32,
    pub s0: f32,
    pub amp_sym: f32,
    pub amp_chase: f32,
    pub well_sigma: f32,
    pub rstar_min: f32,
    pub rstar_span: f32,
    pub profile: Profile,
}

impl Settings {
    /// How many cells the world divides into at this cutoff.
    ///
    /// Rounding down is what makes the 3×3 neighbourhood scan exact rather than
    /// approximate: it guarantees a cell is at least as wide as the cutoff, so nothing
    /// outside the block being scanned can possibly be in range.
    pub fn cells(&self) -> u32 {
        ((1.0 / self.r_cut.max(1e-4)).floor() as u32).clamp(4, crate::sim::MAX_CELLS_1D)
    }

    /// Pair energy at separation `r` for a pair whose landscape values are `s`, `l`, `d01`.
    ///
    /// Everything the system stores, and nothing it does not: kinetic energy is the
    /// caller's business, and colour has no intrinsic energy of its own — only
    /// disagreements between neighbours cost anything.
    pub fn pair_energy(&self, r: f32, s: f32, l: f32, d01: f32) -> f32 {
        if r >= self.r_cut {
            return 0.0;
        }
        let q = 1.0 - r / self.r_cut;
        let mut e = 0.0;

        if r < self.r_core {
            let over = self.r_core - r;
            e += self.k_rep / (self.r_core * self.r_core).max(1e-8) * over * over;
        }

        e += match self.profile {
            Profile::Classic => s * -(q * q),
            Profile::Well => {
                let rstar = self.rstar_min + self.rstar_span * d01;
                let z = (r - rstar) / self.well_sigma.max(1e-4);
                // Flat over the inner half, then eased to zero — so the minimum sits at
                // the separation the colour pair asked for rather than inside it.
                let hold = 0.5 * self.r_cut;
                let t = ((self.r_cut - r) / (self.r_cut - hold)).clamp(0.0, 1.0);
                -s * (-0.5 * z * z).exp() * t * t * (3.0 - 2.0 * t)
            }
        };

        // The colour-mismatch store. This is the reservoir the chase burns, and the only
        // place colour appears in the energy at all.
        e += self.lam * l * l * q * q;
        e
    }
}

/// Total energy of the system: kinetic, plus every pair's contribution counted once.
///
/// O(n²), so this is for the two-body and few-body checks it exists to serve, not for
/// instrumenting a live run of a hundred thousand.
pub fn total_energy(
    settings: &Settings,
    landscape: &Table,
    particles: &[Particle],
    velocities: &[Velocity],
) -> f32 {
    let mut energy: f64 = 0.0;
    for v in velocities {
        energy += 0.5 * (v[0] * v[0] + v[1] * v[1]) as f64;
    }
    for (i, a) in particles.iter().enumerate() {
        for b in &particles[i + 1..] {
            // Minimum image on the unit torus, matching the shader's `round`.
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let dx = dx - dx.round();
            let dy = dy - dy.round();
            let r = (dx * dx + dy * dy).sqrt();
            // Coincident particles are skipped on both sides. The shader has no
            // separation direction to push along there and bails out; an energy that
            // counted them would be charging for a force nothing ever applied.
            if r >= settings.r_cut || r < 1e-6 {
                continue;
            }
            let sampled = landscape.sample(a[2], b[2]);
            let s = settings.s0 + settings.amp_sym * sampled.s;
            let l = settings.amp_chase * sampled.l;
            energy += settings.pair_energy(r, s, l, sampled.d01) as f64;
        }
    }
    energy as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::Landscape;

    fn settings(profile: Profile) -> Settings {
        Settings {
            r_cut: 0.1,
            r_core: 0.02,
            k_rep: 1.0,
            lam: 0.6,
            chase_gain: 1.0,
            damping: 0.0,
            s0: 0.9,
            amp_sym: 1.0,
            amp_chase: 1.0,
            well_sigma: 0.015,
            rstar_min: 0.025,
            rstar_span: 0.05,
            profile,
        }
    }

    /// Every envelope in the system is built from the same taper so that all of them
    /// reach zero together. A term that survived the cutoff would make a neighbour
    /// crossing the radius change the stored energy in a step, and no timestep is small
    /// enough to fix that.
    #[test]
    fn energy_reaches_zero_at_the_cutoff_from_both_sides() {
        for profile in [Profile::Classic, Profile::Well] {
            let s = settings(profile);
            let inside = s.pair_energy(s.r_cut - 1e-4, 0.9, 0.4, 0.5);
            let outside = s.pair_energy(s.r_cut + 1e-4, 0.9, 0.4, 0.5);
            assert_eq!(outside, 0.0, "{profile:?} acts beyond the cutoff");
            assert!(inside.abs() < 1e-4, "{profile:?} steps at the cutoff: {inside}");
        }
    }

    #[test]
    fn the_core_is_repulsive_and_dominates_close_in() {
        let s = settings(Profile::Classic);
        assert!(s.pair_energy(0.001, 0.9, 0.0, 0.5) > s.pair_energy(0.01, 0.9, 0.0, 0.5));
        assert!(s.pair_energy(0.001, 0.9, 0.0, 0.5) > 0.0);
    }

    /// The claim the well profile exists to make: the pair has a preferred separation,
    /// and moving it is what colour does.
    #[test]
    fn the_well_puts_its_minimum_where_the_colour_pair_asks() {
        let s = settings(Profile::Well);
        let minimum_for = |d01: f32| {
            let steps = 2000;
            (0..steps)
                .map(|i| s.r_core + (i as f32 / steps as f32) * (s.r_cut - s.r_core))
                .min_by(|a, b| {
                    s.pair_energy(*a, 1.0, 0.0, d01)
                        .total_cmp(&s.pair_energy(*b, 1.0, 0.0, d01))
                })
                .unwrap()
        };
        let near = minimum_for(0.0);
        let far = minimum_for(1.0);
        assert!(far > near + 0.01, "colour does not move the well: {near} vs {far}");
        assert!((near - s.rstar_min).abs() < 0.005, "near minimum misplaced: {near}");
    }

    #[test]
    fn colour_disagreement_is_the_only_thing_colour_stores() {
        let s = settings(Profile::Classic);
        let agreeing = s.pair_energy(0.05, 0.9, 0.0, 0.5);
        let disagreeing = s.pair_energy(0.05, 0.9, 0.8, 0.5);
        assert!(disagreeing > agreeing, "mismatch stores nothing");
        // And an isolated particle has no capacity to store any: storage is a two-body
        // property, so it scales with local density rather than with particle count.
        assert_eq!(s.pair_energy(s.r_cut, 0.9, 0.8, 0.5), 0.0);
    }

    #[test]
    fn total_energy_counts_each_pair_once_and_includes_motion() {
        let land = Landscape::new(3, 3).table();
        let s = settings(Profile::Classic);
        let particles = vec![[0.0, 0.0, 0.5, 0.0], [0.03, 0.0, 2.5, 0.0]];
        let still = vec![[0.0; 4]; 2];
        let moving = vec![[1.0, 0.0, 0.0, 0.0], [0.0; 4]];
        let e_still = total_energy(&s, &land, &particles, &still);
        let e_moving = total_energy(&s, &land, &particles, &moving);
        assert!((e_moving - e_still - 0.5).abs() < 1e-5, "kinetic term wrong");

        let far = vec![[0.0, 0.0, 0.5, 0.0], [0.3, 0.0, 2.5, 0.0]];
        assert_eq!(total_energy(&s, &land, &far, &still), 0.0, "pairs act beyond the cutoff");
    }

    #[test]
    fn the_grid_is_never_coarser_than_the_cutoff() {
        for r in [0.004f32, 0.01, 0.017, 0.033, 0.08] {
            let mut s = settings(Profile::Classic);
            s.r_cut = r;
            let cell = 1.0 / s.cells() as f32;
            assert!(cell >= r, "cutoff {r} needs cells wider than {cell}");
        }
    }
}
