//! An egui shell for fathom apps: one window, no web technology.
//!
//! This is a third host for the same [`fathom_core::App`] trait, alongside the wasm and
//! Tauri ones — and the app crate it runs is byte-for-byte the one they run. What
//! changes is only who draws the interface.
//!
//! The Tauri host buys a real webview for the panel, and pays for it: two windows on
//! Windows, an IPC hop for every parameter, and a parameter *mirror* on each side of a
//! language boundary. Here the panel is Rust, drawn by egui into the same window and
//! with the same wgpu device as the simulation. So:
//!
//! * one window, which means the taskbar, alt-tab and its preview thumbnail, snapping
//!   and per-monitor DPI all work because the OS is not being tricked;
//! * a slider writes straight into the live parameter block — no mirror, no IPC, no
//!   serialisation anywhere in the frame;
//! * the same shell compiles to the browser through eframe, where egui draws to a
//!   canvas, so this host covers both targets on its own.
//!
//! The simulation renders into an offscreen texture that the panel then shows as an
//! image. That is what keeps the app's `draw` unchanged: it still receives a target view
//! and renders into it, exactly as it does when that view is a swapchain.

pub mod skin;

use std::sync::Arc;

use eframe::egui;
use egui_wgpu::wgpu;
use fathom_core::{
    App, Gpu, InputEvent, KeyEvent, MouseEvent, ParamKind, Runner, ScrollEvent, Viewport,
};

/// The texture the simulation draws into, and its identity inside egui.
struct Target {
    view: wgpu::TextureView,
    id: egui::TextureId,
    width: u32,
    height: u32,
}

pub struct Shell<A: App> {
    runner: Runner<A>,
    target: Option<Target>,
    /// Kept to register and release textures as the viewport resizes.
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    /// Whether the simulation is paused, mirrored here so the transport can show it.
    device: wgpu::Device,
    paused: bool,
    /// Whether the pointer was down over the viewport last frame, so a move can be
    /// reported as a drag rather than a hover.
    dragging: bool,
    /// The option each command-select is showing. Commands are fire-and-forget, so
    /// unlike parameters they have no stored value to read back.
    command_choice: Vec<(&'static str, usize)>,
    /// Whether the drawer is out, on a screen narrow enough to have one.
    drawer_open: bool,
}

impl<A: App> Shell<A> {
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

        Ok(Self {
            runner: Runner::new(gpu, Viewport::new(1, 1, cc.egui_ctx.pixels_per_point())),
            target: None,
            renderer: render_state.renderer.clone(),
            device: render_state.device.clone(),
            paused: false,
            dragging: false,
            command_choice: Vec::new(),
            drawer_open: false,
        })
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
}

impl<A: App> eframe::App for Shell<A> {
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

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // A simulation is never idle, so the shell drives repaints rather than waiting
        // for input the way an ordinary egui app would.
        ui.ctx().request_repaint();

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
            });

    }
}

mod input {
    use super::*;

