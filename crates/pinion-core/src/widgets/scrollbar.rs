//! R55.D.1 §5.45 — `ScrollBar` geometry primitive.
//!
//! Closed-form thumb-rectangle derivation for a visible scrollbar
//! peer of a [`ScrollNode`](crate::scene::ScrollNode). This sub-axis
//! covers the **paint geometry only** — the §5.38 SCXML statechart
//! (Idle / Hover / Pressing / Dragging) and the pointer-event
//! routing that drives the [`ScrollState`](crate::widgets::scroll::ScrollState)
//! offset stay on the R55.D.2 / R55.D.3 carry.
//!
//! ## Why a closed-form helper instead of a widget binding first?
//!
//! pinion's §5.38 widget catalog pattern is `SCXML + Rust struct +
//! External`, with paint geometry derived from the cached state. A
//! visible scrollbar is the rare case where the paint geometry is
//! a **pure function of [`ScrollState`] + track rect**, not of a
//! statechart's transition history — the thumb position is `f(scroll
//! offset, content size, viewport size, track extent)`, period.
//!
//! Splitting the axis into "geometry first / statechart second /
//! routing third" lets the first sub-round land a textbook
//! standalone primitive (consumable by `examples/hello-listbox`
//! today via a manual paint composition) without the
//! [[substrate-incompleteness-signal]] of forcing the first
//! consumer to also wire up the not-yet-extracted pointer routing.
//!
//! ## Reference layout math
//!
//! | Symbol | Meaning |
//! |---|---|
//! | `track_extent` | length of the scrollbar track along the scroll axis (`track.h` for vertical, `track.w` for horizontal) |
//! | `track_cross` | thickness of the scrollbar perpendicular to the scroll axis |
//! | `viewport_extent` | visible content size along the scroll axis (`ScrollNode.viewport.h` / `.w`) |
//! | `content_extent` | full content size along the scroll axis (after layout) |
//! | `scroll_offset` | current `ScrollState::offset_*` value, clamped to `0..=(content - viewport)` |
//! | `min_thumb_size` | floor for thumb size so the thumb stays grabbable even on very long content (Material/UIKit convention: 24–32 px) |
//!
//! ```text
//! thumb_extent = max(min_thumb_size, floor(track_extent * viewport_extent / content_extent))
//! scroll_max   = content_extent - viewport_extent           (clamped to 0)
//! thumb_pos    = floor((track_extent - thumb_extent) * scroll_offset / scroll_max)   if scroll_max > 0
//!              = 0                                                                   if scroll_max == 0
//! ```
//!
//! Degenerate cases:
//! - `content <= viewport`: nothing to scroll — thumb fills the
//!   track, no draggable peer needed.
//! - `track_extent == 0` or `content_extent == 0`: returns a zero-
//!   extent thumb at the track origin (paint backend can elide).
//! - `scroll_offset > scroll_max`: clamped saturating-down inside
//!   the helper, so the caller does not have to pre-validate.

use crate::scene::Rect;

/// Orientation of a scrollbar track. Mirrors the W3C
/// `aria-orientation` enumeration and the §5.38 Slider axis (R51.39)
/// so a future widget binding can reuse the same primitive without
/// translating between two orientation enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollBarOrientation {
    /// Track grows along the y axis; thumb slides up / down. Used
    /// for vertical scroll content (the common case for list-like
    /// widgets such as `hello-listbox`).
    Vertical,
    /// Track grows along the x axis; thumb slides left / right.
    /// Used for horizontally-scrolling content (table / timeline
    /// views, the future R55.E carry).
    Horizontal,
}

/// Result of [`scrollbar_thumb_rect`] — the track that was passed
/// in (so the caller can fan it out into a single paint helper
/// without re-threading the input) plus the derived thumb rect.
/// Both rects share the same coordinate frame as the input `track`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollBarGeometry {
    /// Orientation echoed back so a paint helper can branch on the
    /// axis (e.g. cap shape / hover hit) without re-deriving it.
    pub orientation: ScrollBarOrientation,
    /// Track rectangle in the caller-supplied coordinate frame.
    /// The paint helper draws this as the scrollbar rail background.
    pub track: Rect,
    /// Thumb rectangle in the same frame as [`Self::track`]. Always
    /// contained inside `track` (modulo the `min_thumb_size` floor
    /// honoring — see module docs for the degenerate cases).
    pub thumb: Rect,
}

