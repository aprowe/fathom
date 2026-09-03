//! Initial conditions.
//!
//! Deterministic for a given seed, and CPU-side, so they can be tested without a GPU.
//! Every scene has its net momentum removed at the end: otherwise a scene drifts off
//! screen at a constant velocity, which reads as a bug even though it is just physics.

/// A body: position, mass, and one spare lane (the buffer is `vec4` for alignment).
pub type Body = [f32; 4];
/// A velocity.
pub type Vel = [f32; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Disc,
    TwoGalaxies,
    Ring,
    Uniform,
}

impl Scene {
    /// The order here is the order of the `scene` command's options.
    pub const ALL: [Scene; 4] = [Scene::Disc, Scene::TwoGalaxies, Scene::Ring, Scene::Uniform];
    pub const LABELS: &'static [&'static str] = &["Disc", "Two galaxies", "Ring", "Uniform"];

    pub fn from_index(i: u32) -> Scene {
        Self::ALL[(i as usize).min(Self::ALL.len() - 1)]
    }
}

/// Total mass of a scene. Chosen with the default gravity of 1.0 and a scene radius of
/// about 1.0 so that a circular orbit takes a few seconds: fast enough to watch, slow
/// enough to steer.
const TOTAL_MASS: f32 = 1.0;

/// How much of a disc's mass sits in its central body.
const CORE_FRACTION: f32 = 0.35;

/// A small linear congruential generator. A real RNG crate would work too, but this is
/// three lines, has no wasm caveats, and gives reproducible scenes across targets.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u32 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo)
    }
}

/// Build `n` bodies for a scene. `n` is taken as given; the caller keeps it a multiple
/// of the workgroup size.
pub fn generate(scene: Scene, n: usize, seed: u64) -> (Vec<Body>, Vec<Vel>) {
    let mut rng = Lcg::new(seed);
    let mut bodies = Vec::with_capacity(n);
    let mut vels = Vec::with_capacity(n);

    match scene {
        Scene::Disc => disc(&mut rng, n, [0.0, 0.0], [0.0, 0.0], 1.0, TOTAL_MASS, &mut bodies, &mut vels),
        Scene::TwoGalaxies => {
            let half = n / 2;
            let each = TOTAL_MASS * 0.5;
            let approach = (TOTAL_MASS / 2.4).sqrt() * 0.45;
            disc(&mut rng, half, [-1.2, -0.3], [approach, 0.07], 0.55, each, &mut bodies, &mut vels);
            disc(&mut rng, n - half, [1.2, 0.3], [-approach, -0.07], 0.55, each, &mut bodies, &mut vels);
        }
        Scene::Ring => {
            for _ in 0..n {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let r = rng.range(0.75, 0.95);
                let m = rng.range(0.4, 1.0) / n as f32 * TOTAL_MASS;
                bodies.push([r * a.cos(), r * a.sin(), m, 0.0]);
                // A ring encloses roughly half its own mass inside any point on it.
                let speed = (TOTAL_MASS * 0.5 / r).sqrt();
                vels.push([-a.sin() * speed, a.cos() * speed]);
            }
        }
        Scene::Uniform => {
            for _ in 0..n {
                let m = rng.range(0.4, 1.0) / n as f32 * TOTAL_MASS;
                bodies.push([rng.range(-1.2, 1.2), rng.range(-1.2, 1.2), m, 0.0]);
                vels.push([rng.range(-0.02, 0.02), rng.range(-0.02, 0.02)]);
            }
        }
    }

    remove_net_momentum(&bodies, &mut vels);
    (bodies, vels)
}

