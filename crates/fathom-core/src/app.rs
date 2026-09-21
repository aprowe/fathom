//! The app contract.
//!
//! Shaped after openFrameworks: you get `setup`, `update`, `draw`, and a set of input
//! callbacks that default to doing nothing, so an app only writes the ones it cares
//! about. The framework owns the window, the surface, the clock, the camera and the
//! parameter block; the app owns its buffers, its pipelines and its physics.

use serde::Serialize;
use serde_json::Value;

use crate::camera::Camera;
use crate::event::{KeyEvent, MouseEvent, ScrollEvent};
use crate::gpu::Gpu;
use crate::params::{ParamBlock, ParamDef, Params};
use crate::viewport::Viewport;

/// A button or a select in the panel that triggers something the app cannot express as
/// a parameter — anything that reallocates buffers or reseeds state.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct CommandDef {
    pub name: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    /// Panel tab the group sits on; empty means the first tab.
    pub tab: &'static str,
    /// If non-empty, the interface renders a select instead of a button and dispatches
    /// the command with `{ "value": <index> }`.
    pub options: &'static [&'static str],
    /// Which option the select starts on, so the control agrees with the state the app
    /// actually set up with.
    pub initial: u32,
    /// What the command does, for the control's tooltip. Empty means no tooltip.
    pub description: &'static str,
    /// A button drawn at the top of the panel beside Randomize, on every tab, instead
    /// of under its group: for the two or three actions reached for constantly.
    pub pinned: bool,
}

impl CommandDef {
    pub const fn button(name: &'static str, label: &'static str) -> Self {
        Self { name, label, group: "Actions", tab: "", options: &[], initial: 0, description: "", pinned: false }
    }

    pub const fn choice(name: &'static str, label: &'static str, options: &'static [&'static str]) -> Self {
        Self { name, label, group: "Actions", tab: "", options, initial: 0, description: "", pinned: false }
    }

    pub const fn group(mut self, group: &'static str) -> Self {
        self.group = group;
        self
    }

    pub const fn tab(mut self, tab: &'static str) -> Self {
        self.tab = tab;
        self
    }

    /// Set the option the select starts on.
    pub const fn initial(mut self, initial: u32) -> Self {
        self.initial = initial;
        self
    }

    /// Say what the command does, for the tooltip.
    pub const fn describe(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }

    /// Draw this button at the top of the panel beside Randomize, on every tab. Only
    /// buttons pin; a choice stays with its group.
    pub const fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }
}

/// Everything the interface needs to build itself: sent to the UI once, at startup.
#[derive(Clone, Debug, Serialize)]
pub struct AppDescriptor {
    pub name: &'static str,
    pub params: &'static [ParamDef],
    pub commands: &'static [CommandDef],
    /// Presets compiled into the binary, so the web build has some too. See
    /// [`builtin_presets!`](crate::builtin_presets).
    #[serde(skip)]
    pub presets: Option<&'static include_dir::Dir<'static>>,
    /// Whether the shell draws its own Presets section. An app that wants a different
    /// arrangement turns this off and draws one from the same pieces in its panel hook.
    pub presets_section: bool,
    /// Whether the app draws all of its own chrome: the shell then draws no panel, title
    /// bar or drawer, and calls the app's `chrome` hook to lay the window out around the
    /// canvas.
    pub custom_chrome: bool,
}

impl AppDescriptor {
    pub const fn new(name: &'static str, params: &'static [ParamDef], commands: &'static [CommandDef]) -> Self {
        Self { name, params, commands, presets: None, presets_section: true, custom_chrome: false }
    }

    /// The app lays out the window itself; see `custom_chrome`.
    pub const fn custom_chrome(mut self) -> Self {
        self.custom_chrome = true;
        self
    }

    /// Ship these presets inside the binary.
    pub const fn presets(mut self, dir: &'static include_dir::Dir<'static>) -> Self {
        self.presets = Some(dir);
        self
    }

    /// Leave the Presets section out of the generated panel.
    pub const fn no_presets_section(mut self) -> Self {
        self.presets_section = false;
        self
    }
}

/// The JSON presets under `presets/` next to the calling crate's `Cargo.toml`, compiled
/// in. Hand the result to [`AppDescriptor::presets`].
#[macro_export]
macro_rules! builtin_presets {
    () => {{
        // The expansion names the `include_dir` crate, so it must be in scope.
        use $crate::include_dir;
        static PRESETS: include_dir::Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/presets");
        &PRESETS
    }};
}

/// Handed to [`App::setup`].
pub struct SetupCtx<'a> {
    pub gpu: &'a Gpu,
    pub viewport: Viewport,
    pub params: Params<'a>,
}

