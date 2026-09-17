//! An egui shell for fathom apps: one window, no web technology.
//!
//! The shell owns a [`Runner`] and draws the panel around it. The panel is Rust, drawn
//! by egui into the same window and with the same wgpu device as the simulation, so:
//!
//! * there is one window, which means the taskbar, alt-tab and its preview thumbnail,
//!   snapping and per-monitor DPI all work because the OS is not being tricked;
//! * a slider writes straight into the live parameter block — no mirror, no IPC, no
//!   serialisation anywhere in the frame;
//! * the same shell compiles to the browser through eframe, where egui draws to a
//!   canvas, so one host covers both targets.
//!
//! The simulation renders into an offscreen texture that the panel then shows as an
//! image. That is what keeps the app's `draw` unchanged: it still receives a target view
//! and renders into it, exactly as it would if that view were a swapchain.

pub mod control;
pub mod files;
pub mod hooks;
pub mod presets;
pub mod skin;

use std::sync::Arc;

pub use eframe;
pub use eframe::egui;
use egui_wgpu::wgpu;
use fathom_core::{
    Gpu, InputEvent, KeyEvent, MouseEvent, ParamDef, ParamKind, Runner, ScrollEvent, Viewport,
};
pub use files::FileEvent;
pub use hooks::{Controls, ShellApp, ShellCtx};
pub use presets::{Preset, Source};

use hooks::Request;

/// The extensions the Open… dialog shows and Save… suggests.
const PRESET_EXTENSIONS: &[&str] = &["json"];

/// The texture the simulation draws into, and its identity inside egui.
struct Target {
    view: wgpu::TextureView,
    id: egui::TextureId,
    width: u32,
    height: u32,
}

pub struct Shell<A: ShellApp> {
    runner: Runner<A>,
    target: Option<Target>,
    /// Kept to register and release textures as the viewport resizes.
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    device: wgpu::Device,
    /// Whether the pointer was down over the viewport last frame, so a move can be
    /// reported as a drag rather than a hover.
    dragging: bool,
    /// Which button began the drag: 0 left, 1 middle, 2 right, as the DOM numbers them.
    drag_button: u8,
    /// The option each command-select is showing. Commands are fire-and-forget, so
    /// unlike parameters they have no stored value to read back.
    command_choice: Vec<(&'static str, usize)>,
    /// Whether the drawer is out, on a screen narrow enough to have one.
    drawer_open: bool,
    /// How wide the docked panel is, in points. Owned here rather than left to egui so
    /// the drawer can borrow the same number on a narrow screen.
    panel_width: f32,
    /// The preset name being typed, and the last thing that happened to a file.
    preset_name: String,
    status: String,
    /// The presets the panel lists live in `controls`, where a hook can see them too.
    /// Listed once at startup and again after every save or delete, not every frame: a
    /// directory read per frame is a waste even when it is fast.
    presets_listed: bool,
    /// The hooks' way of asking the shell for things, and the dialogs in flight.
    controls: Controls,
    dialogs: files::Dialogs,
    /// A width a hook asked for, waiting for the next frame's panel to pick it up.
    panel_width_requested: Option<f32>,
    /// Where the simulation was drawn last frame, in points, so a hook can map world
    /// positions onto the screen.
    viewport_rect: egui::Rect,
    /// The context, kept so a command arriving outside a frame callback still has one.
    egui: egui::Context,
    /// State of the panel's Randomize button. Seeded from the clock once; nothing here
    /// needs to be reproducible, it needs to be different each click.
    rng: u64,
}

impl<A: ShellApp> Shell<A> {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .ok_or("fathom's egui shell needs eframe's wgpu backend")?;

        // The simulation renders offscreen, so its target format is ours to choose
        // rather than the swapchain's. A plain UNORM target keeps the app's output
        // exactly as it is on the web canvas, with no extra gamma applied on the way in.
        let gpu = Gpu {
            device: render_state.device.clone(),
            queue: render_state.queue.clone(),
            format: wgpu::TextureFormat::Rgba8Unorm,
            adapter_info: render_state.adapter.get_info(),
        };

        Ok(Self::with_gpu(gpu, render_state.renderer.clone(), &cc.egui_ctx))
    }

    /// A shell over a device you already have, with no window. This is what the tests
    /// use, and what anything embedding the panel in an egui context of its own would.
    pub fn with_gpu(
        gpu: Gpu,
        renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
        egui: &egui::Context,
    ) -> Self {
        let device = gpu.device.clone();
        Self {
            runner: Runner::new(gpu, Viewport::new(1, 1, egui.pixels_per_point())),
            target: None,
            renderer,
            device,
            dragging: false,
            drag_button: 0,
            command_choice: Vec::new(),
            drawer_open: false,
            panel_width: panel::DEFAULT_WIDTH,
            preset_name: String::new(),
            status: String::new(),
            presets_listed: false,
            controls: Controls { panel_width: panel::DEFAULT_WIDTH, ..Controls::default() },
            dialogs: files::Dialogs::default(),
            panel_width_requested: None,
            viewport_rect: egui::Rect::ZERO,
            egui: egui.clone(),
            rng: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
                | 1,
        }
    }

    /// The runner, for a host that wants to drive it directly.
    pub fn runner(&mut self) -> &mut Runner<A> {
        &mut self.runner
    }

