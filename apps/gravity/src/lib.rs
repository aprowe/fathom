//! A 2D N-body gravity simulation — the worked example for the fathom framework.
//!
//! Every body pulls on every other body, exactly, on the GPU. The app itself is only
//! this file: parameters, the openFrameworks-shaped lifecycle, and input. The framework
//! supplies the window, the surface, the clock, the camera, the panel and the two
//! targets; `sim.rs` supplies the pipelines.

pub mod scenes;
pub mod sim;

use fathom_core::{
    App, AppDescriptor, CommandCtx, CommandDef, DrawCtx, EventCtx, KeyEvent, MouseEvent, SetupCtx,
    UpdateCtx, wgpu,
};

use scenes::Scene;
use sim::{Sim, Uniforms};

fathom_core::params! {
    G           => fathom_core::ParamDef::float("g", "Gravity", 1.0, 0.0, 4.0).group("Physics"),
    // The mean spacing in a unit disc of 8k bodies is about 0.02, so a softening of
    // that size lets the heavy core scatter violently off its nearest neighbours: it
    // random-walks and drags the whole disc out of frame within a minute. Keeping the
    // default a few times the spacing is what holds the scene together.
    SOFTENING   => fathom_core::ParamDef::float("softening", "Softening", 0.06, 0.002, 0.3).group("Physics"),
    TIMESCALE   => fathom_core::ParamDef::float("timescale", "Time scale", 1.0, 0.0, 3.0).group("Physics"),
    // What the left button does inside the view. The right button always launches, so
    // both gestures are reachable without visiting the panel.
    MOUSE       => fathom_core::ParamDef::choice("mouse", "Left drag", 0, &["Pull", "Launch"]).group("Mouse"),
    WELL        => fathom_core::ParamDef::float("well", "Pull strength", 9.0, 0.0, 40.0).group("Mouse"),
    // As a fraction of the whole scene's mass: the top of the range is a body that
    // outweighs both stars of the binary put together.
    LAUNCH_MASS => fathom_core::ParamDef::float("launchMass", "Launch mass", 0.1, 0.002, 0.8).group("Mouse"),
    POINT_SIZE  => fathom_core::ParamDef::float("pointSize", "Point size", 1.6, 0.5, 6.0).group("Render"),
    BRIGHTNESS  => fathom_core::ParamDef::float("brightness", "Brightness", 1.3, 0.2, 4.0).group("Render"),
    TRAILS      => fathom_core::ParamDef::toggle("trails", "Trails", true).group("Render"),
    TRAIL_FADE  => fathom_core::ParamDef::float("trailFade", "Trail length", 0.90, 0.50, 0.99).group("Render"),
    COLOR_MODE  => fathom_core::ParamDef::choice("colorMode", "Colour by", 0, &["Speed", "Mass", "Flat"]).group("Render"),
}

/// Body counts offered by the `count` command. All are multiples of the 256-wide
/// workgroup, so no dispatch ends with a half-idle group.
const COUNTS: [u32; 4] = [2_048, 8_192, 20_480, 49_152];
const COUNT_LABELS: &[&str] = &["2,048", "8,192", "20,480", "49,152"];
const DEFAULT_COUNT_INDEX: u32 = 1;
const DEFAULT_SCENE: Scene = Scene::Binary;

/// World units of velocity per world unit of drag. One: pull a body out by the distance
/// it should cover in a second.
const LAUNCH_GAIN: f32 = 1.0;

const COMMANDS: &[CommandDef] = &[
    CommandDef::choice("scene", "Scene", Scene::LABELS)
        .group("Simulation")
        .initial(DEFAULT_SCENE as u32),
    CommandDef::choice("count", "Bodies", COUNT_LABELS)
        .group("Simulation")
        .initial(DEFAULT_COUNT_INDEX),
    CommandDef::button("reset", "Reset").group("Simulation"),
    CommandDef::button("randomize", "Randomise").group("Simulation"),
];

pub struct Gravity {
    sim: Sim,
    /// Where the drag-to-attract well is, and whether it is on. Matches the shader's
    /// `well` uniform: xy position, z strength, w enabled.
    well: [f32; 4],
    /// A body being aimed: where it will start and where the drag has got to, both in
    /// world space. `None` when nothing is being aimed.
    launch: Option<([f32; 2], [f32; 2])>,
    /// Trails have to be cleared the moment they are switched off, or the last frame
    /// of streaks sits there forever.
    trails_were_on: bool,
    next_seed: u64,
}

/// The velocity a drag from `from` to `to` gives a launched body.
///
/// The drag *is* the velocity, drawn out from the body: where the line ends is where
/// the body will be a second later. A slingshot (pull back to fire forward) is the
/// other convention, and it is worse here, because what you are placing is a planet
/// and the line you draw should be the direction it goes.
pub fn launch_velocity(from: [f32; 2], to: [f32; 2]) -> [f32; 2] {
    [(to[0] - from[0]) * LAUNCH_GAIN, (to[1] - from[1]) * LAUNCH_GAIN]
}

impl Gravity {
    fn uniforms(
        &self,
        params: fathom_core::Params<'_>,
        camera: &fathom_core::Camera,
        viewport: fathom_core::Viewport,
        dt: f32,
    ) -> Uniforms {
        let cam = camera.uniform(viewport);
        Uniforms {
            center: cam.center,
            scale: cam.scale,
            well: self.well,
            viewport: [viewport.width as f32, viewport.height as f32],
            dt,
            g: params.float(G),
            softening: params.float(SOFTENING),
            point_size: params.float(POINT_SIZE) * viewport.dpr.max(1.0),
            brightness: params.float(BRIGHTNESS),
            n: self.sim.n,
            color_mode: params.int(COLOR_MODE),
            launch_mass: params.float(LAUNCH_MASS),
            launch_on: self.launch.is_some() as u32 as f32,
            _pad: 0,
            launch: self
                .launch
                .map(|(a, b)| [a[0], a[1], b[0], b[1]])
                .unwrap_or_default(),
        }
    }
}

