//! The look.
//!
//! egui has a strong default appearance, and three things give it away: the typeface,
//! the widget shapes (a round knob on a wide track, a tick-box, uniformly rounded
//! frames), and a flat grey palette. This module replaces all three.
//!
//! The identity is a cool slate casing built like the front of a measurement instrument,
//! with the accent lifted from the simulation's own colour ramp so the panel belongs to
//! the thing it controls. The sliders are the clearest expression of it: a row that *is*
//! the reading, with a bright edge where the value falls, because this is a scale being
//! read rather than a switch being thrown.
//!
//! The typefaces are IBM Plex (OFL, see `fonts/OFL.txt`), embedded rather than fetched
//! so the browser build has no network dependency and the desktop build has none either.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Rect, Response,
    Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

pub const GROUND: Color32 = Color32::from_rgb(0x0e, 0x11, 0x16);
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x17, 0x22);
pub const RAISED: Color32 = Color32::from_rgb(0x1a, 0x21, 0x2c);
pub const LINE: Color32 = Color32::from_rgb(0x23, 0x2c, 0x3a);
pub const TEXT: Color32 = Color32::from_rgb(0xdc, 0xe3, 0xed);
pub const MUTED: Color32 = Color32::from_rgb(0x7c, 0x88, 0x99);
pub const ACCENT: Color32 = Color32::from_rgb(0x6f, 0xd2, 0xff);
pub const WARM: Color32 = Color32::from_rgb(0xff, 0xc9, 0x8a);

/// A heavier family for headings, since egui has no concept of font weight.
pub fn semibold() -> FontFamily {
    FontFamily::Name("plex-semibold".into())
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    install_visuals(ctx);
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add("plex", include_bytes!("../fonts/IBMPlexSans-Regular.ttf"));
    add("plex-semibold", include_bytes!("../fonts/IBMPlexSans-SemiBold.ttf"));
    add("plex-mono", include_bytes!("../fonts/IBMPlexMono-Medium.ttf"));

    // Put ours first and keep egui's own fonts behind them as fallback, so any glyph
    // Plex lacks still renders rather than becoming a blank box.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "plex".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "plex-mono".to_owned());
    fonts
        .families
        .insert(semibold(), vec!["plex-semibold".to_owned(), "plex".to_owned()]);

    ctx.set_fonts(fonts);
}

fn install_visuals(ctx: &egui::Context) {
    // The skin is a single deliberate identity rather than a light/dark pair, so it is
    // installed into both of egui's theme slots and the theme is pinned to dark.
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| restyle(style));
}

fn restyle(style: &mut egui::Style) {
    style.text_styles = [
        (egui::TextStyle::Heading, FontId::new(17.0, semibold())),
        (egui::TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (egui::TextStyle::Button, FontId::new(12.0, FontFamily::Proportional)),
        (egui::TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
        (egui::TextStyle::Monospace, FontId::new(11.0, FontFamily::Monospace)),
    ]
    .into();

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.faint_bg_color = RAISED;
    v.extreme_bg_color = GROUND;
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, LINE);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;

    // Small, even rounding on everything, rather than egui's pill-shaped widgets.
    let radius = CornerRadius::same(4);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius;
        w.bg_fill = RAISED;
        w.weak_bg_fill = RAISED;
        w.bg_stroke = Stroke::new(1.0, LINE);
        w.fg_stroke = Stroke::new(1.0, TEXT);
        // egui grows a widget slightly when hovered; that bounce is one of its tells.
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.55));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0, ACCENT);

    // No drop shadows: this is a flat instrument face, not stacked paper.
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.popup_shadow = egui::epaint::Shadow::NONE;

    style.spacing.item_spacing = vec2(8.0, 5.0);
    style.spacing.button_padding = vec2(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(0);
    style.spacing.interact_size.y = 22.0;
}

/// Height of every control row. One number, so a column of them lines up.
pub const ROW: f32 = 24.0;

