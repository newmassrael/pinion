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
//! The matching scroll bound is preserved by a full-height *sizer* — see [`content_height`]
//! and
//! [`view_virtual_list`](../../../pinion_widget_paint/virtual_list/fn.view_virtual_list.html)
//! in `pinion-widget-paint`, which positions the windowed rows absolutely inside a container
//! sized to `item_count × row_pitch`. The runtime layout pass then reads that sizer's height into
//! [`ScrollState::set_max`](crate::widgets::scroll::ScrollState::set_max) exactly as it does for
//! a fully-materialized column, so the scrollbar peer sizes its thumb against
//! the *total* extent while only the visible window exists in the tree. This
//! is the canonical "spacer-of-total-height + absolutely-positioned visible
//! items" technique web virtualizers (`react-window` `FixedSizeList`, the toolkit list view with `uniformItemSizes`,
//! another retained-mode toolkit `ListView.builder` with `itemExtent`) all converge on — adapted to
//! pinion's existing scroll substrate with **zero** changes to scroll, layout,
//! or the scrollbar.
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
//!   (the retained-mode toolkit / `react-window` shape), not a retained trait object.
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
//! `Scene` IR variant whose visible window is materialized at
//! the *layout pass* (the view-fn returns a template + `item_fn`). R690.A
//! recorded that variant as never implemented, removed the stale 8th-
//! variant numbering, judged the windowed-re-materialize design valid, and
//! deferred the IR variant to R750+ "evidence-first, re-derive against the
//! current 9-variant `Scene` at impl".
//!
//! R744 lands the §5.27 *capability* now via view-fn composition over the
//! existing [`ScrollNode`](crate::scene::ScrollNode): O(window) materialization per
//! frame, AI-introspectable as scene-data, zero new IR. This is the web UI
//! library-school of virtualization (`react-window` / `TanStack`: the view layer windows against
//! scroll + a known viewport); the R32 IR design is the retained-mode
//! toolkit/another declarative toolkit school (the layout/measure phase drives
//! lazy item creation). Both are textbook — this is a peer technique, not a
//! lesser slice of the IR one.
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

/// R927 §5.27 — the 0-based **page indices** a row [`VisibleWindow`] spans,
/// for a list paged into fixed `page_size` chunks. The windowing companion to
/// [`compute_visible_range`]: that maps a scroll offset to the visible row
/// window; this maps that row window to the pages a lazy/async source must
/// have fetched to fill it. Returns an empty iterator for an empty window (a
/// zero / not-yet-known item count fetches no pages) or a zero `page_size`.
///
/// Every paged-virtualized consumer needs this exact arithmetic — the page
/// containing the window's first row through the page containing its last —
/// so it lives once here, lifted R927 from the byte-identical copies in
/// `hello-lazy-list` (page-indexed) and `hello-asset-browser` (query-keyed):
/// an off-by-one in either would be a fetch bug (divergence-is-a-bug).
pub fn pages_in_window(window: &VisibleWindow, page_size: usize) -> impl Iterator<Item = usize> {
    let span = match window.last() {
        Some(last) if page_size > 0 => Some((window.first / page_size)..=(last / page_size)),
        _ => None,
    };
    span.into_iter().flatten()
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
/// behind `react-window`'s `scrollToItem(align:"auto")`, the toolkit
/// `scrollTo(EnsureVisible)`, and another retained-mode toolkit
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
pub fn scroll_offset_to_reveal(
    index: usize,
    offset_y: i32,
    viewport_h: u32,
    row_pitch: u32,
) -> i32 {
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
        row_bottom
            .saturating_sub(u64::from(viewport_h))
            .min(row_top)
    } else {
        // Already fully visible → align-auto leaves the offset alone.
        return offset_y;
    };
    i32::try_from(target).unwrap_or(i32::MAX)
}

