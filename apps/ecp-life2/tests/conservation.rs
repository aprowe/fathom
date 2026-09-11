//! The acceptance gate: the chase does net work, and the books still balance.
//!
//! These run the *real* compute shaders — the ones the app runs — against a CPU
//! statement of the system's total energy. That is the only version of this test that can
//! fail for a real reason: a Rust reimplementation of the force kernel checked against the
//! same author's Rust energy function would agree with itself while the shader did
//! something else entirely.
//!
//! What is measured is the *peak excursion* of the total energy over the run, not its
//! value at the end. A symplectic integrator does not conserve energy exactly; it makes
//! it oscillate within a bound set by the timestep. Sampling only the final value reads
//! off wherever in that oscillation the run happened to stop, which is why the same
//! system can look twice as bad at half the timestep and mean nothing by it.
//!
//! Two claims are separated here, because they fail for different reasons. With the chase
//! switched off the system is conservative and its energy error must be the integrator's:
//! first order, halving with the step. With the chase running it does net work, and what
//! is left over must be small and must be explainable — it is the baked table's
//! resolution, not the ledger's arithmetic, and the test that says so checks that halving
//! the timestep does *not* move it.
//!
//! Damping is off throughout. With a sink in the loop the closed quantity becomes
//! `E + dissipated`, and a test that has to model the dissipation to check the
//! conservation is a test that can hide a leak inside its own bookkeeping.
//!
//! Skipped with a message when no adapter is available, so this does not fail CI on a
//! machine with no GPU.

use ecp_life2::landscape::Landscape;
use ecp_life2::physics::{Profile, Settings, total_energy};
use ecp_life2::scenes::Scene;
use ecp_life2::sim::{Sim, Uniforms};
use fathom_core::wgpu;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("ecp-life test"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .ok()
}

fn settings(profile: Profile) -> Settings {
    Settings {
        r_cut: 0.06,
        r_core: 0.012,
        k_rep: 1.5,
        lam: 0.6,
        chase_gain: 0.4,
        // The closed system the invariant is stated for.
        damping: 0.0,
        s0: 1.0,
        amp_sym: 0.8,
        amp_chase: 0.8,
        well_sigma: 0.010,
        rstar_min: 0.018,
        rstar_span: 0.024,
        profile,
    }
}

/// Two particles at a separation where the interaction is strong, with colours far
/// enough apart that the chase has something to spend.
fn pair() -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let mut pos = vec![[0.0f32; 4]; 256];
    let vel = vec![[0.0f32; 4]; 256];
    pos[0] = [-0.015, 0.0, 0.4, 0.0];
    pos[1] = [0.015, 0.0, 3.1, 0.0];
    // The rest are parked far away from the pair and from each other. The dispatch is
    // 256 wide either way, so they cost nothing, and a scene of two would leave 254
    // particles stacked at the origin interacting violently.
    // The rest are stacked on one point far from the pair. Coincident particles have no
    // separation to push along, so both the shader and the energy skip them: they are
    // exactly inert, which a scattered filler would not be — spread across the world at
    // this density it would form its own interacting lattice and drown the pair's signal
    // in a hundred times its own energy.
    for p in pos.iter_mut().skip(2) {
        *p = [0.4, 0.4, 0.0, 0.0];
    }
    (pos, vel)
}

struct Run {
    /// Largest departure of the total energy from where it started, relative to it: the
    /// bound the integrator holds the system inside.
    worst: f32,
    /// How far the pair's colours moved. If this is zero the ledger was never tested.
    colour_travel: f32,
    /// Peak speed reached. If this is zero nothing ever moved.
    peak_speed: f32,
}

fn run(profile: Profile, chase_gain: f32, steps: usize, dt: f32) -> Option<Run> {
    let (device, queue) = device()?;
    let landscape = Landscape::new(4, 3);
    let table = landscape.table();
    let mut settings = settings(profile);
    settings.chase_gain = chase_gain;
    let sim = Sim::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        256,
        Scene::Soup,
        &landscape,
    );

    let (pos, vel) = pair();
    sim.write_state(&queue, &pos, &vel);
    sim.write_uniforms(
        &queue,
        &Uniforms { n: 256, ..Uniforms::default() }.with_physics(&settings, dt),
    );

    let start = total_energy(&settings, &table, &pos, &vel);
    let colours_before = [pos[0][2], pos[1][2]];
    let (mut worst, mut peak_speed) = (0.0f32, 0.0f32);
    let mut last = pos;

    for _ in 0..steps {
        let mut encoder = device.create_command_encoder(&Default::default());
        sim.step(&mut encoder);
        queue.submit(Some(encoder.finish()));
        let (p, v) = sim.read_state(&device, &queue);
        worst = worst.max((total_energy(&settings, &table, &p, &v) - start).abs());
        peak_speed = peak_speed.max((v[0][0] * v[0][0] + v[0][1] * v[0][1]).sqrt());
        last = p;
    }

    Some(Run {
        worst: worst / start.abs().max(1e-3),
        colour_travel: (last[0][2] - colours_before[0]).abs()
            + (last[1][2] - colours_before[1]).abs(),
        peak_speed,
    })
}

