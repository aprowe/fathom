//! fathom — a framework for GPU simulation apps with an interface around them.
//!
//! An app implements [`App`] (openFrameworks-shaped: `setup`, `update`, `draw`, plus
//! input callbacks) and declares its parameters with [`params!`]. The framework runs it
//! on two targets from the same source:
//!
//! * **web** — compiled to wasm, rendering into a canvas through WebGPU
//! * **native** — a Tauri window whose transparent webview is the interface, floating
//!   over a wgpu child window that the interface tells where to draw
//!
//! Nothing in this crate knows which of those is happening. That is the point: the
//! [`Runner`] owns the loop, and the two hosts only manage a surface.

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
