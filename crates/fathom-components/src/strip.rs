//! The dial strip: a parameter on one dense line, `[lock][chip][name  track  value]`.
//!
//! The strip is the scrubber with two things beside it. The **source chip** says where
//! the dial's value comes from — `F` fixed, one number; `L` a landscape, a field over
//! whatever the app varies it by; `T` a trait, a value each agent carries — and cycles
//! on a click. When the source is not fixed the strip also carries a **band**, `[lo,
//! hi]` in the dial's own units, drawn over the track with a handle at each end; the
//! dial's own value stays visible as the tick inside it, since a varying dial still has
//! a reference. What the sources *mean* is the app's business; the strip only shows and
//! edits them.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use fathom_core::{ParamBlock, ParamDef, ParamKind};

use crate::{control, param, skin};

/// Where a dial's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    Fixed,
    Landscape,
    Trait,
}

impl Source {
    pub fn letter(self) -> &'static str {
        match self {
            Source::Fixed => "F",
            Source::Landscape => "L",
            Source::Trait => "T",
        }
    }

    pub fn next(self) -> Source {
        match self {
            Source::Fixed => Source::Landscape,
            Source::Landscape => Source::Trait,
            Source::Trait => Source::Fixed,
        }
    }

    pub fn colour(self) -> Color32 {
        match self {
            Source::Fixed => skin::MUTED,
            Source::Landscape => LANDSCAPE,
            Source::Trait => TRAIT,
        }
    }
}

/// The two source colours, shared with every instrument that shows a source.
pub const LANDSCAPE: Color32 = Color32::from_rgb(0x3f, 0xc2, 0xb9);
pub const TRAIT: Color32 = Color32::from_rgb(0xe0, 0xb0, 0x40);

/// A range in the dial's units.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Band {
    pub lo: f32,
    pub hi: f32,
}

impl Band {
    /// The band kept honest against the dial: inside the dial's range, and around its
    /// current value, so a varying dial always includes what the fixed one would have
    /// been. A band that was never set, or is inside out, opens to a fifth of the range
    /// each side of the value.
    pub fn clamped(self, base: f32, min: f32, max: f32) -> Band {
        let unset = self.lo == 0.0 && self.hi == 0.0 && base != 0.0;
        let (lo, hi) = if unset || self.hi < self.lo {
            let reach = 0.2 * (max - min);
            (base - reach, base + reach)
        } else {
            (self.lo, self.hi)
        };
        Band { lo: lo.clamp(min, max).min(base), hi: hi.clamp(min, max).max(base) }
    }
}

/// The source and band a strip shows, and gets back edited.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sourced {
    pub source: Source,
    pub band: Band,
    /// A source the chip shows but will not pick: greyed, with `disabled_hint` on hover.
    pub disabled: Option<Source>,
}

/// What a strip reports.
#[derive(Clone, Copy, Debug, Default)]
pub struct StripResponse {
    /// The dial's value changed.
    pub changed: bool,
    /// The chip was clicked and the source moved on.
    pub source_changed: bool,
    /// A band end was dragged.
    pub band_changed: bool,
}

/// Height of a strip.
pub const STRIP: f32 = 20.0;
/// Width of the name column and the value column.
const NAME_WIDTH: f32 = 84.0;
const VALUE_WIDTH: f32 = 46.0;

/// A plain strip: `[lock][name][track][value]` on one 20px line.
pub fn strip(ui: &mut egui::Ui, def: &ParamDef, index: usize, block: &mut ParamBlock) -> StripResponse {
    let mut out = StripResponse::default();
    let locked = param::is_locked(ui.ctx(), def);
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if skin::padlock(ui, def.name, locked).clicked() {
                param::set_locked(ui.ctx(), def.name, !locked);
            }
            out.changed = body(ui, def, index, block, None, locked);
        })
        .response;
    param::finish_row(ui, response, def, index, block, locked);
    out
}

