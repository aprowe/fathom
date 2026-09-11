//! ECP-Life: particle life with relational colour energy, on the GPU.
//!
//! Each particle carries a continuous colour on a circle. How two particles interact is
//! a smooth surface over the pair of their colours — the *interaction landscape* — and
//! that surface splits into an even part, which is an ordinary reciprocal pair force,
//! and an odd part, which is not. The odd part pushes both members of a pair the same
//! way along their separation. It is the only thing in the system that does net work.
//!
//! That work is paid for, locally and in the same step, out of energy stored in the
//! colour mismatch between neighbours. Colours shift to balance the books; when the pair
//! geometry reverses, the flow reverses and mismatch regenerates. Conservation is
//! structural — it falls out of the colour update rule rather than being restored by
//! rescaling afterwards — which is why `tests/conservation.rs` can check it as an
//! invariant instead of the app having to enforce it as a constraint.
//!
//! This file is the app's *behaviour*: what it declares, what the controls mean, and what
//! the commands do. `sim.rs` is the plumbing, `physics.rs` the settings and the energy,
//! `landscape.rs` the interaction surface, `presets.rs` the places worth starting from.

pub mod landscape;
pub mod physics;
pub mod presets;
pub mod scenes;
pub mod sim;

use std::f32::consts::TAU;

use fathom_core::{
    App, AppDescriptor, CommandCtx, CommandDef, DrawCtx, EventCtx, KeyEvent, Params, SetupCtx,
    UpdateCtx, wgpu,
};

use landscape::Landscape;
use physics::{Profile, Settings};
use scenes::Scene;
use sim::{Sim, Uniforms};

fathom_core::params! {
    // -- Physics -------------------------------------------------------------------
    // The cutoff is the unit of length: every other distance below is a fraction of it,
    // because "a third of the way out" is how anyone actually reasons about these, and a
    // slider reading 0.006 is not.
    R_CUT       => fathom_core::ParamDef::float("cutoff", "Cutoff R", 0.02, 0.006, 0.06).group("Physics"),
    CORE_FRAC   => fathom_core::ParamDef::float("core", "Core radius", 0.30, 0.05, 0.70).group("Physics").advanced(),
    K_REP       => fathom_core::ParamDef::float("kRep", "Core stiffness", 1.5, 0.0, 8.0).group("Physics").advanced(),
    // Also the reservoir size. A larger store means a larger colour gradient, so the same
    // power moves colour less far: raise it to make colour the slow variable without
    // changing how much energy is flowing.
    LAM         => fathom_core::ParamDef::float("lam", "Colour store", 0.6, 0.02, 4.0).group("Physics").advanced(),
    // The chase is a force, and it is divided by the cutoff like every other force here,
    // so this reads as "how hard the chase pushes relative to the pair bond" at any
    // cutoff. Around 0.4 the chase is a bit under half the reciprocal force: enough to
    // keep the system churning rather than settling into static clumps, well short of
    // overpowering the bond that holds pairs together in the first place.
    CHASE_GAIN  => fathom_core::ParamDef::float("chase", "Chase gain", 0.4, 0.0, 2.0).group("Physics"),
    // Strongly overdamped is the interesting regime — particles then follow the force
    // field rather than ringing through it — so the useful part of this range is high.
    DAMPING     => fathom_core::ParamDef::float("damping", "Damping", 600.0, 0.0, 3000.0).group("Physics"),
    TIMESCALE   => fathom_core::ParamDef::float("timescale", "Time scale", 0.15, 0.0, 1.0).group("Physics"),
    SUBSTEPS    => fathom_core::ParamDef::int("substeps", "Substeps", 4, 1, 12).group("Physics").advanced(),

    // -- Landscape -----------------------------------------------------------------
    PROFILE     => fathom_core::ParamDef::choice("profile", "Radial profile", 1, Profile::LABELS).group("Landscape"),
    // Baseline attraction is not optional. Without it, the even and odd parts of the
    // landscape can put a pair's attraction near zero exactly where its chase peaks: the
    // pair loses its grip at the moment it is being pushed hardest and self-propels out
    // of range, never to interact again.
    S0          => fathom_core::ParamDef::float("s0", "Baseline attraction", 0.9, 0.0, 2.0).group("Landscape").advanced(),
    AMP_SYM     => fathom_core::ParamDef::float("ampSym", "Pair strength", 0.7, 0.0, 2.0).group("Landscape"),
    AMP_CHASE   => fathom_core::ParamDef::float("ampChase", "Chase strength", 0.7, 0.0, 2.0).group("Landscape"),
    // How many harmonics the landscape is allowed. Low is smooth and broad; high is
    // finely divided, so colours a little apart can want quite different things.
    HARMONICS   => fathom_core::ParamDef::int("harmonics", "Landscape detail", 3, 1, landscape::MAX_HARMONICS).group("Landscape"),
    WELL_SIGMA  => fathom_core::ParamDef::float("wellSigma", "Well width", 0.16, 0.03, 0.50).group("Landscape").advanced(),
    RSTAR_MIN   => fathom_core::ParamDef::float("rstarMin", "Closest liking", 0.30, 0.05, 0.90).group("Landscape").advanced(),
    RSTAR_SPAN  => fathom_core::ParamDef::float("rstarSpan", "Liking spread", 0.45, 0.0, 0.90).group("Landscape").advanced(),

    // -- Render --------------------------------------------------------------------
    POINT_SIZE  => fathom_core::ParamDef::float("pointSize", "Point size", 1.5, 0.5, 6.0).group("Render"),
    BRIGHTNESS  => fathom_core::ParamDef::float("brightness", "Brightness", 1.2, 0.2, 4.0).group("Render"),
    COLOR_MODE  => fathom_core::ParamDef::choice("colorMode", "Colour by", 0, &["Colour", "Speed", "Colour gradient", "Flat"]).group("Render"),

    // -- Instruments ---------------------------------------------------------------
    OVERLAY     => fathom_core::ParamDef::toggle("overlay", "Show instruments", true).group("Instruments"),
    PROBE_A     => fathom_core::ParamDef::float("probeA", "Probe colour A", 0.0, 0.0, TAU).group("Instruments"),
    PROBE_B     => fathom_core::ParamDef::float("probeB", "Probe colour B", TAU * 0.5, 0.0, TAU).group("Instruments"),
}