    /// The app's overlay, in a child that covers exactly `rect`.
    fn draw_overlay(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let mut overlay = ui.new_child(
            egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::LEFT)),
        );
        let egui = ui.ctx().clone();
        self.hook(&egui, |app, ctx| app.overlay(&mut overlay, ctx));
    }

    /// The next random unit interval from a small xorshift.
    fn random_unit(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Roll every unlocked parameter to a value somewhere in its range.
    fn randomize(&mut self, ui: &egui::Ui) {
        let schema = self.runner.schema();
        for (index, def) in schema.iter().enumerate() {
            if panel::is_locked(ui, def.name) {
                continue;
            }
            let u = self.random_unit();
            let word = match def.kind {
                ParamKind::Float => {
                    let mut v = def.min + u * (def.max - def.min);
                    if def.step > 0.0 {
                        v = def.min + ((v - def.min) / def.step).round() * def.step;
                    }
                    v.clamp(def.min, def.max).to_bits()
                }
                // Ints, toggles and choices are all "one of max - min + 1 slots".
                _ => {
                    let slots = (def.max - def.min + 1.0).max(1.0);
                    (def.min + (u * slots).floor().min(slots - 1.0)) as u32
                }
            };
            self.runner.params_mut().set_u32(index, word);
        }
    }

    fn choice_of(&self, name: &'static str, fallback: usize) -> usize {
        self.command_choice
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, i)| *i)
            .unwrap_or(fallback)
    }

    fn remember_choice(&mut self, name: &'static str, index: usize) {
        match self.command_choice.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = index,
            None => self.command_choice.push((name, index)),
        }
    }

    /// Make sure the offscreen target matches the viewport, and hand egui its identity.
    fn ensure_target(&mut self, width: u32, height: u32) {
        if let Some(t) = &self.target {
            if t.width == width && t.height == height {
                return;
            }
        }

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fathom sim target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut renderer = self.renderer.write();
        if let Some(old) = self.target.take() {
            renderer.free_texture(&old.id);
        }
        let id = renderer.register_native_texture(&self.device, &view, wgpu::FilterMode::Linear);

        self.target = Some(Target { view, id, width, height });
    }

    /// Run one hook with a context over the runner's parts, then act on whatever it
    /// asked for.
    fn hook<R>(&mut self, egui: &egui::Context, f: impl FnOnce(&mut A, &mut ShellCtx<'_>) -> R) -> R {
        self.list_presets();
        self.controls.panel_width = self.panel_width;
        self.controls.drawer_open = self.drawer_open;
        let result = hooks::with_ctx(self.runner.parts(), egui, self.viewport_rect, &mut self.controls, f);
        self.apply_requests();
        result
    }

    fn apply_requests(&mut self) {
        let requests = std::mem::take(&mut self.controls.requests);
        for request in requests {
            match request {
                Request::Step => self.runner.step(),
                Request::ResetCamera => self.runner.reset_camera(),
                Request::Drawer(open) => self.drawer_open = open,
                Request::PanelWidth(w) => self.set_panel_width(w),
                Request::SaveDialog => self.save_dialog(),
                Request::OpenDialog => self.open_dialog(),
                Request::LoadPreset(name) => self.load_preset(&name),
                Request::SavePreset(name) => self.save_preset(&name),
                Request::DeletePreset(name) => self.delete_preset(&name),
                Request::Command(name, value) => self.dispatch(&name, value),
                Request::Status(text) => self.status = text,
            }
        }
    }

    /// The docked panel remembers its own width in egui's memory and writes it back at
    /// the end of every show, so a new width has to go in before the next show begins.
    /// Requests usually arrive from inside one; this parks the value until then.
    fn set_panel_width(&mut self, width: f32) {
        let width = width.clamp(panel::MIN_WIDTH, panel::MAX_WIDTH);
        self.panel_width = width;
        self.panel_width_requested = Some(width);
    }

    /// Every command goes through here: the app sees it first and may swallow it, then
    /// the runner handles `fathom.*` and hands the rest to `App::command`.
    fn dispatch(&mut self, name: &str, value: serde_json::Value) {
        let egui = self.egui.clone();
        let swallowed = self.hook(&egui, |app, ctx| app.intercept(name, &value, ctx));
        if !swallowed {
            self.runner.command(name, value);
        }
    }

    fn list_presets(&mut self) {
        if self.presets_listed {
            return;
        }
        let descriptor = Runner::<A>::descriptor();
        self.controls.presets = presets::list(descriptor.presets, &presets::default_root(), descriptor.name);
        self.presets_listed = true;
    }

    /// The current parameters and the app's state as a preset document.
    fn encode_preset(&self) -> String {
        let descriptor = Runner::<A>::descriptor();
        let state = self.runner.app().save_state();
        presets::encode(descriptor.name, self.runner.schema(), self.runner.params(), state)
    }

    fn preset_folder(&self) -> std::path::PathBuf {
        presets::default_root().join(presets::sanitise(Runner::<A>::descriptor().name))
    }

    fn save_dialog(&mut self) {
        let name = if self.preset_name.trim().is_empty() { "preset" } else { self.preset_name.trim() };
        let file_name = format!("{}.json", presets::sanitise(name));
        let text = self.encode_preset();
        let dir = Some(self.preset_folder()).filter(|d| d.is_dir());
        self.dialogs.save(dir, file_name, PRESET_EXTENSIONS, text.into_bytes());
    }

    fn open_dialog(&mut self) {
        let dir = Some(self.preset_folder()).filter(|d| d.is_dir());
        self.dialogs.open(dir, PRESET_EXTENSIONS);
    }

    fn load_preset(&mut self, name: &str) {
        let descriptor = Runner::<A>::descriptor();
        match presets::read(descriptor.presets, &presets::default_root(), descriptor.name, name) {
            Ok(text) => {
                self.receive_file(&format!("{name}.json"), text.as_bytes());
                self.preset_name = name.to_string();
            }
            Err(e) => self.status = format!("not loaded: {e}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_preset(&mut self, name: &str) {
        let descriptor = Runner::<A>::descriptor();
        let text = self.encode_preset();
        match presets::save(&presets::default_root(), descriptor.name, name, &text) {
            Ok(path) => {
                self.status = format!("saved {}", path.display());
                self.presets_listed = false;
            }
            Err(e) => self.status = format!("not saved: {e}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn delete_preset(&mut self, name: &str) {
        let descriptor = Runner::<A>::descriptor();
        self.status = match presets::delete(&presets::default_root(), descriptor.name, name) {
            Ok(()) => format!("deleted {name}"),
            Err(e) => format!("not deleted: {e}"),
        };
        self.presets_listed = false;
    }

    #[cfg(target_arch = "wasm32")]
    fn save_preset(&mut self, _name: &str) {
        self.status = "no folder on the web: use Save… to download".into();
    }

    #[cfg(target_arch = "wasm32")]
    fn delete_preset(&mut self, _name: &str) {
        self.status = "built-in presets cannot be deleted".into();
    }

    /// A file from a dialog, a drop, or the preset list. A preset for this app is
    /// applied; anything else is the app's to interpret.
    fn receive_file(&mut self, name: &str, bytes: &[u8]) {
        let descriptor = Runner::<A>::descriptor();
        let egui = self.egui.clone();
        let preset = std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| presets::decode(text).ok())
            .filter(|doc| doc.is_for(descriptor.name));
        match preset {
            Some(doc) => {
                let n = doc.apply(self.runner.schema(), self.runner.params_mut());
                self.hook(&egui, |app, ctx| app.load_state(&doc.state, ctx));
                self.status = format!("loaded {name}: {n} of {} parameters", descriptor.params.len());
            }
            None => {
                self.hook(&egui, |app, ctx| app.file_received(name, bytes, ctx));
                self.status = format!("passed {name} to the app");
            }
        }
    }

    /// Dialog results and dropped files, once a frame.
    fn receive_files(&mut self, egui: &egui::Context) {
        for event in self.dialogs.poll() {
            match event {
                FileEvent::Opened { name, bytes } => self.receive_file(&name, &bytes),
                FileEvent::Saved { path } => {
                    self.status = format!("saved {}", path.display());
                    self.presets_listed = false;
                }
                FileEvent::Failed(why) => self.status = why,
            }
        }

        // Dropped files are read through the same channel, because on the web reading
        // one is asynchronous too.
        for file in egui.input(|i| i.raw.dropped_files.clone()) {
            self.dialogs.read_dropped(file);
        }
    }
}

impl<A: ShellApp> eframe::App for Shell<A> {
    /// The window is transparent nowhere: the simulation covers the whole canvas.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let g = skin::GROUND;
        [
            g.r() as f32 / 255.0,
            g.g() as f32 / 255.0,
            g.b() as f32 / 255.0,
            1.0,
        ]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // A simulation is never idle, so the shell drives repaints rather than waiting
        // for input the way an ordinary egui app would.
        ui.ctx().request_repaint();

        let egui = ui.ctx().clone();
        self.hook(&egui, |app, ctx| app.frame(&egui, frame, ctx));
        self.receive_files(&egui);

        panel::draw(self, ui);

        let ppp = ui.ctx().pixels_per_point();
        egui::CentralPanel::no_frame()
            .show(ui, |ui| {
                let size = ui.available_size();
                let width = ((size.x * ppp).round() as u32).max(1);
                let height = ((size.y * ppp).round() as u32).max(1);

                self.ensure_target(width, height);
                self.runner.resize(Viewport::new(width, height, ppp));

                let (rect, response) =
                    ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                self.viewport_rect = rect;
                input::forward(self, ui, rect, &response, ppp);

                let now_ms = ui.input(|i| i.time) * 1000.0;
                if let Some(target) = &self.target {
                    // Borrowed separately from `runner` so the frame can render into the
                    // target while the runner mutates itself.
                    let view = target.view.clone();
                    self.runner.frame(now_ms, &view);
                    ui.painter().image(
                        target.id,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

                // Drawn after the image so it sits on top.
                self.draw_overlay(ui, rect);
            });
    }
}

mod input {
    use super::*;

    /// Turn egui's pointer and keyboard state into the [`InputEvent`]s the runner
    /// consumes. Coordinates are viewport-local device pixels, which is what the camera
    /// expects.
    pub fn forward<A: ShellApp>(
        shell: &mut Shell<A>,
        ui: &egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
        ppp: f32,
    ) {
        let local = |p: egui::Pos2| ((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);

        let (shift, ctrl, alt, pointer, scroll) = ui.input(|i| {
            (
                i.modifiers.shift,
                i.modifiers.ctrl,
                i.modifiers.alt,
                i.pointer.interact_pos(),
                i.smooth_scroll_delta.y,
            )
        });

        let mouse = |p: egui::Pos2, button: u8, buttons: u8| {
            let (x, y) = local(p);
            MouseEvent { x, y, button, buttons, shift, ctrl, alt }
        };

        // The runner reads DOM conventions: a button index on the event, and a bitmask
        // of held buttons where left is 1, right is 2 and middle is 4.
        let mask = |button: u8| match button {
            1 => 0b100,
            2 => 0b010,
            _ => 0b001,
        };

        if let Some(p) = pointer {
            if response.drag_started() {
                let button = if response.drag_started_by(egui::PointerButton::Secondary) {
                    2
                } else if response.drag_started_by(egui::PointerButton::Middle) {
                    1
                } else {
                    0
                };
                shell.dragging = true;
                shell.drag_button = button;
                shell.runner.input(InputEvent::MousePressed(mouse(p, button, mask(button))));
            } else if response.dragged() {
                let button = shell.drag_button;
                shell.runner.input(InputEvent::MouseDragged(mouse(p, button, mask(button))));
            } else if response.drag_stopped() {
                shell.dragging = false;
                shell.runner.input(InputEvent::MouseReleased(mouse(p, shell.drag_button, 0)));
            } else if response.hovered() {
                shell.runner.input(InputEvent::MouseMoved(mouse(p, 0, 0)));
            }

            if response.hovered() && scroll != 0.0 {
                let (x, y) = local(p);
                // egui scrolls positive upward and the DOM positive downward.
                shell.runner.input(InputEvent::Scrolled(ScrollEvent {
                    x,
                    y,
                    delta_y: -scroll,
                    shift,
                    ctrl,
                    alt,
                }));
            }
        }

        // Keys are global: a shortcut should work wherever the pointer happens to be.
        let keys: Vec<String> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, .. } => Some(key.name().to_lowercase()),
                    _ => None,
                })
                .collect()
        });
        for key in keys {
            shell.runner.input(InputEvent::KeyPressed(KeyEvent { key, shift, ctrl, alt }));
        }
    }
}

