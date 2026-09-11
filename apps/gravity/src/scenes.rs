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
    System,
    Binary,
    Disc,
    TwoGalaxies,
    Ring,
    Uniform,
}

impl Scene {
    /// The order here is the order of the `scene` command's options.
    pub const ALL: [Scene; 6] = [
        Scene::System,
        Scene::Binary,
        Scene::Disc,
        Scene::TwoGalaxies,
        Scene::Ring,
        Scene::Uniform,
    ];
    pub const LABELS: &'static [&'static str] =
        &["Sun and planet", "Binary", "Disc", "Two galaxies", "Ring", "Uniform"];

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

/// The sun of the planetary scene. Light, deliberately: orbital speed goes with the
/// square root of the central mass, and a planet that crosses the screen in a couple
/// of seconds is a bullet, not a planet. At this mass the planet takes about twelve
/// seconds to go round.
pub const SUN_MASS: f32 = 0.12;
pub const PLANET_MASS: f32 = 0.012;
const PLANET_ORBIT: f32 = 0.72;

/// The mass of each star in the binary. Two of them hold most of the scene's mass,
/// which is what makes the swarm around them *orbit* rather than mill about.
pub const BINARY_STAR_MASS: f32 = 0.3;
/// Half the separation of the pair.
const BINARY_HALF_SEP: f32 = 0.42;

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
        Scene::System => system(&mut rng, n, &mut bodies, &mut vels),
        Scene::Binary => binary(&mut rng, n, &mut bodies, &mut vels),
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

/// A sun, a planet in a circular orbit around it, a few moons around the planet, and a
/// thin disc of dust around the sun that the planet ploughs through.
///
/// The dust is far lighter than the planet, so it decorates the orbit rather than
/// perturbing it; the moons are lighter still.
fn system(rng: &mut Lcg, n: usize, bodies: &mut Vec<Body>, vels: &mut Vec<Vel>) {
    let sun = SUN_MASS;
    let planet = PLANET_MASS;
    let dust_mass = sun * 0.05;
    let moon_mass = planet * 0.02;

    bodies.push([0.0, 0.0, sun, 0.0]);
    vels.push([0.0, 0.0]);

    let r = PLANET_ORBIT;
    let v = (sun / r).sqrt();
    let at = [r, 0.0];
    let drift = [0.0, v];
    bodies.push([at[0], at[1], planet, 0.0]);
    vels.push(drift);

    let n_rest = n.saturating_sub(2);
    let n_moons = (n_rest / 40).max(1).min(n_rest);
    let n_dust = n_rest - n_moons;

    for _ in 0..n_moons {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let rm = rng.range(0.03, 0.07);
        let m = rng.range(0.4, 1.0) / n_moons as f32 * moon_mass;
        bodies.push([at[0] + rm * a.cos(), at[1] + rm * a.sin(), m, 0.0]);
        let speed = (planet / rm).sqrt();
        vels.push([drift[0] - a.sin() * speed, drift[1] + a.cos() * speed]);
    }

    for _ in 0..n_dust {
        let a = rng.range(0.0, std::f32::consts::TAU);
        // Even areal density between an inner clearing and the edge of the view.
        let rd = (0.12 * 0.12 + rng.unit() * (1.1 * 1.1 - 0.12 * 0.12)).sqrt();
        let m = rng.range(0.4, 1.0) / n_dust.max(1) as f32 * dust_mass;
        bodies.push([rd * a.cos(), rd * a.sin(), m, 0.0]);
        let enclosed = sun + dust_mass * ((rd * rd) / (1.1 * 1.1)).min(1.0);
        let speed = (enclosed / rd).sqrt();
        vels.push([-a.sin() * speed, a.cos() * speed]);
    }
}