    /// Turn egui's pointer and keyboard state into the same [`InputEvent`]s the web and
    /// Tauri hosts send. Coordinates are viewport-local device pixels, which is what the
    /// camera expects.
    pub fn forward<A: App>(
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

        if let Some(p) = pointer {
            if response.drag_started() {
                shell.dragging = true;
                shell.runner.input(InputEvent::MousePressed(mouse(p, 0, 1)));
            } else if response.dragged() {
                // egui reports the middle button separately; the runner reads the DOM
                // bitmask, where bit 2 is the middle button.
                let middle = ui.input(|i| i.pointer.middle_down());
                let buttons = if middle { 0b100 } else { 0b1 };
                shell.runner.input(InputEvent::MouseDragged(mouse(p, 0, buttons)));
            } else if response.drag_stopped() {
                shell.dragging = false;
                shell.runner.input(InputEvent::MouseReleased(mouse(p, 0, 0)));
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

    /// The width of the drawer, and of the docked panel.
    const PANEL_WIDTH: f32 = 296.0;

    /// The whole interface, built from the app's declared schema.
    ///
    /// This is the Rust twin of `<AutoControls/>` in the React host, and it is the same
    /// bargain: declaring a parameter is most of the work of getting a control for it.
    ///
    /// Docked beside the simulation on a wide screen; a drawer that slides in over it on
    /// a narrow one, because a docked column would take half a phone screen from the
    /// thing it controls.
    pub fn draw<A: App>(shell: &mut Shell<A>, ui: &mut egui::Ui) {
        let compact = ui.ctx().content_rect().width() < COMPACT_WIDTH;

        if compact {
            drawer(shell, ui);
        } else {
            shell.drawer_open = false;
            egui::Panel::right("controls")
                .exact_size(PANEL_WIDTH)
                .resizable(false)
                .frame(panel_frame())
                .show(ui, |ui| contents(shell, ui, false));
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
    /// web panel — the simulation's target texture is sized from the central panel, so a
    /// drawer that pushed would reallocate that texture, and the trail buffer with it, on
    /// every frame of the slide.
    fn drawer<A: App>(shell: &mut Shell<A>, ui: &mut egui::Ui) {
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

        let width = (screen.width() * 0.86).min(PANEL_WIDTH);
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
    fn contents<A: App>(shell: &mut Shell<A>, ui: &mut egui::Ui, compact: bool) {
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
        ui.add_space(4.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Groups in declaration order, parameters before the commands filed
            // under the same heading.
            let mut groups: Vec<&'static str> = Vec::new();
            for p in descriptor.params {
                if !groups.contains(&p.group) {
                    groups.push(p.group);
                }
            }
            for c in descriptor.commands {
                if !groups.contains(&c.group) {
                    groups.push(c.group);
                }
            }

            for group in groups {
                skin::group_heading(ui, group);

                for (index, def) in descriptor.params.iter().enumerate() {
                    if def.group != group {
                        continue;
                    }
                    let block = shell.runner.params_mut();
                    match def.kind {
                        ParamKind::Float | ParamKind::Int => {
                            let is_int = def.kind == ParamKind::Int;
                            let mut value = if is_int {
                                block.u32(index) as f32
                            } else {
                                block.f32(index)
                            };

                            let span = def.max - def.min;
                            let decimals = if is_int {
                                0
                            } else if span >= 20.0 {
                                0
                            } else if span >= 2.0 {
                                2
                            } else {
                                3
                            };
                            skin::readout(ui, def.label, &format!("{value:.decimals$}"));

                            let step = if is_int { Some(1.0) } else { None };
                            if skin::needle_slider(ui, &mut value, def.min, def.max, step) {
                                if is_int {
                                    block.set_u32(index, value.round() as u32);
                                } else {
                                    block.set_f32(index, value);
                                }
                            }
                        }
                        ParamKind::Toggle => {
                            let mut on = block.u32(index) != 0;
                            let changed =
                                skin::row(ui, def.label, |ui| skin::pill_toggle(ui, &mut on));
                            if changed {
                                block.set_u32(index, on as u32);
                            }
                        }
                        ParamKind::Choice => {
                            let mut value = block.u32(index) as usize;
                            let current = def.options.get(value).copied().unwrap_or("");
                            skin::row(ui, def.label, |ui| {
                                egui::ComboBox::from_id_salt(def.name)
                                    .selected_text(current)
                                    .show_ui(ui, |ui| {
                                        for (i, option) in def.options.iter().enumerate() {
                                            ui.selectable_value(&mut value, i, *option);
                                        }
                                    });
                            });
                            if value as u32 != block.u32(index) {
                                block.set_u32(index, value as u32);
                            }
                        }
                    }
                    ui.add_space(4.0);
                }

                // Selects first, then the buttons together on one row, the same
                // arrangement the web panel uses.
                for def in descriptor.commands.iter().filter(|c| c.group == group) {
                    if def.options.is_empty() {
                        continue;
                    }
                    let selected = shell.choice_of(def.name, def.initial as usize);
                    let mut value = selected;
                    skin::row(ui, def.label, |ui| {
                        egui::ComboBox::from_id_salt(def.name)
                            .selected_text(def.options.get(value).copied().unwrap_or(""))
                            .show_ui(ui, |ui| {
                                for (i, option) in def.options.iter().enumerate() {
                                    ui.selectable_value(&mut value, i, *option);
                                }
                            });
                    });
                    if value != selected {
                        shell.remember_choice(def.name, value);
                        shell
                            .runner
                            .command(def.name, serde_json::json!({ "value": value }));
                    }
                    ui.add_space(4.0);
                }

                let buttons: Vec<_> = descriptor
                    .commands
                    .iter()
                    .filter(|c| c.group == group && c.options.is_empty())
                    .collect();
                if !buttons.is_empty() {
                    ui.horizontal(|ui| {
                        for def in buttons {
                            if skin::action(ui, def.label).clicked() {
                                shell.runner.command(def.name, serde_json::json!({}));
                            }
                        }
                    });
                }
                ui.add_space(6.0);
            }
        });
    }
    /// Play, step, recentre, and the frame counters.
    /// A floating bar rather than a docked one, the same arrangement the web panel uses:
    /// interface over simulation, and no strip of chrome eating height on a phone.
    fn transport<A: App>(shell: &mut Shell<A>, ui: &mut egui::Ui, compact: bool) {
        let ctx = ui.ctx().clone();

        egui::Area::new(egui::Id::new("fathom-transport"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(16.0, -16.0))
            .show(&ctx, |ui| {
                floating_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                let label = if shell.paused { "Run" } else { "Pause" };
                if skin::action(ui, label).clicked() {
                    shell.paused = !shell.paused;
                    let command =
                        if shell.paused { "fathom.pause" } else { "fathom.resume" };
                    shell.runner.command(command, serde_json::Value::Null);
                }
                ui.add_enabled_ui(shell.paused, |ui| {
                    if skin::action(ui, "Step").clicked() {
                        shell.runner.command("fathom.step", serde_json::Value::Null);
                    }
                });
                if skin::action(ui, "Recentre").clicked() {
                    shell.runner.command("fathom.reset_camera", serde_json::Value::Null);
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
                let (dot, word) = if shell.paused {
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
pub fn run_native<A: App>(title: &str) -> eframe::Result {
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
pub async fn run_web<A: App>(
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