/// A section heading that folds, with a chevron that turns.
///
/// Returns whether the group is open. The state lives in egui's own memory rather than
/// in the shell: the panel is rebuilt from the schema every frame, so anything it wants
/// to remember has to be keyed by something stable, and the group name already is.
pub fn group_header(ui: &mut Ui, text: &str, default_open: bool) -> bool {
    let id = ui.make_persistent_id(("fathom-group", text));
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, default_open));

    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(id, open));
    }

    let turn = ui.ctx().animate_bool_responsive(id, open);
    let ink = if response.hovered() { TEXT } else { MUTED };
    let font = FontId::new(12.0, semibold());
    let painter = ui.painter();

    // The chevron rotates a quarter turn rather than swapping glyph, so the fold reads as
    // one thing moving instead of two things blinking.
    let c = pos2(rect.left() + 5.0, rect.center().y);
    let (sin, cos) = (turn * std::f32::consts::FRAC_PI_2).sin_cos();
    let arm = |dx: f32, dy: f32| pos2(c.x + dx * cos - dy * sin, c.y + dx * sin + dy * cos);
    painter.line_segment([arm(-1.5, -4.0), arm(2.5, 0.0)], Stroke::new(1.4, ink));
    painter.line_segment([arm(2.5, 0.0), arm(-1.5, 4.0)], Stroke::new(1.4, ink));

    let text_left = rect.left() + 16.0;
    painter.text(
        pos2(text_left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font.clone(),
        ink,
    );

    // A hairline from the end of the label to the right edge separates the sections
    // without drawing a box around each one.
    let label_width = painter.layout_no_wrap(text.to_owned(), font, ink).size().x;
    let line_left = text_left + label_width + 10.0;
    if line_left < rect.right() {
        painter.hline(line_left..=rect.right(), rect.center().y, Stroke::new(1.0, LINE));
    }

    open
}

/// The disclosure holding a group's advanced controls.
///
/// Quieter than a group heading on purpose: it is a way further into the same section,
/// not a new one.
pub fn more_toggle(ui: &mut Ui, key: &str, count: usize) -> bool {
    let id = ui.make_persistent_id(("fathom-more", key));
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, false));

    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(id, open));
    }

    let label = if open { "Fewer".to_owned() } else { format!("{count} more") };
    let ink = if response.hovered() { ACCENT } else { MUTED };
    ui.painter().text(
        pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::new(11.0, FontFamily::Proportional),
        ink,
    );
    open
}

/// One row that is the whole control: a fill showing the value, the label over it, the
/// number on the right.
///
/// Drawn rather than configured, because egui's slider is a knob on a groove and needs a
/// second row above it to carry its label and value. At twenty parameters that second row
/// is the difference between a panel you scan and one you scroll.
///
/// Drag anywhere on the row to change it, hold shift to go slowly, double-click to type a
/// number. The drag is relative — see `control::drag` for why that matters.
pub fn scrubber(ui: &mut Ui, label: &str, value: &mut f32, min: f32, max: f32, step: f32) -> bool {
    let edit_id = ui.make_persistent_id(("fathom-scrub-edit", label));
    let editing: Option<String> = ui.data_mut(|d| d.get_temp(edit_id));

    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click_and_drag());
    let mut changed = false;

    // While editing, the row becomes a text field occupying exactly the same rectangle,
    // so nothing reflows under the pointer mid-edit.
    if let Some(text) = editing {
        let mut buffer = text;
        let output = egui::TextEdit::singleline(&mut buffer)
            .font(egui::TextStyle::Monospace)
            .margin(egui::Margin::symmetric(8, 4))
            .show(&mut ui.new_child(egui::UiBuilder::new().max_rect(rect)));
        output.response.request_focus();

        let (commit, cancel) = ui.input(|i| {
            (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape))
        });
        if cancel {
            ui.data_mut(|d| d.remove::<String>(edit_id));
        } else if commit || output.response.lost_focus() {
            if let Some(parsed) = crate::control::parse(&buffer, min, max, step)
                && parsed != *value
            {
                *value = parsed;
                changed = true;
            }
            ui.data_mut(|d| d.remove::<String>(edit_id));
        } else {
            ui.data_mut(|d| d.insert_temp(edit_id, buffer));
        }
        return changed;
    }

    if response.double_clicked() {
        let shown = crate::control::format(*value, min, max, step);
        ui.data_mut(|d| d.insert_temp(edit_id, shown));
    } else if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let next =
            crate::control::drag(*value, min, max, step, response.drag_delta().x, rect.width(), fine);
        if next != *value {
            *value = next;
            changed = true;
        }
    }

    let lit = response.hovered() || response.dragged();
    let t = crate::control::fraction(*value, min, max);
    let painter = ui.painter();

    painter.rect_filled(rect, CornerRadius::same(4), RAISED);
    let edge = rect.left() + t * rect.width();
    if edge > rect.left() + 0.5 {
        // Low alpha: the fill is a reading, and the label has to stay legible on top of it
        // at every value. A solid bar would swallow the first half of every label.
        painter.rect_filled(
            Rect::from_min_max(rect.min, pos2(edge, rect.max.y)),
            CornerRadius::same(4),
            ACCENT.gamma_multiply(if lit { 0.30 } else { 0.20 }),
        );
        // The needle survives as the bright edge of the fill, so the exact position stays
        // readable — a soft tint alone would lose it.
        painter.vline(
            edge,
            rect.top() + 1.0..=rect.bottom() - 1.0,
            Stroke::new(1.0, ACCENT.gamma_multiply(if lit { 1.0 } else { 0.7 })),
        );
    }
    if lit {
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.0, ACCENT.gamma_multiply(0.5)),
            StrokeKind::Inside,
        );
    }

    painter.text(
        pos2(rect.left() + 9.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::new(12.0, FontFamily::Proportional),
        if lit { TEXT } else { TEXT.gamma_multiply(0.92) },
    );
    painter.text(
        pos2(rect.right() - 9.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        crate::control::format(*value, min, max, step),
        FontId::new(11.0, FontFamily::Monospace),
        if lit { ACCENT } else { ACCENT.gamma_multiply(0.85) },
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    changed
}

/// A row of connected buttons with one of them lit, returning the index clicked.
///
/// Used instead of a dropdown whenever the options fit. A dropdown hides every choice but
/// one behind a click; on a two-option control, those hidden choices are the dropdown's
/// entire content.
pub fn segmented(ui: &mut Ui, key: &str, options: &[&str], selected: usize) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::same(4),
        RAISED,
        Stroke::new(1.0, LINE),
        StrokeKind::Inside,
    );

    let mut picked = None;
    let width = rect.width() / options.len().max(1) as f32;
    for (i, option) in options.iter().enumerate() {
        let cell =
            Rect::from_min_size(pos2(rect.left() + i as f32 * width, rect.top()), vec2(width, ROW));
        let response =
            ui.interact(cell, ui.make_persistent_id(("fathom-seg", key, i)), Sense::click());
        if response.clicked() {
            picked = Some(i);
        }

        let on = i == selected;
        let painter = ui.painter();
        if on {
            painter.rect_filled(cell.shrink(2.0), CornerRadius::same(3), ACCENT.gamma_multiply(0.22));
        } else if i > 0 {
            painter.vline(cell.left(), cell.top() + 6.0..=cell.bottom() - 6.0, Stroke::new(1.0, LINE));
        }
        painter.text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            *option,
            FontId::new(11.0, FontFamily::Proportional),
            if on {
                ACCENT
            } else if response.hovered() {
                TEXT
            } else {
                MUTED
            },
        );
    }
    picked
}