/// R793.1 §5.27 — **rows per measured viewport-ful**: the `PageUp` /
/// `PageDown` step a keyboard-navigation controller takes over a
/// uniform-pitch virtualized collection, derived from the [`ScrollState`](crate::widgets::scroll::ScrollState)'s
/// measured viewport height and `row_pitch`. Clamped to ≥ 1 (a viewport
/// shorter than one row, or an unmeasured `0`-height viewport, still pages
/// by a single row), with the `row_pitch.max(1)` guard against a divide by
/// zero. The viewport-geometry sibling of [`scroll_offset_to_reveal`]; both
/// `nav_select_key` and `dir_nav_key` read it so the page step is computed
/// one way (the R792 self-grep lifted this byte-identical 2-controller
/// derivation into its natural home next to the reveal computation).
#[must_use]
pub fn page_rows(scroll: &crate::widgets::scroll::ScrollState, row_pitch: u32) -> usize {
    let (_, viewport_h) = scroll.measured_viewport();
    usize::try_from(viewport_h / row_pitch.max(1))
        .unwrap_or(1)
        .max(1)
}

/// R793.1 §5.27 — scroll a uniform-pitch virtualized collection so the row
/// at `index` is revealed, from the [`ScrollState`](crate::widgets::scroll::ScrollState)'s current offset. The
/// `&ScrollState`-applying wrapper around [`scroll_offset_to_reveal`]:
/// reads the current offset + measured viewport, computes the align-auto
/// target, and hands it to [`ScrollState::scroll_to`](crate::widgets::scroll::ScrollState::scroll_to)
/// (which clamps to `[0, max_y]`). Lifted (R792 self-grep) from the
/// byte-identical reveal-glue both `nav_select_key` and `dir_nav_key`
/// carried, so the scroll-into-view idiom lives once next to the geometry
/// it applies.
pub fn reveal_row(scroll: &crate::widgets::scroll::ScrollState, index: usize, row_pitch: u32) {
    let (_, viewport_h) = scroll.measured_viewport();
    let offset = scroll_offset_to_reveal(index, scroll.offset_y(), viewport_h, row_pitch);
    scroll.scroll_to(0, offset);
}

/// R996/R1005 §5.27 — whether the viewport sits at the newest row (the bottom)
/// of a growable virtualized list. **Tail-follow is derived, not stored:** "if
/// you were at the bottom, stay at the bottom as rows append" — `tail -f` /
/// a terminal's autoscroll. Read *before* a count grows (the was-following
/// decision) and by the view (Following / Paused status) — one predicate, no
/// `following` flag to keep consistent. The degenerate empty list (`0`/`0`) is
/// at its bottom.
///
/// R1005 lift (the [`follow_tail`] reducer's pure half), on the 2nd streaming
/// consumer (`hello-streaming-log` in-memory + `hello-paged-stream` paged):
/// R996 left it local as the 1st consumer; the paged view is the 2nd, so the
/// predicate lives here once next to the windowing geometry it reads.
///
/// Distinct from the RPC `ScrollEdges::at_bottom` wire field (`pinion-rpc`,
/// `offset == max`): that is a §5.12 wire-introspection edge in a W3C-style
/// four-edge set; this is the windowing reducer's tail-follow predicate (`>=`,
/// robust to an over-clamped offset). Different layers, **not** one SSOT —
/// `pinion-core` cannot depend on `pinion-rpc`.
#[must_use]
pub fn at_bottom(offset_y: i32, max_y: i32) -> bool {
    offset_y >= max_y
}