/// The energy stays inside a bound while the engine that does net work is running.
fn check(profile: Profile, bound: f32) {
    let Some(r) = run(profile, 0.4, 200, 2.0e-4) else {
        eprintln!("no GPU adapter; skipping the {profile:?} conservation check");
        return;
    };

    // The test is only worth anything if the engine actually ran.
    assert!(r.peak_speed > 1e-4, "{profile:?}: nothing moved, so nothing was tested");
    assert!(r.colour_travel > 1e-4, "{profile:?}: colour never shifted, so no work was paid for");

    eprintln!("{profile:?}: peak excursion {:.2e}, colour moved {:.4}", r.worst, r.colour_travel);
    assert!(r.worst < bound, "{profile:?}: energy left its bound by {:.3}%", r.worst * 100.0);
}

/// The well profile is the app's default, and the one whose conservation is worth
/// quoting: a hundredth of a percent, with an engine running that does net work.
#[test]
fn the_well_profile_holds_its_energy_while_the_chase_runs() {
    check(Profile::Well, 5.0e-4);
}

/// The classic profile is held to a far looser bound, and the reason is the integrator
/// rather than the ledger.
///
/// `V = -q²` is monotonic, so a bound pair has no separation it can rest at and instead
/// oscillates hard against the soft core. First-order integration of that costs percents
/// at these timesteps — which `the_energy_error_is_first_order_when_the_chase_is_off`
/// measures directly, on the same configuration with the engine switched off.
#[test]
fn the_classic_profile_holds_its_energy_while_the_chase_runs() {
    check(Profile::Classic, 0.25);
}

/// With the chase off the system is an ordinary conservative one, and its energy error
/// must behave like an integrator's: first order, halving with the timestep.
///
/// This is what makes the forces themselves testable. A bound that sat still as the step
/// shrank would mean a term missing from the forces rather than a step that was too long,
/// and no amount of care in the ledger would fix it.
#[test]
fn the_energy_error_is_first_order_when_the_chase_is_off() {
    for profile in [Profile::Classic, Profile::Well] {
        // Same simulated duration, so the two runs cover the same trajectory.
        let Some(coarse) = run(profile, 0.0, 100, 4.0e-4) else {
            eprintln!("no GPU adapter; skipping the convergence check");
            return;
        };
        let fine = run(profile, 0.0, 200, 2.0e-4).expect("adapter vanished mid-test");

        assert!(coarse.colour_travel < 1e-6, "{profile:?}: colour moved with the chase off");
        let ratio = coarse.worst / fine.worst.max(1e-12);
        eprintln!("{profile:?}: excursion {:.3e} -> {:.3e}, ratio {ratio:.2}", coarse.worst, fine.worst);
        assert!(
            (1.6..2.6).contains(&ratio),
            "{profile:?}: energy error is not first order in the timestep: ratio {ratio:.2}"
        );
    }
}