/// A rotating disc with a heavier core, centred at `at` and drifting at `drift`.
///
/// Velocities are the true circular speed for the mass enclosed at each radius, not a
/// fraction of it. That distinction is the whole scene: at a fraction of circular speed
/// the disc free-falls into its own centre, rebounds through it, and disperses into an
/// even cloud within a couple of seconds.
fn disc(
    rng: &mut Lcg,
    n: usize,
    at: [f32; 2],
    drift: [f32; 2],
    radius: f32,
    mass: f32,
    bodies: &mut Vec<Body>,
    vels: &mut Vec<Vel>,
) {
    // A heavy core holds the disc together and gives the outer bodies something to
    // orbit; without it the inner orbits are dominated by nearest neighbours.
    let core = mass * CORE_FRACTION;
    let disc_mass = mass - core;

    for i in 0..n {
        let a = rng.range(0.0, std::f32::consts::TAU);
        // sqrt keeps the areal density even instead of piling everything at the centre.
        let r = rng.unit().sqrt() * radius + 0.03;
        let m = rng.range(0.4, 1.0) / n as f32 * disc_mass;
        bodies.push([at[0] + r * a.cos(), at[1] + r * a.sin(), m, 0.0]);

        // Enclosed mass for an evenly dense disc grows with the square of the radius.
        let enclosed = core + disc_mass * (r / radius).powi(2).min(1.0);
        let speed = (enclosed / r).sqrt();
        vels.push([drift[0] - a.sin() * speed, drift[1] + a.cos() * speed]);

        if i == 0 {
            let last = bodies.len() - 1;
            bodies[last] = [at[0], at[1], core, 0.0];
            vels[last] = drift;
        }
    }
}

/// Subtract the mass-weighted mean velocity so the system stays put.
fn remove_net_momentum(bodies: &[Body], vels: &mut [Vel]) {
    let mut total_mass = 0.0f32;
    let mut p = [0.0f32; 2];
    for (b, v) in bodies.iter().zip(vels.iter()) {
        total_mass += b[2];
        p[0] += b[2] * v[0];
        p[1] += b[2] * v[1];
    }
    if total_mass <= 0.0 {
        return;
    }
    let mean = [p[0] / total_mass, p[1] / total_mass];
    for v in vels.iter_mut() {
        v[0] -= mean[0];
        v[1] -= mean[1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net_momentum(bodies: &[Body], vels: &[Vel]) -> f32 {
        let mut p = [0.0f32; 2];
        for (b, v) in bodies.iter().zip(vels) {
            p[0] += b[2] * v[0];
            p[1] += b[2] * v[1];
        }
        (p[0] * p[0] + p[1] * p[1]).sqrt()
    }

    #[test]
    fn every_scene_produces_exactly_the_requested_body_count() {
        for scene in Scene::ALL {
            let (bodies, vels) = generate(scene, 1024, 7);
            assert_eq!(bodies.len(), 1024, "{scene:?}");
            assert_eq!(vels.len(), 1024, "{scene:?}");
        }
    }

    #[test]
    fn every_scene_starts_with_no_net_drift() {
        for scene in Scene::ALL {
            let (bodies, vels) = generate(scene, 2048, 3);
            assert!(net_momentum(&bodies, &vels) < 1e-3, "{scene:?} drifts");
        }
    }

    #[test]
    fn scenes_stay_inside_the_starting_view() {
        for scene in Scene::ALL {
            let (bodies, _) = generate(scene, 2048, 11);
            for b in &bodies {
                assert!(b[0].abs() < 3.0 && b[1].abs() < 3.0, "{scene:?} body at {b:?}");
                assert!(b[2] > 0.0, "{scene:?} has a massless body");
            }
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_scene() {
        let a = generate(Scene::Disc, 512, 42);
        let b = generate(Scene::Disc, 512, 42);
        let c = generate(Scene::Disc, 512, 43);
        assert_eq!(a.0, b.0);
        assert_ne!(a.0, c.0);
    }

    #[test]
    fn two_galaxies_splits_the_bodies_between_two_places() {
        let (bodies, _) = generate(Scene::TwoGalaxies, 2048, 5);
        let left = bodies.iter().filter(|b| b[0] < 0.0).count();
        assert!(left > 900 && left < 1150, "unbalanced: {left}");
    }
}