/// A strip with a source chip and, when the source is not fixed, a band over the track.
/// `sourced` is edited in place. Only a float dial can carry a band; for any other kind
/// the chip alone is drawn.
pub fn sourced_strip(
    ui: &mut egui::Ui,
    def: &ParamDef,
    index: usize,
    block: &mut ParamBlock,
    sourced: &mut Sourced,
    disabled_hint: &str,
) -> StripResponse {
    let mut out = StripResponse::default();
    let locked = param::is_locked(ui.ctx(), def);
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if skin::padlock(ui, def.name, locked).clicked() {
                param::set_locked(ui.ctx(), def.name, !locked);
            }
            if chip(ui, def.name, sourced, disabled_hint) {
                out.source_changed = true;
            }
            let band = (def.kind == ParamKind::Float && sourced.source != Source::Fixed).then_some((sourced.source.colour(), &mut sourced.band, &mut out.band_changed));
            out.changed = body(ui, def, index, block, band, locked);
        })
        .response;
    param::finish_row(ui, response, def, index, block, locked);
    out
}

/// A strip's name, track and value — everything after the lock and the chip. A float
/// or int dial gets the track; a toggle or choice gets its control, right-aligned.
fn body(
    ui: &mut egui::Ui,
    def: &ParamDef,
    index: usize,
    block: &mut ParamBlock,
    band: Option<(Color32, &mut Band, &mut bool)>,
    locked: bool,
) -> bool {
    // A locked dial reads quieter; the lit padlock says why.
    let name_ink = if locked { skin::MUTED } else { skin::TEXT.gamma_multiply(0.75) };
    let (name_rect, _) = ui.allocate_exact_size(vec2(NAME_WIDTH, STRIP), Sense::hover());
    let font = FontId::new(11.0, FontFamily::Proportional);
    let painter = ui.painter();
    let mut label = def.label.to_string();
    // Truncate the name to its column rather than let it run under the track.
    while painter.layout_no_wrap(label.clone(), font.clone(), name_ink).size().x > NAME_WIDTH && label.len() > 1 {
        label.pop();
    }
    if label.len() < def.label.len() {
        label.pop();
        label.push('…');
    }
    painter.text(pos2(name_rect.left(), name_rect.center().y), egui::Align2::LEFT_CENTER, label, font, name_ink);

    match def.kind {
        ParamKind::Float | ParamKind::Int => {
            let is_int = def.kind == ParamKind::Int;
            let step = if is_int { 1.0 } else { def.step };
            let mut value = if is_int { block.u32(index) as f32 } else { block.f32(index) };
            let changed = track(ui, def, &mut value, step, band);
            if changed {
                if is_int {
                    block.set_u32(index, value.round() as u32);
                } else {
                    block.set_f32(index, value);
                }
            }
            changed
        }
        ParamKind::Toggle => {
            let mut on = block.u32(index) != 0;
            let changed = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| skin::pill_toggle(ui, &mut on)).inner;
            if changed {
                block.set_u32(index, on as u32);
            }
            changed
        }
        ParamKind::Choice => {
            let current = block.u32(index) as usize;
            let picked = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let widths = skin::segment_widths(ui, def.options);
                    if control::segments_fit(&widths, 18.0, ui.available_width()) {
                        skin::segmented(ui, def.name, def.options, current)
                    } else {
                        let mut value = current;
                        egui::ComboBox::from_id_salt(def.name)
                            .selected_text(def.options.get(current).copied().unwrap_or(""))
                            .show_ui(ui, |ui| {
                                for (i, option) in def.options.iter().enumerate() {
                                    ui.selectable_value(&mut value, i, *option);
                                }
                            });
                        (value != current).then_some(value)
                    }
                })
                .inner;
            if let Some(p) = picked {
                block.set_u32(index, p as u32);
                return true;
            }
            false
        }
    }
}