/// R996/R1005 §5.27 — the **tail-follow reducer shape** for a growable
/// virtualized list: after the row `count` has grown, grow the scroll bound to
/// the new content extent and, when the viewport `was_following` the tail, pin
/// it to the new bottom.
///
/// [`ScrollState::scroll_to`](crate::widgets::scroll::ScrollState::scroll_to)
/// clamps to the *current* `max_y`, which the layout pass only grows on the
/// *next* frame — so this grows the bound itself (through the
/// [`max_scroll_offset`](crate::widgets::scroll::max_scroll_offset) /
/// [`content_height`] SSOTs the layout pass also uses; it re-affirms the
/// identical value next frame, Signal-equality-skipped) *before* pinning to the
/// new bottom, so the autoscroll lands on the same frame as the append. When
/// not following, the bound still grows (the scrollbar extent tracks the log)
/// but the paused viewport stays put.
///
/// The bound and the pin use the **same** `viewport_h`: when following, the new
/// tail is below the window, so the pin target *is* the just-computed bottom
/// bound. (It deliberately does not route through
/// [`reveal_row`], which would re-derive a viewport height
/// from `measured_viewport` — that can differ from `viewport_h` before the
/// first layout pass, growing the bound while silently not pinning.)
///
/// The caller captures `was_following` via [`at_bottom`] **before** the count
/// grows, performs its domain-specific append (an in-memory `Vec` push, or a
/// paged source's count bump + tail-page [`invalidate`](crate::reactive::ResourceCache::invalidate)),
/// then calls this. R1005 lift on the 2nd streaming consumer.
///
/// R1445 — this reducer needs the extent to be **arithmetic**: `count` ×
/// `row_pitch` is what lets it name the bound before the layout pass does. A
/// consumer whose extent is layout-measured (wrapped prose, mixed-height
/// widgets) has no such number and belongs to the sibling,
/// [`ScrollState::follow_measured_tail`](crate::widgets::scroll::ScrollState::follow_measured_tail),
/// which defers the identical grow-then-pin to the pass that measures it.
pub fn follow_tail(
    scroll: &crate::widgets::scroll::ScrollState,
    count: usize,
    row_pitch: u32,
    viewport_h: u32,
    was_following: bool,
) {
    let bottom =
        crate::widgets::scroll::max_scroll_offset(content_height(count, row_pitch), viewport_h);
    scroll.set_max(0, bottom);
    if was_following && count > 0 {
        scroll.scroll_to(0, bottom);
    }
}

/// R745 §5.27 — a prefix-sum offset table over **explicit per-row
/// heights**, enabling O(log n) windowing for a variable-height
/// virtualized list.
///
/// This is the variable-pitch peer of [`compute_visible_range`]'s integer divide: where the
/// uniform path computes a row's top as `index · pitch` in O(1), a variable-height list
/// must remember where every row starts. The canonical structure (`react-window` `VariableSizeList`,
/// `TanStack Virtual`, the toolkit header view's section offsets) is a cumulative sum: `offsets[i]` is
/// the total height of rows `0..i`, i.e. the top edge of row `i`, and the final
/// entry is the total content height. Because the sums are monotonically
/// non-decreasing, the visible window is found by binary search ([`compute_visible_range_variable`]).
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
        let (Some(&top), Some(&bottom)) = (self.offsets.get(index), self.offsets.get(index + 1))
        else {
            return 0;
        };
        u32::try_from(bottom - top).unwrap_or(u32::MAX)
    }

    /// Index of the row whose slot contains vertical `pixel` (`row_top ≤
    /// pixel < row_bottom`), clamped to the last row; `0` for an empty
    /// table. The prefix-sum analogue of `floor(pixel / pitch)` — because
    /// the cumulative tops are monotone, the containing row is
    /// `partition_point(|t| t ≤ pixel) − 1`.
    ///
    /// The shared kernel behind both [`compute_visible_range_variable`]'s
    /// window edges and [`scroll_anchor`]'s anchor row, so the
    /// row-at-pixel search lives once (divergence-is-a-bug).
    #[must_use]
    pub fn row_at(&self, pixel: u32) -> usize {
        self.row_at_px(u64::from(pixel))
    }

    /// `u64`-pixel variant of [`Self::row_at`] for the windowing math,
    /// whose viewport-bottom edge (`offset + viewport_h`) can exceed
    /// `u32::MAX` before the row clamp. Private: the public API is the
    /// `u32`-pixel [`Self::row_at`] (a scroll offset derives from an
    /// `i32`).
    #[must_use]
    fn row_at_px(&self, pixel: u64) -> usize {
        let max_index = self.item_count().saturating_sub(1);
        self.offsets
            .partition_point(|&t| t <= pixel)
            .saturating_sub(1)
            .min(max_index)
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

    // The row containing the viewport-top pixel, and the row containing
    // the last visible pixel (`bottom - 1`; `bottom >= 1` after the guard).
    // Both go through the [`RowOffsets::row_at`] prefix-sum kernel — the
    // variable-pitch analogue of `floor(x / pitch)` — so the row-at-pixel
    // search lives once (shared with [`scroll_anchor`]).
    let first_visible = offsets.row_at_px(offset);
    let last_visible = offsets.row_at_px(bottom - 1);

    let first = first_visible.saturating_sub(overscan);
    let last = last_visible.saturating_add(overscan).min(max_index);
    VisibleWindow {
        first,
        count: last - first + 1,
    }
}

