//! The widgets fathom apps are built from, with no shell behind them.
//!
//! `fathom-core` says what a simulation is; `fathom-shell` puts one in a window on
//! native and web. This crate is the layer between: the look, and every control and
//! instrument, each a function of an egui `Ui` and — where it edits a parameter — the
//! schema and a parameter block. The shell's default panel is one arrangement of these;
//! an app that wants its own chrome lays them out itself.

pub mod control;
pub mod instruments;
pub mod param;
pub mod presets;
pub mod skin;
pub mod strip;
pub mod transport;

pub use eframe::egui;
