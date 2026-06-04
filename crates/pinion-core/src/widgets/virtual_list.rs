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
//! - **Uniform pitch** is this module's O(1) path
//!   ([`compute_visible_range`] / [`content_height`]). **Variable /
//!   explicit row heights** are the R745 peer path
//!   ([`RowOffsets`] + [`compute_visible_range_variable`]): a prefix-sum
//!   offset table searched in O(log n) — `react-window`'s `VariableSizeList`
//!   to the fixed path's `FixedSizeList`. Both are kept (the uniform path
//!   avoids building an n-entry table; they are peer techniques, not one
//!   subsuming the other — see the R745 design note below). *Measured*
//!   heights (render → read back the laid-out height → feed the table) are
//!   still deferred: that needs a layout-pass measurement round-trip, the
//!   same capability the IR variant wants (see the §5.27 note).
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

/// R776 §5.27 — the minimal scroll offset that brings item `index`'s slot
/// fully into a `viewport_h`-tall window, given the current `offset_y` and
/// a uniform `row_pitch`.
///
/// This is the **scroll-into-view** (align: auto) arithmetic every
/// virtualized collection needs the moment selection becomes keyboard-
/// navigable: the target row may not be materialized, so "scroll to the
/// selected row" cannot be a DOM `scrollIntoView` on a node — it is an
/// offset computed from the row's known slot. The canonical primitive
/// behind `react-window`'s `scrollToItem(align:"auto")`, Qt
/// `QAbstractItemView::scrollTo(EnsureVisible)`, and Flutter
/// `Scrollable.ensureVisible`. It is the windowing peer of
/// [`compute_visible_range`]: that maps an offset to the visible window;
/// this maps a target index to the offset that reveals it.
///
/// Align-auto semantics (never move a target that is already visible —
/// the least-jarring scroll):
///
/// - Row already wholly inside the window → returns `offset_y` unchanged.
/// - Row top above the window top → align the row's top to the viewport
///   top (returns `index · pitch`).
/// - Row bottom below the window bottom → align the row's bottom to the
///   viewport bottom (returns `index · pitch + pitch - viewport_h`).
/// - Row taller than the whole viewport (degenerate uniform-pitch case) →
///   align its top, so navigation always reveals the row's start rather
///   than oscillating between its edges.
///
/// The result is the **desired** `offset_y`; the caller hands it to
/// [`ScrollState::scroll_to`](crate::widgets::scroll::ScrollState::scroll_to),
/// which clamps it to `[0, max_y]`. A zero-height viewport (the pre-layout
/// first-paint state of a flex `AutoSizer` list) or a zero pitch returns
/// `offset_y` unchanged — there is no window to reveal into yet.
#[must_use]
pub fn scroll_offset_to_reveal(index: usize, offset_y: i32, viewport_h: u32, row_pitch: u32) -> i32 {
    if viewport_h == 0 || row_pitch == 0 {
        return offset_y;
    }
    let pitch = u64::from(row_pitch);
    let row_top = (index as u64).saturating_mul(pitch);
    let row_bottom = row_top.saturating_add(pitch);
    // `offset_y.max(0)` is non-negative, so `unsigned_abs` reads the
    // magnitude without a sign-loss cast (mirrors `compute_visible_range`).
    let cur = u64::from(offset_y.max(0).unsigned_abs());
    let view_bottom = cur.saturating_add(u64::from(viewport_h));

    let target = if row_top < cur {
        // Above the window → align the row top to the viewport top.
        row_top
    } else if row_bottom > view_bottom {
        // Below the window → align the row bottom to the viewport bottom.
        // `.min(row_top)` collapses to align-top when the row is taller
        // than the viewport (then `row_bottom - viewport_h > row_top`),
        // so a degenerate over-tall row reveals its start, not its end.
        row_bottom.saturating_sub(u64::from(viewport_h)).min(row_top)
    } else {
        // Already fully visible → align-auto leaves the offset alone.
        return offset_y;
    };
    i32::try_from(target).unwrap_or(i32::MAX)
}