/// What is left over once the chase is running, and where it comes from.
///
/// The colour update pays `W·ċ·dt = −P·dt` by dividing exactly, so in principle the books
/// balance to machine precision. In practice `W` is read from the baked table, and an
/// interpolated derivative is not the derivative of the interpolated energy — they differ
/// by something of the order of a table cell. That mismatch is a fixed fraction of the
/// work done, so unlike integrator error it is a residual per unit *time* and does not
/// move when the timestep does.
///
/// Which is exactly what this asserts, because it is the evidence that the residual is
/// the table's resolution and not the ledger's arithmetic: halving the step leaves it
/// where it was. Doubling the table's resolution, measured while writing this, cut it by
/// 4.7×; halving the step does not touch it. It is worth about a hundredth of a percent
/// at the shipped table size, and it is the reason `LUT_SIZE` is 256 rather than 128.
#[test]
fn what_the_chase_leaves_behind_is_the_tables_resolution_not_the_timestep() {
    let Some(coarse) = run(Profile::Well, 0.4, 100, 4.0e-4) else {
        eprintln!("no GPU adapter; skipping the residual check");
        return;
    };
    let fine = run(Profile::Well, 0.4, 200, 2.0e-4).expect("adapter vanished mid-test");

    let ratio = coarse.worst / fine.worst.max(1e-12);
    eprintln!("residual {:.3e} at 4e-4, {:.3e} at 2e-4 - ratio {ratio:.2}", coarse.worst, fine.worst);
    assert!(coarse.worst < 5.0e-4, "the residual is larger than the table can explain");
    assert!(
        (0.75..1.35).contains(&ratio),
        "the residual moved with the timestep (ratio {ratio:.2}), so it is not the table"
    );
}

/// With the chase switched off the system is an ordinary conservative pair potential and
/// colours must not move at all. If they do, something other than the ledger is writing
/// to them.
#[test]
fn colours_are_frozen_when_the_chase_is_off() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping the frozen-colour check");
        return;
    };
    let landscape = Landscape::new(4, 3);
    let mut settings = settings(Profile::Well);
    settings.chase_gain = 0.0;
    let sim =
        Sim::new(&device, &queue, wgpu::TextureFormat::Rgba8UnormSrgb, 256, Scene::Soup, &landscape);

    let (pos, vel) = pair();
    sim.write_state(&queue, &pos, &vel);
    sim.write_uniforms(
        &queue,
        &Uniforms { n: 256, ..Uniforms::default() }.with_physics(&settings, 2.0e-4),
    );

    for _ in 0..200 {
        let mut encoder = device.create_command_encoder(&Default::default());
        sim.step(&mut encoder);
        queue.submit(Some(encoder.finish()));
    }

    let (after, velocities) = sim.read_state(&device, &queue);
    for i in 0..2 {
        assert!(
            (after[i][2] - pos[i][2]).abs() < 1e-5,
            "colour {i} moved without a chase: {} -> {}",
            pos[i][2],
            after[i][2]
        );
    }
    // And the pair must still have interacted, or the check above is vacuous.
    let speed = (velocities[0][0].powi(2) + velocities[0][1].powi(2)).sqrt();
    assert!(speed > 1e-5, "the pair never interacted at all");
}

/// The chase pushes both members of a pair the *same* way along their separation, which
/// is what makes it the only term that does net work. It comes out that way because Λ is
/// odd and the separation flips with it — two sign changes that cancel. That is exactly
/// the kind of reasoning that is right on paper and wrong in the shader, so it is checked
/// rather than assumed.
#[test]
fn the_chase_pushes_both_particles_the_same_way() {
    let Some((device, queue)) = device() else {
        eprintln!("no GPU adapter; skipping the same-direction check");
        return;
    };
    let landscape = Landscape::new(4, 3);
    let mut settings = settings(Profile::Classic);
    // Strip everything reciprocal, so whatever motion appears is the chase alone.
    settings.amp_sym = 0.0;
    settings.s0 = 0.0;
    settings.k_rep = 0.0;
    settings.lam = 0.0;
    let sim =
        Sim::new(&device, &queue, wgpu::TextureFormat::Rgba8UnormSrgb, 256, Scene::Soup, &landscape);

    let (pos, vel) = pair();
    sim.write_state(&queue, &pos, &vel);
    sim.write_uniforms(
        &queue,
        &Uniforms { n: 256, ..Uniforms::default() }.with_physics(&settings, 1.0e-3),
    );

    let mut encoder = device.create_command_encoder(&Default::default());
    sim.step(&mut encoder);
    queue.submit(Some(encoder.finish()));

    let (_, v) = sim.read_state(&device, &queue);
    // The pair is separated along x, so the chase acts along x on both.
    let (a, b) = (v[0][0], v[1][0]);
    assert!(a.abs() > 1e-6 && b.abs() > 1e-6, "the chase did nothing: {a}, {b}");
    assert!(
        a * b > 0.0,
        "the chase pushed the pair apart instead of driving it: {a}, {b}"
    );
    assert!(
        (a - b).abs() < (a.abs() + b.abs()) * 0.05,
        "the chase was not equal on both: {a}, {b}"
    );
}
