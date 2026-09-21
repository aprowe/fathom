//! The arithmetic behind the controls, kept apart from the drawing.
//!
//! Everything here is a pure function of numbers and strings. That is the point: a
//! widget is a tangle of painting, hit-testing and state that a test can barely reach,
//! but the parts of it that are actually *wrong* when a control misbehaves — how far a
//! drag moves a value, how many decimals to show, whether a row of buttons fits — are
//! ordinary arithmetic. Pulling them out here is what makes them testable at all.

/// How much slower a drag moves the value while shift is held.
const FINE: f32 = 6.0;

/// Where a value sits in its range, as `0..=1`.
///
/// A range of zero width is a control that cannot be moved; it reads as empty rather
/// than as a division by zero.
pub fn fraction(value: f32, min: f32, max: f32) -> f32 {
    let span = max - min;
    if span.abs() < f32::EPSILON {
        return 0.0;
    }
    ((value - min) / span).clamp(0.0, 1.0)
}

/// The value after dragging `dx` pixels across a control `width` pixels wide.
///
/// Relative, not absolute: the value moves *by* the drag rather than jumping to wherever
/// the pointer landed. Absolute is easier to implement and worse to use — a slider you
/// touch has already lost its old value before you have moved a pixel, which on a panel
/// with twenty of them means every accidental brush destroys a setting.
pub fn drag(value: f32, min: f32, max: f32, step: f32, dx: f32, width: f32, fine: bool) -> f32 {
    let span = max - min;
    if span.abs() < f32::EPSILON || width <= 0.0 {
        return value;
    }
    // A plain drag sweeps the whole range across the control's width; shift makes it
    // crawl. Note the multiply: dividing by a fractional sensitivity here would make the
    // fine drag six times *coarser*, which is the sort of thing that reads correctly
    // right up until you use it.
    let scale = if fine { 1.0 / FINE } else { 1.0 };
    let next = value + dx / width * span * scale;
    quantize(next, min, max, step)
}

/// Clamp into range, and onto the step grid if there is one.
///
/// Stepping is measured from `min` rather than from zero, so a control running 1..=12
/// in steps of 1 can actually reach 1 — quantising against zero would be the same thing
/// here, but on a range like 0.5..=2.5 stepping by 0.4 it would not.
pub fn quantize(value: f32, min: f32, max: f32, step: f32) -> f32 {
    let clamped = value.clamp(min.min(max), max.max(min));
    if step > 0.0 {
        let steps = ((clamped - min) / step).round();
        (min + steps * step).clamp(min.min(max), max.max(min))
    } else {
        clamped
    }
}

/// How many decimal places to show for a range of this size.
///
/// Driven by the span rather than the value, so the number keeps the same width as it
/// moves. A readout that gains and loses digits as you drag jitters sideways, and on a
/// column of twenty of them that is the difference between a panel you can scan and one
/// you have to read.
pub fn decimals(min: f32, max: f32, step: f32) -> usize {
    if step >= 1.0 {
        return 0;
    }
    match (max - min).abs() {
        s if s >= 100.0 => 0,
        s if s >= 20.0 => 1,
        s if s >= 2.0 => 2,
        _ => 3,
    }
}

/// Format a value for its readout.
pub fn format(value: f32, min: f32, max: f32, step: f32) -> String {
    let places = decimals(min, max, step);
    // Negative zero is arithmetically correct and reads as a bug.
    let value = if value == 0.0 { 0.0 } else { value };
    format!("{value:.places$}")
}

/// Parse a typed value back, rejecting anything that is not a number.
///
/// Out-of-range input is clamped rather than refused: someone typing 900 into a control
/// that stops at 600 means "as far as it goes", and throwing the edit away teaches them
/// nothing about why.
pub fn parse(text: &str, min: f32, max: f32, step: f32) -> Option<f32> {
    let value: f32 = text.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(quantize(value, min, max, step))
}