/// Particle counts offered by the `count` command. All are multiples of the 256-wide
/// workgroup, so no dispatch ends with a half-idle group.
const COUNTS: [u32; 4] = [4_096, 16_384, 65_536, 131_072];
const COUNT_LABELS: &[&str] = &["4,096", "16,384", "65,536", "131,072"];
const DEFAULT_COUNT_INDEX: u32 = 1;

const COMMANDS: &[CommandDef] = &[
    CommandDef::choice("preset", "Preset", presets::LABELS).group("Simulation"),
    CommandDef::choice("scene", "Start from", Scene::LABELS).group("Simulation"),
    CommandDef::choice("count", "Particles", COUNT_LABELS)
        .group("Simulation")
        .initial(DEFAULT_COUNT_INDEX),
    CommandDef::button("randomize", "New landscape").group("Simulation"),
    CommandDef::button("reseed", "Reseed particles").group("Simulation"),
    CommandDef::button("reset", "Reset").group("Simulation"),
];

pub struct EcpLife {
    sim: Sim,
    landscape: Landscape,
    /// The landscape depends on a parameter, not only on a command, so the value it was
    /// baked at is remembered and compared: rebaking every frame would be wasteful, and
    /// rebaking only on command would leave the detail slider doing nothing.
    baked_harmonics: u32,
    landscape_seed: u64,
    particle_seed: u64,
}

/// Turn the panel's fractions and choices into the absolute quantities the shader wants.
fn settings(params: Params<'_>) -> Settings {
    let r_cut = params.float(R_CUT);
    // The well has to fit inside the cutoff. Two independent sliders can ask for a
    // preferred separation beyond it, where the taper has already killed the bond and the
    // pair would feel a chase with nothing holding on.
    let span = params.float(RSTAR_SPAN).min(0.95 - params.float(RSTAR_MIN)).max(0.0);
    Settings {
        r_cut,
        r_core: params.float(CORE_FRAC) * r_cut,
        k_rep: params.float(K_REP),
        lam: params.float(LAM),
        chase_gain: params.float(CHASE_GAIN),
        damping: params.float(DAMPING),
        s0: params.float(S0),
        amp_sym: params.float(AMP_SYM),
        amp_chase: params.float(AMP_CHASE),
        well_sigma: params.float(WELL_SIGMA) * r_cut,
        rstar_min: params.float(RSTAR_MIN) * r_cut,
        rstar_span: span * r_cut,
        profile: Profile::from_index(params.int(PROFILE)),
    }
}