/// R745 §5.27 — a prefix-sum offset table over **explicit per-row
/// heights**, enabling O(log n) windowing for a variable-height
/// virtualized list.
///
/// This is the variable-pitch peer of [`compute_visible_range`]'s integer
/// divide: where the uniform path computes a row's top as `index · pitch`
/// in O(1), a variable-height list must remember where every row starts.
/// The canonical structure (`react-window` `VariableSizeList`,
/// `TanStack Virtual`, Qt `QHeaderView`'s section offsets) is a cumulative
/// sum: `offsets[i]` is the total height of rows `0..i`, i.e. the top edge
/// of row `i`, and the final entry is the total content height. Because
/// the sums are monotonically non-decreasing, the visible window is found
/// by binary search ([`compute_visible_range_variable`]).
///
/// The table is built once from the height slice and reused across frames
/// (the consumer caches it; rebuilding only when the heights change), so
/// the O(n) construction is amortized to nothing per frame while every
/// scroll resolves in O(log n).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowOffsets {
    /// Cumulative tops: `offsets[i]` = Σ heights[0..i]. Length is
    /// `item_count + 1`; `offsets[0] == 0` and `offsets[item_count]` is
    /// the total content height. Held as `u64` so the prefix sum cannot
    /// overflow before the final saturating cast to a `u32` pixel extent.
    offsets: Vec<u64>,
}

impl RowOffsets {
    /// Build the prefix-sum table from a slice of per-row heights (logical
    /// pixels). `heights[i]` is the height of row `i`; a height of `0` is
    /// valid (a collapsed row) and simply contributes no extent.
    ///
    /// An empty slice yields a single-entry table (`offsets == [0]`):
    /// zero items, zero total height.
    #[must_use]
    pub fn from_heights(heights: &[u32]) -> Self {
        let mut offsets = Vec::with_capacity(heights.len() + 1);
        let mut acc: u64 = 0;
        offsets.push(0);
        for &h in heights {
            acc = acc.saturating_add(u64::from(h));
            offsets.push(acc);
        }
        Self { offsets }
    }

    /// Number of rows in the table.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Whether the table has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.item_count() == 0
    }

    /// Total intrinsic content height of all rows, saturated to `u32`
    /// (the sizer extent — the variable-height analogue of
    /// [`content_height`]). See the module `u32` ceiling note.
    #[must_use]
    pub fn total_height(&self) -> u32 {
        u32::try_from(*self.offsets.last().unwrap_or(&0)).unwrap_or(u32::MAX)
    }

    /// Top edge of row `index` (logical pixels), saturated to `u32`.
    /// Returns the total height for `index == item_count` (the one-past
    /// sentinel) and saturates to the total for any out-of-range index.
    #[must_use]
    pub fn row_top(&self, index: usize) -> u32 {
        let top = self
            .offsets
            .get(index)
            .or_else(|| self.offsets.last())
            .copied()
            .unwrap_or(0);
        u32::try_from(top).unwrap_or(u32::MAX)
    }

    /// Height of row `index` (logical pixels), or `0` for an
    /// out-of-range index.
    #[must_use]
    pub fn row_height(&self, index: usize) -> u32 {
        let (Some(&top), Some(&bottom)) =
            (self.offsets.get(index), self.offsets.get(index + 1))
        else {
            return 0;
        };
        u32::try_from(bottom - top).unwrap_or(u32::MAX)
    }
}

