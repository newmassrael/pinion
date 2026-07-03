//! R1194 §5.27 — reactive state for a **measured** variable-height
//! virtualized list.
//!
//! The measured-list peer of [`ScrollState`].
//! A [`Scene::Scroll`](crate::scene::Scene::Scroll) whose rows have
//! content-driven heights that cannot be known until they are laid out
//! (wrapped log/packet rows, variable document paragraphs, differently
//! sized asset thumbnails) carries one of these alongside its
//! `ScrollState`: the scroll offset lives in `ScrollState`, the
//! progressively-discovered row heights live here.
//!
//! ## The measurement round-trip
//!
//! The windowing math ([`compute_visible_range_variable`](crate::widgets::virtual_list::compute_visible_range_variable))
//! runs in the *view fn*, before layout — but a measured list does not know
//! its row heights before layout. This is the same chicken-and-egg the
//! R774 `AutoSizer` container-height feedback solves, applied per row:
//!
//! 1. The view fn windows against the current [`MeasuredHeights`] table
//!    ([`MeasuredRowState::offsets`]) — the estimate on the first frame,
//!    refined thereafter — and tags each rendered row so the harvester finds it.
//! 2. The runtime layout pass lays out the windowed rows to their natural
//!    content height, then feeds each row's laid-out height back via
//!    [`MeasuredRowState::harvest`].
//! 3. `harvest` records the heights, bumps the generation signal (re-running
//!    the view against the refined table), and — because rows resolving
//!    from `estimated` to a taller/shorter measured height shift the whole
//!    column — restores the scroll offset so the viewport-top row stays put
//!    (the no-jump correction, [`scroll_anchor`] / [`anchor_preserving_offset`]).
//!
//! It converges in the two-frame warmup every measured virtualizer has
//! (`TanStack Virtual` `measureElement`, `react-virtualized` `CellMeasurer`):
//! a measure frame followed by a settled frame whose remeasure finds the
//! same heights and changes nothing (generation stays put, Signal
//! equality-skip stops the loop).

use std::cell::RefCell;
use std::rc::Rc;

use crate::reactive::{Owner, Signal};
use crate::widgets::scroll::{ScrollState, max_scroll_offset};
use crate::widgets::virtual_list::{
    MeasuredHeights, RowOffsets, anchor_preserving_offset, scroll_anchor,
};

/// R1194 §5.27 — tag prefix for a windowed measured-list row slot,
/// `measured-row:<index>`. The single source of truth for the row-slot tag
/// encoding: the view fn (`pinion_widget_paint::view_measured_list`) stamps
/// it and the runtime layout-pass harvest
/// (`pinion_runtime::layout`) parses it back to an index, so an
/// encode/decode divergence would be a measurement bug, not a style choice.
pub const MEASURED_ROW_TAG_PREFIX: &str = "measured-row:";

/// Build the tag for the windowed measured-list row at `index`, scoped under
/// the owning scroll's `scroll_tag` so two measured lists in one window never
/// collide in the flat `scene/snapshot` tag namespace (§2 #7): a tagged list
/// emits `<scroll_tag>/measured-row:<index>`, an untagged one (rare — test
/// fixtures) the bare `measured-row:<index>`. Paired with [`measured_row_index`],
/// which parses the index from either form. (R1199 — R1196 emitted only the bare
/// index-scoped tag, so a second list's `measured-row:0` shadowed the first's.)
#[must_use]
pub fn measured_row_tag(scroll_tag: Option<&str>, index: usize) -> String {
    match scroll_tag {
        Some(tag) => format!("{tag}/{MEASURED_ROW_TAG_PREFIX}{index}"),
        None => format!("{MEASURED_ROW_TAG_PREFIX}{index}"),
    }
}

/// Parse a measured-list row-slot tag back to its row index, or `None` if `tag`
/// carries no `measured-row:<index>` segment. Accepts both the scroll-scoped
/// (`<scroll_tag>/measured-row:<index>`) and bare (`measured-row:<index>`)
/// forms. Paired with [`measured_row_tag`]; the layout-pass harvest only needs
/// the index (it is already subtree-scoped to one scroll's content), so parsing
/// past any scope prefix is sufficient.
#[must_use]
pub fn measured_row_index(tag: &str) -> Option<usize> {
    tag.rsplit_once(MEASURED_ROW_TAG_PREFIX)?.1.parse().ok()
}