/// R1194 §5.27 — progressively-**measured** row heights with an estimated
/// fallback for not-yet-rendered rows.
///
/// The *measured* peer of [`RowOffsets`]. Where `RowOffsets` needs every
/// height up-front (the caller already knows them), a measured list starts
/// from a single `estimated` height for every row and **refines each row
/// as it is rendered**: the layout pass measures the row's laid-out height
/// and feeds it back via [`Self::measure`] (the "layout-pass measurement
/// round-trip" the `RowOffsets` scope note reserves). This is the
/// substrate for content whose height cannot be known without laying it
/// out — wrapped log/packet rows, variable-height document paragraphs, an
/// asset browser's differently-sized thumbnails — the
/// `react-virtualized` `CellMeasurer` / `TanStack Virtual`
/// `measureElement` / the toolkit `ResizeToContents` capability.
///
/// A row's height is its measured value once known, else `estimated`, so
/// [`Self::offsets`] always yields a complete [`RowOffsets`] table — the
/// window is correct from the first frame (against the estimate) and the
/// total content height *refines toward the exact sum* as rows scroll into
/// view. `estimated` is clamped to `≥ 1`: a zero estimate would give a
/// zero-height list that renders no rows, so no row is ever measured — a
/// deadlock this guard removes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredHeights {
    /// Fallback height for a row whose real height has not been measured
    /// yet. Clamped to `≥ 1` on construction (see the type docs).
    estimated: u32,
    /// `measured[i] == Some(h)` once row `i` has been laid out and
    /// harvested; `None` while it still falls back to `estimated`.
    measured: Vec<Option<u32>>,
}

impl MeasuredHeights {
    /// A table of `item_count` rows, every row initially unmeasured (using
    /// `estimated`). `estimated` is clamped to `≥ 1` (see the type docs).
    #[must_use]
    pub fn new(item_count: usize, estimated: u32) -> Self {
        Self {
            estimated: estimated.max(1),
            measured: vec![None; item_count],
        }
    }

    /// Number of rows.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.measured.len()
    }

    /// Whether the table has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.measured.is_empty()
    }

    /// The estimated fallback height (always `≥ 1`).
    #[must_use]
    pub fn estimated(&self) -> u32 {
        self.estimated
    }

    /// Resize to `n` rows, **preserving** measured heights for rows that
    /// stay in range; rows beyond `n` are dropped and new rows start
    /// unmeasured. The growable/streaming path (a live log gaining lines).
    pub fn set_count(&mut self, n: usize) {
        self.measured.resize(n, None);
    }

    /// Height of row `index`: its measured height if known, else
    /// `estimated`. An out-of-range index reports `estimated` (defensive —
    /// the offset table never indexes out of range).
    #[must_use]
    pub fn height(&self, index: usize) -> u32 {
        self.measured
            .get(index)
            .copied()
            .flatten()
            .unwrap_or(self.estimated)
    }

    /// Record row `index`'s laid-out height. Returns `true` iff this
    /// **changed** the stored value (first measurement of the row, or a
    /// remeasure to a different height) — the harvest uses the bit to
    /// decide whether to reflow and anchor-correct the scroll. An
    /// out-of-range index is ignored and returns `false`.
    pub fn measure(&mut self, index: usize, height: u32) -> bool {
        match self.measured.get_mut(index) {
            Some(slot) if *slot != Some(height) => {
                *slot = Some(height);
                true
            }
            _ => false,
        }
    }

    /// The measured height of row `index`, or `None` if it still uses the
    /// estimate. Distinguishes a measured row from an estimated one — unlike
    /// [`Self::height`], which returns the estimate for both.
    #[must_use]
    pub fn measured_height(&self, index: usize) -> Option<u32> {
        self.measured.get(index).copied().flatten()
    }

    /// How many rows have a measured height.
    #[must_use]
    pub fn measured_count(&self) -> usize {
        self.measured.iter().filter(|h| h.is_some()).count()
    }

    /// Whether every row has been measured — the table is now the exact
    /// content height, no estimate remaining. `false` for an empty table.
    #[must_use]
    pub fn is_fully_measured(&self) -> bool {
        !self.measured.is_empty() && self.measured.iter().all(Option::is_some)
    }

    /// Build the prefix-sum [`RowOffsets`] table for the current
    /// measured-or-estimated heights, for windowing
    /// ([`compute_visible_range_variable`]) and anchor math
    /// ([`scroll_anchor`]). O(n) per call, like `RowOffsets::from_heights`;
    /// the reactive wrapper rebuilds it once per measurement generation, so
    /// steady-state frames reuse the cached table. Measured lists target
    /// the variable-content scale (thousands of rows); the million-row
    /// path stays uniform/paged.
    #[must_use]
    pub fn offsets(&self) -> RowOffsets {
        let heights: Vec<u32> = (0..self.item_count()).map(|i| self.height(i)).collect();
        RowOffsets::from_heights(&heights)
    }

    /// Total content height for the current heights (measured where known,
    /// estimated elsewhere), saturated to `u32` — the sizer extent, which
    /// refines toward the exact sum as rows are measured. Convenience for
    /// callers that need only the total; a caller that also needs the
    /// per-row geometry should build [`Self::offsets`] once and read
    /// [`RowOffsets::total_height`] off it.
    #[must_use]
    pub fn total_height(&self) -> u32 {
        let total: u64 = (0..self.item_count())
            .map(|i| u64::from(self.height(i)))
            .sum();
        u32::try_from(total).unwrap_or(u32::MAX)
    }
}

