//! R744 §5.27 — **fixed-pitch list virtualization** windowing core.
//!
//! The Model/View entry slice of the Phase-B substrate roadmap
//! (`assets → DnD → Model/View → undo`). This module owns the pure,
//! backend-agnostic arithmetic that decouples a list's *total item
//! count* from the *number of scene nodes actually rendered* — the one
//! piece every virtualized collection (Vello list, TUI list, Phase-C
//! data inspector) shares regardless of how it paints a row.
//!
//! ## The problem it solves
//!
//! Pre-R744 every list binding (`hello-listbox`, `hello-table`, the
//! `todomvc` todo column) built **one [`Scene`](crate::Scene) node per
//! item, eagerly, for the whole dataset**, then wrapped the column in a
//! [`ScrollNode`](crate::scene::ScrollNode) that clips at paint time.
//! That is correct for a 12-row fruit list and catastrophic for the
//! 10 000-row data grids the Phase-D editor needs: the layout pass walks
//! every node, the cache holds every node, the introspection snapshot
//! serializes every node.
//!
//! ## The windowing contract
//!
//! Given the current scroll `offset_y`, the clip-window height, the
//! total `item_count`, and a uniform `row_pitch` (the per-row vertical
//! slot in logical pixels), [`compute_visible_range`] returns the
//! half-open span of item indices whose slots intersect the viewport,
//! padded by `overscan` rows on each side so a fast wheel-flick never
//! exposes an un-built row before the next frame lands. The consumer
//! builds scene nodes for **only** that window.
//!
//! The matching scroll bound is preserved by a full-height *sizer* —
//! see [`content_height`] and
//! [`view_virtual_list`](../../../pinion_widget_paint/virtual_list/fn.view_virtual_list.html)
//! in `pinion-widget-paint`, which positions the windowed rows
//! absolutely inside a container sized to `item_count × row_pitch`. The
//! runtime layout pass then reads that sizer's height into
//! [`ScrollState::set_max`](crate::widgets::scroll::ScrollState::set_max)
//! exactly as it does for a fully-materialized column, so the scrollbar
//! peer sizes its thumb against the *total* extent while only the
//! visible window exists in the tree. This is the canonical
//! "spacer-of-total-height + absolutely-positioned visible items"
//! technique web virtualizers (`react-window` `FixedSizeList`, Qt
//! `QListView` with `uniformItemSizes`, Flutter `ListView.builder` with
//! `itemExtent`) all converge on — adapted to pinion's existing scroll
//! substrate with **zero** changes to scroll, layout, or the scrollbar.
//!
//! ## Scope of this slice (honest boundaries)
//!
//! - **Uniform pitch only.** Variable / measured row heights need a
//!   prefix-sum offset table + a binary search in place of the integer
//!   divides here; that is the next mini-series round, kept out so the
//!   first consumer lands on the simplest correct core.
//! - **No selection / sort / `Model` trait.** The "model" is the
//!   consumer's `item_count` plus a `FnMut(usize) -> Scene` row builder
//!   (the Flutter / `react-window` shape), not a retained trait object.
//!   A formal `VirtualListModel` trait waits for the second consumer
//!   that proves the shape — premature here (abstraction needs a second
//!   consumer). Selection reuses the
//!   existing [`selection`](crate::widgets::selection) helpers when a
//!   selectable virtualized list arrives.
//! - **`u32` pixel ceiling.** A list whose total height exceeds
//!   `u32::MAX` logical pixels saturates [`content_height`]; browsers
//!   cap scroll height the same way. Beyond the first slice.
//!
//! ## Relationship to the §5.27 ratified design
//!
//! §5.27 (ratified R32) originally specced virtualization as a dedicated
//! `Scene::VirtualList` IR variant whose visible window is materialized at
//! the *layout pass* (the view-fn returns a template + `item_fn`). R690.A
//! recorded that variant as never implemented, removed the stale 8th-
//! variant numbering, judged the windowed-re-materialize design valid, and
//! deferred the IR variant to R750+ "evidence-first, re-derive against the
//! current 9-variant `Scene` at impl".
//!
//! R744 lands the §5.27 *capability* now via view-fn composition over the
//! existing [`ScrollNode`](crate::scene::ScrollNode): O(window)
//! materialization per frame, AI-introspectable as scene-data, zero new IR.
//! This is the React-school of virtualization (`react-window` / `TanStack`:
//! the view layer windows against scroll + a known viewport); the R32 IR design
//! is the Flutter/Compose school (the layout/measure phase drives lazy item
//! creation). Both are textbook — this is a peer technique, not a lesser
//! slice of the IR one.
//!
//! Why the IR variant stays deferred (R744.1 honest correction — the
//! earlier "blocked on Scene-clone, a `Box<dyn Fn>` cannot derive `Clone`"
//! note was wrong): `Scene` is *already* not `Clone` (it carries
//! `ExternalNode`'s `Box<dyn External>`), and a cloneable closure variant
//! could anyway use `Rc<dyn Fn>` exactly as `Scene::ImmediateModeNode`
//! (R681) already carries `Rc<RefCell<dyn ImmediateMode>>` — so clone is a
//! non-issue. The real reasons are (1) **evidence-first**: one
//! fixed-viewport consumer exists; the IR variant's payoff (decoupling the
//! view-fn from viewport geometry, so a *flex/resizable* container
//! virtualizes without a caller-supplied viewport) has no consumer yet, and
//! (2) layout-pass node *synthesis* (calling `item_fn` and injecting nodes
//! mid-layout) is a genuinely new layout capability that R690.A flagged for
//! re-derivation. They are additive when a flex-viewport consumer arrives,
//! not a precondition for this slice.
//!
//! Honest limitation of *this* slice: the viewport is caller-supplied
//! (`view_virtual_list(viewport, …)`), mirroring `ScrollNode`'s own
//! caller-supplied `viewport.{w,h}`. It does not auto-adapt to a
//! flex-/resize-sized container — that adaptation is exactly the IR
//! variant's deferred benefit above.

