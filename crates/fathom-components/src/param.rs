//! One parameter as a control: padlock, the control its kind calls for, a tooltip that
//! says what it does, and a right-click that puts it back to its default.
//!
//! The lock lives in egui's persisted data, keyed by the parameter's name, so the shell
//! (which honours locks in Randomize) and any app drawing its own chrome agree on it
//! without passing state around.

use eframe::egui;
use fathom_core::{ParamBlock, ParamDef, ParamKind};

use crate::{control, skin};

/// Whether a parameter is held where it is: Randomize leaves a locked control alone.
/// A control starts locked only if its definition says so: the ones that set up the
/// experiment rather than being part of it.
pub fn is_locked(ctx: &egui::Context, def: &ParamDef) -> bool {
    ctx.data_mut(|d| *d.get_persisted_mut_or(lock_id(def.name), def.locked))
}

pub fn set_locked(ctx: &egui::Context, name: &str, locked: bool) {
    ctx.data_mut(|d| d.insert_persisted(lock_id(name), locked));
}

/// Keyed on the name alone, not off a `Ui`: the flag is read in a row's parent `Ui`
/// and written in a child, and `make_persistent_id` would give each its own key.
fn lock_id(name: &str) -> egui::Id {
    egui::Id::new(("fathom-lock", name))
}

/// The parameter's default, as its control would show it.
pub fn default_text(def: &ParamDef) -> String {
    match def.kind {
        ParamKind::Float => control::format(def.default, def.min, def.max, def.step),
        ParamKind::Int => format!("{}", def.default as u32),
        ParamKind::Toggle => if def.default != 0.0 { "on".into() } else { "off".into() },
        ParamKind::Choice => def.options.get(def.default as usize).copied().unwrap_or("?").into(),
    }
}

/// The parameter row: padlock on the left, the control filling the rest.
pub fn param_row(ui: &mut egui::Ui, def: &ParamDef, index: usize, block: &mut ParamBlock) -> egui::Response {
    let locked = is_locked(ui.ctx(), def);
    let response = ui
        .horizontal(|ui| {
            if skin::padlock(ui, def.name, locked).clicked() {
                set_locked(ui.ctx(), def.name, !locked);
            }
            ui.vertical(|ui| control_for(ui, def, index, block));
        })
        .response;
    finish_row(ui, response, def, index, block, locked)
}

/// What every row shares once its control is drawn: the tooltip and the reset. Public
/// so a denser row (the dial strip) can share it.
pub fn finish_row(
    ui: &mut egui::Ui,
    mut response: egui::Response,
    def: &ParamDef,
    index: usize,
    block: &mut ParamBlock,
    locked: bool,
) -> egui::Response {
    if !ui.rect_contains_pointer(response.rect) {
        return response;
    }
    // egui reports only the topmost interactive widget as hovered, and over most of
    // the row that is the control itself, so the row never counts as hovered and its
    // tooltip would never show. The pointer is in the row: say so.
    response.flags |= egui::response::Flags::HOVERED;
    // Read from the raw pointer rather than the control's response: the control owns
    // the primary button for dragging, and the reset must work anywhere on the row.
    if ui.input(|i| i.pointer.secondary_clicked()) {
        block.set_u32(index, def.default_word());
    }
    let default = default_text(def);
    response.clone().on_hover_ui(|ui| {
        ui.set_max_width(240.0);
        if !def.description.is_empty() {
            ui.label(egui::RichText::new(def.description).size(12.0).color(skin::TEXT));
        }
        let held = if locked { " · locked: kept through Randomize" } else { "" };
        ui.label(
            egui::RichText::new(format!("default {default} · right-click to reset{held}"))
                .size(11.0)
                .color(skin::MUTED),
        );
    });
    response
}

/// The control alone, as the parameter's kind calls for. Returns whether it changed.
pub fn control_for(ui: &mut egui::Ui, def: &ParamDef, index: usize, block: &mut ParamBlock) -> bool {
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
                return true;
            }
            false
        }
        ParamKind::Toggle => {
            let mut on = block.u32(index) != 0;
            if skin::row(ui, def.label, |ui| skin::pill_toggle(ui, &mut on)) {
                block.set_u32(index, on as u32);
                return true;
            }
            false
        }
        ParamKind::Choice => {
            let current = block.u32(index) as usize;
            if let Some(picked) = choice(ui, def.name, def.label, def.options, current) {
                block.set_u32(index, picked as u32);
                return true;
            }
            false
        }
    }
}

/// A one-of-N control, segmented if the options fit and a dropdown if they do not.
///
/// Measured rather than guessed from the option count: "Classic | Well" fits in a
/// narrow panel and five presets named like sentences do not, and which is which
/// depends on how wide the panel has been dragged.
pub fn choice(ui: &mut egui::Ui, key: &str, label: &str, options: &[&'static str], selected: usize) -> Option<usize> {
    let widths = skin::segment_widths(ui, options);
    if control::segments_fit(&widths, 22.0, ui.available_width()) {
        ui.label(egui::RichText::new(label).size(11.0).color(skin::MUTED));
        skin::segmented(ui, key, options, selected)
    } else {
        skin::dropdown(ui, key, label, options, selected)
    }
}