/// Two heavy stars in a circular orbit about their common centre, each with a small
/// swarm bound to it, and a wider ring around the pair.
///
/// The pair is the scene: the swarms are light enough that the stars' orbit is set by
/// each other, so their period follows from the separation alone. Each star's swarm
/// orbits *that star* at its local circular speed plus the star's own velocity, which
/// is what keeps a swarm attached rather than left behind on the first pass.
fn binary(rng: &mut Lcg, n: usize, bodies: &mut Vec<Body>, vels: &mut Vec<Vel>) {
    let m = BINARY_STAR_MASS;
    let d = BINARY_HALF_SEP;

    let light = TOTAL_MASS - 2.0 * m;
    let n_swarm = n.saturating_sub(2);
    // Most of the light bodies belong to a star; the rest circle the pair from outside.
    let n_ring = n_swarm / 5;
    let n_each = (n_swarm - n_ring) / 2;
    let swarm_mass = light * 0.25;
    let ring_mass = light - 2.0 * swarm_mass;
    let swarm_radius = 0.24;

    // Each star drags its swarm with it, so what orbits the centre is the star *system*.
    // Two equal systems of mass M a distance 2d apart, each on a circle of radius d:
    // M v^2 / d = M^2 / (2d)^2, so v = sqrt(M / (4d)). Set the pair's speed from the
    // star alone and it falls short, the orbit turns elliptical, and the swarms drain
    // the pair's energy until the stars merge.
    let system = m + swarm_mass;
    let v = (system / (4.0 * d)).sqrt();
    let stars = [([-d, 0.0], [0.0, -v]), ([d, 0.0], [0.0, v])];

    for (at, drift) in stars {
        bodies.push([at[0], at[1], m, 0.0]);
        vels.push(drift);
        for _ in 0..n_each {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = rng.unit().sqrt() * swarm_radius + 0.03;
            let mass = rng.range(0.4, 1.0) / n_each as f32 * swarm_mass;
            bodies.push([at[0] + r * a.cos(), at[1] + r * a.sin(), mass, 0.0]);
            // The star plus however much of the swarm lies inside this radius.
            let enclosed = m + swarm_mass * (r / swarm_radius).powi(2).min(1.0);
            let speed = (enclosed / r).sqrt();
            vels.push([drift[0] - a.sin() * speed, drift[1] + a.cos() * speed]);
        }
    }

    let n_ring = n - bodies.len();
    for _ in 0..n_ring {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let r = rng.range(1.0, 1.3);
        let mass = rng.range(0.4, 1.0) / n_ring.max(1) as f32 * ring_mass;
        bodies.push([r * a.cos(), r * a.sin(), mass, 0.0]);
        // From out here the pair reads as one mass at the centre.
        let speed = (TOTAL_MASS / r).sqrt();
        vels.push([-a.sin() * speed, a.cos() * speed]);
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
    fn the_binary_is_two_heavy_stars_orbiting_each_other_and_a_light_swarm() {
        let (bodies, vels) = generate(Scene::Binary, 2048, 5);
        let heavy: Vec<usize> = (0..bodies.len()).filter(|&i| bodies[i][2] > 0.1).collect();
        assert_eq!(heavy.len(), 2, "exactly two stars");
        let (a, b) = (heavy[0], heavy[1]);
        assert!((bodies[a][2] - bodies[b][2]).abs() < 1e-6, "equal masses");
        // Opposite positions, opposite velocities: a circular orbit about the origin.
        // (Only nearly opposite: the random swarm's net momentum is taken out of every
        // body at the end, and that shifts the stars by the same small amount.)
        assert!((bodies[a][0] + bodies[b][0]).abs() < 1e-5);
        assert!((vels[a][1] + vels[b][1]).abs() < 0.05);
        assert!(vels[a][1] * vels[b][1] < 0.0 && vels[a][1].abs() > 0.1, "the stars move apart");
        // The swarm is light: no single particle is within a hundredth of a star.
        let heaviest_light = bodies
            .iter()
            .map(|b| b[2])
            .filter(|&m| m <= 0.1)
            .fold(0.0f32, f32::max);
        assert!(heaviest_light < BINARY_STAR_MASS / 100.0);
    }

    #[test]
    fn the_system_is_a_sun_with_one_planet_going_round_it_slowly() {
        let (bodies, vels) = generate(Scene::System, 2048, 5);
        assert!((bodies[0][2] - SUN_MASS).abs() < 1e-6 && (bodies[1][2] - PLANET_MASS).abs() < 1e-6);
        let r = (bodies[1][0] * bodies[1][0] + bodies[1][1] * bodies[1][1]).sqrt();
        let v = ((vels[1][0] - vels[0][0]).powi(2) + (vels[1][1] - vels[0][1]).powi(2)).sqrt();
        let period = std::f32::consts::TAU * r / v;
        assert!(period > 8.0 && period < 20.0, "period {period}s");
        // Everything else is dust: no third body comes near the planet's mass.
        let third = bodies[2..].iter().map(|b| b[2]).fold(0.0f32, f32::max);
        assert!(third < PLANET_MASS / 20.0);
    }

    #[test]
    fn two_galaxies_splits_the_bodies_between_two_places() {
        let (bodies, _) = generate(Scene::TwoGalaxies, 2048, 5);
        let left = bodies.iter().filter(|b| b[0] < 0.0).count();
        assert!(left > 900 && left < 1150, "unbalanced: {left}");
    }
}