/// R1194 §5.27 — reactive store of progressively-measured row heights for
/// one measured variable-height list.
///
/// Holds a mutable [`MeasuredHeights`] table behind a generation
/// [`Signal`]: reads through [`Self::offsets`] (and the status accessors)
/// subscribe the calling view fn to the generation, and a
/// height-changing [`Self::harvest`] bumps the generation, so the view
/// re-runs with the refined table. A remeasure that finds the same height
/// bumps nothing (the [`MeasuredHeights::measure`] change bit gates the
/// bump), so a settled list schedules no paint.
///
/// Lifecycle mirrors `ScrollState`: created lazily via [`use_measured_rows`]
/// (delegating to [`Owner::cache`](crate::reactive::Owner::cache)), so the
/// same key resolves to the same `Rc<MeasuredRowState>` across view re-runs
/// and the accumulated measurements persist across paints.
#[derive(Debug)]
pub struct MeasuredRowState {
    /// The measured-or-estimated heights, mutated by [`Self::harvest`] and
    /// [`Self::set_count`].
    heights: RefCell<MeasuredHeights>,
    /// Bumped whenever a harvest changes a height (or the count changes).
    /// The reactive read channel: [`Self::offsets`] subscribes to it, so a
    /// refined measurement re-runs the view. Monotonic; Signal
    /// equality-skip is moot because it only ever increases when something
    /// actually changed.
    generation: Signal<u64>,
    /// (R1199) The prefix-sum table memoized by the `generation`
    /// it was built at. [`Self::offsets`] rebuilds only when the generation has
    /// advanced (a harvested remeasure / a `set_count`); a plain scroll frame —
    /// which moves the *offset* Signal, not the generation — returns the cached
    /// `Rc` in O(1), so windowing a settled list never re-walks the O(n) table.
    /// `Rc` so the return is a cheap handle clone, not an O(n) copy.
    cached: RefCell<Option<(u64, Rc<RowOffsets>)>>,
    /// Canonical input-router / introspection tag, set by
    /// [`use_measured_rows`] from the `Owner::cache` key (the paired
    /// `ScrollState` derives the matching `ScrollNode` tag from the same
    /// key). `None` for directly constructed states (tests / fixtures).
    tag: Option<&'static str>,
}

impl MeasuredRowState {
    /// A store of `item_count` rows, each initially unmeasured (using
    /// `estimated`, clamped to `≥ 1` by [`MeasuredHeights::new`]).
    #[must_use]
    pub fn new(item_count: usize, estimated: u32) -> Self {
        Self {
            heights: RefCell::new(MeasuredHeights::new(item_count, estimated)),
            generation: Signal::new(0),
            cached: RefCell::new(None),
            tag: None,
        }
    }