mod panel {
    use super::*;

    /// Below this width the panel becomes a drawer rather than a dock.
    pub const COMPACT_WIDTH: f32 = 760.0;

    /// What the docked panel starts at, and what the drawer uses.
    pub const DEFAULT_WIDTH: f32 = 340.0;
    /// The docked panel's id salt, so its remembered width can be found and changed.
    pub const DOCK_ID: &str = "fathom-controls";
    /// Narrow enough to give the simulation the screen; wide enough that a long label
    /// and its readout still fit on one row.
    pub const MIN_WIDTH: f32 = 280.0;
    pub const MAX_WIDTH: f32 = 560.0;

    /// The whole interface, built from the app's declared schema.
    ///
    /// Declaring a parameter is most of the work of getting a control for it: the panel
    /// is generated from the schema, grouped as declared.
    ///
    /// Docked beside the simulation on a wide screen; a drawer that slides in over it on
    /// a narrow one, because a docked column would take half a phone screen from the
    /// thing it controls.
    pub fn draw<A: ShellApp>(shell: &mut Shell<A>, ui: &mut egui::Ui) {
        let compact = ui.ctx().content_rect().width() < COMPACT_WIDTH;

        if compact {
            drawer(shell, ui);
        } else {
            shell.drawer_open = false;
            if let Some(width) = shell.panel_width_requested.take() {
                let id = egui::Id::new(DOCK_ID);
                if let Some(mut state) = egui::containers::panel::PanelState::load(ui.ctx(), id) {
                    state.outer_rect.min.x = state.outer_rect.max.x - width;
                    ui.ctx().data_mut(|d| d.insert_persisted(id, state));
                }
            }
            let response = egui::Panel::right(DOCK_ID)
                .default_size(shell.panel_width)
                .min_size(MIN_WIDTH)
                .max_size(MAX_WIDTH)
                .resizable(true)
                .frame(panel_frame())
                .show(ui, |ui| contents(shell, ui, false));
            // Remembered so the drawer and a later resize agree about the width, and so
            // the number survives the panel being rebuilt every frame.
            shell.panel_width = response.response.rect.width();
        }

        transport(shell, ui, compact);
    }