/// What each option's label measures, so the caller can decide whether they fit.
pub fn segment_widths(ui: &Ui, options: &[&str]) -> Vec<f32> {
    let font = FontId::new(11.0, FontFamily::Proportional);
    let painter = ui.painter();
    options
        .iter()
        .map(|o| painter.layout_no_wrap((*o).to_owned(), font.clone(), TEXT).size().x)
        .collect()
}

/// A labelled row carrying a dropdown, for choices too long to segment.
pub fn dropdown(ui: &mut Ui, key: &str, label: &str, options: &[&str], selected: usize) -> Option<usize> {
    let mut value = selected;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.0).color(TEXT.gamma_multiply(0.92)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt(key)
                .selected_text(options.get(value).copied().unwrap_or(""))
                .show_ui(ui, |ui| {
                    for (i, option) in options.iter().enumerate() {
                        ui.selectable_value(&mut value, i, *option);
                    }
                });
        });
    });
    (value != selected).then_some(value)
}

/// A sliding pill, in place of egui's tick-box.
pub fn pill_toggle(ui: &mut Ui, on: &mut bool) -> bool {
    let size = Vec2::new(30.0, 16.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());

    let mut changed = false;
    if response.clicked() {
        *on = !*on;
        changed = true;
        response.mark_changed();
    }

    let how = ui.ctx().animate_bool_responsive(response.id, *on);
    let border = if *on { ACCENT.gamma_multiply(0.55) } else { LINE };
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(8), RAISED, Stroke::new(1.0, border), StrokeKind::Inside);

    let travel = rect.width() - 16.0;
    let knob = pos2(rect.left() + 8.0 + travel * how, rect.center().y);
    painter.circle_filled(knob, 5.0, if *on { ACCENT } else { MUTED });

    changed
}

/// A labelled row holding any right-aligned control.
pub fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.0).color(TEXT.gamma_multiply(0.92)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add).inner
    })
    .inner
}

/// A flat action button that lights up on hover instead of inflating.
pub fn action(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(egui::RichText::new(label).size(12.0)).corner_radius(4.0))
}