/// R55.D.1 §5.45 — closed-form thumb-rect derivation.
///
/// Pure function of the input arguments — no `ScrollState` borrow,
/// no `Signal` subscription, no allocation. The caller passes the
/// authoritative scroll-axis scalars (extracted via
/// [`ScrollState::offset`](crate::widgets::scroll::ScrollState::offset)
/// and the matching [`ScrollNode`](crate::scene::ScrollNode)'s
/// viewport / content extents at layout time) and the helper
/// returns the thumb rect for the paint pass.
///
/// `min_thumb_size` is the floor (Material/UIKit convention: 24
/// px). Setting it to `0` reverts to the strict ratio-of-content
/// thumb size, which lets thumb vanish for very long content — not
/// recommended for production, but useful for unit-test math
/// verification.
///
/// See module docs for the reference math, the closed-form
/// degenerate-case table, and the rationale for splitting geometry
/// (this sub-round) from statechart + pointer routing (R55.D.2 /
/// R55.D.3 carry).
#[must_use]
pub fn scrollbar_thumb_rect(
    orientation: ScrollBarOrientation,
    track: Rect,
    viewport_extent: u32,
    content_extent: u32,
    scroll_offset: u32,
    min_thumb_size: u32,
) -> ScrollBarGeometry {
    let (track_extent, track_cross) = match orientation {
        ScrollBarOrientation::Vertical => (track.h, track.w),
        ScrollBarOrientation::Horizontal => (track.w, track.h),
    };

    // Degenerate guards: empty track or empty content → zero thumb
    // anchored at the track origin. Paint backend can elide the
    // zero-area rect; no signed-arithmetic invariants to defend.
    if track_extent == 0 || content_extent == 0 {
        let thumb = thumb_at(orientation, track, 0, 0, track_cross);
        return ScrollBarGeometry { orientation, track, thumb };
    }

    // Nothing to scroll: thumb fills the entire track. Material /
    // UIKit / web all hide the bar entirely in this case, but the
    // helper stays paint-agnostic; the caller can match `thumb ==
    // track` to elide the paint at the composition layer.
    if content_extent <= viewport_extent {
        let thumb = thumb_at(orientation, track, 0, track_extent, track_cross);
        return ScrollBarGeometry { orientation, track, thumb };
    }

    // Thumb size = floor(track_extent * viewport / content), with
    // floor at `min_thumb_size` and ceiling at `track_extent`. The
    // u64 widening avoids overflow on the multiplication; the
    // inputs are u32 but their product can exceed u32::MAX (e.g.
    // 100_000 viewport × 100_000 track = 10^10). The division
    // result is mathematically `<= track_extent` (since content >
    // viewport on this path), so `u32::try_from` cannot fail; the
    // `unwrap_or(track_extent)` is the unreachable saturation
    // fallback that satisfies clippy's `cast_possible_truncation`
    // gate without an explicit allow.
    let product = u64::from(track_extent) * u64::from(viewport_extent);
    let ratio_thumb =
        u32::try_from(product / u64::from(content_extent)).unwrap_or(track_extent);
    let thumb_extent = ratio_thumb.max(min_thumb_size).min(track_extent);

    // Scroll range and offset clamp. content > viewport guard above
    // ensures `scroll_max >= 1`.
    let scroll_max = content_extent - viewport_extent;
    let clamped_offset = scroll_offset.min(scroll_max);

    // Available travel for the thumb's top-edge / left-edge. Same
    // mathematical bound as above: `(thumb_travel * clamped_offset)
    // / scroll_max <= thumb_travel <= track_extent <= u32::MAX`, so
    // `try_from` saturates only on the unreachable overflow case.
    let thumb_travel = track_extent - thumb_extent;
    let travel_product = u64::from(thumb_travel) * u64::from(clamped_offset);
    let thumb_pos_offset =
        u32::try_from(travel_product / u64::from(scroll_max)).unwrap_or(thumb_travel);

    let thumb = thumb_at(orientation, track, thumb_pos_offset, thumb_extent, track_cross);
    ScrollBarGeometry { orientation, track, thumb }
}