    fn panel_frame() -> egui::Frame {
        egui::Frame::NONE
            .fill(skin::PANEL)
            .inner_margin(egui::Margin::symmetric(18, 16))
    }

    /// The panel as a drawer: it slides in over the simulation and retracts again.
    ///
    /// It overlays rather than pushing the viewport aside. That is not only to match the
    /// web panel â€” the simulation's target texture is sized from the central panel, so a
    /// drawer that pushed would reallocate that texture, and the trail buffer with it, on
    /// every frame of the slide.
    fn drawer<A: ShellApp>(shell: &mut Shell<A>, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            shell.drawer_open = false;
        }

        let t = ctx.animate_bool_with_time(egui::Id::new("fathom-drawer"), shell.drawer_open, 0.24);
        if t <= 0.0 {
            return;
        }

        // Dim the simulation behind the drawer, and dismiss on a tap.
        let scrim = egui::Area::new(egui::Id::new("fathom-scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(&ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha((150.0 * t) as u8));
                response
            });
        if scrim.inner.clicked() {
            shell.drawer_open = false;
        }

        let width = (screen.width() * 0.86).min(shell.panel_width);
        let left = screen.right() - width * t;

        egui::Area::new(egui::Id::new("fathom-drawer-panel"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(left, screen.top()))
            .show(&ctx, |ui| {
                ui.set_width(width);
                panel_frame()
                    .stroke(egui::Stroke::new(1.0, skin::LINE))
                    .show(ui, |ui| {
                        ui.set_width(width - 36.0);
                        ui.set_min_height(screen.height() - 32.0);
                        contents(shell, ui, true);
                    });
            });
    }

