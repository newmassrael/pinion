//! §5.39 §5.16 (R807) — single source of truth for keeping an overlay's
//! bordered box clear of the vello framebuffer-top flood row.
//!
//! ## The upstream defect this guards against
//!
//! vello's sparse-strip rasteriser floods its entire top 16px coarse tile
//! when a stroke's coverage reaches the framebuffer `y = 0` scanline. A 2px
//! border on a box flush at the window top therefore rasterises a ~16px-thick
//! top band instead of 2px — a render-vs-intent bug invisible to
//! `scene/snapshot` (the scene carries a faithful 2px border). It is an
//! upstream vello bug (reproduced with a *pure* `vello::Scene::stroke`, no
//! pinion translation, under both `AaConfig::Area` and `AaConfig::Msaa16` —
//! `pinion_shell::headless_screenshot::r806_1_top_tile_flood_is_in_vello_not_the_adapter`),
//! pre-existing and not patchable from this tree (vello is a cargo
//! dependency, not vendored for patching). The vendored-debt entry tracks the
//! upstream report.
//!
//! ## Why this lives in the overlay crate (the SSOT scope)
//!
//! Every box that triggers the flood in practice is a **transparent-fill,
//! bordered overlay flush at the window top** — the §5.39 focus ring and the
//! §5.33 AI highlight. Both are leaf boxes with a transparent fill, so nudging
//! the box geometry off the flood row moves *only the border* (no fill gap)
//! and — because the same paint scene feeds both the renderer and
//! `scene/snapshot from: paint` — keeps scene-as-data byte-faithful to the
//! pixels (§2 #7 preserved). A renderer-side stroke clamp was rejected: it
//! would shift the painted stroke without the scene reflecting it, breaking
//! §2 #7. Filled widget borders flush at the top are not affected because, in
//! practice, top-flush widget borders are anchored at `x = 0` (full-width bars
//! / left rails), and the flood does not trigger against the left edge — only
//! the top scanline. Both overlay constructors
//! ([`crate::focus_ring::inject_focus_ring`], [`crate::inject_highlight`])
//! funnel their final box rect through [`clamp_top_off_flood_row`], so the
//! workaround — and the constant — live in exactly one place.

use pinion_core::scene::Rect;

/// Minimum gap (logical px) an overlay border box keeps below the framebuffer
/// **top** edge so its top stroke's coverage never reaches the `y = 0` flood
/// row. The left/right/bottom edges do not flood, so only the top is clamped.
pub(crate) const TOP_EDGE_INSET: u32 = 1;

/// Clamp `rect`'s top edge to [`TOP_EDGE_INSET`], preserving the bottom edge
/// (the height shrinks by the clamp) so the box stays anchored where it was.
/// No-op when the rect already starts at or below the inset — i.e. only a box
/// flush (or nearly flush) against the framebuffer top is moved, and only by
/// the 0..`TOP_EDGE_INSET` px needed to clear the flood row. Idempotent.
pub(crate) fn clamp_top_off_flood_row(rect: Rect) -> Rect {
    if rect.y >= TOP_EDGE_INSET {
        return rect;
    }
    let bottom = rect.y.saturating_add(rect.h);
    let y = TOP_EDGE_INSET;
    Rect::new(rect.x, y, rect.w, bottom.saturating_sub(y))
}

/// R1022 §5.39 — clamp `rect`'s **far** (right/bottom) edges into the framebuffer
/// extent, preserving the near origin (the width/height shrink by the clamp).
/// The far-edge sibling of [`clamp_top_off_flood_row`]: an overlay box whose far
/// edge lands past the framebuffer rasterises that stroke off-screen, so a widget
/// flush against the window's bottom/right edge loses ring/highlight strokes. The
/// default `Inside` border alignment draws the stroke within the box rect, so a
/// far edge clamped to the framebuffer extent lands the stroke fully visible.
///
/// `viewport` is the layout viewport `(width, height)` the shell fed
/// `compute_layout` (R1006 canon — the value `use_viewport_size` reports). `None`
/// = extent unknown (headless / pure-geometry callers): no clamp, so the box is
/// never shrunk against a size that was never measured. A non-flush box
/// (`far <= viewport`) is unaffected — the `min` is a no-op.
pub(crate) fn clamp_far_edges_into_viewport(rect: Rect, viewport: Option<(u32, u32)>) -> Rect {
    let Some((vw, vh)) = viewport else {
        return rect;
    };
    let right = rect.x.saturating_add(rect.w).min(vw);
    let bottom = rect.y.saturating_add(rect.h).min(vh);
    Rect::new(
        rect.x,
        rect.y,
        right.saturating_sub(rect.x),
        bottom.saturating_sub(rect.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_top_is_clamped_bottom_preserved() {
        // A box flush at the top: top moves to the inset, bottom edge stays.
        let out = clamp_top_off_flood_row(Rect::new(94, 0, 100, 42));
        assert_eq!(out, Rect::new(94, 1, 100, 41));
        assert_eq!(out.y + out.h, 42, "bottom edge preserved");
    }

    #[test]
    fn already_clear_is_unchanged() {
        let r = Rect::new(10, 5, 40, 30);
        assert_eq!(clamp_top_off_flood_row(r), r, "no-op when clear of the row");
    }

    #[test]
    fn at_inset_is_unchanged() {
        let r = Rect::new(10, 1, 40, 30);
        assert_eq!(clamp_top_off_flood_row(r), r, "exactly at the inset is fine");
    }

    #[test]
    fn idempotent() {
        let once = clamp_top_off_flood_row(Rect::new(0, 0, 98, 42));
        let twice = clamp_top_off_flood_row(once);
        assert_eq!(once, twice, "clamping a clamped rect is a no-op");
    }

    #[test]
    fn far_clamp_none_viewport_is_unchanged() {
        let r = Rect::new(10, 20, 999, 999);
        assert_eq!(clamp_far_edges_into_viewport(r, None), r, "unknown extent: no clamp");
    }

    #[test]
    fn far_clamp_in_bounds_is_unchanged() {
        let r = Rect::new(10, 20, 40, 30); // far edges 50, 50 — well inside 200x100
        assert_eq!(clamp_far_edges_into_viewport(r, Some((200, 100))), r, "no-op when inside");
    }

    #[test]
    fn far_clamp_shrinks_overflowing_far_edges_per_axis() {
        // Box overflowing both far edges of a 200x100 viewport: right 0+202>200,
        // bottom 1+102>100. The near origin is preserved; the spans shrink.
        let out = clamp_far_edges_into_viewport(Rect::new(0, 1, 202, 102), Some((200, 100)));
        assert_eq!(out, Rect::new(0, 1, 200, 99));
        assert_eq!(out.x + out.w, 200, "right clamped to the viewport");
        assert_eq!(out.y + out.h, 100, "bottom clamped to the viewport");
    }

    #[test]
    fn far_clamp_collapses_to_zero_when_origin_past_far_edge() {
        // A box whose near origin is already beyond the far edge collapses to a
        // zero span (the caller treats this as "off-screen, skip"). saturating_sub
        // floors at 0 rather than wrapping.
        let out = clamp_far_edges_into_viewport(Rect::new(250, 10, 40, 30), Some((200, 100)));
        assert_eq!(out, Rect::new(250, 10, 0, 30), "right=min(290,200)=200 < x=250 -> width 0");
    }
}