/// R745 §5.27 — compute the visible window for a **variable-height** list
/// from its prefix-sum [`RowOffsets`] table.
///
/// The variable-pitch peer of [`compute_visible_range`]: instead of
/// dividing the scroll offset by a uniform pitch, it binary-searches the
/// cumulative tops for the rows straddling the viewport edges. The
/// `overscan` padding and the clamp to `0..item_count` are identical to
/// the uniform path, and the result is the same [`VisibleWindow`] — so the
/// view assembly and a11y windowing are shared across both paths.
///
/// # Parameters
///
/// - `offset_y` — current vertical scroll offset (logical pixels);
///   negatives treated as `0`.
/// - `viewport_h` — clip-window height (logical pixels).
/// - `offsets` — the prefix-sum table built from the row heights.
/// - `overscan` — extra rows rendered on each side of the strict window.
///
/// # Returns
///
/// The [`VisibleWindow`] of indices whose `[top_i, top_i + height_i)` slot
/// intersects `[offset_y, offset_y + viewport_h)`, expanded by `overscan`
/// and clamped to `0..item_count`. [`VisibleWindow::EMPTY`] when the table
/// is empty, the viewport is zero-height, or the total content height is
/// zero (every row collapsed).
#[must_use]
pub fn compute_visible_range_variable(
    offset_y: i32,
    viewport_h: u32,
    offsets: &RowOffsets,
    overscan: usize,
) -> VisibleWindow {
    let item_count = offsets.item_count();
    if item_count == 0 || viewport_h == 0 || offsets.total_height() == 0 {
        return VisibleWindow::EMPTY;
    }
    let offset = u64::from(offset_y.max(0).unsigned_abs());
    let bottom = offset + u64::from(viewport_h);
    let max_index = item_count - 1;

    // `tops` = `offsets.offsets`, the sorted cumulative tops including the
    // one-past total at the end. `partition_point(|t| t <= x)` returns the
    // count of tops `<= x`; subtracting one gives the index of the row
    // *containing* pixel `x` (mirrors `floor(x / pitch)` in the uniform
    // path), clamped to the last real row.
    let tops = &offsets.offsets;
    let first_visible = tops
        .partition_point(|&t| t <= offset)
        .saturating_sub(1)
        .min(max_index);
    // The last row inside the viewport is the one whose top is strictly
    // above `bottom` (`t < bottom` ⟺ `t <= bottom - 1`, the last visible
    // pixel — written as `< bottom` so the count never needs `bottom - 1`).
    let last_visible = tops
        .partition_point(|&t| t < bottom)
        .saturating_sub(1)
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

    // ── R776 scroll-into-view (scroll_offset_to_reveal) ─────────────

    #[test]
    fn reveal_already_visible_row_does_not_scroll() {
        // offset 0, viewport 5 rows: rows 0..=4 fully visible. Revealing
        // any of them is a no-op (align: auto never moves a visible row).
        for index in 0..5 {
            assert_eq!(
                scroll_offset_to_reveal(index, 0, VP, PITCH),
                0,
                "row {index} already visible — no scroll",
            );
        }
    }

    #[test]
    fn reveal_row_below_window_aligns_to_bottom() {
        // offset 0, viewport 200 (5 rows): row 5 (top 200, bottom 240) is
        // just below. Align bottom → offset = 240 - 200 = 40.
        assert_eq!(scroll_offset_to_reveal(5, 0, VP, PITCH), 40);
        // Row 6 (bottom 280) → 280 - 200 = 80.
        assert_eq!(scroll_offset_to_reveal(6, 0, VP, PITCH), 80);
    }

    #[test]
    fn reveal_row_above_window_aligns_to_top() {
        // Scrolled to offset 400 (rows 10..). Reveal row 3 (top 120),
        // which is above → align top → offset = 120.
        assert_eq!(scroll_offset_to_reveal(3, 400, VP, PITCH), 120);
        // Reveal row 0 → offset 0.
        assert_eq!(scroll_offset_to_reveal(0, 400, VP, PITCH), 0);
    }

    #[test]
    fn reveal_deep_index_from_top_scrolls_far_down() {
        // The headline: navigate to row 9 999 (End key) from the top.
        // Row 9 999 bottom = 10_000 * 40 = 400_000; align bottom →
        // 400_000 - 200 = 399_800. The row was never materialized.
        assert_eq!(scroll_offset_to_reveal(9_999, 0, VP, PITCH), 399_800);
    }

    #[test]
    fn reveal_partial_bottom_row_aligns_it_fully() {
        // offset 20: rows straddle (row 0 partly off-top, row 5 partly
        // off-bottom at top 200..220 inside 20..220). Revealing row 5
        // (bottom 240 > view_bottom 220) aligns bottom → 240 - 200 = 40.
        assert_eq!(scroll_offset_to_reveal(5, 20, VP, PITCH), 40);
    }

    #[test]
    fn reveal_zero_viewport_is_a_noop() {
        // Pre-layout flex AutoSizer state: no window to reveal into yet.
        assert_eq!(scroll_offset_to_reveal(100, 500, 0, PITCH), 500);
    }

    #[test]
    fn reveal_zero_pitch_is_a_noop() {
        assert_eq!(scroll_offset_to_reveal(3, 0, VP, 0), 0);
    }

    #[test]
    fn reveal_row_taller_than_viewport_aligns_top() {
        // Degenerate: pitch 300 > viewport 200. Row 2 (top 600). From a
        // deep offset (1000, above the row) → align top 600; from above
        // (offset 0, row below) → would align bottom 900-200=700, but the
        // .min(row_top=600) collapses to align-top 600 so the row's start
        // is shown rather than its end.
        assert_eq!(scroll_offset_to_reveal(2, 1000, 200, 300), 600);
        assert_eq!(scroll_offset_to_reveal(2, 0, 200, 300), 600);
    }

    #[test]
    fn reveal_negative_offset_treated_as_top() {
        // A programmatic negative offset reads as 0 (mirrors the windowing
        // math); revealing row 5 from there aligns its bottom the same as
        // from offset 0.
        assert_eq!(
            scroll_offset_to_reveal(5, -100, VP, PITCH),
            scroll_offset_to_reveal(5, 0, VP, PITCH),
        );
    }

    // ── R745 variable-height (RowOffsets + binary-search windowing) ──

    // A 4-row repeating height pattern, distinctly non-uniform so the
    // prefix sums are irregular: 20, 60, 20, 60, … Tops: 0, 20, 80, 100,
    // 160, 180, 240, … (every 2 rows = 80 px).
    fn var_heights(n: usize) -> Vec<u32> {
        (0..n).map(|i| if i % 2 == 0 { 20 } else { 60 }).collect()
    }

    #[test]
    fn row_offsets_builds_prefix_sums_and_total() {
        let o = RowOffsets::from_heights(&[20, 60, 20, 60]);
        assert_eq!(o.item_count(), 4);
        assert!(!o.is_empty());
        assert_eq!(o.total_height(), 160);
        assert_eq!(o.row_top(0), 0);
        assert_eq!(o.row_top(1), 20);
        assert_eq!(o.row_top(2), 80);
        assert_eq!(o.row_top(3), 100);
        // One-past sentinel = total.
        assert_eq!(o.row_top(4), 160);
        assert_eq!(o.row_height(0), 20);
        assert_eq!(o.row_height(1), 60);
        assert_eq!(o.row_height(3), 60);
        // Out-of-range height is 0.
        assert_eq!(o.row_height(4), 0);
    }

    #[test]
    fn row_offsets_empty_slice_is_empty_table() {
        let o = RowOffsets::from_heights(&[]);
        assert_eq!(o.item_count(), 0);
        assert!(o.is_empty());
        assert_eq!(o.total_height(), 0);
        // row_top of an empty table saturates to the (zero) total.
        assert_eq!(o.row_top(0), 0);
        assert_eq!(o.row_height(0), 0);
    }

    #[test]
    fn variable_equals_uniform_when_all_heights_equal() {
        // A variable table of equal heights must window identically to the
        // O(1) uniform path — the two are peers, not divergent.
        let o = RowOffsets::from_heights(&vec![PITCH; N]);
        for &offset in &[0, 20, 400, 4000, 39_960] {
            for overscan in [0usize, 2, 4] {
                assert_eq!(
                    compute_visible_range_variable(offset, VP, &o, overscan),
                    compute_visible_range(offset, VP, N, PITCH, overscan),
                    "variable must match uniform at offset {offset}, overscan {overscan}",
                );
            }
        }
    }

    #[test]
    fn empty_table_is_empty_window() {
        let o = RowOffsets::from_heights(&[]);
        assert_eq!(
            compute_visible_range_variable(0, VP, &o, 0),
            VisibleWindow::EMPTY
        );
    }

    #[test]
    fn zero_viewport_variable_is_empty_window() {
        let o = RowOffsets::from_heights(&var_heights(100));
        assert_eq!(
            compute_visible_range_variable(0, 0, &o, 0),
            VisibleWindow::EMPTY
        );
    }

    #[test]
    fn all_zero_heights_is_empty_window() {
        // Total content height 0 → nothing to show, even with rows.
        let o = RowOffsets::from_heights(&[0, 0, 0, 0]);
        assert_eq!(o.item_count(), 4);
        assert_eq!(o.total_height(), 0);
        assert_eq!(
            compute_visible_range_variable(0, VP, &o, 0),
            VisibleWindow::EMPTY
        );
    }

    #[test]
    fn variable_top_aligned_windows_by_height() {
        // Tops 0,20,80,100,160,180,240,260,320,340,400,...; viewport 200.
        // first_visible 0 (top 0 <= 0). last pixel 199: largest top <= 199
        // is 180 (row 5) → rows 0..=5.
        let o = RowOffsets::from_heights(&var_heights(1000));
        let w = compute_visible_range_variable(0, VP, &o, 0);
        assert_eq!(w, VisibleWindow { first: 0, count: 6 });
    }

    #[test]
    fn variable_partial_first_row_includes_straddled_top() {
        // offset 50 sits inside row 1 (top 20, bottom 80) → first_visible 1.
        // bottom 250, last pixel 249: largest top <= 249 is 240 (row 6) →
        // rows 1..=6.
        let o = RowOffsets::from_heights(&var_heights(1000));
        let w = compute_visible_range_variable(50, VP, &o, 0);
        assert_eq!(w, VisibleWindow { first: 1, count: 6 });
    }

    #[test]
    fn variable_overscan_pads_and_clamps_at_top() {
        // Same as top-aligned (rows 0..=5) but overscan 2: top saturates to
        // 0, bottom extends to row 7 → rows 0..=7.
        let o = RowOffsets::from_heights(&var_heights(1000));
        let w = compute_visible_range_variable(0, VP, &o, 2);
        assert_eq!(w, VisibleWindow { first: 0, count: 8 });
    }

    #[test]
    fn variable_middle_offset_windows_correctly() {
        // The pattern repeats every 2 rows = 80 px. offset 800 = 20 pairs
        // down → top of row 20 is exactly 800 → first_visible 20. bottom
        // 1000, last pixel 999: 999 = 12*80 + 39 → within pair 12 from row
        // 24... compute: top of row 24 = 960, row 25 = 980, row 26 = 1040.
        // largest top <= 999 is 980 (row 25) → rows 20..=25.
        let o = RowOffsets::from_heights(&var_heights(1000));
        let w = compute_visible_range_variable(800, VP, &o, 0);
        assert_eq!(w, VisibleWindow { first: 20, count: 6 });
    }

    #[test]
    fn variable_negative_offset_treated_as_top() {
        let o = RowOffsets::from_heights(&var_heights(1000));
        assert_eq!(
            compute_visible_range_variable(-500, VP, &o, 0),
            compute_visible_range_variable(0, VP, &o, 0),
        );
    }

    #[test]
    fn variable_viewport_taller_than_content_shows_all_rows() {
        let o = RowOffsets::from_heights(&[20, 60, 20]); // total 100
        let w = compute_visible_range_variable(0, 1000, &o, 0);
        assert_eq!(w, VisibleWindow { first: 0, count: 3 });
    }

    #[test]
    fn variable_bottom_edge_overscan_clamps_to_last_index() {
        // 1000 rows, total height = 500 pairs * 80 = 40_000. Scroll near
        // the bottom; overscan 4 cannot exceed the last index.
        let o = RowOffsets::from_heights(&var_heights(1000));
        let max_off = i32::try_from(o.total_height() - VP).unwrap();
        let w = compute_visible_range_variable(max_off, VP, &o, 4);
        assert_eq!(w.last(), Some(999));
    }

    #[test]
    fn variable_total_height_saturates_at_u32_max() {
        // Many tall rows whose sum exceeds u32::MAX saturate (the u64 prefix
        // sum stays exact internally; only the public extent caps).
        let o = RowOffsets::from_heights(&vec![1_000_000; 5000]); // 5e9 > u32::MAX
        assert_eq!(o.total_height(), u32::MAX);
        // Windowing still resolves a small span near the top.
        let w = compute_visible_range_variable(0, VP, &o, 0);
        assert_eq!(w.first, 0);
        assert!(w.count >= 1);
    }
}