impl App for Gravity {
    fn describe() -> AppDescriptor {
        AppDescriptor { name: "Gravity", params: SCHEMA, commands: COMMANDS }
    }

    fn setup(ctx: &mut SetupCtx<'_>) -> Self {
        let sim = Sim::new(
            &ctx.gpu.device,
            &ctx.gpu.queue,
            ctx.gpu.format,
            COUNTS[DEFAULT_COUNT_INDEX as usize],
            DEFAULT_SCENE,
        );
        let mut app = Self {
            sim,
            well: [0.0; 4],
            launch: None,
            trails_were_on: ctx.params.toggle(TRAILS),
            next_seed: 1,
        };
        app.sim.reseed(
            &ctx.gpu.device,
            &ctx.gpu.queue,
            DEFAULT_SCENE,
            COUNTS[DEFAULT_COUNT_INDEX as usize],
            app.next_seed,
        );
        app
    }

    fn update(&mut self, ctx: &mut UpdateCtx<'_>) {
        let trails = ctx.params.toggle(TRAILS);
        if trails != self.trails_were_on {
            self.sim.clear_trails();
            self.trails_were_on = trails;
        }

        self.well[2] = ctx.params.float(WELL);
        let dt = ctx.dt * ctx.params.float(TIMESCALE);

        let uniforms = self.uniforms(ctx.params, ctx.camera, ctx.viewport, dt);
        self.sim.write_uniforms(&ctx.gpu.queue, &uniforms);

        // The simulation gets its own encoder: `draw` receives the frame's encoder, and
        // keeping the physics separate means a paused frame submits no compute work.
        let mut encoder = ctx
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gravity step") });
        self.sim.step(&mut encoder);
        ctx.gpu.queue.submit(Some(encoder.finish()));
    }

    fn draw(&mut self, ctx: &mut DrawCtx<'_>) {
        // Rewritten here as well as in `update` so panning and zooming still respond
        // while the simulation is paused.
        let uniforms = self.uniforms(ctx.params, ctx.camera, ctx.viewport, 0.0);
        self.sim.write_uniforms(&ctx.gpu.queue, &uniforms);

        self.sim.render(
            &ctx.gpu.device,
            ctx.encoder,
            ctx.target,
            ctx.viewport.width,
            ctx.viewport.height,
            ctx.params.toggle(TRAILS),
            ctx.params.float(TRAIL_FADE),
        );
    }

    fn mouse_pressed(&mut self, e: &MouseEvent, ctx: &mut EventCtx<'_>) {
        // Shift-drag is the framework's pan gesture, so both of these stay out of its
        // way; so is the middle button.
        if e.shift || e.button == 1 {
            return;
        }
        let world = ctx.camera.screen_to_world(e.x, e.y, ctx.viewport);
        let launching = e.button == 2 || ctx.params.int(MOUSE) == 1;
        if launching {
            self.launch = Some((world, world));
        } else {
            self.well = [world[0], world[1], ctx.params.float(WELL), 1.0];
        }
    }

    fn mouse_dragged(&mut self, e: &MouseEvent, ctx: &mut EventCtx<'_>) {
        let world = ctx.camera.screen_to_world(e.x, e.y, ctx.viewport);
        if let Some((_, to)) = &mut self.launch {
            *to = world;
        } else if self.well[3] > 0.5 {
            self.well[0] = world[0];
            self.well[1] = world[1];
        }
    }

    fn mouse_released(&mut self, _e: &MouseEvent, ctx: &mut EventCtx<'_>) {
        self.well[3] = 0.0;
        if let Some((from, to)) = self.launch.take() {
            let mass = ctx.params.float(LAUNCH_MASS);
            self.sim.launch(&ctx.gpu.queue, from, mass, launch_velocity(from, to));
        }
    }

    fn key_pressed(&mut self, e: &KeyEvent, ctx: &mut EventCtx<'_>) {
        if e.key == "r" {
            self.next_seed = self.next_seed.wrapping_add(1);
            let (scene, n) = (self.sim.scene, self.sim.n);
            self.sim.reseed(&ctx.gpu.device, &ctx.gpu.queue, scene, n, self.next_seed);
        }
    }

    fn command(&mut self, ctx: &mut CommandCtx<'_>) {
        let (device, queue) = (&ctx.gpu.device, &ctx.gpu.queue);
        let (scene, n, seed) = (self.sim.scene, self.sim.n, self.sim.seed);

        match ctx.name {
            "scene" => {
                let scene = Scene::from_index(ctx.index());
                self.sim.reseed(device, queue, scene, n, seed);
            }
            "count" => {
                let n = COUNTS[(ctx.index() as usize).min(COUNTS.len() - 1)];
                self.sim.reseed(device, queue, scene, n, seed);
            }
            "reset" => {
                self.sim.reseed(device, queue, scene, n, seed);
                *ctx.camera = fathom_core::Camera::default();
            }
            "randomize" => {
                self.next_seed = self.next_seed.wrapping_add(1);
                self.sim.reseed(device, queue, scene, n, self.next_seed);
            }
            other => log::warn!("gravity: unknown command {other}"),
        }
    }
}

/// Start the app in a canvas.
///
/// The browser gets the same egui shell the desktop binary runs — eframe draws egui to
/// the canvas and wgpu talks to WebGPU — so no part of the interface is HTML.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement) -> Result<(), wasm_bindgen::JsValue> {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    let _ = console_log::init_with_level(log::Level::Warn);
    fathom_shell::run_web::<Gravity>(canvas).await
}
