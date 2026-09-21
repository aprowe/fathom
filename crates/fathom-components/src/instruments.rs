//! Instruments: small painted readouts an app fills from its own numbers. None of them
//! knows what it is showing; each takes values and draws them in the skin.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::skin;

/// A caption in the instrument style: small caps-feeling label above a block.
pub fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text.to_uppercase()).size(10.0).color(skin::MUTED).family(FontFamily::Monospace));
}

/// A history as a line, scaled to its own range, latest value at the right.
pub fn sparkline(ui: &mut egui::Ui, values: &[f32], size: egui::Vec2, ink: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), skin::RAISED.gamma_multiply(0.7));
    if values.len() < 2 {
        return response;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in values {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let span = (hi - lo).max(1e-6);
    let inner = rect.shrink(2.0);
    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = inner.left() + inner.width() * i as f32 / (values.len() - 1) as f32;
            let y = inner.bottom() - inner.height() * (v - lo) / span;
            pos2(x, y)
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.2, ink)));
    response
}

/// A shaded band along the x axis of a curve, and a vertical marker on it.
pub struct XBand {
    pub from: f32,
    pub to: f32,
    pub ink: Color32,
}

pub struct XMark {
    pub at: f32,
    pub label: &'static str,
    pub ink: Color32,
}

/// One trace on a curve.
pub struct Trace<'a> {
    pub points: &'a [[f32; 2]],
    pub ink: Color32,
    pub dashed: bool,
}

/// A y-of-x plot: traces over `x_range`, y scaled to the traces with a zero line if zero
/// is in view, bands behind and marks in front.
pub fn curve(ui: &mut egui::Ui, size: egui::Vec2, x_range: [f32; 2], traces: &[Trace<'_>], bands: &[XBand], marks: &[XMark]) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), skin::RAISED.gamma_multiply(0.7));
    let inner = rect.shrink2(vec2(6.0, 8.0));
    let (mut lo, mut hi) = (0.0f32, 0.0f32);
    for t in traces {
        for p in t.points {
            if p[1].is_finite() {
                lo = lo.min(p[1]);
                hi = hi.max(p[1]);
            }
        }
    }
    if hi - lo < 1e-6 {
        hi = lo + 1.0;
    }
    let pad = 0.08 * (hi - lo);
    let (lo, hi) = (lo - pad, hi + pad);
    let xw = (x_range[1] - x_range[0]).max(1e-6);
    let to_x = |x: f32| inner.left() + (x - x_range[0]) / xw * inner.width();
    let to_y = |y: f32| inner.bottom() - (y - lo) / (hi - lo) * inner.height();
    for b in bands {
        painter.rect_filled(Rect::from_min_max(pos2(to_x(b.from), inner.top()), pos2(to_x(b.to), inner.bottom())), 0.0, b.ink.gamma_multiply(0.12));
    }
    if lo < 0.0 && hi > 0.0 {
        painter.hline(inner.x_range(), to_y(0.0), Stroke::new(1.0, skin::LINE.gamma_multiply(1.6)));
    }
    for t in traces {
        let pts: Vec<egui::Pos2> = t.points.iter().filter(|p| p[1].is_finite()).map(|p| pos2(to_x(p[0]), to_y(p[1]))).collect();
        if t.dashed {
            painter.add(egui::Shape::dashed_line(&pts, Stroke::new(1.0, t.ink), 3.0, 3.0));
        } else {
            painter.add(egui::Shape::line(pts, Stroke::new(1.5, t.ink)));
        }
    }
    for m in marks {
        let x = to_x(m.at);
        painter.vline(x, inner.y_range(), Stroke::new(1.0, m.ink.gamma_multiply(0.8)));
        painter.text(pos2(x + 3.0, inner.top()), egui::Align2::LEFT_TOP, m.label, FontId::new(9.0, FontFamily::Monospace), m.ink);
    }
    response
}

/// A square image with wrapped axes and a crosshair at `(u, v)` in `0..1`. Dragging on
/// it returns the new `(u, v)`.
pub fn sheet(ui: &mut egui::Ui, texture: egui::TextureId, size: f32, crosshair: [f32; 2], ink: Color32) -> (egui::Response, Option<[f32; 2]>) {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let painter = ui.painter();
    painter.image(texture, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, skin::LINE), StrokeKind::Inside);
    let x = rect.left() + crosshair[0].rem_euclid(1.0) * rect.width();
    let y = rect.top() + crosshair[1].rem_euclid(1.0) * rect.height();
    painter.vline(x, rect.y_range(), Stroke::new(1.0, ink.gamma_multiply(0.9)));
    painter.hline(rect.x_range(), y, Stroke::new(1.0, ink.gamma_multiply(0.9)));
    painter.circle_stroke(pos2(x, y), 4.0, Stroke::new(1.0, ink));
    let picked = if response.dragged() || response.clicked() {
        response.interact_pointer_pos().map(|p| [((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0), ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0)])
    } else {
        None
    };
    (response, picked)
}

/// Label/value rows in tabular numerals, value right-aligned.
pub fn readout(ui: &mut egui::Ui, rows: &[(&str, String, Color32)]) {
    for (label, value, ink) in rows {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(*label).size(11.0).color(skin::MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(value).size(11.0).family(FontFamily::Monospace).color(*ink));
            });
        });
    }
}

/// A short horizontal bar showing where `value` and a `[lo, hi]` band sit in
/// `[min, max]`: the row of a sources board.
pub fn range_bar(ui: &mut egui::Ui, width: f32, min: f32, max: f32, value: f32, band: Option<[f32; 2]>, ink: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 10.0), Sense::hover());
    let painter = ui.painter();
    let span = (max - min).max(1e-6);
    let to_x = |v: f32| rect.left() + ((v - min) / span).clamp(0.0, 1.0) * rect.width();
    painter.hline(rect.x_range(), rect.center().y, Stroke::new(2.0, skin::LINE.gamma_multiply(1.5)));
    if let Some([lo, hi]) = band {
        painter.rect_filled(Rect::from_min_max(pos2(to_x(lo), rect.top()), pos2(to_x(hi), rect.bottom())), CornerRadius::same(2), ink.gamma_multiply(0.35));
    }
    painter.vline(to_x(value), rect.y_range(), Stroke::new(2.0, if band.is_some() { skin::TEXT } else { ink }));
}
