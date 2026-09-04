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
        [0.02, 0.024, 0.035, 1.0]
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

    /// The whole interface, built from the app's declared schema.
    ///
    /// This is the Rust twin of `<AutoControls/>` in the React host, and it is the same
    /// bargain: declaring a parameter is most of the work of getting a control for it.
    /// A control's name on the left and its value on the right, in tabular figures so
    /// the number does not shift sideways as it changes.
    fn labelled(ui: &mut egui::Ui, label: &str, value: &str) {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(
                    egui::RichText::new(value).color(egui::Color32::from_rgb(0x6f, 0xd2, 0xff)),
                );
            });
        });
    }

    pub fn draw<A: App>(shell: &mut Shell<A>, ui: &mut egui::Ui) {
        let descriptor = Runner::<A>::descriptor();

        egui::Panel::bottom("transport").resizable(false).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let label = if shell.paused { "Run" } else { "Pause" };
                if ui.button(label).clicked() {
                    shell.paused = !shell.paused;
                    let command =
                        if shell.paused { "fathom.pause" } else { "fathom.resume" };
                    shell.runner.command(command, serde_json::Value::Null);
                }
                if ui.add_enabled(shell.paused, egui::Button::new("Step")).clicked() {
                    shell.runner.command("fathom.step", serde_json::Value::Null);
                }
                if ui.button("Recentre").clicked() {
                    shell.runner.command("fathom.reset_camera", serde_json::Value::Null);
                }

                ui.separator();
                let stats = shell.runner.stats();
                ui.monospace(format!("{:>3.0} fps", stats.fps));
                ui.monospace(format!("{:>5.1} ms", stats.frame_ms));
                ui.label(if shell.paused { "Paused" } else { "Running" });
            });
            ui.add_space(4.0);
        });

        egui::Panel::right("controls")
            .exact_size(288.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(10.0);
                ui.heading(descriptor.name);
                ui.label(
                    egui::RichText::new(shell.runner.gpu().describe_adapter())
                        .small()
                        .weak(),
                );
                ui.add_space(6.0);

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
                        ui.separator();
                        ui.label(egui::RichText::new(group).strong());
                        ui.add_space(4.0);

                        for (index, def) in descriptor.params.iter().enumerate() {
                            if def.group != group {
                                continue;
                            }
                            let width = ui.available_width();
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
                                    labelled(ui, def.label, &format!("{value:.decimals$}"));

                                    // egui lays a slider out as [handle][value][label] on
                                    // one line, which has no room at panel width. The
                                    // label and value go above instead, and the slider
                                    // takes the full width beneath them.
                                    ui.spacing_mut().slider_width = width;
                                    let slider = egui::Slider::new(&mut value, def.min..=def.max)
                                        .show_value(false);
                                    let slider = if is_int { slider.step_by(1.0) } else { slider };
                                    if ui.add(slider).changed() {
                                        if is_int {
                                            block.set_u32(index, value.round() as u32);
                                        } else {
                                            block.set_f32(index, value);
                                        }
                                    }
                                }
                                ParamKind::Toggle => {
                                    let mut on = block.u32(index) != 0;
                                    if ui.checkbox(&mut on, def.label).changed() {
                                        block.set_u32(index, on as u32);
                                    }
                                }
                                ParamKind::Choice => {
                                    let mut value = block.u32(index) as usize;
                                    let current = def.options.get(value).copied().unwrap_or("");
                                    ui.horizontal(|ui| {
                                        ui.label(def.label);
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
                            ui.add_space(8.0);
                        }

                        // Selects first, then the buttons together on one row, the same
                        // arrangement the web panel uses.
                        for def in descriptor.commands.iter().filter(|c| c.group == group) {
                            if def.options.is_empty() {
                                continue;
                            }
                            let selected = shell.choice_of(def.name, def.initial as usize);
                            let mut value = selected;
                            ui.horizontal(|ui| {
                                ui.label(def.label);
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
                            ui.add_space(8.0);
                        }

                        let buttons: Vec<_> = descriptor
                            .commands
                            .iter()
                            .filter(|c| c.group == group && c.options.is_empty())
                            .collect();
                        if !buttons.is_empty() {
                            ui.horizontal(|ui| {
                                for def in buttons {
                                    if ui.button(def.label).clicked() {
                                        shell.runner.command(def.name, serde_json::json!({}));
                                    }
                                }
                            });
                        }
                        ui.add_space(6.0);
                    }
                });
            });
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
            theme::apply(&cc.egui_ctx);
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
                theme::apply(&cc.egui_ctx);
                Ok(Box::new(Shell::<A>::new(cc)?) as Box<dyn eframe::App>)
            }),
        )
        .await
}

mod theme {
    use super::*;

    /// The same instrument casing the web panel wears, in egui's terms: a cool slate
    /// ground with the accent lifted from the simulation's own colour ramp.
    pub fn apply(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        let ground = egui::Color32::from_rgb(0x12, 0x17, 0x22);
        let raised = egui::Color32::from_rgb(0x1a, 0x21, 0x2c);
        let line = egui::Color32::from_rgb(0x23, 0x2c, 0x3a);
        let accent = egui::Color32::from_rgb(0x6f, 0xd2, 0xff);

        visuals.panel_fill = ground;
        visuals.window_fill = ground;
        visuals.extreme_bg_color = egui::Color32::from_rgb(0x0e, 0x11, 0x16);
        visuals.widgets.noninteractive.bg_fill = raised;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, line);
        visuals.widgets.inactive.bg_fill = raised;
        visuals.widgets.hovered.bg_fill = line;
        visuals.widgets.active.bg_fill = accent.gamma_multiply(0.5);
        visuals.selection.bg_fill = accent.gamma_multiply(0.4);
        visuals.selection.stroke = egui::Stroke::new(1.0, accent);
        visuals.override_text_color = Some(egui::Color32::from_rgb(0xdc, 0xe3, 0xed));

        ctx.set_visuals(visuals);
    }
}