/// R1194 §5.27 — the scroll **anchor** for a variable-height list: the row
/// at the top of the viewport, and how many pixels of it are scrolled
/// above the top edge — `(row_index, pixels_into_row)`.
///
/// The stable reference point for the measured list's "no-jump"
/// correction. When rows above the viewport resolve from `estimated` to
/// their measured height the whole column below them shifts; capturing the
/// anchor *before* the heights change and restoring the offset *after*
/// ([`anchor_preserving_offset`]) keeps the visible content pinned. This
/// is `TanStack Virtual`'s measure-then-restore and `react-virtualized`
/// `CellMeasurer` scroll compensation.
///
/// The anchor row is the one *containing* the viewport-top pixel
/// (`offset_y`, via [`RowOffsets::row_at`]) and `pixels_into_row =
/// offset_y − row_top(anchor)`. An empty table anchors at `(0, 0)`.
#[must_use]
pub fn scroll_anchor(offset_y: i32, offsets: &RowOffsets) -> (usize, u32) {
    if offsets.is_empty() {
        return (0, 0);
    }
    let offset = offset_y.max(0).unsigned_abs();
    let anchor = offsets.row_at(offset);
    // `row_at` returns the row containing `offset`, so its top is ≤ offset.
    let sub = offset.saturating_sub(offsets.row_top(anchor));
    (anchor, sub)
}