/// R744 §5.27 — the half-open span `[first, first + count)` of item
/// indices a virtualized list must build scene nodes for this frame.
///
/// `count == 0` means nothing is visible (empty dataset, zero-height
/// viewport, or a degenerate pitch); callers should render no rows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VisibleWindow {
    /// First item index in the window (inclusive). Always `0` when the
    /// window is empty.
    pub first: usize,
    /// Number of consecutive item indices in the window. `0` when
    /// nothing is visible.
    pub count: usize,
}

impl VisibleWindow {
    /// The empty window — `first = 0`, `count = 0`.
    pub const EMPTY: Self = Self { first: 0, count: 0 };

    /// Whether the window contains no items.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Last item index in the window (inclusive), or `None` when empty.
    #[must_use]
    pub const fn last(&self) -> Option<usize> {
        if self.count == 0 {
            None
        } else {
            Some(self.first + self.count - 1)
        }
    }

    /// Iterator over the item indices in the window, ascending.
    pub fn indices(&self) -> impl Iterator<Item = usize> {
        self.first..self.first + self.count
    }
}

/// R744 §5.27 — total intrinsic content height of a fixed-pitch list of
/// `item_count` rows, each occupying `row_pitch` logical pixels.
///
/// This is the height the consumer stamps on the full-height *sizer*
/// container so the runtime scroll-bound pass derives the same `max_y`
/// it would from a fully-materialized column. Saturates at `u32::MAX`
/// (see the module-level `u32` ceiling note).
#[must_use]
pub fn content_height(item_count: usize, row_pitch: u32) -> u32 {
    let total = (item_count as u64).saturating_mul(u64::from(row_pitch));
    u32::try_from(total).unwrap_or(u32::MAX)
}

