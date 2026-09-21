//! The shared loop.
//!
//! Both hosts — the wasm one on web and the child-window one on native — own a
//! `Runner`. Everything about *running* an app lives here, so the hosts are reduced to
//! surface management and marshalling. That is what keeps the two targets honest.

use serde_json::Value;

use crate::app::{App, AppDescriptor, CommandCtx, DrawCtx, EventCtx, SetupCtx, UpdateCtx, WorkReport};
use crate::camera::Camera;
use crate::clock::{Clock, FrameStats};
use crate::event::{InputEvent, MouseEvent};
use crate::gpu::Gpu;
use crate::pacer::Pacer;
use crate::params::{ParamBlock, ParamDef, Params};
use crate::viewport::Viewport;

/// Commands the framework handles itself rather than passing to the app. The prefix
/// keeps them out of an app's namespace.
pub const CMD_PREFIX: &str = "fathom.";

/// The runner's state with the app borrowed separately from the rest, so the app can be
/// handed all of it at once. See [`Runner::parts`].
pub struct RunnerParts<'a, A: App> {
    pub app: &'a mut A,
    pub gpu: &'a Gpu,
    pub viewport: Viewport,
    pub camera: &'a mut Camera,
    pub schema: &'static [ParamDef],
    pub block: &'a mut ParamBlock,
    pub paused: &'a mut bool,
}

pub struct Runner<A: App> {
    gpu: Gpu,
    app: A,
    schema: &'static [ParamDef],
    block: ParamBlock,
    camera: Camera,
    clock: Clock,
    pacer: Pacer,
    viewport: Viewport,
    paused: bool,
    /// Set by `fathom.step`: run exactly one update while paused.
    step_once: bool,
    /// Last cursor position, so middle-drag panning has something to measure against.
    last_cursor: Option<(f32, f32)>,
}

impl<A: App> Runner<A> {
    pub fn descriptor() -> AppDescriptor {
        A::describe()
    }

    pub fn new(gpu: Gpu, viewport: Viewport) -> Self {
        let descriptor = A::describe();
        let block = ParamBlock::from_defaults(descriptor.params);
        let app = {
            let mut ctx = SetupCtx {
                gpu: &gpu,
                viewport,
                params: Params::new(descriptor.params, &block),
            };
            A::setup(&mut ctx)
        };
        Self {
            gpu,
            app,
            schema: descriptor.params,
            block,
            camera: Camera::default(),
            clock: Clock::default(),
            pacer: Pacer::default(),
            viewport,
            paused: false,
            step_once: false,
            last_cursor: None,
        }
    }

    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn param_byte_len(&self) -> usize {
        self.block.byte_len()
    }

