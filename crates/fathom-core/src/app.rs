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
    /// If non-empty, the interface renders a select instead of a button and dispatches
    /// the command with `{ "value": <index> }`.
    pub options: &'static [&'static str],
    /// Which option the select starts on, so the control agrees with the state the app
    /// actually set up with.
    pub initial: u32,
}

impl CommandDef {
    pub const fn button(name: &'static str, label: &'static str) -> Self {
        Self { name, label, group: "Actions", options: &[], initial: 0 }
    }

    pub const fn choice(name: &'static str, label: &'static str, options: &'static [&'static str]) -> Self {
        Self { name, label, group: "Actions", options, initial: 0 }
    }

    pub const fn group(mut self, group: &'static str) -> Self {
        self.group = group;
        self
    }

    /// Set the option the select starts on.
    pub const fn initial(mut self, initial: u32) -> Self {
        self.initial = initial;
        self
    }
}

/// Everything the interface needs to build itself: sent to the UI once, at startup.
#[derive(Clone, Debug, Serialize)]
pub struct AppDescriptor {
    pub name: &'static str,
    pub params: &'static [ParamDef],
    pub commands: &'static [CommandDef],
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
    /// Seconds since setup, excluding paused time.
    pub time: f32,
    pub frame: u64,
    pub params: Params<'a>,
    pub viewport: Viewport,
    pub camera: &'a mut Camera,
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