/// R1194 §5.27 — the scroll offset that keeps `anchor` (from
/// [`scroll_anchor`]) at the same screen position against a possibly
/// changed heights table: the anchor row's *new* top plus the same
/// `pixels_into_row`.
///
/// Saturates into `i32`; the caller hands the result to
/// [`ScrollState::scroll_to`](crate::widgets::scroll::ScrollState::scroll_to),
/// which clamps to `[0, max_y]`. Applied only when a measurement actually
/// changed a height (otherwise the offset is already correct), so a
/// steady-state frame is untouched.
#[must_use]
pub fn anchor_preserving_offset(anchor: (usize, u32), offsets: &RowOffsets) -> i32 {
    let (index, sub) = anchor;
    let top = u64::from(offsets.row_top(index));
    i32::try_from(top.saturating_add(u64::from(sub))).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Canonical fixture: 1000 rows of 40-px pitch in a 200-px viewport
    // (exactly 5 rows tall).
    const PITCH: u32 = 40;
    const VP: u32 = 200;
    const N: usize = 1000;

    // ── R793.1 lifted viewport-geometry helpers ─────────────────────

    #[test]
    fn page_rows_is_measured_viewport_over_pitch_clamped() {
        use crate::widgets::scroll::ScrollState;
        let s = ScrollState::new();
        // An unmeasured (0-height) viewport still pages by at least one row.
        assert_eq!(page_rows(&s, 32), 1, "unmeasured viewport pages by one row");
        s.set_measured_viewport(360, 320);
        assert_eq!(
            page_rows(&s, 32),
            10,
            "320px / 32px pitch = 10 rows per page"
        );
        assert_eq!(
            page_rows(&s, 0),
            320,
            "zero pitch is guarded against div-by-zero"
        );
        s.set_measured_viewport(360, 20);
        assert_eq!(
            page_rows(&s, 32),
            1,
            "a viewport shorter than one row pages by one"
        );
    }

    #[test]
    fn reveal_row_scrolls_a_deep_row_into_view_from_current_offset() {
        use crate::widgets::scroll::ScrollState;
        let s = ScrollState::new();
        s.set_max(0, 320_000);
        s.set_measured_viewport(360, 384);
        reveal_row(&s, 9_999, 32);
        assert!(
            s.offset_y() > 300_000,
            "deep row revealed, offset {}",
            s.offset_y()
        );
        reveal_row(&s, 0, 32);
        assert_eq!(s.offset_y(), 0, "row 0 reveals at the top");
    }

    #[test]
    fn at_bottom_is_inclusive_and_handles_the_empty_extent() {
        assert!(at_bottom(864, 864), "exactly at the bottom follows");
        assert!(at_bottom(900, 864), "past the bottom (clamped) follows");
        assert!(!at_bottom(863, 864), "one px above the bottom is paused");
        assert!(at_bottom(0, 0), "an empty list is at its bottom");
    }

    #[test]
    fn follow_tail_grows_the_bound_and_pins_only_when_following() {
        use crate::widgets::scroll::{ScrollState, max_scroll_offset};
        const PITCH: u32 = 24;
        const VH: u32 = 14 * PITCH; // 336
        // Following: a freshly-grown 50-row list pins to the new bottom.
        let s = ScrollState::new();
        s.set_measured_viewport(400, VH);
        let bottom = max_scroll_offset(content_height(50, PITCH), VH);
        follow_tail(&s, 50, PITCH, VH, true);
        assert_eq!(s.max().1, bottom, "bound grew to the new extent");
        assert!(bottom > 0, "50 rows exceed the 14-row viewport");
        assert_eq!(s.offset_y(), bottom, "followed to the new bottom");
        // Paused: the bound still grows, but the viewport stays put.
        let s2 = ScrollState::new();
        s2.set_measured_viewport(400, VH);
        s2.scroll_to(0, 0);
        follow_tail(&s2, 50, PITCH, VH, false);
        assert_eq!(
            s2.max().1,
            bottom,
            "bound grows even when paused (scrollbar tracks)"
        );
        assert_eq!(s2.offset_y(), 0, "paused viewport is not pinned");
    }

    #[test]
    fn empty_dataset_is_empty_window() {
        assert_eq!(
            compute_visible_range(0, VP, 0, PITCH, 0),
            VisibleWindow::EMPTY
        );
    }

    #[test]
    fn zero_viewport_is_empty_window() {
        assert_eq!(
            compute_visible_range(0, 0, N, PITCH, 0),
            VisibleWindow::EMPTY
        );
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
        assert_eq!(
            w,
            VisibleWindow {
                first: 10,
                count: 5
            }
        );
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

    // ── R927 pages_in_window (row window → paged source pages) ───────

    #[test]
    fn pages_in_window_spans_first_through_last_page() {
        // Rows 250..=370 (count 121) at page_size 100 → pages 2 and 3.
        let w = VisibleWindow {
            first: 250,
            count: 121,
        };
        assert_eq!(w.last(), Some(370));
        assert_eq!(pages_in_window(&w, 100).collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn pages_in_window_single_page_when_window_fits() {
        // Rows 10..=29 are all on page 0.
        let w = VisibleWindow {
            first: 10,
            count: 20,
        };
        assert_eq!(pages_in_window(&w, 100).collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn pages_in_window_partial_last_page_still_spans_it() {
        // Rows 1600..=1666 (a partial last page under a filtered count) → page 16.
        let w = VisibleWindow {
            first: 1600,
            count: 67,
        };
        assert_eq!(pages_in_window(&w, 100).collect::<Vec<_>>(), vec![16]);
    }

    #[test]
    fn pages_in_window_empty_window_is_no_pages() {
        assert_eq!(pages_in_window(&VisibleWindow::EMPTY, 100).count(), 0);
    }

    #[test]
    fn pages_in_window_zero_page_size_is_no_pages() {
        let w = VisibleWindow { first: 0, count: 5 };
        assert_eq!(pages_in_window(&w, 0).count(), 0);
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
        assert_eq!(
            w,
            VisibleWindow {
                first: 20,
                count: 6
            }
        );
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

    // ── R744.1/R1194 row_at (shared row-at-pixel kernel) ─────────────

    #[test]
    fn row_at_finds_the_containing_row() {
        // Tops 0,20,80,100,160,... A pixel inside a row's slot resolves to
        // that row; a pixel exactly on a top belongs to the row it opens.
        let o = RowOffsets::from_heights(&var_heights(1000));
        assert_eq!(o.row_at(0), 0); // top of row 0
        assert_eq!(o.row_at(19), 0); // last px of row 0 (h 20)
        assert_eq!(o.row_at(20), 1); // top of row 1
        assert_eq!(o.row_at(79), 1); // last px of row 1 (h 60)
        assert_eq!(o.row_at(80), 2); // top of row 2
        // Past the end clamps to the last row.
        assert_eq!(o.row_at(u32::MAX), 999);
    }

    #[test]
    fn row_at_empty_table_is_zero() {
        let o = RowOffsets::from_heights(&[]);
        assert_eq!(o.row_at(0), 0);
        assert_eq!(o.row_at(500), 0);
    }

    // ── R1194 MeasuredHeights (estimated fallback + refinement) ──────

    #[test]
    fn measured_heights_start_all_estimated() {
        let m = MeasuredHeights::new(100, 24);
        assert_eq!(m.item_count(), 100);
        assert!(!m.is_empty());
        assert_eq!(m.estimated(), 24);
        assert_eq!(m.measured_count(), 0);
        assert!(!m.is_fully_measured());
        // Every row reports the estimate; total = count · estimate.
        assert_eq!(m.height(0), 24);
        assert_eq!(m.height(99), 24);
        assert_eq!(m.total_height(), 2400);
    }

    #[test]
    fn measured_heights_clamp_a_zero_estimate_to_one() {
        // A zero estimate would render a zero-height list (no rows → never
        // measured → deadlock); the guard makes it ≥ 1.
        let m = MeasuredHeights::new(10, 0);
        assert_eq!(m.estimated(), 1);
        assert_eq!(m.total_height(), 10);
    }

    #[test]
    fn measure_records_and_reports_change() {
        let mut m = MeasuredHeights::new(5, 20);
        // First measurement of a row changes the value.
        assert!(m.measure(2, 55));
        assert_eq!(m.height(2), 55);
        assert_eq!(m.measured_count(), 1);
        // Re-measuring to the same height is a no-op (no reflow needed).
        assert!(!m.measure(2, 55));
        // Re-measuring to a different height changes it (content reflowed).
        assert!(m.measure(2, 40));
        assert_eq!(m.height(2), 40);
        assert_eq!(m.measured_count(), 1);
        // `measured_height` distinguishes measured from estimated.
        assert_eq!(m.measured_height(2), Some(40));
        assert_eq!(m.measured_height(0), None, "unmeasured row reports None");
        // Out-of-range measure is ignored.
        assert!(!m.measure(99, 100));
        assert_eq!(m.height(99), 20, "out-of-range reports the estimate");
        assert_eq!(m.measured_height(99), None, "out-of-range reports None");
    }

    #[test]
    fn total_height_refines_toward_the_exact_sum() {
        let mut m = MeasuredHeights::new(4, 20); // est total 80
        assert_eq!(m.total_height(), 80);
        m.measure(0, 60);
        m.measure(1, 10);
        // Rows 0,1 measured (60+10), rows 2,3 estimated (20+20) → 110.
        assert_eq!(m.total_height(), 110);
        m.measure(2, 30);
        m.measure(3, 30);
        assert!(m.is_fully_measured());
        assert_eq!(m.total_height(), 130, "exact sum once fully measured");
    }

    #[test]
    fn offsets_uses_measured_where_known_estimated_elsewhere() {
        let mut m = MeasuredHeights::new(4, 20);
        m.measure(1, 60); // heights: 20, 60, 20, 20
        let o = m.offsets();
        assert_eq!(o.row_top(0), 0);
        assert_eq!(o.row_top(1), 20);
        assert_eq!(o.row_top(2), 80);
        assert_eq!(o.row_top(3), 100);
        assert_eq!(o.total_height(), 120);
        assert_eq!(o.row_height(1), 60);
    }

    #[test]
    fn set_count_preserves_measured_and_drops_removed() {
        let mut m = MeasuredHeights::new(3, 20);
        m.measure(0, 50);
        m.measure(2, 70);
        // Grow: existing measurements survive, the new row is unmeasured.
        m.set_count(5);
        assert_eq!(m.item_count(), 5);
        assert_eq!(m.height(0), 50);
        assert_eq!(m.height(2), 70);
        assert_eq!(m.height(4), 20, "new row estimated");
        assert_eq!(m.measured_count(), 2);
        // Shrink below a measured row drops it.
        m.set_count(1);
        assert_eq!(m.item_count(), 1);
        assert_eq!(m.height(0), 50);
        assert_eq!(m.measured_count(), 1);
    }

    #[test]
    fn empty_measured_heights_is_never_fully_measured() {
        let m = MeasuredHeights::new(0, 20);
        assert!(m.is_empty());
        assert_eq!(m.item_count(), 0);
        assert!(!m.is_fully_measured());
        assert_eq!(m.total_height(), 0);
    }

    // ── R1194 scroll anchor (the measured list's no-jump correction) ──

    #[test]
    fn scroll_anchor_at_top_is_row_zero() {
        let o = MeasuredHeights::new(100, 20).offsets();
        assert_eq!(scroll_anchor(0, &o), (0, 0));
        // Negative programmatic offset reads as the top.
        assert_eq!(scroll_anchor(-40, &o), (0, 0));
    }

    #[test]
    fn scroll_anchor_reports_row_and_pixels_into_it() {
        // Uniform 20-px rows: offset 510 sits 10 px into row 25 (top 500).
        let o = MeasuredHeights::new(100, 20).offsets();
        assert_eq!(scroll_anchor(510, &o), (25, 10));
        // Exactly on a row top: zero pixels in.
        assert_eq!(scroll_anchor(500, &o), (25, 0));
    }

    #[test]
    fn scroll_anchor_empty_table_is_origin() {
        let o = MeasuredHeights::new(0, 20).offsets();
        assert_eq!(scroll_anchor(300, &o), (0, 0));
    }

    #[test]
    fn anchor_round_trips_when_the_table_is_unchanged() {
        // Capturing then restoring against the same table returns the same
        // offset (a steady-state frame moves nothing).
        let o = MeasuredHeights::new(100, 20).offsets();
        for off in [0, 137, 500, 510, 1999] {
            let a = scroll_anchor(off, &o);
            assert_eq!(anchor_preserving_offset(a, &o), off, "round-trip at {off}");
        }
    }

    #[test]
    fn anchor_preserving_offset_absorbs_growth_above_the_viewport() {
        // The headline no-jump property. 100 rows estimated at 20px; the
        // viewport top sits exactly on row 25 (offset 500).
        let mut m = MeasuredHeights::new(100, 20);
        let before = m.offsets();
        let anchor = scroll_anchor(500, &before);
        assert_eq!(anchor, (25, 0));

        // Rows 0..10 turn out to be 60px (measured, 40px taller each): the
        // 400px of new height all lies *above* the anchor row.
        for i in 0..10 {
            m.measure(i, 60);
        }
        let after = m.offsets();
        // Without correction the offset would still be 500 and row 25 would
        // jump up 400px. The correction moves it to row 25's new top so the
        // same content stays under the viewport top.
        assert_eq!(after.row_top(25), 900);
        assert_eq!(anchor_preserving_offset(anchor, &after), 900);
    }

    #[test]
    fn anchor_preserving_offset_keeps_sub_row_position() {
        // A non-zero pixels-into-row is preserved across a height change
        // above the anchor.
        let mut m = MeasuredHeights::new(50, 20);
        let before = m.offsets();
        let anchor = scroll_anchor(207, &before); // row 10 (top 200) + 7 px
        assert_eq!(anchor, (10, 7));
        m.measure(0, 120); // +100 px above the anchor row
        let after = m.offsets();
        assert_eq!(after.row_top(10), 300); // 120 + 9·20
        assert_eq!(
            anchor_preserving_offset(anchor, &after),
            307,
            "row 10's new top (300) + the same 7 px in",
        );
    }
}