/// The track and the value: a rail, the fill up to the value, a thumb, the number.
/// Drag on the rail to set the value by position, shift for fine; double-click the
/// number to type it. With a band, the ends of the band take the drag when the pointer
/// is on them.
fn track(ui: &mut egui::Ui, def: &ParamDef, value: &mut f32, step: f32, mut band: Option<(Color32, &mut Band, &mut bool)>) -> bool {
    let mut changed = false;
    let width = (ui.available_width() - VALUE_WIDTH - 5.0).max(40.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, STRIP), Sense::click_and_drag());
    let to_v = |x: f32| def.min + ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * (def.max - def.min);
    let to_x = |v: f32| rect.left() + control::fraction(v, def.min, def.max) * rect.width();
    let lit = response.hovered() || response.dragged();

    // Which end of the band, if any, the drag began on: remembered for the drag.
    let grab_id = response.id.with("grab");
    let mut grab: u8 = ui.data(|d| d.get_temp(grab_id)).unwrap_or(0);
    if response.drag_started() {
        grab = 0;
        if let (Some(p), Some((_, b, _))) = (response.interact_pointer_pos(), band.as_ref()) {
            let (dl, dh) = ((p.x - to_x(b.lo)).abs(), (p.x - to_x(b.hi)).abs());
            grab = if dl < 7.0 && dl <= dh { 1 } else if dh < 7.0 { 2 } else { 0 };
        }
        ui.data_mut(|d| d.insert_temp(grab_id, grab));
    }
    if response.dragged()
        && let Some(p) = response.interact_pointer_pos()
    {
        let raw = to_v(p.x);
        let fine = ui.input(|i| i.modifiers.shift);
        match (grab, band.as_mut()) {
            (1, Some((_, b, moved))) => {
                b.lo = control::quantize(raw, def.min, def.max, step).min(*value);
                **moved = true;
            }
            (2, Some((_, b, moved))) => {
                b.hi = control::quantize(raw, def.min, def.max, step).max(*value);
                **moved = true;
            }
            _ => {
                let next = if fine {
                    control::drag(*value, def.min, def.max, step, response.drag_delta().x, rect.width(), true)
                } else {
                    control::quantize(raw, def.min, def.max, step)
                };
                if next != *value {
                    *value = next;
                    changed = true;
                }
            }
        }
    } else if response.double_clicked() {
        *value = def.default;
        changed = true;
    }
    if lit {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    let painter = ui.painter();
    let cy = rect.center().y;
    let x = to_x(*value);
    let rail = skin::LINE.gamma_multiply(1.4);
    painter.hline(rect.x_range(), cy, Stroke::new(2.0, rail));
    painter.hline(rect.left()..=x, cy, Stroke::new(2.0, skin::ACCENT.gamma_multiply(if lit { 0.9 } else { 0.55 })));
    let (thumb_w, thumb_ink) = match &band {
        Some((ink, b, _)) => {
            let (xl, xh) = (to_x(b.lo), to_x(b.hi));
            painter.rect_filled(Rect::from_min_max(pos2(xl, cy - 2.0), pos2(xh, cy + 2.0)), CornerRadius::same(1), ink.gamma_multiply(0.35));
            painter.vline(xl, cy - 6.0..=cy + 6.0, Stroke::new(2.0, *ink));
            painter.vline(xh, cy - 6.0..=cy + 6.0, Stroke::new(2.0, *ink));
            painter.hline(xl..=xh, cy + 6.0, Stroke::new(1.0, *ink));
            (1.0, skin::TEXT.gamma_multiply(0.7))
        }
        None => (3.0, if lit { skin::TEXT } else { skin::TEXT.gamma_multiply(0.85) }),
    };
    painter.rect_filled(Rect::from_center_size(pos2(x, cy), vec2(thumb_w, 10.0)), CornerRadius::same(1), thumb_ink);

    // The value: a number, editable on a double-click.
    let edit_id = ui.make_persistent_id(("fathom-dial-edit", def.name));
    let editing: Option<String> = ui.data_mut(|d| d.get_temp(edit_id));
    let (vrect, vresponse) = ui.allocate_exact_size(vec2(VALUE_WIDTH, STRIP), Sense::click());
    if let Some(text) = editing {
        let mut buffer = text;
        let output = egui::TextEdit::singleline(&mut buffer)
            .font(FontId::new(10.5, FontFamily::Monospace))
            .margin(egui::Margin::symmetric(4, 2))
            .show(&mut ui.new_child(egui::UiBuilder::new().max_rect(vrect)));
        output.response.request_focus();
        let (commit, cancel) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if cancel {
            ui.data_mut(|d| d.remove::<String>(edit_id));
        } else if commit || output.response.lost_focus() {
            if let Some(parsed) = control::parse(&buffer, def.min, def.max, step)
                && parsed != *value
            {
                *value = parsed;
                changed = true;
            }
            ui.data_mut(|d| d.remove::<String>(edit_id));
        } else {
            ui.data_mut(|d| d.insert_temp(edit_id, buffer));
        }
    } else {
        if vresponse.double_clicked() {
            ui.data_mut(|d| d.insert_temp(edit_id, control::format(*value, def.min, def.max, step)));
        }
        ui.painter().text(
            pos2(vrect.right(), vrect.center().y),
            egui::Align2::RIGHT_CENTER,
            control::format(*value, def.min, def.max, step),
            FontId::new(10.5, FontFamily::Monospace),
            if lit || vresponse.hovered() { skin::ACCENT } else { skin::ACCENT.gamma_multiply(0.85) },
        );
    }
    changed
}

