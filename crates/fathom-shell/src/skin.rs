//! The look.
//!
//! egui has a strong default appearance, and three things give it away: the typeface,
//! the widget shapes (a round knob on a wide track, a tick-box, uniformly rounded
//! frames), and a flat grey palette. This module replaces all three.
//!
//! The identity is the one the web panel already wears, so the three hosts read as one
//! product: a cool slate casing built like the front of a measurement instrument, with
//! the accent lifted from the simulation's own colour ramp so the panel belongs to the
//! thing it controls. The sliders are the clearest expression of it — a hairline track
//! with a needle rather than a knob, because this is a scale being read, not a switch
//! being thrown.
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

    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(0);
    style.spacing.interact_size.y = 22.0;

}

/// A section heading: quiet, weighted, with a hairline under it rather than a box.
pub fn group_heading(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(text)
            .family(semibold())
            .size(12.0)
            .color(MUTED),
    );
    let y = ui.cursor().top() + 2.0;
    let x = ui.max_rect().x_range();
    ui.painter().hline(x, y, Stroke::new(1.0, LINE));
    ui.add_space(4.0);
}

/// A control's name on the left, its value on the right in tabular figures so the
/// number does not shift sideways as it changes.
pub fn readout(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .family(FontFamily::Monospace)
                    .size(11.0)
                    .color(ACCENT),
            );
        });
    });
}

/// A needle on a scale, not a knob on a track.
///
/// Drawn rather than configured: egui's own slider has a round handle and a thick
/// groove, and no amount of restyling gets it to this.
pub fn needle_slider(ui: &mut Ui, value: &mut f32, min: f32, max: f32, step: Option<f32>) -> bool {
    let height = 18.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());

    let span = (max - min).max(f32::EPSILON);
    let mut changed = false;

    if response.dragged() || response.clicked() {
        if let Some(p) = response.interact_pointer_pos() {
            let t = ((p.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
            let mut next = min + t * span;
            if let Some(s) = step {
                if s > 0.0 {
                    next = (next / s).round() * s;
                }
            }
            let next = next.clamp(min, max);
            if next != *value {
                *value = next;
                changed = true;
            }
        }
    }

    let t = ((*value - min) / span).clamp(0.0, 1.0);
    let mid = rect.center().y;
    let x = rect.left() + t * rect.width();
    let painter = ui.painter();

    let track = Rect::from_min_max(pos2(rect.left(), mid - 1.0), pos2(rect.right(), mid + 1.0));
    painter.rect_filled(track, 1.0, LINE);
    let filled = Rect::from_min_max(pos2(rect.left(), mid - 1.0), pos2(x, mid + 1.0));
    painter.rect_filled(filled, 1.0, ACCENT);

    let lit = response.hovered() || response.dragged();
    let needle = Rect::from_min_max(pos2(x - 1.0, mid - 7.0), pos2(x + 1.0, mid + 7.0));
    painter.rect_filled(needle, 1.0, if lit { ACCENT } else { TEXT });

    changed
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
    painter.rect(
        rect,
        CornerRadius::same(8),
        RAISED,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );

    let travel = rect.width() - 16.0;
    let knob = pos2(rect.left() + 8.0 + travel * how, rect.center().y);
    painter.circle_filled(knob, 5.0, if *on { ACCENT } else { MUTED });

    changed
}

/// A labelled row holding any right-aligned control.
pub fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add).inner
    })
    .inner
}

/// A flat action button that lights up on hover instead of inflating.
pub fn action(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(egui::RichText::new(label).size(12.0)).corner_radius(4.0))
}