    /// The app's declared parameters.
    pub fn schema(&self) -> &'static [ParamDef] {
        self.schema
    }

    pub fn params(&self) -> &ParamBlock {
        &self.block
    }

    /// The live parameter values, for an interface that shares this process and can edit
    /// them directly rather than shipping a mirror across a boundary.
    pub fn params_mut(&mut self) -> &mut ParamBlock {
        &mut self.block
    }

    pub fn stats(&self) -> FrameStats {
        FrameStats { steps: self.pacer.steps(), work_ms: self.pacer.work_ms(), ..self.clock.stats() }
    }

    /// The step pacer, to retune or reset — after a reseed, say, when the old count
    /// means nothing.
    pub fn pacer_mut(&mut self) -> &mut Pacer {
        &mut self.pacer
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Run one update on the next frame even while paused.
    pub fn step(&mut self) {
        self.step_once = true;
    }

    pub fn reset_camera(&mut self) {
        self.camera = Camera::default();
    }

    pub fn app(&self) -> &A {
        &self.app
    }

    /// Everything at once, borrowed apart, for a host that lets the app reach past the
    /// contexts the trait hands it. The trait's contexts are the paved road; this is the
    /// ground under it, and a host is expected to expose it rather than fence it off.
    pub fn parts(&mut self) -> RunnerParts<'_, A> {
        RunnerParts {
            app: &mut self.app,
            gpu: &self.gpu,
            viewport: self.viewport,
            camera: &mut self.camera,
            schema: self.schema,
            block: &mut self.block,
            paused: &mut self.paused,
        }
    }

    /// The interface told us where it wants the sim drawn.
    pub fn resize(&mut self, viewport: Viewport) {
        if viewport == self.viewport {
            return;
        }
        self.viewport = viewport;
        let mut ctx = EventCtx {
            gpu: &self.gpu,
            viewport: self.viewport,
            camera: &mut self.camera,
            params: Params::new(self.schema, &self.block),
        };
        self.app.resized(&mut ctx);
    }

    /// Take the interface's mirror of the parameter block.
    pub fn write_params(&mut self, bytes: &[u8]) {
        self.block.write_bytes(bytes);
    }

    pub fn input(&mut self, event: InputEvent) {
        // Pointer coordinates only mean something once the interface has reported a
        // viewport. Before that, mapping them to world space divides by a placeholder
        // size and produces enormous positions — which an app will happily act on, for
        // instance by planting a gravity well thousands of units off screen.
        if self.viewport.width <= 1 || self.viewport.height <= 1 {
            return;
        }

        // Pan and zoom are framework behaviour, applied before the app sees the event,
        // so every app gets them without writing any code. The app still receives the
        // event and can do whatever else it likes with it.
        self.apply_camera_controls(&event);

        let mut ctx = EventCtx {
            gpu: &self.gpu,
            viewport: self.viewport,
            camera: &mut self.camera,
            params: Params::new(self.schema, &self.block),
        };
        match &event {
            InputEvent::MousePressed(e) => self.app.mouse_pressed(e, &mut ctx),
            InputEvent::MouseMoved(e) => self.app.mouse_moved(e, &mut ctx),
            InputEvent::MouseDragged(e) => self.app.mouse_dragged(e, &mut ctx),
            InputEvent::MouseReleased(e) => self.app.mouse_released(e, &mut ctx),
            InputEvent::Scrolled(e) => self.app.mouse_scrolled(e, &mut ctx),
            InputEvent::KeyPressed(e) => self.app.key_pressed(e, &mut ctx),
            InputEvent::KeyReleased(e) => self.app.key_released(e, &mut ctx),
        }
    }

    /// Dispatch a command. Framework commands are handled here; everything else goes to
    /// the app.
    pub fn command(&mut self, name: &str, value: Value) {
        if let Some(builtin) = name.strip_prefix(CMD_PREFIX) {
            match builtin {
                "pause" => self.paused = true,
                "resume" => self.paused = false,
                "toggle_pause" => self.paused = !self.paused,
                "step" => self.step_once = true,
                "reset_camera" => self.camera = Camera::default(),
                other => log::warn!("unknown framework command: {other}"),
            }
            return;
        }

        let mut ctx = CommandCtx {
            gpu: &self.gpu,
            name,
            value: &value,
            viewport: self.viewport,
            camera: &mut self.camera,
            schema: self.schema,
            block: &mut self.block,
        };
        self.app.command(&mut ctx);
    }

    /// Run one frame into `target`. Returns the encoded work, already submitted.
    pub fn frame(&mut self, now_ms: f64, target: &wgpu::TextureView) {
        let dt = self.clock.tick(now_ms);

        if !self.paused || self.step_once {
            // A single step while paused should advance by a plausible frame, not by
            // however long the user sat on the pause button.
            let step_dt = if self.paused { 1.0 / 60.0 } else { dt };
            let steps = if self.paused { 1 } else { self.pacer.steps() };
            self.step_once = false;
            // Whatever the GPU still has from last frame — its draw, the panel, a
            // readback — is not this frame's work: let it finish before the clock
            // starts, so the steps alone are what gets measured.
            let mut started = web_time::Instant::now();
            let mut reported = None;
            let mut ctx = UpdateCtx {
                gpu: &self.gpu,
                dt: step_dt,
                steps,
                work_started: &mut started,
                work_reported: &mut reported,
                time: self.clock.time,
                frame: self.clock.frame,
                params: Params::new(self.schema, &self.block),
                viewport: self.viewport,
                camera: &mut self.camera,
            };
            self.app.update(&mut ctx);
            if !self.paused {
                match reported {
                    Some(WorkReport::Measured { steps, ms }) => self.pacer.observe(steps, ms),
                    // The app measures; its readback has not landed this frame.
                    Some(WorkReport::Pending) => {}
                    // No measurement from the app: wait for the work and time that. Under
                    // vsync this reads as the frame period, so the pacer then holds at
                    // about a frame's worth of steps — safe, if not the whole budget.
                    // The browser cannot wait at all, and holds at one step.
                    #[cfg(not(target_arch = "wasm32"))]
                    None => {
                        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
                        self.pacer.observe(steps, started.elapsed().as_secs_f32() * 1000.0);
                    }
                    #[cfg(target_arch = "wasm32")]
                    None => {}
                }
            }
        }

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("fathom frame") });
        {
            let mut ctx = DrawCtx {
                gpu: &self.gpu,
                encoder: &mut encoder,
                target,
                viewport: self.viewport,
                camera: &self.camera,
                params: Params::new(self.schema, &self.block),
                time: self.clock.time,
            };
            self.app.draw(&mut ctx);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
    }

    fn apply_camera_controls(&mut self, event: &InputEvent) {
        const MIDDLE_BUTTON_MASK: u8 = 0b100;
        match event {
            InputEvent::Scrolled(e) => {
                // One notch is ~100px of deltaY in most browsers; the exponential keeps
                // zoom speed even across trackpads and wheels.
                let factor = (-e.delta_y / 400.0).exp();
                self.camera.zoom_at(e.x, e.y, factor, self.viewport);
            }
            InputEvent::MousePressed(e) => self.last_cursor = Some((e.x, e.y)),
            InputEvent::MouseDragged(e) => {
                let last = self.last_cursor.unwrap_or((e.x, e.y));
                if e.buttons & MIDDLE_BUTTON_MASK != 0 || (e.buttons & 1 != 0 && e.shift) {
                    self.camera.pan_pixels(e.x - last.0, e.y - last.1, self.viewport);
                }
                self.last_cursor = Some((e.x, e.y));
            }
            InputEvent::MouseMoved(e) | InputEvent::MouseReleased(e) => {
                self.last_cursor = Some((e.x, e.y));
            }
            _ => {}
        }
    }
}

/// A no-op mouse event, useful when synthesising input in tests.
pub fn mouse_at(x: f32, y: f32) -> MouseEvent {
    MouseEvent { x, y, ..Default::default() }
}
