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
}
