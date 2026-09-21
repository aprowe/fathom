//! The transport: run/pause, a single step, the frame rate and a state dot.

use eframe::egui::{self, FontFamily, Sense, vec2};

use crate::skin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportAction {
    /// Pause if running, run if paused.
    Toggle,
    /// One update, even while paused.
    Step,
}

/// The transport on one row. `compact` drops the state word and keeps the dot;
/// `with_step` adds a single-step button.
pub fn transport(ui: &mut egui::Ui, paused: bool, fps: f32, compact: bool, with_step: bool) -> Option<TransportAction> {
    let mut action = None;
    if skin::action(ui, if paused { "Run" } else { "Pause" }).on_hover_text("Space").clicked() {
        action = Some(TransportAction::Toggle);
    }
    if with_step && skin::action(ui, "Step").on_hover_text("One update, even while paused.").clicked() {
        action = Some(TransportAction::Step);
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(format!("{fps:.0}")).family(FontFamily::Monospace).size(11.0).color(skin::TEXT));
    ui.label(egui::RichText::new("fps").size(11.0).color(skin::MUTED));
    ui.add_space(4.0);
    // A dot rather than a word carries the state at a glance; the word is there for
    // anyone who needs it spelled out.
    let (dot, word) = if paused { (skin::WARM, "Paused") } else { (skin::ACCENT, "Running") };
    let (r, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 3.0, dot);
    if !compact {
        ui.label(egui::RichText::new(word).size(11.0).color(skin::MUTED));
    }
    action
}