/// The source chip: a lettered square that cycles on a click.
fn chip(ui: &mut egui::Ui, key: &str, sourced: &mut Sourced, disabled_hint: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, STRIP), Sense::click());
    let now = match sourced.source {
        Source::Fixed => "Fixed: one number for everyone.",
        Source::Landscape => "Landscape: varies over the colour pair.",
        Source::Trait => "Trait: each particle carries its own value.",
    };
    let mut next = sourced.source.next();
    let skipped = if Some(next) == sourced.disabled {
        next = next.next();
        format!(" {} is not available: {disabled_hint}", sourced.source.next().letter())
    } else {
        String::new()
    };
    let response = response.on_hover_text(format!("{now} Click for {}.{skipped}", next.letter()));
    let mut changed = false;
    if response.clicked() {
        let mut next = sourced.source.next();
        if Some(next) == sourced.disabled {
            next = next.next();
        }
        sourced.source = next;
        changed = true;
    }
    let ink = sourced.source.colour();
    let painter = ui.painter();
    let square = Rect::from_center_size(rect.center(), vec2(14.0, 14.0));
    let fill = if sourced.source == Source::Fixed { Color32::TRANSPARENT } else { ink.gamma_multiply(0.22) };
    painter.rect(square, CornerRadius::same(3), fill, Stroke::new(1.0, ink.gamma_multiply(if response.hovered() { 1.0 } else { 0.7 })), StrokeKind::Inside);
    painter.text(square.center(), egui::Align2::CENTER_CENTER, sourced.source.letter(), FontId::new(9.5, FontFamily::Monospace), ink);
    let _ = key;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_band_stays_in_the_range_and_around_the_base() {
        let b = Band { lo: 0.1, hi: 0.5 }.clamped(0.3, 0.0, 1.0);
        assert_eq!((b.lo, b.hi), (0.1, 0.5));
        // The base moved outside: the band stretches to keep it.
        let b = Band { lo: 0.1, hi: 0.5 }.clamped(0.7, 0.0, 1.0);
        assert_eq!((b.lo, b.hi), (0.1, 0.7));
        // Beyond the dial's range: clipped.
        let b = Band { lo: -1.0, hi: 4.0 }.clamped(0.5, 0.0, 1.0);
        assert_eq!((b.lo, b.hi), (0.0, 1.0));
        // Never set, or inside out: opens a fifth of the range each side of the base.
        let b = Band { lo: 0.0, hi: 0.0 }.clamped(0.5, 0.0, 1.0);
        assert!((b.lo - 0.3).abs() < 1e-6 && (b.hi - 0.7).abs() < 1e-6);
        let b = Band { lo: 0.6, hi: 0.4 }.clamped(0.5, 0.0, 1.0);
        assert!((b.lo - 0.3).abs() < 1e-6 && (b.hi - 0.7).abs() < 1e-6);
    }

    #[test]
    fn the_chip_cycles_and_skips_a_disabled_source() {
        assert_eq!(Source::Fixed.next(), Source::Landscape);
        assert_eq!(Source::Trait.next(), Source::Fixed);
        let mut s = Sourced { disabled: Some(Source::Landscape), ..Default::default() };
        s.source = s.source.next();
        if Some(s.source) == s.disabled {
            s.source = s.source.next();
        }
        assert_eq!(s.source, Source::Trait);
    }
}
