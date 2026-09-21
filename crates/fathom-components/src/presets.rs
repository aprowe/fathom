//! The preset picker as a widget: a dropdown of names, a delete for the current one
//! when it can be deleted, and Save…, Import…, Export…. It reports what was chosen; who
//! lists the presets and does the file work is the caller's business (the shell's, in a
//! fathom app).

use eframe::egui;

use crate::skin;

/// One entry the picker shows.
pub struct PresetEntry<'a> {
    pub name: &'a str,
    /// Built into the app rather than saved by the person: shown dimmer, not deletable.
    pub builtin: bool,
}

/// What the person asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetAction {
    Load(String),
    Delete(String),
    Save(String),
    Import,
    Export,
}

/// The picker on one row. `current` is the entry shown as loaded; `name` is the text
/// the Save… popover edits, kept by the caller so it survives frames. `can_save` is
/// false where there is no folder to save into (the web), leaving Export… as the way.
pub fn picker(ui: &mut egui::Ui, entries: &[PresetEntry<'_>], current: Option<usize>, name: &mut String, can_save: bool) -> Option<PresetAction> {
    let mut action = None;
    let selected_text = current.map(|i| entries[i].name).unwrap_or("—");
    egui::ComboBox::from_id_salt("fathom.preset.picker")
        .selected_text(selected_text)
        .width(150.0)
        .show_ui(ui, |ui| {
            if entries.is_empty() {
                ui.label(egui::RichText::new("none yet").size(12.0).color(skin::MUTED));
            }
            for (i, e) in entries.iter().enumerate() {
                let mut text = egui::RichText::new(e.name);
                if e.builtin {
                    text = text.color(skin::TEXT.gamma_multiply(0.75));
                }
                let response = ui.selectable_label(current == Some(i), text);
                let response = if e.builtin { response.on_hover_text("built in") } else { response };
                if response.clicked() {
                    action = Some(PresetAction::Load(e.name.to_string()));
                }
            }
        });
    if let Some(i) = current
        && !entries[i].builtin
        && skin::action(ui, "×").on_hover_text("Delete this preset from the folder.").clicked()
    {
        action = Some(PresetAction::Delete(entries[i].name.to_string()));
    }
    if can_save {
        let button = skin::action(ui, "Save…").on_hover_text("Keep the current settings in the presets folder under a name of your choosing.");
        skin::popover(&button, 220.0, |ui| {
            ui.label(egui::RichText::new("Save as").size(11.0).color(skin::MUTED));
            let edit = egui::TextEdit::singleline(name).hint_text("name").font(egui::FontId::proportional(12.0)).desired_width(f32::INFINITY);
            let response = ui.add(edit);
            if button.clicked() {
                response.request_focus();
            }
            let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(4.0);
            let confirmed = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| skin::action(ui, "Save").clicked()).inner;
            if (entered || confirmed) && !name.trim().is_empty() {
                action = Some(PresetAction::Save(name.trim().to_string()));
                ui.close();
            }
        });
    }
    if skin::action(ui, "Import…").on_hover_text("Load settings from a .json file. Dropping one on the window does the same.").clicked() {
        action = Some(PresetAction::Import);
    }
    if skin::action(ui, "Export…")
        .on_hover_text(if can_save { "Write the current settings to a .json file of your choosing." } else { "Download the current settings as a .json file." })
        .clicked()
    {
        action = Some(PresetAction::Export);
    }
    action
}