    /// [`Self::new`] tagged with `key` — the [`use_measured_rows`]
    /// `Owner::cache` factory shape.
    #[must_use]
    pub fn with_tag(key: &'static str, item_count: usize, estimated: u32) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(item_count, estimated)
        }
    }

    /// Canonical tag for this measured list, or `None`.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Current prefix-sum [`RowOffsets`] for the measured-or-estimated heights,
    /// as a shared handle. **Subscribes** the calling view fn to the measurement
    /// generation, so a harvested remeasure re-runs the view against the refined
    /// table.
    ///
    /// (R1199) Memoized by `generation`: the O(n) table is
    /// rebuilt only when a measurement (or a `set_count`) advanced the
    /// generation; a plain scroll frame returns the cached `Rc` in O(1). This
    /// restores the "built once, reused across frames" contract `RowOffsets`
    /// documents (a scroll moves the offset Signal, not the generation).
    #[must_use]
    pub fn offsets(&self) -> Rc<RowOffsets> {
        // Subscribe the caller to the measurement generation AND read its value.
        let generation = self.generation.get();
        {
            let cached = self.cached.borrow();
            if let Some((cached_generation, offsets)) = cached.as_ref() {
                if *cached_generation == generation {
                    return Rc::clone(offsets);
                }
            }
        }
        let offsets = Rc::new(self.heights.borrow().offsets());
        *self.cached.borrow_mut() = Some((generation, Rc::clone(&offsets)));
        offsets
    }

    /// Total dataset size (does not subscribe — the count is caller-owned,
    /// synced via [`Self::set_count`]).
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.heights.borrow().item_count()
    }

    /// How many rows have been measured so far. **Subscribes** — a "measured
    /// N / M" status line refreshes as rows resolve.
    #[must_use]
    pub fn measured_count(&self) -> usize {
        let _ = self.generation.get();
        self.heights.borrow().measured_count()
    }

    /// The measured height of row `index`, or `None` if it still uses the
    /// estimate. **Subscribes** — flips from `None` to `Some` when the row
    /// is first harvested.
    #[must_use]
    pub fn measured_height(&self, index: usize) -> Option<u32> {
        let _ = self.generation.get();
        self.heights.borrow().measured_height(index)
    }

    /// Current total content height (measured where known, estimated
    /// elsewhere). **Subscribes** — refines toward the exact sum as rows are
    /// measured.
    #[must_use]
    pub fn total_height(&self) -> u32 {
        let _ = self.generation.get();
        self.heights.borrow().total_height()
    }

    /// Whether every row has been measured (the table is now exact).
    /// **Subscribes**.
    #[must_use]
    pub fn is_fully_measured(&self) -> bool {
        let _ = self.generation.get();
        self.heights.borrow().is_fully_measured()
    }

    /// Resize the dataset to `n` rows, **preserving** measured heights for
    /// rows that stay in range (the growable/streaming path). Bumps the
    /// generation only when the count changed, so the sizer + window
    /// re-derive.
    ///
    /// Call this from a [`reconcile_frame`](crate::widget_core::WidgetCore::reconcile_frame)
    /// pre-view hook (the sanctioned place to mutate reactive view state),
    /// **not** from the view fn — the generation bump on a growth would
    /// schedule a re-run mid-render.
    pub fn set_count(&self, n: usize) {
        let changed = {
            let mut h = self.heights.borrow_mut();
            if h.item_count() == n {
                false
            } else {
                h.set_count(n);
                true
            }
        };
        if changed {
            self.generation.set_with(|g| g + 1);
        }
    }

    /// Harvest a laid-out frame's row heights and keep the scroll anchored.
    ///
    /// `rows` yields `(row_index, laid_out_height)` for each row the layout
    /// pass rendered this frame. This:
    ///
    /// 1. Captures the viewport-top **anchor** against the table the
    ///    just-laid frame was built from (before applying the new heights).
    /// 2. Applies every measurement (a no-op for a row whose height is
    ///    unchanged).
    /// 3. If any height changed: bumps the generation (re-running the view),
    ///    grows the scroll bound to the refined total, and restores the
    ///    offset so the anchor row stays at the same screen position — the
    ///    no-jump correction.
    ///
    /// Returns `true` iff a measurement changed a height; the caller folds
    /// this into the frame's dirty bit so the view re-runs with the refined
    /// table (the same same-frame re-pass the R774/R57.X scroll-bound
    /// feedback uses). The bound is grown *before* the offset is restored so
    /// the anchor-preserving target is not clamped against the stale
    /// pre-harvest max — the `follow_tail` grow-then-pin idiom.
    pub fn harvest(
        &self,
        scroll: &ScrollState,
        viewport_h: u32,
        rows: impl IntoIterator<Item = (usize, u32)>,
    ) -> bool {
        // Anchor against the table the just-laid frame was built from.
        let pre_offsets = self.heights.borrow().offsets();
        let anchor = scroll_anchor(scroll.offset_y(), &pre_offsets);
        drop(pre_offsets);

        let mut changed = false;
        {
            let mut h = self.heights.borrow_mut();
            for (index, height) in rows {
                changed |= h.measure(index, height);
            }
        }
        if !changed {
            return false;
        }

        self.generation.set_with(|g| g + 1);
        let offsets = self.heights.borrow().offsets();
        // Grow the bound to the refined total before restoring the offset,
        // so the anchor-preserving target clamps against the new extent, not
        // the stale one (grow-then-pin, per `follow_tail`).
        scroll.set_max(
            scroll.max().0,
            max_scroll_offset(offsets.total_height(), viewport_h),
        );
        let corrected = anchor_preserving_offset(anchor, &offsets);
        scroll.scroll_to(scroll.offset().0, corrected);
        true
    }
}