/// Whether a row of segmented buttons fits, given the width each label needs.
///
/// The alternative is a dropdown, and the choice between them is made by measuring
/// rather than by guessing from the option count: "Classic | Well" fits in a narrow
/// panel and four presets named like sentences do not, and which is which depends on
/// the width the panel happens to have been dragged to.
pub fn segments_fit(label_widths: &[f32], padding: f32, available: f32) -> bool {
    if label_widths.is_empty() || label_widths.len() > 5 {
        return false;
    }
    let total: f32 = label_widths.iter().map(|w| w + padding).sum();
    total <= available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_sits_where_its_range_puts_it() {
        assert_eq!(fraction(0.0, 0.0, 10.0), 0.0);
        assert_eq!(fraction(5.0, 0.0, 10.0), 0.5);
        assert_eq!(fraction(10.0, 0.0, 10.0), 1.0);
        // Outside the range reads as the end of it, not as an overflowing bar.
        assert_eq!(fraction(-3.0, 0.0, 10.0), 0.0);
        assert_eq!(fraction(30.0, 0.0, 10.0), 1.0);
    }

    #[test]
    fn a_range_of_no_width_does_not_divide_by_zero() {
        assert_eq!(fraction(4.0, 4.0, 4.0), 0.0);
        assert_eq!(drag(4.0, 4.0, 4.0, 0.0, 50.0, 200.0, false), 4.0);
    }

    /// The property that makes relative dragging worth the trouble: touching a control
    /// without moving leaves it exactly as it was.
    #[test]
    fn a_drag_of_nothing_changes_nothing() {
        for value in [0.0f32, 0.3, 0.75, 1.0] {
            assert_eq!(drag(value, 0.0, 1.0, 0.0, 0.0, 200.0, false), value);
        }
    }

    #[test]
    fn dragging_the_full_width_sweeps_the_full_range() {
        let v = drag(0.0, 0.0, 10.0, 0.0, 200.0, 200.0, false);
        assert!((v - 10.0).abs() < 1e-5, "full sweep landed at {v}");
        let back = drag(10.0, 0.0, 10.0, 0.0, -200.0, 200.0, false);
        assert!(back.abs() < 1e-5, "reverse sweep landed at {back}");
    }

    #[test]
    fn holding_shift_moves_the_value_a_sixth_as_far() {
        let coarse = drag(5.0, 0.0, 10.0, 0.0, 20.0, 200.0, false) - 5.0;
        let fine = drag(5.0, 0.0, 10.0, 0.0, 20.0, 200.0, true) - 5.0;
        assert!(fine > 0.0, "fine drag does not move at all");
        assert!(fine < coarse, "fine drag is not finer than a plain one");
        assert!((coarse / fine - FINE).abs() < 0.01, "fine drag is {}x, not {FINE}x", coarse / fine);
    }

    #[test]
    fn a_drag_cannot_leave_the_range() {
        assert_eq!(drag(9.0, 0.0, 10.0, 0.0, 9999.0, 200.0, false), 10.0);
        assert_eq!(drag(1.0, 0.0, 10.0, 0.0, -9999.0, 200.0, false), 0.0);
    }

    /// A stepped control must be able to reach both of its ends. Quantising against zero
    /// rather than the minimum is the classic way to lose one of them.
    #[test]
    fn a_stepped_control_reaches_both_of_its_ends() {
        assert_eq!(quantize(0.9, 1.0, 12.0, 1.0), 1.0);
        assert_eq!(quantize(11.7, 1.0, 12.0, 1.0), 12.0);
        assert_eq!(quantize(4.4, 1.0, 12.0, 1.0), 4.0);
        assert_eq!(quantize(4.6, 1.0, 12.0, 1.0), 5.0);
        // And on a grid that zero is not on:
        assert_eq!(quantize(1.31, 0.5, 2.5, 0.4), 1.3);
    }

    #[test]
    fn a_readout_keeps_its_width_as_the_value_moves() {
        let width = |v: f32| format(v, 0.0, 1.0, 0.0).len();
        assert_eq!(width(0.0), width(0.5));
        assert_eq!(width(0.5), width(1.0));
        // Wide ranges drop the decimals rather than showing noise.
        assert_eq!(format(600.0, 0.0, 3000.0, 0.0), "600");
        assert_eq!(format(0.02, 0.006, 0.06, 0.0), "0.020");
        // Integers never show a decimal point.
        assert_eq!(format(4.0, 1.0, 12.0, 1.0), "4");
    }

    #[test]
    fn a_readout_never_shows_negative_zero() {
        assert_eq!(format(-0.0, -1.0, 1.0, 0.0), "0.00");
        assert_eq!(format(-0.0, 0.0, 1.0, 0.0), "0.000");
    }

    #[test]
    fn typed_values_are_taken_when_they_are_numbers_and_clamped_when_they_are_far() {
        assert_eq!(parse("0.5", 0.0, 1.0, 0.0), Some(0.5));
        assert_eq!(parse("  0.5  ", 0.0, 1.0, 0.0), Some(0.5));
        assert_eq!(parse("900", 0.0, 600.0, 0.0), Some(600.0));
        assert_eq!(parse("-4", 0.0, 600.0, 0.0), Some(0.0));
        assert_eq!(parse("7.6", 1.0, 12.0, 1.0), Some(8.0));
        assert_eq!(parse("", 0.0, 1.0, 0.0), None);
        assert_eq!(parse("wide", 0.0, 1.0, 0.0), None);
        assert_eq!(parse("inf", 0.0, 1.0, 0.0), None);
        assert_eq!(parse("NaN", 0.0, 1.0, 0.0), None);
    }

    #[test]
    fn segments_are_used_when_they_fit_and_not_when_they_do_not() {
        // "Classic | Well" in a narrow panel.
        assert!(segments_fit(&[44.0, 28.0], 20.0, 200.0));
        // Five presets named like sentences in the same panel.
        assert!(!segments_fit(&[80.0, 40.0, 40.0, 62.0, 110.0], 20.0, 200.0));
        // The same two-option control still fits once the panel is dragged wider, and
        // the same long one still does not.
        assert!(segments_fit(&[44.0, 28.0], 20.0, 520.0));
        assert!(!segments_fit(&[180.0, 200.0, 190.0], 20.0, 300.0));
    }

    #[test]
    fn a_choice_with_no_options_or_too_many_is_never_segmented() {
        assert!(!segments_fit(&[], 20.0, 500.0));
        assert!(!segments_fit(&[10.0; 6], 20.0, 5000.0));
    }
}