impl EcpLife {
    fn uniforms(
        &self,
        params: Params<'_>,
        camera: &fathom_core::Camera,
        viewport: fathom_core::Viewport,
        dt: f32,
    ) -> Uniforms {
        let cam = camera.uniform(viewport);
        Uniforms {
            center: cam.center,
            scale: cam.scale,
            viewport: [viewport.width as f32, viewport.height as f32],
            point_size: params.float(POINT_SIZE) * viewport.dpr.max(1.0),
            brightness: params.float(BRIGHTNESS),
            probe_a: params.float(PROBE_A),
            probe_b: params.float(PROBE_B),
            n: self.sim.n,
            color_mode: params.int(COLOR_MODE),
            overlay: u32::from(params.toggle(OVERLAY)),
            ..Uniforms::default()
        }
        .with_physics(&settings(params), dt)
    }

    /// Bake a new landscape and hand it to the GPU.
    fn rebake(&mut self, queue: &wgpu::Queue, seed: u64, harmonics: u32) {
        self.landscape = Landscape::new(seed, harmonics);
        self.landscape_seed = seed;
        self.baked_harmonics = harmonics;
        self.sim.write_landscape(queue, &self.landscape);
    }

    /// The landscape a preset or a randomise leaves behind, for tests and for anything
    /// that wants to check the picture against the physics.
    pub fn landscape(&self) -> &Landscape {
        &self.landscape
    }
}

impl App for EcpLife {
    fn describe() -> AppDescriptor {
        AppDescriptor { name: "ECP-Life", params: SCHEMA, commands: COMMANDS }
    }

    fn setup(ctx: &mut SetupCtx<'_>) -> Self {
        let harmonics = ctx.params.int(HARMONICS);
        let landscape = Landscape::new(1, harmonics);
        let sim = Sim::new(
            &ctx.gpu.device,
            &ctx.gpu.queue,
            ctx.gpu.format,
            COUNTS[DEFAULT_COUNT_INDEX as usize],
            Scene::Soup,
            &landscape,
        );
        Self {
            sim,
            landscape,
            baked_harmonics: harmonics,
            landscape_seed: 1,
            particle_seed: 1,
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx<'_>) {
        let harmonics = ctx.params.int(HARMONICS);
        if harmonics != self.baked_harmonics {
            let seed = self.landscape_seed;
            self.rebake(&ctx.gpu.queue, seed, harmonics);
        }

        // A stall must not be allowed to advance the simulation by however long it
        // lasted: the timestep is what keeps the stiff core stable, and one enormous
        // frame is enough to throw every overlapping pair to infinity.
        let substeps = ctx.params.int(SUBSTEPS).max(1);
        let frame = ctx.dt.min(1.0 / 30.0) * ctx.params.float(TIMESCALE);
        let dt = frame / substeps as f32;

        let uniforms = self.uniforms(ctx.params, ctx.camera, ctx.viewport, dt);
        self.sim.write_uniforms(&ctx.gpu.queue, &uniforms);

        // The simulation gets its own encoder: `draw` receives the frame's encoder, and
        // keeping the physics separate means a paused frame submits no compute work.
        let mut encoder = ctx
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ecp step") });
        for _ in 0..substeps {
            self.sim.step(&mut encoder);
        }
        ctx.gpu.queue.submit(Some(encoder.finish()));
    }

    fn draw(&mut self, ctx: &mut DrawCtx<'_>) {
        // Rewritten here as well as in `update` so panning, zooming and the instruments
        // still respond while the simulation is paused.
        let uniforms = self.uniforms(ctx.params, ctx.camera, ctx.viewport, 0.0);
        self.sim.write_uniforms(&ctx.gpu.queue, &uniforms);

        self.sim.render(
            &ctx.gpu.device,
            ctx.encoder,
            ctx.target,
            ctx.viewport.width,
            ctx.viewport.height,
            ctx.params.toggle(OVERLAY),
        );
    }

    fn key_pressed(&mut self, e: &KeyEvent, ctx: &mut EventCtx<'_>) {
        match e.key.as_str() {
            "r" => {
                self.particle_seed = self.particle_seed.wrapping_add(1);
                let (scene, n) = (self.sim.scene, self.sim.n);
                self.sim.reseed(&ctx.gpu.device, scene, n, self.particle_seed);
            }
            "n" => {
                let seed = self.landscape_seed.wrapping_add(1);
                let harmonics = self.baked_harmonics;
                self.rebake(&ctx.gpu.queue, seed, harmonics);
            }
            _ => {}
        }
    }

    fn command(&mut self, ctx: &mut CommandCtx<'_>) {
        let (scene, n) = (self.sim.scene, self.sim.n);

        match ctx.name {
            "preset" => {
                let preset = presets::from_index(ctx.index());
                preset.apply(ctx);
                // A preset is a landscape as much as it is a set of numbers: the same
                // sliders over different terrain do something else entirely.
                let harmonics = preset.harmonics;
                self.rebake(&ctx.gpu.queue, preset.seed, harmonics);
                self.sim.reseed(&ctx.gpu.device, preset.scene, n, self.particle_seed);
            }
            "scene" => {
                let scene = Scene::from_index(ctx.index());
                self.sim.reseed(&ctx.gpu.device, scene, n, self.particle_seed);
            }
            "count" => {
                let n = COUNTS[(ctx.index() as usize).min(COUNTS.len() - 1)];
                self.sim.reseed(&ctx.gpu.device, scene, n, self.particle_seed);
            }
            "randomize" => {
                let seed = self.landscape_seed.wrapping_add(1);
                let harmonics = self.baked_harmonics;
                self.rebake(&ctx.gpu.queue, seed, harmonics);
            }
            "reseed" => {
                self.particle_seed = self.particle_seed.wrapping_add(1);
                self.sim.reseed(&ctx.gpu.device, scene, n, self.particle_seed);
            }
            "reset" => {
                self.sim.reseed(&ctx.gpu.device, scene, n, self.particle_seed);
                *ctx.camera = fathom_core::Camera::default();
            }
            other => log::warn!("ecp-life: unknown command {other}"),
        }
    }
}

/// Start the app in a canvas. The browser runs the same egui shell the desktop binary
/// does — eframe draws egui to the canvas and wgpu talks to WebGPU — so no part of the
/// interface is HTML.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement) -> Result<(), wasm_bindgen::JsValue> {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    let _ = console_log::init_with_level(log::Level::Warn);
    fathom_shell::run_web::<EcpLife>(canvas).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use fathom_core::ParamBlock;

