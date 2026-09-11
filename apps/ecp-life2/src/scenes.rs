//! Initial conditions.
//!
//! Deterministic for a given seed and CPU-side, so they can be tested without a GPU.
//!
//! The world is the unit torus: positions live in `[-0.5, 0.5)` on both axes and wrap.
//! Fixing the world at one unit makes the cutoff `R` the only length in the system that
//! anyone has to think about — every other distance is read as a fraction of it — and it
//! means changing the particle count changes the density, which is the knob that
//! actually decides whether the system forms structure or a gas.

use std::f32::consts::TAU;

/// A particle as the GPU stores it: position, colour, and one spare lane so the buffer
/// is `vec4`-aligned.
pub type Particle = [f32; 4];
/// Velocity, plus two lanes the force pass writes its per-particle scalars into.
pub type Velocity = [f32; 4];

/// Side of the world. One, by definition; see the module comment.
pub const WORLD: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// Every particle a random place and a random colour: the least assumed.
    Soup,
    /// Colour banded across x, so the landscape's structure shows up as bands
    /// interacting before anything has had to find its own partners.
    Bands,
    /// Blobs, each starting near one colour. Structure that already exists, to see
    /// whether the dynamics keep it or dissolve it.
    Clumps,
    /// Two opposed colours only. The chase between them is the strongest the landscape
    /// offers, so this is the scene that moves first.
    Opposed,
}

impl Scene {
    /// The order here is the order of the `scene` command's options.
    pub const ALL: [Scene; 4] = [Scene::Soup, Scene::Bands, Scene::Clumps, Scene::Opposed];
    pub const LABELS: &'static [&'static str] = &["Soup", "Bands", "Clumps", "Opposed"];

    pub fn from_index(i: u32) -> Scene {
        Self::ALL[(i as usize).min(Self::ALL.len() - 1)]
    }
}

/// Build `n` particles for a scene.
///
/// Everything starts at rest. The system has an energy source of its own — the colour
/// mismatch between neighbours — so handing it kinetic energy at t=0 only obscures where
/// the motion is coming from.
pub fn generate(scene: Scene, n: usize, seed: u64) -> (Vec<Particle>, Vec<Velocity>) {
    let mut rng = Lcg::new(seed);
    let mut particles = Vec::with_capacity(n);

    match scene {
        Scene::Soup => {
            for _ in 0..n {
                particles.push([rng.coord(), rng.coord(), rng.unit() * TAU, 0.0]);
            }
        }
        Scene::Bands => {
            for _ in 0..n {
                let x = rng.coord();
                // Colour is a circle, so a band across the world is one full turn of it:
                // the two edges of the world meet in colour as well as in space.
                particles.push([x, rng.coord(), (x + 0.5) * TAU, 0.0]);
            }
        }
        Scene::Clumps => {
            const BLOBS: usize = 7;
            let centres: Vec<[f32; 3]> = (0..BLOBS)
                .map(|_| [rng.coord(), rng.coord(), rng.unit() * TAU])
                .collect();
            for i in 0..n {
                let c = centres[i % BLOBS];
                let spread = 0.06;
                particles.push([
                    wrap(c[0] + rng.gauss() * spread),
                    wrap(c[1] + rng.gauss() * spread),
                    // A little colour spread inside a blob, or the whole blob sits at
                    // exactly one colour where the chase is identically zero and nothing
                    // ever starts.
                    c[2] + rng.gauss() * 0.3,
                    0.0,
                ]);
            }
        }
        Scene::Opposed => {
            for i in 0..n {
                let colour = if i % 2 == 0 { 0.0 } else { TAU * 0.5 };
                particles.push([rng.coord(), rng.coord(), colour + rng.gauss() * 0.15, 0.0]);
            }
        }
    }

    for p in &mut particles {
        p[2] = p[2].rem_euclid(TAU);
    }
    let velocities = vec![[0.0; 4]; particles.len()];
    (particles, velocities)
}

/// Fold a coordinate back into the world.
pub fn wrap(x: f32) -> f32 {
    (x + 0.5).rem_euclid(WORLD) - 0.5
}

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }

    fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u32 << 24) as f32
    }

    /// A world coordinate: uniform in `[-0.5, 0.5)`.
    fn coord(&mut self) -> f32 {
        self.unit() - 0.5
    }

    /// Roughly normal, mean 0, unit variance. Six uniforms is plenty for scattering
    /// particles and costs nothing worth measuring at setup time.
    fn gauss(&mut self) -> f32 {
        let mut sum = 0.0;
        for _ in 0..6 {
            sum += self.unit();
        }
        (sum - 3.0) * std::f32::consts::FRAC_1_SQRT_2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_produces_exactly_the_requested_count() {
        for scene in Scene::ALL {
            let (p, v) = generate(scene, 1024, 7);
            assert_eq!(p.len(), 1024, "{scene:?}");
            assert_eq!(v.len(), 1024, "{scene:?}");
        }
    }

    #[test]
    fn every_scene_starts_inside_the_world_and_on_the_colour_circle() {
        for scene in Scene::ALL {
            for p in generate(scene, 4096, 3).0 {
                assert!((-0.5..0.5).contains(&p[0]), "{scene:?} x out of world: {}", p[0]);
                assert!((-0.5..0.5).contains(&p[1]), "{scene:?} y out of world: {}", p[1]);
                assert!((0.0..TAU).contains(&p[2]), "{scene:?} colour off circle: {}", p[2]);
            }
        }
    }

    #[test]
    fn every_scene_starts_at_rest() {
        for scene in Scene::ALL {
            assert!(generate(scene, 512, 1).1.iter().all(|v| v == &[0.0; 4]), "{scene:?}");
        }
    }

    /// A scene where every particle shares one colour has no mismatch anywhere, so the
    /// chase is zero and the simulation never starts. Each scene must supply some.
    #[test]
    fn every_scene_starts_with_colour_disagreement_to_burn() {
        for scene in Scene::ALL {
            let colours: Vec<f32> = generate(scene, 2048, 5).0.iter().map(|p| p[2]).collect();
            let mean = colours.iter().sum::<f32>() / colours.len() as f32;
            let spread =
                (colours.iter().map(|c| (c - mean).powi(2)).sum::<f32>() / colours.len() as f32).sqrt();
            assert!(spread > 0.05, "{scene:?} is monochrome: {spread}");
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_scene() {
        assert_eq!(generate(Scene::Clumps, 512, 42).0, generate(Scene::Clumps, 512, 42).0);
        assert_ne!(generate(Scene::Clumps, 512, 42).0, generate(Scene::Clumps, 512, 43).0);
    }

    #[test]
    fn wrapping_maps_anything_back_into_the_world() {
        for x in [-3.7f32, -0.5, -0.4999, 0.0, 0.4999, 0.5, 2.6] {
            assert!((-0.5..0.5).contains(&wrap(x)), "{x} wrapped to {}", wrap(x));
        }
    }

    #[test]
    fn clumps_are_actually_clumped() {
        let particles = generate(Scene::Clumps, 4096, 9).0;
        // A blob of standard deviation 0.06 keeps nearly everything within 0.2 of its
        // centre; a uniform scatter would put barely a tenth of the particles that close
        // to any seven points.
        let near_first: usize = particles
            .iter()
            .step_by(7)
            .filter(|p| {
                let c = particles[0];
                let d = (wrap(p[0] - c[0]).powi(2) + wrap(p[1] - c[1]).powi(2)).sqrt();
                d < 0.2
            })
            .count();
        let total = particles.iter().step_by(7).count();
        assert!(near_first * 2 > total, "clumps are not clumped: {near_first}/{total}");
    }
}
