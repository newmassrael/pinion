//! R1546 §5.3 — WCAG 2.x contrast over [`Color`].
//!
//! # Why this is in `pinion-core`
//!
//! These three functions were written for `pinion-chart`'s value-labelled
//! heatmap (R1438): a number painted on a colour ramp is unreadable unless the
//! ink is COMPUTED per cell, and "compute" here means the published WCAG
//! quantity, not a lightness guess. They lived in `pinion_chart::color_scale`
//! while that was their only caller.
//!
//! R1546 gave them a second one that is nowhere near a chart. A text run can
//! now declare its own background ([`TextStyle::bg_color`]), so a run carries a
//! foreground / background PAIR — and the §7 wire publishes the contrast of
//! that pair, which is how a headless agent tells "I set a highlight" from "I
//! set a *readable* highlight" without looking at pixels. Contrast is a
//! property of two [`Color`]s and of nothing else, so with a consumer on each
//! side of the dependency edge its home is the crate that owns `Color`.
//! `pinion-chart` re-exports all three, so its own callers are unchanged.
//!
//! # Alpha
//!
//! Every function here ignores alpha, and that is a statement rather than an
//! omission: a translucent colour's rendered luminance depends on what is
//! behind it, which the caller knows and this module does not. A caller that
//! needs the contrast of a translucent layer composites it against its own
//! backdrop first and passes the result — or, like the §7 background band,
//! declines to publish a number it cannot compute.
//!
//! [`TextStyle::bg_color`]: crate::style::TextStyle::bg_color

use crate::style::Color;

/// A colour's WCAG relative luminance, `0.0..=1.0`.
///
/// The ITU-R BT.709 coefficients over LINEAR-light channels (the sRGB EOTF is
/// already what [`Color::to_linear`] applies) — the same quantity WCAG 2.x
/// contrast is defined on, so a value computed here is directly comparable
/// against the published thresholds. Alpha is ignored (see the module doc).
#[must_use]
pub fn relative_luminance(color: Color) -> f32 {
    let linear = color.to_linear();
    0.2126 * linear.x + 0.7152 * linear.y + 0.0722 * linear.z
}

/// The WCAG contrast ratio between two colours, `1.0..=21.0` (identical
/// colours give `1.0`; black against white gives `21.0`).
///
/// Symmetric in its arguments — the ratio is defined lighter-over-darker, so
/// the caller does not have to know which is which. WCAG 2.x asks for `4.5` for
/// body text and `3.0` for large text.
#[must_use]
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Whichever of two inks reads better on `background` — the one with the higher
/// [`contrast_ratio`].
///
/// COMPUTED per background, never assumed from a lightness threshold. A ramp's
/// endpoints are often theme-derived, so "dark ink on the lower half" is only
/// correct for ramps that happen to span the full lightness range; a ramp whose
/// dark end is merely mid-luminance wants the dark ink almost everywhere, and a
/// fixed rule would put unreadable light ink on half its cells. Ties go to
/// `first`, so a caller can express a preference for equal-contrast inks.
#[must_use]
pub fn readable_ink(background: Color, first: Color, second: Color) -> Color {
    if contrast_ratio(background, first) >= contrast_ratio(background, second) {
        first
    } else {
        second
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Color = Color::rgb(0, 0, 0);
    const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);

    #[test]
    fn luminance_spans_the_unit_interval_at_the_extremes() {
        assert!((relative_luminance(BLACK) - 0.0).abs() < 1e-6);
        assert!((relative_luminance(WHITE) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn contrast_of_black_on_white_is_the_published_maximum() {
        assert!((contrast_ratio(BLACK, WHITE) - 21.0).abs() < 0.01);
        // Symmetric: the ratio is lighter-over-darker whichever way round.
        assert!((contrast_ratio(WHITE, BLACK) - 21.0).abs() < 0.01);
    }

    #[test]
    fn contrast_of_a_colour_with_itself_is_one() {
        assert!((contrast_ratio(WHITE, WHITE) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn readable_ink_picks_the_higher_contrast_and_ties_go_to_first() {
        assert_eq!(readable_ink(WHITE, BLACK, WHITE), BLACK);
        assert_eq!(readable_ink(BLACK, BLACK, WHITE), WHITE);
        assert_eq!(readable_ink(WHITE, BLACK, BLACK), BLACK);
    }

    /// Alpha is ignored — stated in the module doc, asserted here so a future
    /// change that starts reading it fails loudly rather than silently
    /// altering every published ratio.
    #[test]
    fn alpha_does_not_move_the_luminance() {
        let opaque = Color::rgb(0x40, 0x80, 0xC0);
        let faint = Color::rgb(0x40, 0x80, 0xC0).with_alpha(0x10);
        assert!((relative_luminance(opaque) - relative_luminance(faint)).abs() < 1e-6);
    }
}