/// Helper that materialises the thumb rectangle in the right
/// orientation from the scroll-axis pose (`offset_along` from track
/// origin + `size_along` the scroll axis) and the cross-axis
/// thickness. Extracted so the three early-return paths in
/// [`scrollbar_thumb_rect`] share one source of truth for the
/// orientation-to-rect mapping.
fn thumb_at(
    orientation: ScrollBarOrientation,
    track: Rect,
    offset_along: u32,
    size_along: u32,
    cross: u32,
) -> Rect {
    match orientation {
        ScrollBarOrientation::Vertical => Rect::new(
            track.x,
            track.y.saturating_add(offset_along),
            cross,
            size_along,
        ),
        ScrollBarOrientation::Horizontal => Rect::new(
            track.x.saturating_add(offset_along),
            track.y,
            size_along,
            cross,
        ),
    }
}

#[cfg(test)]
mod r55_d1_tests {
    //! R55.D.1 §5.45 — `scrollbar_thumb_rect` closed-form regression
    //! battery. Covers the textbook math (proportional thumb size +
    //! position) plus every degenerate case the module-doc table
    //! enumerates.

    use super::{
        scrollbar_thumb_rect, ScrollBarGeometry, ScrollBarOrientation,
    };
    use crate::scene::Rect;

    fn v(
        track: Rect,
        viewport: u32,
        content: u32,
        offset: u32,
        min: u32,
    ) -> ScrollBarGeometry {
        scrollbar_thumb_rect(
            ScrollBarOrientation::Vertical,
            track,
            viewport,
            content,
            offset,
            min,
        )
    }

    fn h(
        track: Rect,
        viewport: u32,
        content: u32,
        offset: u32,
        min: u32,
    ) -> ScrollBarGeometry {
        scrollbar_thumb_rect(
            ScrollBarOrientation::Horizontal,
            track,
            viewport,
            content,
            offset,
            min,
        )
    }

    #[test]
    fn vertical_thumb_at_top_of_track() {
        // track 200 tall, viewport 100 of 400 content (1/4 visible),
        // offset 0 → thumb (h=50) anchored at track top.
        let g = v(Rect::new(0, 0, 8, 200), 100, 400, 0, 0);
        assert_eq!(g.thumb, Rect::new(0, 0, 8, 50));
        assert_eq!(g.orientation, ScrollBarOrientation::Vertical);
    }

    #[test]
    fn vertical_thumb_at_bottom_of_track() {
        // Same shape, offset = scroll_max (400-100=300) → thumb at
        // bottom of available travel (200-50=150).
        let g = v(Rect::new(0, 0, 8, 200), 100, 400, 300, 0);
        assert_eq!(g.thumb, Rect::new(0, 150, 8, 50));
    }

    #[test]
    fn vertical_thumb_at_mid_track() {
        // offset half of scroll_max → thumb at half of available
        // travel: floor(150 * 150 / 300) = 75.
        let g = v(Rect::new(0, 0, 8, 200), 100, 400, 150, 0);
        assert_eq!(g.thumb, Rect::new(0, 75, 8, 50));
    }

    #[test]
    fn horizontal_mirrors_vertical_math() {
        // Same math, rotated. content 400 wide, viewport 100, track
        // 200 wide, offset mid → thumb (w=50) at x=75.
        let g = h(Rect::new(0, 0, 200, 8), 100, 400, 150, 0);
        assert_eq!(g.thumb, Rect::new(75, 0, 50, 8));
        assert_eq!(g.orientation, ScrollBarOrientation::Horizontal);
    }