/// Handed to [`App::update`] once per frame, before drawing.
pub struct UpdateCtx<'a> {
    pub gpu: &'a Gpu,
    /// Seconds since the last frame, clamped so a stall cannot blow up an integrator.
    pub dt: f32,
    /// How many simulation steps to run this frame: what the pacer found fits in the
    /// frame's budget. One while paused and stepping. An app that steps a fixed
    /// timestep runs this many; an app that integrates `dt` may ignore it.
    pub steps: u32,
    /// When the frame's simulation work began, for the pacer. Starts with `update`;
    /// an app that does other GPU work first — a readback for a selection, say —
    /// calls [`UpdateCtx::start_work`] just before it submits its steps, so only the
    /// steps are measured.
    pub work_started: &'a mut web_time::Instant,
    /// The work time the app measured itself, if it did: see
    /// [`UpdateCtx::report_work_ms`].
    pub work_reported: &'a mut Option<f32>,
    /// Seconds since setup, excluding paused time.
    pub time: f32,
    pub frame: u64,
    pub params: Params<'a>,
    pub viewport: Viewport,
    pub camera: &'a mut Camera,
}

impl UpdateCtx<'_> {
    /// Mark the start of this frame's simulation work: call it right before submitting
    /// the steps, after any other GPU work `update` does.
    pub fn start_work(&mut self) {
        *self.work_started = web_time::Instant::now();
    }

    /// Hand the pacer a work time the app measured on the GPU — timestamp queries
    /// around its steps, read back a frame later. Preferred over the runner's own
    /// fence timing, which under vsync measures the frame period rather than the work;
    /// when this is called the runner does not wait on the device at all.
    pub fn report_work_ms(&mut self, ms: f32) {
        *self.work_reported = Some(ms);
    }
}

/// Handed to [`App::draw`]. Record into `encoder`; the host handles present.
pub struct DrawCtx<'a> {
    pub gpu: &'a Gpu,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub target: &'a wgpu::TextureView,
    pub viewport: Viewport,
    pub camera: &'a Camera,
    pub params: Params<'a>,
    pub time: f32,
}

/// Handed to the input callbacks. The camera is mutable here so an app can implement
/// its own pan and zoom feel.
pub struct EventCtx<'a> {
    pub gpu: &'a Gpu,
    pub viewport: Viewport,
    pub camera: &'a mut Camera,
    pub params: Params<'a>,
}

/// Handed to [`App::command`]. `gpu` is available because commands are exactly the
/// things that need to reallocate.
///
/// This is also the one context that can *write* parameters. Everywhere else the app
/// only reads them, because everywhere else the interface is the author of their values
/// and an app writing underneath it would fight the widget the user is holding. A
/// command is the exception by nature: applying a preset means moving the sliders, and
/// a preset that could not move a slider would not be a preset.
pub struct CommandCtx<'a> {
    pub gpu: &'a Gpu,
    pub name: &'a str,
    pub value: &'a Value,
    pub viewport: Viewport,
    pub camera: &'a mut Camera,
    pub schema: &'static [ParamDef],
    pub block: &'a mut ParamBlock,
}

impl CommandCtx<'_> {
    /// The `value` field as an integer, for commands declared with options.
    pub fn index(&self) -> u32 {
        self.value.get("value").and_then(Value::as_u64).unwrap_or(0) as u32
    }

    /// Read the parameters, the same view every other callback receives.
    pub fn params(&self) -> Params<'_> {
        Params::new(self.schema, self.block)
    }

    /// Write a float parameter by index constant.
    pub fn set_float(&mut self, index: usize, value: f32) {
        self.block.set_f32(index, value);
    }

    /// Write an int, toggle or choice parameter by index constant.
    pub fn set_int(&mut self, index: usize, value: u32) {
        self.block.set_u32(index, value);
    }
}

/// A fathom app: a GPU simulation with an interface around it.
pub trait App: Sized + 'static {
    /// Declare the name, parameters and commands. Called before `setup`.
    fn describe() -> AppDescriptor;

    fn setup(ctx: &mut SetupCtx<'_>) -> Self;

    /// Advance the simulation by `ctx.dt`. Not called while paused.
    fn update(&mut self, ctx: &mut UpdateCtx<'_>);

    /// Render the current state. Called every frame, including while paused.
    fn draw(&mut self, ctx: &mut DrawCtx<'_>);

    fn mouse_pressed(&mut self, _e: &MouseEvent, _ctx: &mut EventCtx<'_>) {}
    fn mouse_moved(&mut self, _e: &MouseEvent, _ctx: &mut EventCtx<'_>) {}
    fn mouse_dragged(&mut self, _e: &MouseEvent, _ctx: &mut EventCtx<'_>) {}
    fn mouse_released(&mut self, _e: &MouseEvent, _ctx: &mut EventCtx<'_>) {}
    fn mouse_scrolled(&mut self, _e: &ScrollEvent, _ctx: &mut EventCtx<'_>) {}
    fn key_pressed(&mut self, _e: &KeyEvent, _ctx: &mut EventCtx<'_>) {}
    fn key_released(&mut self, _e: &KeyEvent, _ctx: &mut EventCtx<'_>) {}

    /// The viewport changed size. Reallocate size-dependent targets here.
    fn resized(&mut self, _ctx: &mut EventCtx<'_>) {}

    /// A declared command fired.
    fn command(&mut self, _ctx: &mut CommandCtx<'_>) {}
}