    /// The panel's contents: which app this is, then every declared group.
    ///
    /// Built entirely from the schema, so declaring a parameter is still most of the work
    /// of getting a control for it. What the schema now also carries is which controls are
    /// worth showing first: a panel that shows everything at once shows nothing in
    /// particular, and most apps have a handful of parameters worth reaching for and a
    /// long tail that exists so the first few can be trusted.
    fn contents<A: ShellApp>(shell: &mut Shell<A>, ui: &mut egui::Ui, compact: bool) {
        let descriptor = Runner::<A>::descriptor();

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(descriptor.name)
                    .family(skin::semibold())
                    .size(17.0)
                    .color(skin::TEXT),
            );
            if compact {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if skin::action(ui, "Close").clicked() {
                        shell.drawer_open = false;
                    }
                });
            }
        });
        ui.label(
            egui::RichText::new(shell.runner.gpu().describe_adapter())
                .size(11.0)
                .color(skin::MUTED),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Presets and Randomize sit above the tabs: they act on the whole block, so
            // they belong to no tab and must be reachable from every one.
            if descriptor.presets_section {
                preset_section(shell, ui);
            }
            ui.horizontal(|ui| {
                if skin::action(ui, "Randomize")
                    .on_hover_text("Roll every unlocked parameter. Click a padlock to keep one where it is.")
                    .clicked()
                {
                    shell.randomize(ui);
                }
            });
            ui.add_space(8.0);

            // Tabs in declaration order. A control that names no tab goes on the first
            // one; an app that names none at all gets a single untabbed panel.
            let mut tabs: Vec<&'static str> = Vec::new();
            for t in descriptor.params.iter().map(|p| p.tab).chain(descriptor.commands.iter().map(|c| c.tab)) {
                if !t.is_empty() && !tabs.contains(&t) {
                    tabs.push(t);
                }
            }
            let tab_id = ui.make_persistent_id(("fathom-tab", descriptor.name));
            let mut current = ui.data_mut(|d| *d.get_persisted_mut_or(tab_id, 0usize)).min(tabs.len().saturating_sub(1));
            if tabs.len() > 1 {
                if let Some(picked) = skin::tabs(ui, "fathom-tabs", &tabs, current) {
                    current = picked;
                    ui.data_mut(|d| d.insert_persisted(tab_id, current));
                }
                ui.add_space(8.0);
            }
            let on_tab = |t: &'static str| tabs.is_empty() || t == tabs[current] || (t.is_empty() && current == 0);

            // Groups in declaration order, parameters before the commands filed under the
            // same heading.
            let mut groups: Vec<&'static str> = Vec::new();
            for p in descriptor.params {
                if on_tab(p.tab) && !groups.contains(&p.group) {
                    groups.push(p.group);
                }
            }
            for c in descriptor.commands {
                if on_tab(c.tab) && !groups.contains(&c.group) {
                    groups.push(c.group);
                }
            }

            for group in groups {
                if !skin::group_header(ui, group, true) {
                    continue;
                }

                // The plain controls, then the tail behind a disclosure. Commands go with
                // the plain ones: an app files a command under a group because it belongs
                // beside those controls, and burying a Reset button would be perverse.
                for (index, def) in descriptor.params.iter().enumerate() {
                    if def.group == group && on_tab(def.tab) && !def.advanced {
                        parameter(shell, ui, index, def);
                    }
                }
                commands(shell, ui, group, &descriptor);

                let advanced: Vec<usize> = descriptor
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| d.group == group && on_tab(d.tab) && d.advanced)
                    .map(|(i, _)| i)
                    .collect();
                if !advanced.is_empty() && skin::more_toggle(ui, group, advanced.len()) {
                    for index in advanced {
                        parameter(shell, ui, index, &descriptor.params[index]);
                    }
                }

                ui.add_space(8.0);
            }

            // Whatever the app wants below the generated controls.
            let egui = ui.ctx().clone();
            shell.hook(&egui, |app, ctx| app.panel(ui, ctx));
        });
    }

    /// Whether a parameter is held out of Randomize. Remembered by name, with the rest
    /// of the panel's state. Everything starts locked: Randomize is opt-in per control,
    /// so a stray click cannot scramble a block the user has just tuned.
    pub fn is_locked(ui: &egui::Ui, name: &str) -> bool {
        let id = lock_id(name);
        ui.data_mut(|d| *d.get_persisted_mut_or(id, true))
    }

    /// Keyed on the name alone, not off a `Ui`: the flag is read in the row's parent
    /// `Ui` and written in a child, and `make_persistent_id` would give each its own key.
    fn lock_id(name: &str) -> egui::Id {
        egui::Id::new(("fathom-lock", name))
    }

    fn set_locked(ui: &egui::Ui, name: &str, locked: bool) {
        let id = lock_id(name);
        ui.data_mut(|d| d.insert_persisted(id, locked));
    }

    /// Save… and Open… as files, a typed name that saves into the folder, and the list
    /// of what is available: built in and from the folder, merged. See [`presets`].
    fn preset_section<A: ShellApp>(shell: &mut Shell<A>, ui: &mut egui::Ui) {
        if !skin::group_header(ui, "Presets", true) {
            return;
        }
        shell.list_presets();

        ui.horizontal(|ui| {
            if skin::action(ui, "Save…")
                .on_hover_text(if cfg!(target_arch = "wasm32") {
                    "Download the current parameters as a .json file."
                } else {
                    "Write the current parameters to a file of your choosing."
                })
                .clicked()
            {
                shell.save_dialog();
            }
            if skin::action(ui, "Open…")
                .on_hover_text("Load parameters from a .json file. Dropping one on the window does the same.")
                .clicked()
            {
                shell.open_dialog();
            }
        });

        // The folder is a native thing. On the web the list is what was built in, and
        // a file you want to keep is a download.
        let mut save = false;
        if !cfg!(target_arch = "wasm32") {
            ui.horizontal(|ui| {
                let edit = egui::TextEdit::singleline(&mut shell.preset_name)
                    .hint_text("name")
                    .font(egui::FontId::proportional(12.0))
                    .desired_width(ui.available_width() - 56.0);
                let response = ui.add(edit);
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                if skin::action(ui, "Save")
                    .on_hover_text("Save into the presets folder under this name, so it appears in the list below.")
                    .clicked()
                {
                    save = true;
                }
            });
        }
        if save {
            let name = shell.preset_name.clone();
            shell.save_preset(&name);
        }

        let listed = shell.controls.presets.clone();
        let mut load: Option<String> = None;
        let mut remove: Option<String> = None;
        for preset in &listed {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&preset.name).size(12.0).color(skin::TEXT.gamma_multiply(0.92)));
                if preset.source == Source::BuiltIn {
                    ui.label(egui::RichText::new("built in").size(10.0).color(skin::MUTED));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if preset.source == Source::Folder
                        && skin::action(ui, "×").on_hover_text("delete this preset").clicked()
                    {
                        remove = Some(preset.name.clone());
                    }
                    if skin::action(ui, "Load").clicked() {
                        load = Some(preset.name.clone());
                    }
                });
            });
        }
        if let Some(name) = load {
            shell.load_preset(&name);
        }
        if let Some(name) = remove {
            shell.delete_preset(&name);
        }

        if !shell.status.is_empty() {
            ui.label(egui::RichText::new(&shell.status).size(11.0).color(skin::MUTED));
        }
        ui.add_space(8.0);
    }

    /// One parameter, drawn as whatever its kind calls for, with two things every kind
    /// shares: a right-click puts it back to its default, and resting the pointer on it
    /// shows what it does.
    fn parameter<A: ShellApp>(
        shell: &mut Shell<A>,
        ui: &mut egui::Ui,
        index: usize,
        def: &ParamDef,
    ) {
        let locked = is_locked(ui, def.name);
        let response = ui
            .horizontal(|ui| {
                if skin::padlock(ui, def.name, locked).clicked() {
                    set_locked(ui, def.name, !locked);
                }
                ui.vertical(|ui| control_for(shell, ui, index, def));
            })
            .response;
        if !ui.rect_contains_pointer(response.rect) {
            return;
        }
        // Read from the raw pointer rather than the control's response: the control
        // owns the primary button for dragging, and the reset must work anywhere on the
        // row, label included.
        if ui.input(|i| i.pointer.secondary_clicked()) {
            shell.runner.params_mut().set_u32(index, def.default_word());
        }
        let default = match def.kind {
            ParamKind::Float => control::format(def.default, def.min, def.max, def.step),
            ParamKind::Int => format!("{}", def.default as u32),
            ParamKind::Toggle => if def.default != 0.0 { "on".into() } else { "off".into() },
            ParamKind::Choice => def.options.get(def.default as usize).copied().unwrap_or("?").into(),
        };
        response.on_hover_ui(|ui| {
            ui.set_max_width(240.0);
            if !def.description.is_empty() {
                ui.label(egui::RichText::new(def.description).size(12.0).color(skin::TEXT));
            }
            let held = if locked { " · locked out of Randomize" } else { "" };
            ui.label(
                egui::RichText::new(format!("default {default} · right-click to reset{held}"))
                    .size(11.0)
                    .color(skin::MUTED),
            );
        });
    }

    fn control_for<A: ShellApp>(
        shell: &mut Shell<A>,
        ui: &mut egui::Ui,
        index: usize,
        def: &ParamDef,
    ) {
        let block = shell.runner.params_mut();
        match def.kind {
            ParamKind::Float | ParamKind::Int => {
                let is_int = def.kind == ParamKind::Int;
                let mut value = if is_int { block.u32(index) as f32 } else { block.f32(index) };
                // An int is a float with a step of one everywhere except in storage, which
                // is what lets both share a control.
                let step = if is_int { 1.0 } else { def.step };
                if skin::scrubber(ui, def.label, &mut value, def.min, def.max, step) {
                    if is_int {
                        block.set_u32(index, value.round() as u32);
                    } else {
                        block.set_f32(index, value);
                    }
                }
            }
            ParamKind::Toggle => {
                let mut on = block.u32(index) != 0;
                if skin::row(ui, def.label, |ui| skin::pill_toggle(ui, &mut on)) {
                    block.set_u32(index, on as u32);
                }
            }
            ParamKind::Choice => {
                let current = block.u32(index) as usize;
                if let Some(picked) = choice(ui, def.name, def.label, def.options, current) {
                    shell.runner.params_mut().set_u32(index, picked as u32);
                }
            }
        }
    }

    /// A one-of-N control, segmented if the options fit and a dropdown if they do not.
    ///
    /// Measured rather than guessed from the option count: "Classic | Well" fits in a
    /// narrow panel and five presets named like sentences do not, and which is which
    /// depends on how wide the panel has been dragged.
    fn choice(
        ui: &mut egui::Ui,
        key: &str,
        label: &str,
        options: &[&'static str],
        selected: usize,
    ) -> Option<usize> {
        let widths = skin::segment_widths(ui, options);
        if control::segments_fit(&widths, 22.0, ui.available_width()) {
            ui.label(egui::RichText::new(label).size(11.0).color(skin::MUTED));
            skin::segmented(ui, key, options, selected)
        } else {
            skin::dropdown(ui, key, label, options, selected)
        }
    }

    /// The commands filed under one group: selects first, then the buttons on one row.
    fn commands<A: ShellApp>(
        shell: &mut Shell<A>,
        ui: &mut egui::Ui,
        group: &str,
        descriptor: &fathom_core::AppDescriptor,
    ) {
        for def in descriptor.commands.iter().filter(|c| c.group == group) {
            if def.options.is_empty() {
                continue;
            }
            let selected = shell.choice_of(def.name, def.initial as usize);
            let scoped = ui.scope(|ui| choice(ui, def.name, def.label, def.options, selected));
            if !def.description.is_empty() {
                scoped.response.on_hover_text(def.description);
            }
            if let Some(picked) = scoped.inner {
                shell.remember_choice(def.name, picked);
                shell.dispatch(def.name, serde_json::json!({ "value": picked }));
            }
        }

        let buttons: Vec<_> = descriptor
            .commands
            .iter()
            .filter(|c| c.group == group && c.options.is_empty())
            .collect();
        if !buttons.is_empty() {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                for def in buttons {
                    let response = skin::action(ui, def.label);
                    let response = if def.description.is_empty() {
                        response
                    } else {
                        response.on_hover_text(def.description)
                    };
                    if response.clicked() {
                        shell.dispatch(def.name, serde_json::json!({}));
                    }
                }
            });
        }
    }

    /// Play, step, recentre, and the frame counters.
    /// A floating bar rather than a docked one, the same arrangement the web panel uses:
    /// interface over simulation, and no strip of chrome eating height on a phone.
    fn transport<A: ShellApp>(shell: &mut Shell<A>, ui: &mut egui::Ui, compact: bool) {
        let ctx = ui.ctx().clone();

        egui::Area::new(egui::Id::new("fathom-transport"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(16.0, -16.0))
            .show(&ctx, |ui| {
                floating_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                let paused = shell.runner.is_paused();
                let label = if paused { "Run" } else { "Pause" };
                if skin::action(ui, label).clicked() {
                    let command = if paused { "fathom.resume" } else { "fathom.pause" };
                    shell.dispatch(command, serde_json::Value::Null);
                }
                ui.add_enabled_ui(paused, |ui| {
                    if skin::action(ui, "Step").clicked() {
                        shell.dispatch("fathom.step", serde_json::Value::Null);
                    }
                });
                if skin::action(ui, "Recentre").clicked() {
                    shell.dispatch("fathom.reset_camera", serde_json::Value::Null);
                }

                ui.add_space(6.0);
                let stats = shell.runner.stats();
                let mut readouts = vec![(format!("{:.0}", stats.fps), "fps")];
                if !compact {
                    readouts.push((format!("{:.1}", stats.frame_ms), "ms"));
                }
                for (value, unit) in readouts {
                    ui.label(
                        egui::RichText::new(value)
                            .family(egui::FontFamily::Monospace)
                            .size(11.0)
                            .color(skin::TEXT),
                    );
                    if !compact {
                        ui.label(egui::RichText::new(unit).size(11.0).color(skin::MUTED));
                    }
                    ui.add_space(4.0);
                }

                // A dot rather than a word carries the state at a glance; the word
                // is there for anyone who needs it spelled out.
                let (dot, word) = if shell.runner.is_paused() {
                    (skin::WARM, "Paused")
                } else {
                    (skin::ACCENT, "Running")
                };
                let (r, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 3.0, dot);
                if !compact {
                    ui.label(egui::RichText::new(word).size(11.0).color(skin::MUTED));
                }

                    });
                });
            });

        // While the drawer is out it covers this corner, and the scrim, Escape and the
        // drawer's own Close button are all available, so the toggle stands down.
        if compact && !shell.drawer_open {
            egui::Area::new(egui::Id::new("fathom-controls-toggle"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
                .show(&ctx, |ui| {
                    floating_frame().show(ui, |ui| {
                        if skin::action(ui, "Controls").clicked() {
                            shell.drawer_open = true;
                        }
                    });
                });
        }
    }

    /// The casing shared by the things that float over the simulation.
    fn floating_frame() -> egui::Frame {
        egui::Frame::NONE
            .fill(skin::PANEL.gamma_multiply(0.94))
            .stroke(egui::Stroke::new(1.0, skin::LINE))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(12, 7))
    }
}