/// R744 §5.27 — compute the window of item indices to render for the
/// current scroll position.
///
/// # Parameters
///
/// - `offset_y` — current vertical scroll offset (logical pixels).
///   Negative values are treated as `0` (the scroll substrate clamps
///   to `0` in practice; this guards a programmatic caller).
/// - `viewport_h` — clip-window height (logical pixels).
/// - `item_count` — total number of items in the dataset.
/// - `row_pitch` — uniform per-row vertical slot (logical pixels).
/// - `overscan` — extra rows rendered above and below the strictly
///   visible span, so a fast scroll does not expose a blank gap before
///   the next frame builds the newly-revealed rows. `0` is valid
///   (strict window); `2`–`4` is the usual smoothing buffer.
///
/// # Returns
///
/// The [`VisibleWindow`] of indices whose `[i·pitch, (i+1)·pitch)` slot
/// intersects `[offset_y, offset_y + viewport_h)`, expanded by
/// `overscan` and clamped to `0..item_count`. [`VisibleWindow::EMPTY`]
/// when `item_count`, `viewport_h`, or `row_pitch` is zero.
#[must_use]
pub fn compute_visible_range(
    offset_y: i32,
    viewport_h: u32,
    item_count: usize,
    row_pitch: u32,
    overscan: usize,
) -> VisibleWindow {
    if item_count == 0 || viewport_h == 0 || row_pitch == 0 {
        return VisibleWindow::EMPTY;
    }
    let pitch = u64::from(row_pitch);
    // `offset_y.max(0)` is non-negative, so `unsigned_abs` reads the
    // magnitude without a sign-loss cast.
    let offset = u64::from(offset_y.max(0).unsigned_abs());
    let bottom = offset + u64::from(viewport_h);
    let max_index = item_count - 1;

    // First slot whose bottom edge `(i+1)·pitch` exceeds `offset` —
    // i.e. the first row with any pixel at or below the viewport top —
    // is `floor(offset / pitch)`.
    let first_visible = usize::try_from(offset / pitch)
        .unwrap_or(usize::MAX)
        .min(max_index);
    // Last slot whose top edge `i·pitch` is strictly above the viewport
    // bottom is `floor((bottom - 1) / pitch)`. `bottom >= 1` here
    // (viewport_h >= 1 after the guard), so the subtraction never
    // underflows.
    let last_visible = usize::try_from((bottom - 1) / pitch)
        .unwrap_or(usize::MAX)
        .min(max_index);

    let first = first_visible.saturating_sub(overscan);
    let last = last_visible.saturating_add(overscan).min(max_index);
    VisibleWindow {
        first,
        count: last - first + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Canonical fixture: 1000 rows of 40-px pitch in a 200-px viewport
    // (exactly 5 rows tall).
    const PITCH: u32 = 40;
    const VP: u32 = 200;
    const N: usize = 1000;

    #[test]
    fn empty_dataset_is_empty_window() {
        assert_eq!(
            compute_visible_range(0, VP, 0, PITCH, 0),
            VisibleWindow::EMPTY
        );
    }

    #[test]
    fn zero_viewport_is_empty_window() {
        assert_eq!(compute_visible_range(0, 0, N, PITCH, 0), VisibleWindow::EMPTY);
    }

    #[test]
    fn zero_pitch_is_empty_window() {
        assert_eq!(compute_visible_range(0, VP, N, 0, 0), VisibleWindow::EMPTY);
    }

    #[test]
    fn top_aligned_no_overscan_shows_exactly_viewport_rows() {
        // offset 0: rows 0..=4 (tops 0,40,80,120,160 all < 200; row 5
        // top 200 is not < 200).
        let w = compute_visible_range(0, VP, N, PITCH, 0);
        assert_eq!(w, VisibleWindow { first: 0, count: 5 });
        assert_eq!(w.last(), Some(4));
    }

    #[test]
    fn overscan_pads_both_sides_and_clamps_at_top() {
        // offset 0, overscan 2: top side saturates to 0, bottom extends
        // by 2 → rows 0..=6.
        let w = compute_visible_range(0, VP, N, PITCH, 2);
        assert_eq!(w, VisibleWindow { first: 0, count: 7 });
    }

    #[test]
    fn middle_offset_windows_correctly() {
        // offset 400: first_visible 10, bottom 600, last_visible
        // floor(599/40)=14 → rows 10..=14.
        let w = compute_visible_range(400, VP, N, PITCH, 0);
        assert_eq!(w, VisibleWindow { first: 10, count: 5 });
    }

    #[test]
    fn middle_offset_with_overscan_expands_symmetrically() {
        // Same as above ±1 overscan → rows 9..=15.
        let w = compute_visible_range(400, VP, N, PITCH, 1);
        assert_eq!(w, VisibleWindow { first: 9, count: 7 });
    }

    #[test]
    fn partial_first_row_includes_the_straddled_top_row() {
        // offset 20: floor(20/40)=0 so row 0 still partly visible;
        // bottom 220, last_visible floor(219/40)=5 → rows 0..=5.
        let w = compute_visible_range(20, VP, N, PITCH, 0);
        assert_eq!(w, VisibleWindow { first: 0, count: 6 });
    }

    #[test]
    fn negative_offset_treated_as_top() {
        assert_eq!(
            compute_visible_range(-500, VP, N, PITCH, 0),
            compute_visible_range(0, VP, N, PITCH, 0),
        );
    }

    #[test]
    fn viewport_taller_than_content_shows_all_rows() {
        // 3 rows, 1000-px viewport → every row visible, last clamps to 2.
        let w = compute_visible_range(0, 1000, 3, PITCH, 0);
        assert_eq!(w, VisibleWindow { first: 0, count: 3 });
    }

    #[test]
    fn bottom_edge_overscan_clamps_to_last_index() {
        // Scroll to the very bottom: offset = (N-5)*pitch so rows
        // 995..=999 are visible; overscan 4 cannot exceed index 999.
        let offset = i32::try_from((N - 5) * PITCH as usize).unwrap();
        let w = compute_visible_range(offset, VP, N, PITCH, 4);
        assert_eq!(w.last(), Some(N - 1));
        assert_eq!(w.first, 995 - 4);
    }

    #[test]
    fn content_height_is_count_times_pitch() {
        assert_eq!(content_height(N, PITCH), 40_000);
        assert_eq!(content_height(0, PITCH), 0);
    }

    #[test]
    fn content_height_saturates_at_u32_max() {
        assert_eq!(content_height(usize::MAX, 1_000_000), u32::MAX);
    }

    #[test]
    fn window_indices_iterates_the_span() {
        let w = compute_visible_range(400, VP, N, PITCH, 0);
        assert_eq!(w.indices().collect::<Vec<_>>(), vec![10, 11, 12, 13, 14]);
    }

    #[test]
    fn huge_dataset_no_overflow() {
        // 100 million rows, scrolled deep — the u64 internals keep the
        // divides exact and the window stays a 5-row span.
        let n = 100_000_000usize;
        let offset = 1_000_000_000; // ~25M rows down at pitch 40
        let w = compute_visible_range(offset, VP, n, PITCH, 0);
        assert_eq!(w.count, 5);
        assert_eq!(w.first, 25_000_000);
    }
}
