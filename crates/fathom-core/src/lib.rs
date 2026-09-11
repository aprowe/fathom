//! fathom — a framework for GPU simulation apps with an interface around them.
//!
//! An app implements [`App`] (openFrameworks-shaped: `setup`, `update`, `draw`, plus
//! input callbacks) and declares its parameters with [`params!`]. The framework runs it
//! on two targets from the same source:
//!
//! * **native** — one window, the panel drawn by egui beside the simulation, both on
//!   the same wgpu device
//! * **web** — the same shell compiled to wasm, egui drawing into a canvas and wgpu
//!   talking to WebGPU
//!
//! Nothing in this crate knows which of those is happening. That is the point: the
//! [`Runner`] owns the loop, and the shell only manages a surface and draws a panel.

pub mod app;
pub mod camera;
pub mod clock;
pub mod event;
pub mod gpu;
pub mod params;
pub mod runner;
pub mod viewport;

pub use app::{
    App, AppDescriptor, CommandCtx, CommandDef, DrawCtx, EventCtx, SetupCtx, UpdateCtx,
};
pub use camera::{Camera, CameraUniform};
pub use clock::FrameStats;
pub use event::{InputEvent, KeyEvent, Modifiers, MouseEvent, ScrollEvent};
pub use gpu::Gpu;
pub use params::{ParamBlock, ParamDef, ParamKind, Params};
pub use runner::Runner;
pub use viewport::{Viewport, ViewportRect};

/// Re-exported so apps and hosts always agree on the wgpu version.
pub use wgpu;
