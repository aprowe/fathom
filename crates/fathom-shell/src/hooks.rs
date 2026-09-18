//! Where an app reaches past the paved road.
//!
//! [`App`](fathom_core::App) describes a simulation: parameters in, pixels out. That is
//! enough for the panel to build itself and for the tests to run headless, and it is
//! deliberately all `fathom-core` knows. It is not enough for everything an app might
//! want on screen — a table of bodies, a label pinned to the heaviest one, a window of
//! its own, a file of its own — and a framework that answers those with "not supported"
//! is not aiding, it is limiting.
//!
//! So the shell adds a second trait with every method defaulted. Each one hands the app
//! the same things the shell itself has: the egui context, the panel `Ui`, the viewport,
//! the live parameter block, the camera, the pause flag, and a way to ask the shell for
//! anything it does on its own behalf. An app that wants none of it writes
//! `impl ShellApp for MyApp {}` and is done.

use eframe::egui;
use fathom_core::{App, Camera, Gpu, ParamBlock, ParamDef, Params, RunnerParts, Viewport};
use serde_json::Value;

use crate::presets::Preset;

/// The hooks. Every method has a default that does nothing, so implement the ones you
/// need and no others.
pub trait ShellApp: App {
    /// Draw into the controls panel, below the generated groups. `ui` is a plain egui
    /// `Ui` inside the panel's scroll area: any widget goes.
    fn panel(&mut self, _ui: &mut egui::Ui, _ctx: &mut ShellCtx<'_>) {}

    /// Draw over the simulation. `ui` covers exactly the viewport, so
    /// `ui.min_rect()` is the viewport in points and [`ShellCtx::to_screen`] maps a
    /// world position onto it. Labels, HUD text, gizmos, debug readouts.
    fn overlay(&mut self, _ui: &mut egui::Ui, _ctx: &mut ShellCtx<'_>) {}

    /// Once per frame, before the panel and the simulation are drawn, with the whole
    /// egui context and the eframe frame. Open a window, read raw input, change the
    /// title, send a viewport command — nothing is off limits here.
    fn frame(&mut self, _egui: &egui::Context, _frame: &mut eframe::Frame, _ctx: &mut ShellCtx<'_>) {}

    /// Every command the shell is about to dispatch, `fathom.*` ones included, before
    /// it acts on them. Return `true` to swallow the command.
    fn intercept(&mut self, _name: &str, _value: &Value, _ctx: &mut ShellCtx<'_>) -> bool {
        false
    }

    /// App-owned state to save beside the parameters. `Null` saves nothing extra.
    fn save_state(&self) -> Value {
        Value::Null
    }

    /// The `state` a file was saved with, when one is opened. Called after the
    /// parameters in it have been applied.
    fn load_state(&mut self, _state: &Value, _ctx: &mut ShellCtx<'_>) {}

    /// A file dropped on the window, or chosen with Import…, that is not a preset for this
    /// app. Whatever the app makes of it is its business.
    fn file_received(&mut self, _name: &str, _bytes: &[u8], _ctx: &mut ShellCtx<'_>) {}
}

/// What every hook receives: the runner's state borrowed apart, the egui context, and a
/// place to leave requests for the shell.
///
/// Parameters are writable here for the same reason they are in
/// [`CommandCtx`](fathom_core::CommandCtx): a hook is something the user is doing, not
/// something happening underneath a widget they are holding.
pub struct ShellCtx<'a> {
    pub gpu: &'a Gpu,
    pub viewport: Viewport,
    pub camera: &'a mut Camera,
    pub schema: &'static [ParamDef],
    pub block: &'a mut ParamBlock,
    /// Whether the simulation is paused. Write it to pause or resume.
    pub paused: &'a mut bool,
    pub egui: &'a egui::Context,
    /// The viewport's rectangle on screen, in points. Zero-sized before the first frame.
    pub viewport_rect: egui::Rect,
    pub shell: &'a mut Controls,
}

impl ShellCtx<'_> {
    /// Read the parameters, the same view every `App` callback receives.
    pub fn params(&self) -> Params<'_> {
        Params::new(self.schema, self.block)
    }

    pub fn set_float(&mut self, index: usize, value: f32) {
        self.block.set_f32(index, value);
    }

    pub fn set_int(&mut self, index: usize, value: u32) {
        self.block.set_u32(index, value);
    }

    /// A world position as a point on screen, through the camera and the viewport.
    pub fn to_screen(&self, world: [f32; 2]) -> egui::Pos2 {
        let [px, py] = self.camera.world_to_screen(world, self.viewport);
        let ppp = self.viewport.dpr.max(f32::EPSILON);
        egui::pos2(self.viewport_rect.min.x + px / ppp, self.viewport_rect.min.y + py / ppp)
    }

    /// A point on screen as a world position.
    pub fn to_world(&self, screen: egui::Pos2) -> [f32; 2] {
        let ppp = self.viewport.dpr;
        let px = (screen.x - self.viewport_rect.min.x) * ppp;
        let py = (screen.y - self.viewport_rect.min.y) * ppp;
        self.camera.screen_to_world(px, py, self.viewport)
    }
}

/// Things the shell does that a hook may ask for. They are queued rather than done on
/// the spot because the hook is holding the runner's state, and applied the moment the
/// hook returns, in order.
#[derive(Default)]
pub struct Controls {
    pub(crate) requests: Vec<Request>,
    pub(crate) presets: Vec<Preset>,
    pub(crate) panel_width: f32,
    pub(crate) drawer_open: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum Request {
    Step,
    ResetCamera,
    Drawer(bool),
    PanelWidth(f32),
    SaveDialog,
    OpenDialog,
    LoadPreset(String),
    SavePreset(String),
    DeletePreset(String),
    Command(String, Value),
    Status(String),
    SaveFile(String, Vec<u8>),
}

impl Controls {
    /// Run one update on the next frame, even while paused.
    pub fn step(&mut self) {
        self.requests.push(Request::Step);
    }

    pub fn reset_camera(&mut self) {
        self.requests.push(Request::ResetCamera);
    }

    /// Slide the controls drawer out or away, on a screen narrow enough to have one.
    pub fn open_drawer(&mut self, open: bool) {
        self.requests.push(Request::Drawer(open));
    }

    pub fn drawer_open(&self) -> bool {
        self.drawer_open
    }

    pub fn set_panel_width(&mut self, points: f32) {
        self.requests.push(Request::PanelWidth(points));
    }

    pub fn panel_width(&self) -> f32 {
        self.panel_width
    }

    /// Show the Export… dialog: the OS's natively, a download on the web.
    pub fn save_dialog(&mut self) {
        self.requests.push(Request::SaveDialog);
    }

    /// Show the Import… dialog: the OS's natively, a file picker on the web.
    pub fn open_dialog(&mut self) {
        self.requests.push(Request::OpenDialog);
    }

    /// Offer `bytes` to the user as a file: the OS save dialog natively, a download on
    /// the web. `file_name` is the suggestion, extension included.
    pub fn save_file(&mut self, file_name: impl Into<String>, bytes: Vec<u8>) {
        self.requests.push(Request::SaveFile(file_name.into(), bytes));
    }

    /// The presets available right now, built in and from the folder, merged.
    pub fn presets(&self) -> &[Preset] {
        &self.presets
    }

    pub fn load_preset(&mut self, name: impl Into<String>) {
        self.requests.push(Request::LoadPreset(name.into()));
    }

    /// Write the current parameters and state to `presets/<app>/<name>.json`. Native
    /// only: on the web it does nothing, and [`save_dialog`](Self::save_dialog) is the
    /// way to keep a file.
    pub fn save_preset(&mut self, name: impl Into<String>) {
        self.requests.push(Request::SavePreset(name.into()));
    }

    /// Remove a preset from the folder. Built-in ones cannot be removed.
    pub fn delete_preset(&mut self, name: impl Into<String>) {
        self.requests.push(Request::DeletePreset(name.into()));
    }

    /// Dispatch a command as if the panel had: through `intercept`, then to the runner
    /// or the app.
    pub fn command(&mut self, name: impl Into<String>, value: Value) {
        self.requests.push(Request::Command(name.into(), value));
    }

    /// Put a line under the Presets section, where the shell reports its own outcomes.
    pub fn status(&mut self, text: impl Into<String>) {
        self.requests.push(Request::Status(text.into()));
    }
}

/// Build a context over the runner's parts and run `f` with the app and it.
pub(crate) fn with_ctx<A: App, R>(
    parts: RunnerParts<'_, A>,
    egui: &egui::Context,
    viewport_rect: egui::Rect,
    controls: &mut Controls,
    f: impl FnOnce(&mut A, &mut ShellCtx<'_>) -> R,
) -> R {
    let mut ctx = ShellCtx {
        gpu: parts.gpu,
        viewport: parts.viewport,
        camera: parts.camera,
        schema: parts.schema,
        block: parts.block,
        paused: parts.paused,
        egui,
        viewport_rect,
        shell: controls,
    };
    f(parts.app, &mut ctx)
}