/// Run an app in a native window.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native<A: ShellApp>(title: &str) -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title(title),
        ..Default::default()
    };

    eframe::run_native(
        title,
        options,
        Box::new(|cc| {
            skin::install(&cc.egui_ctx);
            Ok(Box::new(Shell::<A>::new(cc)?) as Box<dyn eframe::App>)
        }),
    )
}

/// Run an app in a browser canvas.
///
/// The same shell, the same panel, the same app: on this target eframe draws egui into
/// the canvas and wgpu talks to WebGPU, so no part of the interface is HTML.
#[cfg(target_arch = "wasm32")]
pub async fn run_web<A: ShellApp>(
    canvas: web_sys::HtmlCanvasElement,
) -> Result<(), eframe::wasm_bindgen::JsValue> {
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| {
                skin::install(&cc.egui_ctx);
                Ok(Box::new(Shell::<A>::new(cc)?) as Box<dyn eframe::App>)
            }),
        )
        .await
}


/// The hooks, exercised through a real shell over a real device with no window: each
/// one must fire, see the runner's state, and have its requests honoured.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use fathom_core::{AppDescriptor, CommandDef, DrawCtx, ParamDef, SetupCtx, UpdateCtx};
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// What the probe app saw, shared with the test through an `Rc` because the shell
    /// owns the app.
    #[derive(Debug)]
    struct Seen {
        panel: u32,
        overlay: u32,
        overlay_rect: egui::Rect,
        intercepted: Vec<String>,
        state: Option<Value>,
        files: Vec<(String, usize)>,
    }

    struct Probe {
        seen: Rc<RefCell<Seen>>,
    }

    impl Default for Seen {
        fn default() -> Self {
            Self {
                panel: 0,
                overlay: 0,
                overlay_rect: egui::Rect::ZERO,
                intercepted: Vec::new(),
                state: None,
                files: Vec::new(),
            }
        }
    }

    thread_local! {
        static SEEN: Rc<RefCell<Seen>> = Rc::default();
    }

    const SCHEMA: &[ParamDef] = &[
        ParamDef::float("g", "Gravity", 1.0, 0.0, 4.0).describe("pull"),
        ParamDef::toggle("trails", "Trails", true),
    ];
    const COMMANDS: &[CommandDef] = &[CommandDef::button("reset", "Reset").describe("start again")];

    impl fathom_core::App for Probe {
        fn describe() -> AppDescriptor {
            AppDescriptor::new("Probe", SCHEMA, COMMANDS)
        }
        fn setup(_ctx: &mut SetupCtx<'_>) -> Self {
            Self { seen: SEEN.with(Rc::clone) }
        }
        fn update(&mut self, _ctx: &mut UpdateCtx<'_>) {}
        fn draw(&mut self, _ctx: &mut DrawCtx<'_>) {}
    }

    impl ShellApp for Probe {
        fn panel(&mut self, ui: &mut egui::Ui, ctx: &mut ShellCtx<'_>) {
            self.seen.borrow_mut().panel += 1;
            ui.label("from the app");
            // A hook writes a parameter, pauses, and asks the shell for things.
            ctx.set_float(0, 2.5);
            *ctx.paused = true;
            ctx.shell.set_panel_width(400.0);
            ctx.shell.status("hello from the panel hook");
        }

        fn overlay(&mut self, ui: &mut egui::Ui, ctx: &mut ShellCtx<'_>) {
            let mut seen = self.seen.borrow_mut();
            seen.overlay += 1;
            seen.overlay_rect = ui.max_rect();
            assert_eq!(ctx.viewport_rect, ui.max_rect());
        }

        fn intercept(&mut self, name: &str, _value: &Value, _ctx: &mut ShellCtx<'_>) -> bool {
            self.seen.borrow_mut().intercepted.push(name.to_string());
            name == "fathom.pause"
        }

        fn save_state(&self) -> Value {
            json!({ "answer": 42 })
        }

        fn load_state(&mut self, state: &Value, _ctx: &mut ShellCtx<'_>) {
            self.seen.borrow_mut().state = Some(state.clone());
        }

        fn file_received(&mut self, name: &str, bytes: &[u8], _ctx: &mut ShellCtx<'_>) {
            self.seen.borrow_mut().files.push((name.to_string(), bytes.len()));
        }
    }

    fn shell() -> Option<(Shell<Probe>, egui::Context)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fathom shell test"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        }))
        .ok()?;
        let gpu = Gpu {
            device: device.clone(),
            queue,
            format: wgpu::TextureFormat::Rgba8Unorm,
            adapter_info: adapter.get_info(),
        };
        let renderer = egui_wgpu::Renderer::new(&device, gpu.format, egui_wgpu::RendererOptions::default());
        let egui = egui::Context::default();
        skin::install(&egui);
        Some((Shell::with_gpu(gpu, Arc::new(egui::mutex::RwLock::new(renderer)), &egui), egui))
    }

    fn seen() -> Rc<RefCell<Seen>> {
        SEEN.with(Rc::clone)
    }

    #[test]
    fn every_hook_fires_and_its_requests_are_honoured() {
        let Some((mut shell, egui)) = shell() else {
            eprintln!("no GPU adapter: skipping");
            return;
        };
        *seen().borrow_mut() = Seen::default();

        // Intercept sees framework commands and can swallow them: pause is refused,
        // step is not.
        shell.dispatch("fathom.pause", Value::Null);
        assert!(!shell.runner.is_paused(), "the app swallowed the pause");
        shell.dispatch("reset", json!({}));
        assert_eq!(seen().borrow().intercepted, vec!["fathom.pause", "reset"]);

        // The panel hook is drawn below the generated controls and its requests land.
        let input_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)));
        let input = egui::RawInput { screen_rect: input_rect, ..Default::default() };
        let mut output = egui.run_ui(input, |ui| {
            panel::draw(&mut shell, ui);
            egui::CentralPanel::no_frame().show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                shell.viewport_rect = rect;
                shell.draw_overlay(ui, rect);
            });
        });
        // Nothing here uploads the font atlas; egui asserts it was not forgotten.
        output.textures_delta.clear();
        {
            let seen = seen();
            let seen = seen.borrow();
            assert_eq!(seen.panel, 1, "panel hook drawn once per frame");
            assert_eq!(seen.overlay, 1, "overlay hook drawn once per frame");
            assert!(seen.overlay_rect.width() > 100.0, "the overlay covers the viewport");
        }
        assert_eq!(shell.runner.params().f32(0), 2.5, "a hook can write a parameter");
        assert!(shell.runner.is_paused(), "a hook can pause");
        assert_eq!(shell.status, "hello from the panel hook");
        // The docked panel reports its width after its contents are drawn, so a width
        // asked for during a frame shows in the next one.
        let mut output = egui.run_ui(egui::RawInput { screen_rect: input_rect, ..Default::default() }, |ui| {
            panel::draw(&mut shell, ui);
        });
        output.textures_delta.clear();
        assert_eq!(shell.panel_width, 400.0, "a hook can resize the panel");

        // A preset for this app applies its params and hands the app its state.
        let file = presets::encode("Probe", SCHEMA, &fathom_core::ParamBlock::from_defaults(SCHEMA), json!({ "answer": 42 }));
        shell.receive_file("one.json", file.as_bytes());
        assert_eq!(shell.runner.params().f32(0), 1.0, "the preset's params were applied");
        assert_eq!(seen().borrow().state, Some(json!({ "answer": 42 })));

        // Anything else — another app's preset, or not JSON at all — goes to the app.
        shell.receive_file("other.json", br#"{ "app": "Other", "params": {} }"#);
        shell.receive_file("notes.txt", b"hello");
        assert_eq!(seen().borrow().files, vec![("other.json".to_string(), 32), ("notes.txt".to_string(), 5)]);
        assert_eq!(shell.runner.params().f32(0), 1.0, "another app's file changed nothing");

        // What Save… would write carries the app's state.
        let doc = presets::decode(&shell.encode_preset()).unwrap();
        assert_eq!(doc.app, "Probe");
        assert_eq!(doc.state, json!({ "answer": 42 }));
    }
}
