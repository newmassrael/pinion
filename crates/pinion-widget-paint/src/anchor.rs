//! R1378 — the anchored-overlay **vertical flip** positioner.
//!
//! A floating overlay (a tooltip, a select / property-grid / data-grid choice
//! dropdown, a colour swatch popup) opens flush against an anchor edge, and
//! *flips* to the opposite side when the preferred side would push it past the
//! viewport edge — the native dropdown behaviour near a screen edge.
//!
//! That drop-below-or-flip-above decision is pure geometry (an anchor span, a
//! panel height, a viewport height, a preferred side → a `y`), independent of
//! what the overlay paints. It was copy-derived across three consumers before
//! this lift — the tooltip positioner ([`crate::tooltip::anchor_position`]),
//! the property grid's `popup_origin`, and the data grid's `popup_anchor` — a
//! Rule-of-Three the popup surface's module doc had earmarked ([`crate::popup`]).
//! The SSOT lives here so those three plus any new dropdown (a select box) share
//! ONE flip decision rather than each re-deriving it.
//!
//! Only the *vertical* decision lifts. The tooltip's horizontal left-clamp is a
//! separate concern with a single consumer today (the grids anchor at a fixed /
//! trigger-relative `x`), so it stays inline in `tooltip` until a 2nd binding
//! needs it — the same 2nd-consumer gate this vertical flip just cleared.

/// Which side of the anchor an overlay prefers to open on. It flips to the
/// opposite side when the preferred side overflows the viewport and the
/// opposite side fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorSide {
    /// Open above the anchor (the panel's bottom flush against the anchor top).
    Above,
    /// Open below the anchor (the panel's top flush against the anchor bottom).
    Below,
}

/// The top `y` of a `panel_h`-tall overlay opened against an anchor spanning
/// `[anchor_y, anchor_y + anchor_h]`, inside a viewport of height `viewport_h`,
/// all in the same coordinate space (window- or container-local; the caller
/// passes whichever bottom bound it means as `viewport_h`).
///
/// The overlay is placed flush against the preferred side (gap 0). It **flips**
/// to the opposite side only when the preferred side overflows the viewport
/// *and* the opposite side fits — so a flip never trades a partial overflow for
/// a fully-clipped panel. When NEITHER side fits (a viewport shorter than the
/// panel) the preferred side is kept (a clamp-only fallback, no oscillation).
///
/// This is the vertical half of [`crate::tooltip::anchor_position`], extracted
/// verbatim; a `Below`-preferred call reproduces the grids' `popup_origin` /
/// `popup_anchor` flip for every reachable (short-panel) input.
#[must_use]
pub fn flip_y(
    anchor_y: u32,
    anchor_h: u32,
    panel_h: u32,
    viewport_h: u32,
    prefer: AnchorSide,
) -> u32 {
    let below_top = anchor_y + anchor_h;
    let above_top = anchor_y.saturating_sub(panel_h);
    let fits_below = below_top.saturating_add(panel_h) <= viewport_h;
    let fits_above = anchor_y >= panel_h;
    match prefer {
        // Prefer below; flip up only if it overflows *and* up fits.
        AnchorSide::Below => {
            if fits_below || !fits_above {
                below_top
            } else {
                above_top
            }
        }
        // Prefer above; flip down only if it overflows *and* down fits.
        AnchorSide::Above => {
            if fits_above || !fits_below {
                above_top
            } else {
                below_top
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_places_flush_under_anchor_with_room() {
        // Anchor [40, 70), panel 24, roomy viewport → drops flush below (70).
        assert_eq!(flip_y(40, 30, 24, 400, AnchorSide::Below), 70);
    }

    #[test]
    fn above_places_flush_over_anchor_with_room() {
        // Anchor top 200, panel 24 → flush above at 176 (bottom touches 200).
        assert_eq!(flip_y(200, 30, 24, 400, AnchorSide::Above), 176);
        assert_eq!(176 + 24, 200, "panel bottom touches the anchor top");
    }

    #[test]
    fn below_flips_above_on_bottom_overflow() {
        // Anchor low: below (370+24=394, +24=418) overflows a 400 viewport, up
        // fits → flip above to 370 - 24 = 346.
        assert_eq!(flip_y(370, 24, 24, 400, AnchorSide::Below), 346);
    }

    #[test]
    fn above_flips_below_on_top_overflow() {
        // Anchor near the top: above (10 < 24) overflows, down fits → flip
        // below to 10 + 24 = 34.
        assert_eq!(flip_y(10, 24, 24, 400, AnchorSide::Above), 34);
    }

    #[test]
    fn below_kept_when_neither_side_fits() {
        // Viewport shorter than the panel: keep the preferred side (no flip),
        // a clamp-only fallback rather than trading one clip for another.
        assert_eq!(flip_y(0, 10, 500, 100, AnchorSide::Below), 10);
    }

    #[test]
    fn above_kept_when_neither_side_fits() {
        // Symmetric: prefer-above with neither side fitting keeps above.
        assert_eq!(flip_y(20, 10, 500, 100, AnchorSide::Above), 0);
    }

    #[test]
    fn above_flip_saturates_at_zero() {
        // A tall panel flipped above a shallow anchor clamps its top to 0.
        assert_eq!(flip_y(5, 10, 40, 30, AnchorSide::Above), 0);
    }

    /// The old grids' hand-rolled `Below`-preferred flip (an unconditional
    /// flip-above on overflow), reproduced to pin the shared primitive
    /// byte-identical for every reachable (short-panel) grid input.
    fn hand_rolled_below(row_top: u32, row_h: u32, panel_h: u32, viewport: u32) -> u32 {
        let below = row_top + row_h;
        if below + panel_h <= viewport {
            below
        } else {
            row_top.saturating_sub(panel_h)
        }
    }

    #[test]
    fn grid_below_case_matches_hand_rolled_flip() {
        // The grids' reachable inputs: a mid-list row that fits below.
        assert_eq!(
            flip_y(120, 28, 3 * 32 + 12, 600, AnchorSide::Below),
            hand_rolled_below(120, 28, 3 * 32 + 12, 600),
        );
    }

    #[test]
    fn grid_flip_case_matches_hand_rolled_flip() {
        // A low row whose popup overflows → both flip above to the same y.
        assert_eq!(
            flip_y(520, 28, 5 * 32 + 12, 600, AnchorSide::Below),
            hand_rolled_below(520, 28, 5 * 32 + 12, 600),
        );
    }
}