    fn defaults() -> ParamBlock {
        ParamBlock::from_defaults(SCHEMA)
    }

    #[test]
    fn the_defaults_resolve_into_a_usable_configuration() {
        let block = defaults();
        let s = settings(Params::new(SCHEMA, &block));
        assert!(s.r_core < s.r_cut, "the core fills the whole interaction range");
        assert!(s.rstar_min + s.rstar_span <= s.r_cut, "the well can sit outside the cutoff");
        assert!(s.cells() >= 4 && s.cells() <= sim::MAX_CELLS_1D);
    }

    /// Two sliders can between them ask for a preferred separation past the cutoff, where
    /// the taper has already killed the bond. The chase would then act on a pair with
    /// nothing holding it together — the failure the baseline attraction exists to
    /// prevent, arriving by a different door.
    #[test]
    fn the_well_is_kept_inside_the_cutoff_however_the_sliders_are_set() {
        let mut block = defaults();
        block.set_f32(RSTAR_MIN, 0.9);
        block.set_f32(RSTAR_SPAN, 0.9);
        let s = settings(Params::new(SCHEMA, &block));
        assert!(s.rstar_min + s.rstar_span <= s.r_cut, "well escapes the cutoff");
    }

    /// A group whose every control is filed away shows as a heading with nothing under
    /// it but a disclosure, which reads as a bug rather than as tidiness.
    #[test]
    fn every_group_keeps_something_visible() {
        let mut groups: Vec<&str> = Vec::new();
        for p in SCHEMA {
            if !groups.contains(&p.group) {
                groups.push(p.group);
            }
        }
        for group in groups {
            let shown = SCHEMA.iter().filter(|p| p.group == group && !p.advanced).count();
            assert!(shown > 0, "{group} has nothing to show until it is expanded");
        }
    }

    /// The point of the disclosure is that the panel opens short. If the tail ever stops
    /// being most of the schema, the fold has stopped earning its complexity.
    #[test]
    fn most_of_the_schema_starts_folded_away_or_the_panel_is_long_again() {
        let advanced = SCHEMA.iter().filter(|p| p.advanced).count();
        assert!(advanced >= 8, "only {advanced} controls are filed away");
        assert!(advanced < SCHEMA.len() / 2, "more is hidden than shown");
    }

    #[test]
    fn every_particle_count_fills_whole_workgroups() {
        for n in COUNTS {
            assert_eq!(n % sim::WORKGROUP, 0, "{n} leaves a partly idle dispatch");
        }
        assert_eq!(COUNTS.len(), COUNT_LABELS.len());
    }

    #[test]
    fn the_grid_stays_within_its_allocation_across_the_whole_cutoff_range() {
        let mut block = defaults();
        for r in [0.006f32, 0.01, 0.02, 0.04, 0.06] {
            block.set_f32(R_CUT, r);
            let s = settings(Params::new(SCHEMA, &block));
            assert!(s.cells() <= sim::MAX_CELLS_1D, "cutoff {r} overruns the grid");
            assert!(1.0 / s.cells() as f32 >= r, "cutoff {r} outruns its cell");
        }
    }
}