/// R1194 §5.27 — obtain the [`MeasuredRowState`] for `key`, created lazily
/// and cached on the active [`Owner`] so it persists across view re-runs
/// (the [`use_scroll_state`](crate::widgets::scroll::use_scroll_state)
/// sibling). `item_count` / `estimated` seed the **first** construction; a
/// growable dataset syncs its count each frame via
/// [`MeasuredRowState::set_count`] from a `reconcile_frame` hook.
///
/// # Panics
///
/// Panics if called outside an active `Owner` scope (a view fn / update /
/// key hook), or if the key was previously bound to a different concrete
/// type in the same owner — see [`Owner::cache`](crate::reactive::Owner::cache).
#[must_use]
pub fn use_measured_rows(
    key: &'static str,
    item_count: usize,
    estimated: u32,
) -> Rc<MeasuredRowState> {
    Owner::current()
        .expect("use_measured_rows requires an active Owner scope")
        .cache(key, || {
            MeasuredRowState::with_tag(key, item_count, estimated)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 40-row list, estimated at 20 px/row, in a 200-px viewport.
    const N: usize = 40;
    const EST: u32 = 20;
    const VH: u32 = 200;

    fn state() -> MeasuredRowState {
        MeasuredRowState::new(N, EST)
    }

    #[test]
    fn starts_all_estimated() {
        let s = state();
        assert_eq!(s.item_count(), N);
        assert_eq!(s.measured_count(), 0);
        assert!(!s.is_fully_measured());
        assert_eq!(s.total_height(), u32::try_from(N).unwrap() * EST);
        assert_eq!(s.offsets().row_top(10), 200);
    }

    #[test]
    fn offsets_are_memoized_by_generation() {
        let s = state();
        let a = s.offsets();
        let b = s.offsets();
        assert!(
            Rc::ptr_eq(&a, &b),
            "unchanged generation returns the cached Rc — no O(n) rebuild per scroll frame",
        );
        // A harvested remeasure advances the generation → rebuild → new Rc.
        let scroll = ScrollState::new();
        s.harvest(&scroll, VH, [(0, 99)]);
        let c = s.offsets();
        assert!(!Rc::ptr_eq(&b, &c), "a remeasure rebuilds the table");
        assert_eq!(
            c.row_height(0),
            99,
            "the rebuilt table reflects the measurement"
        );
        assert!(Rc::ptr_eq(&c, &s.offsets()), "settled again → cached");
    }

    #[test]
    fn harvest_records_heights_and_reports_change() {
        let s = state();
        let scroll = ScrollState::new();
        // First harvest of a fresh window measures rows → changed.
        let changed = s.harvest(&scroll, VH, [(0, 50), (1, 50), (2, 50)]);
        assert!(changed);
        assert_eq!(s.measured_count(), 3);
        assert_eq!(s.offsets().row_top(1), 50);
        // Re-harvesting the same heights changes nothing (settled frame).
        let again = s.harvest(&scroll, VH, [(0, 50), (1, 50), (2, 50)]);
        assert!(!again, "a remeasure to the same heights is a no-op");
    }

    #[test]
    fn harvest_grows_the_scroll_bound_to_the_refined_total() {
        let s = state();
        let scroll = ScrollState::new();
        // Every row turns out to be 80 px (4× the estimate): total 3200,
        // bound = 3200 − 200 = 3000.
        let rows: Vec<(usize, u32)> = (0..N).map(|i| (i, 80)).collect();
        s.harvest(&scroll, VH, rows);
        assert!(s.is_fully_measured());
        assert_eq!(s.total_height(), u32::try_from(N).unwrap() * 80);
        assert_eq!(
            scroll.max().1,
            3000,
            "bound grew to refined total − viewport"
        );
    }

    #[test]
    fn harvest_anchor_correction_prevents_the_scroll_jump() {
        // The headline. Scroll to row 25 at the estimate (offset 500), then
        // discover rows 0..10 are 60 px each (+40 px each, 400 px total, all
        // above the viewport). The offset must move to keep row 25 pinned to
        // the viewport top rather than letting 400 px of new height above
        // shove it upward.
        let s = state();
        let scroll = ScrollState::new();
        // Bound wide enough that 500 is a legal offset at the estimate.
        scroll.set_max(0, i32::try_from(s.total_height()).unwrap());
        scroll.scroll_to(0, 500);
        assert_eq!(scroll.offset_y(), 500);
        // Anchor is row 25 (top 500 at the estimate).
        s.harvest(&scroll, VH, (0..10).map(|i| (i, 60)));
        // Row 25's new top = 10·60 + 15·20 = 900; the offset follows so the
        // same row stays under the viewport top.
        assert_eq!(s.offsets().row_top(25), 900);
        assert_eq!(
            scroll.offset_y(),
            900,
            "anchor row 25 stays pinned, no jump"
        );
    }

    #[test]
    fn harvest_below_the_anchor_does_not_move_the_offset() {
        // Measuring rows at/below the viewport top refines the total but must
        // not move the offset (nothing above the anchor changed).
        let s = state();
        let scroll = ScrollState::new();
        scroll.set_max(0, i32::try_from(s.total_height()).unwrap());
        scroll.scroll_to(0, 200); // anchor row 10
        // Measure rows 10..15 (the anchor row and below) taller.
        s.harvest(&scroll, VH, (10..15).map(|i| (i, 90)));
        assert_eq!(
            scroll.offset_y(),
            200,
            "row 10's top is unchanged (rows above it untouched), so no correction",
        );
    }

    #[test]
    fn set_count_grows_and_preserves_measurements() {
        let s = state();
        let scroll = ScrollState::new();
        s.harvest(&scroll, VH, [(5, 44)]);
        assert_eq!(s.offsets().row_height(5), 44);
        s.set_count(60);
        assert_eq!(s.item_count(), 60);
        assert_eq!(
            s.offsets().row_height(5),
            44,
            "measurement survived the grow"
        );
        assert_eq!(s.measured_count(), 1);
        // No-op set_count (same count) is idempotent.
        s.set_count(60);
        assert_eq!(s.item_count(), 60);
    }

    #[test]
    fn row_tag_round_trips_scoped_and_bare_and_rejects_foreign() {
        for i in [0usize, 1, 37, 999_999] {
            // Bare (untagged scroll) and scroll-scoped both parse to the index.
            assert_eq!(measured_row_index(&measured_row_tag(None, i)), Some(i));
            assert_eq!(
                measured_row_index(&measured_row_tag(Some("list_a"), i)),
                Some(i)
            );
        }
        // Two lists produce DISTINCT tags for the same index (no §2 #7 collision).
        assert_ne!(
            measured_row_tag(Some("a"), 0),
            measured_row_tag(Some("b"), 0)
        );
        // Foreign / malformed tags reject.
        assert_eq!(measured_row_index("row:5"), None);
        assert_eq!(measured_row_index("measured-row:"), None);
        assert_eq!(measured_row_index("list/measured-row:x"), None);
    }

    #[test]
    fn use_measured_rows_caches_across_runs() {
        // The same key resolves to the same Rc across owner runs, so the
        // accumulated measurements persist across paints.
        let owner = Owner::new();
        owner.run(|| {
            let s = use_measured_rows("list", N, EST);
            let scroll = ScrollState::new();
            s.harvest(&scroll, VH, [(3, 77)]);
            assert_eq!(s.measured_count(), 1);
        });
        owner.run(|| {
            let s = use_measured_rows("list", N, EST);
            assert_eq!(s.measured_count(), 1, "measurement persisted across runs");
            assert_eq!(s.offsets().row_height(3), 77);
            assert_eq!(s.tag(), Some("list"));
        });
    }
}