    #[test]
    fn content_smaller_than_viewport_fills_track() {
        // Content fits inside viewport → no scroll possible → thumb
        // fills the whole track. Composition layer can elide the
        // paint by matching `thumb == track`.
        let g = v(Rect::new(0, 0, 8, 200), 100, 50, 0, 24);
        assert_eq!(g.thumb, Rect::new(0, 0, 8, 200));
    }

    #[test]
    fn content_equals_viewport_fills_track() {
        // Edge case: content == viewport → scroll_max would be 0 →
        // helper short-circuits to "fill track" before the divide.
        let g = v(Rect::new(0, 0, 8, 200), 100, 100, 0, 24);
        assert_eq!(g.thumb, Rect::new(0, 0, 8, 200));
    }

    #[test]
    fn min_thumb_size_clamps_tiny_ratio() {
        // Very long content: 200 * 100 / 10_000 = 2 — would be
        // nearly invisible. min=24 raises the thumb to a grabbable
        // size. Travel reduces proportionally: (200-24) = 176.
        let g = v(Rect::new(0, 0, 8, 200), 100, 10_000, 0, 24);
        assert_eq!(g.thumb, Rect::new(0, 0, 8, 24));
    }

    #[test]
    fn min_thumb_size_position_uses_reduced_travel() {
        // Same shape, offset = scroll_max (9_900) → thumb at
        // bottom of reduced travel: (200-24) = 176.
        let g = v(Rect::new(0, 0, 8, 200), 100, 10_000, 9_900, 24);
        assert_eq!(g.thumb, Rect::new(0, 176, 8, 24));
    }

    #[test]
    fn saturating_offset_clamps_to_scroll_max() {
        // Caller passes offset > scroll_max — helper clamps
        // saturating-down rather than panicking. Output identical
        // to the "at bottom" case.
        let g = v(Rect::new(0, 0, 8, 200), 100, 400, 9_999_999, 0);
        assert_eq!(g.thumb, Rect::new(0, 150, 8, 50));
    }

    #[test]
    fn zero_track_extent_returns_zero_thumb() {
        // Layout pass collapsed the scrollbar to zero along the
        // scroll axis — defensive path. Thumb keeps the track's
        // cross-axis thickness (`w=8`) for layout consistency but
        // reports a zero `h`, so its area is zero and the paint
        // backend can elide.
        let g = v(Rect::new(10, 20, 8, 0), 100, 400, 0, 24);
        assert_eq!(g.thumb, Rect::new(10, 20, 8, 0));
    }

    #[test]
    fn zero_content_returns_zero_thumb() {
        // Content not measured yet (`set_max` not called). Helper
        // stays defensive — zero-extent thumb at track origin.
        let g = v(Rect::new(10, 20, 8, 200), 100, 0, 0, 24);
        assert_eq!(g.thumb, Rect::new(10, 20, 8, 0));
    }

    #[test]
    fn track_origin_offset_propagates() {
        // Track positioned at (x=100, y=50) inside a parent — thumb
        // rect honors the same origin (helper does NOT relativize
        // to the track's own origin; output shares the input
        // coordinate frame).
        let g = v(Rect::new(100, 50, 8, 200), 100, 400, 150, 0);
        assert_eq!(g.thumb, Rect::new(100, 125, 8, 50));
    }

    #[test]
    fn no_overflow_on_large_extents() {
        // 100_000 × 1_000_000 product overflows u32 (~10^11). The
        // helper widens to u64 internally; this test pins that the
        // widening is in place (a regression that removed it would
        // overflow and wrap).
        let g = v(Rect::new(0, 0, 8, 100_000), 100_000, 1_000_000, 450_000, 0);
        // ratio_thumb = 100_000 * 100_000 / 1_000_000 = 10_000.
        // thumb_travel = 100_000 - 10_000 = 90_000.
        // scroll_max = 1_000_000 - 100_000 = 900_000.
        // thumb_pos = 90_000 * 450_000 / 900_000 = 45_000.
        assert_eq!(g.thumb, Rect::new(0, 45_000, 8, 10_000));
    }
}
