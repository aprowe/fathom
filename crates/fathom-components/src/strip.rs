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

/// A plain strip: the parameter row at bench density (the padlock and the scrubber
/// already are one line; this is a name for using them that way).
pub fn strip(ui: &mut egui::Ui, def: &ParamDef, index: usize, block: &mut ParamBlock) -> StripResponse {
    let response = param::param_row(ui, def, index, block);
    StripResponse { changed: response.changed(), ..Default::default() }
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
            if skin::padlock(ui, def.name, locked).clicked() {
                param::set_locked(ui.ctx(), def.name, !locked);
            }
            if chip(ui, def.name, sourced, disabled_hint) {
                out.source_changed = true;
            }
            ui.vertical(|ui| {
                if def.kind != ParamKind::Float || sourced.source == Source::Fixed {
                    out.changed = param::control_for(ui, def, index, block);
                    return;
                }
                // The scrubber, then the band's handles laid over the same rectangle:
                // a widget added later wins the pointer, so a handle under the pointer
                // takes the drag and the scrubber does not see it.
                let before = ui.cursor().min;
                out.changed = param::control_for(ui, def, index, block);
                let rect = Rect::from_min_size(before, vec2(ui.min_rect().width(), skin::ROW));
                let base = block.f32(index);
                let mut band = sourced.band.clamped(base, def.min, def.max);
                if band_over(ui, rect, def, base, &mut band, sourced.source.colour()) {
                    sourced.band = band;
                    out.band_changed = true;
                } else if band != sourced.band {
                    sourced.band = band;
                }
            });
        })
        .response;
    param::finish_row(ui, response, def, index, block, locked);
    out
}

/// The source chip: a lettered square that cycles on a click.
fn chip(ui: &mut egui::Ui, key: &str, sourced: &mut Sourced, disabled_hint: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, skin::ROW), Sense::click());
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

/// The band over a scrubber's rectangle: a bracket from `lo` to `hi` with a handle at
/// each end, the dial's value as the tick between them. Returns whether an end moved.
fn band_over(ui: &mut egui::Ui, rect: Rect, def: &ParamDef, base: f32, band: &mut Band, ink: Color32) -> bool {
    let to_x = |v: f32| rect.left() + control::fraction(v, def.min, def.max) * rect.width();
    let to_v = |x: f32| def.min + ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * (def.max - def.min);
    let (xl, xh) = (to_x(band.lo), to_x(band.hi));
    let painter = ui.painter();
    let y0 = rect.top() + 2.0;
    let y1 = rect.bottom() - 2.0;
    painter.rect_filled(Rect::from_min_max(pos2(xl, y0), pos2(xh, y1)), CornerRadius::same(2), ink.gamma_multiply(0.16));
    painter.hline(xl..=xh, y1 - 1.0, Stroke::new(2.0, ink.gamma_multiply(0.8)));
    let mut moved = false;
    for (end, x) in [(0, xl), (1, xh)] {
        let handle = Rect::from_center_size(pos2(x, rect.center().y), vec2(10.0, rect.height()));
        let id = ui.id().with(("fathom-band", def.name, end));
        let response = ui.interact(handle, id, Sense::drag());
        let lit = response.hovered() || response.dragged();
        painter.vline(x, y0..=y1, Stroke::new(if lit { 2.5 } else { 1.5 }, ink));
        painter.circle_filled(pos2(x, y1 - 1.0), if lit { 3.5 } else { 2.5 }, ink);
        if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            let v = control::quantize(to_v(p.x), def.min, def.max, def.step);
            if end == 0 {
                band.lo = v.min(base);
            } else {
                band.hi = v.max(base);
            }
            moved = true;
        }
        if lit {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    moved
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
