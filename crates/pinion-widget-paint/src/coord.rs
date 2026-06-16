//! R958.1 §5.16 — paint-coordinate narrowing seam.
//!
//! [`saturating_f32_to_u32`] is the single source for the layout-space
//! `f32` → paint-space `u32` cast every widget-paint helper performs at
//! the seam between parley/taffy geometry (`f32`) and the `Scene`'s
//! integer `Rect` (`u32`). It lived in [`text_field`](crate::text_field)
//! from R657 (private), but it is a fully generic numeric narrowing with
//! nothing text-field-specific; R956 made it `pub` for the line-number
//! gutter, and the [`menu`](crate::menu) `anchor_px` independently
//! re-rolled the same seam — the classic [[helper-crate-home-ssot-axis]]
//! signature of a mis-homed helper. R958.1 moves it here so the field
//! paint, the gutter, and the menu anchor all round identically from one
//! home (a divergence in rounding misaligns overlays with the glyphs).

/// (R657 §5.16) Saturating cast from layout-space `f32` to paint-space
/// `u32`. Negative values clamp to `0`; out-of-range positives clamp to
/// [`u32::MAX`]; `NaN` / `Infinity` clamp to `0` (defensive — parley's
/// [`caret_rect_for_byte_offset`](pinion_text::caret_rect_for_byte_offset)
/// is `finite`-guaranteed by the R56.1.b.2 test battery, but the
/// saturating-cast convention stays the textbook narrowing seam per
/// [[r56-1-b-2-parley-f32-narrowing]]).
///
/// The one home every widget-paint `f32` → `u32` coordinate cast routes
/// through (`text_field::view_field` / `ime_caret_rect_for`, the
/// `hello-textarea` line-number gutter, `menu::anchor_px`), so an overlay
/// and the glyphs it frames round to the same pixel row.
#[must_use]
pub fn saturating_f32_to_u32(v: f32) -> u32 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "u32::MAX -> f32 rounds to a single saturating ceiling"
    )]
    let ceiling = u32::MAX as f32;
    if !v.is_finite() || v < 0.0 {
        0
    } else if v >= ceiling {
        u32::MAX
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "guarded by is_finite / >=0 / < ceiling above"
        )]
        let out = v as u32;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::saturating_f32_to_u32;

    #[test]
    fn finite_in_range_truncates_toward_zero() {
        assert_eq!(saturating_f32_to_u32(0.0), 0);
        assert_eq!(saturating_f32_to_u32(3.9), 3);
        assert_eq!(saturating_f32_to_u32(42.0), 42);
    }

    #[test]
    fn negative_and_non_finite_clamp_to_zero() {
        // `NaN` / +/-`Infinity` are defensively clamped to 0 (not saturated):
        // the R657 contract is "non-finite -> 0", so an unexpected non-finite
        // input pins to the origin rather than a far-off pixel.
        assert_eq!(saturating_f32_to_u32(-1.0), 0);
        assert_eq!(saturating_f32_to_u32(f32::NAN), 0);
        assert_eq!(saturating_f32_to_u32(f32::NEG_INFINITY), 0);
        assert_eq!(saturating_f32_to_u32(f32::INFINITY), 0);
    }

    #[test]
    fn out_of_range_finite_positive_saturates_to_max() {
        // A large but FINITE value past the u32 ceiling saturates to u32::MAX.
        assert_eq!(saturating_f32_to_u32(1.0e30), u32::MAX);
    }
}
