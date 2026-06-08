//! R778 §5.27 §5.40 — **multi-column sort view-order proxy for a Model/View
//! data grid at scale**.
//!
//! The grid analog of [`view_order`](crate::widgets::view_order): R775 / R777
//! land a virtualized data grid (window an N-row × C-column dataset) with
//! selection held by data index. This module adds the sort axis a data grid
//! needs — a clicked column header sorts the whole grid — **without
//! materializing the rendered rows**. The view windows over *view positions*
//! `0..view_len`; each position resolves to a source row through the
//! [`order`](GridSortState::order) permutation; the row builder paints the
//! source row; the R777 [`VirtualSelectExternal`](crate::widgets::virtual_select)
//! still holds a **source** index, so re-sorting moves a selected row's
//! visual position while keeping it selected — selection ⊥ ordering, both
//! data-indexed (the same decisive Model/View separation the 1-D list draws,
//! mirroring Qt's `QSortFilterProxyModel` ⊥ `QItemSelectionModel`).
//!
//! ## Why a peer of [`ViewOrderState`](crate::widgets::view_order::ViewOrderState), not a reuse
//!
//! The 1-D list proxy sorts a **single key** column with a generic [`Ord`]
//! comparison ([`compute_order`](crate::widgets::view_order::compute_order)).
//! A data grid sorts **any** of its columns, and a column of numbers sorted
//! lexicographically (`"12" < "9"`) is visibly wrong — so the grid proxy is
//! multi-column and **numeric-aware**, sorting through the
//! [`cell_cmp`](crate::widgets::table::cell_cmp) /
//! [`grid_order_by`](crate::widgets::table::grid_order_by) SSOT the eager
//! [`Table`](crate::widgets::table::Table) already uses. The two proxies share
//! that comparator + the [`cycle_col_sort`](crate::widgets::table::cycle_col_sort)
//! transition, not their whole shape: the sort *representation* differs
//! (`Option<(col, bool)>` vs `Option<bool>`), so a forced merge would be a
//! wrong abstraction.
//!
//! ## One reactive source of truth ([`GridSortState`] + [`use_grid_sort`])
//!
//! The active `(col, dir)` and the derived `order` permutation are held
//! **once** in a reactive [`GridSortState`] — the `ScrollState` /
//! [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) pattern this
//! crate shares an interactive axis with. The [`GridSortExternal`] is a thin
//! adapter that *mutates* the shared state on `invoke` / a clicked header; the
//! view and the a11y tree *read* the same state through `use_grid_sort(key)`
//! (the same `Rc`), so the `order` is computed **once** (memoized on the sort
//! key) and the rows the view paints are the rows the proxy's
//! `query("source_at.<n>")` reports — by construction.
//!
//! ## Scope (honest boundaries)
//!
//! - **Sort only.** Filtering at scale is the 1-D proxy's `filter` axis; the
//!   grid gains it additively when a consumer needs it (a follow-up, like the
//!   list's R747 sort → later filter arc).
//! - **No undo.** The list proxy records a reversible `SortFilterEdit`
//!   (R748); the grid's reversible sort is the immediate follow-up, kept out
//!   of this slice so the sort core lands clean first (mirroring R730 table
//!   sort → R748 undoable list sort).
//! - **Materialized cells.** The proxy holds the source cell grid (it must,
//!   to sort by any column) — the dataset, not rendered rows; identical to
//!   [`ViewOrderState`](crate::widgets::view_order::ViewOrderState)
//!   materializing its key column.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use crate::external::{
    query_proxy_external_impl, ExternalIntrospect, InterveneError, IntrospectSchema,
    IntrospectValue, InvokeError,
};
use crate::reactive::{Owner, Signal};
use crate::undo::{UndoCommand, UndoStack};
use crate::widgets::order_memo::{source_at_value, OrderMemo};
use crate::widgets::table::{cell_cmp, cycle_col_sort, grid_order_by};
use crate::widgets::view_order::{sort_dir_from_str, sort_dir_str};

/// R778 §5.40 — render `(col, dir)` as a single wire string: `"none"` when
/// unsorted, else `"<col>:<dir>"` (e.g. `"1:ascending"`). The compound form
/// lets an admin / AI client read or restore the **whole** grid sort state in
/// one round-trip (the column matters for a grid, unlike the 1-D list's bare
/// direction string). The direction half reuses
/// [`sort_dir_str`](crate::widgets::view_order::sort_dir_str).
#[must_use]
pub fn grid_sort_str(sort: Option<(usize, bool)>) -> String {
    match sort {
        None => "none".to_string(),
        Some((col, dir)) => format!("{col}:{}", sort_dir_str(Some(dir))),
    }
}

/// R778 §5.40 — parse a `(col, dir)` from its [`grid_sort_str`] form. Any
/// string without a `"<col>:<dir>"` shape whose direction parses as
/// ascending / descending yields `None` (unsorted) — the safe default for a
/// malformed wire payload (mirrors
/// [`sort_dir_from_str`](crate::widgets::view_order::sort_dir_from_str)).
#[must_use]
pub fn grid_sort_from_str(s: &str) -> Option<(usize, bool)> {
    let (col, dir) = s.split_once(':')?;
    let col = col.parse::<usize>().ok()?;
    let dir = sort_dir_from_str(dir)?;
    Some((col, dir))
}

/// R783 §5.40 — a single-facet **column filter** for a data grid: keep only
/// rows whose column [`col`](Self::col) cell text equals [`value`](Self::value)
/// exactly (the canonical column filter — Qt `QSortFilterProxyModel`
/// `setFilterFixedString` on a column, `TanStack` `columnFilters`). It is the
/// grid peer of the 1-D proxy's `Option<usize>` category facet
/// ([`ViewOrderState::filter`](crate::widgets::view_order::ViewOrderState::filter));
/// the grid expresses the facet as a **column + value** because it already
/// holds the cell text, so no parallel category column is needed — and the
/// string value is AI-readable (`set_filter "2=Active"`). Multi-facet /
/// substring / predicate filters are a later additive axis (one fixed-string
/// facet here, mirroring the list's single-category first slice).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GridFilter {
    /// Zero-based column the facet matches on.
    pub col: usize,
    /// The exact cell text a row must carry in `col` to pass the filter.
    pub value: String,
}

/// R783 §5.40 — render an [`Option<GridFilter>`] as a single wire string:
/// `"none"` when unfiltered, else `"<col>=<value>"` (e.g. `"2=Active"`). The
/// compound form lets an admin / AI client read or restore the whole filter in
/// one round-trip; the peer of [`grid_filter_from_str`]. `=` splits at the
/// first occurrence, so a value may itself contain `=`.
#[must_use]
pub fn grid_filter_str(filter: Option<&GridFilter>) -> String {
    match filter {
        None => "none".to_string(),
        Some(GridFilter { col, value }) => format!("{col}={value}"),
    }
}

/// R783 §5.40 — parse an [`Option<GridFilter>`] from its [`grid_filter_str`]
/// form. A string without a `"<col>=<value>"` shape (no `=`, or a
/// non-numeric column) yields `None` (unfiltered) — the safe default for a
/// malformed or `"none"` wire payload. `=` splits at the first occurrence so
/// the value half is taken verbatim.
#[must_use]
pub fn grid_filter_from_str(s: &str) -> Option<GridFilter> {
    let (col, value) = s.split_once('=')?;
    let col = col.parse::<usize>().ok()?;
    Some(GridFilter { col, value: value.to_string() })
}

/// R783 §5.40 — the [`OrderMemo`] cache key for a [`GridSortState`]: the
/// active `(sort, filter)` config the visual→source permutation is memoized
/// against, so the order recomputes only when the sort key *or* the filter
/// facet changes.
type GridOrderKey = (Option<(usize, bool)>, Option<GridFilter>);

/// R778 §5.27 §5.40 — the reactive **single source of truth** for a data
/// grid's column sort view order.
///
/// Holds the source cell grid (materialized once), the active `(col, dir)` as
/// a reactive [`Signal`], and the derived `order` permutation (memoized). The
/// `ScrollState` of the grid-sort axis: created once via [`use_grid_sort`], it
/// is shared by the [`GridSortExternal`] (which mutates it) and the view /
/// a11y tree (which read it) through the same `Rc`. Reading
/// [`sort`](Self::sort) / [`order`](Self::order) inside a view-fn
/// auto-subscribes, so a sort change repaints exactly like a scroll-offset
/// change.
pub struct GridSortState {
    tag: Option<&'static str>,
    /// Row-major source cells — `cells[row][col]`. The dataset the sort
    /// reorders (the proxy holds data, never rendered rows).
    cells: Vec<Vec<String>>,
    col_count: usize,
    sort: Signal<Option<(usize, bool)>>,
    /// R783 §5.40 — active single-facet column filter (`None` = unfiltered).
    /// Subscribed views repaint on a filter change exactly as on a sort change.
    filter: Signal<Option<GridFilter>>,
    /// Memoized visual→source permutation, recomputed only when the
    /// [`GridOrderKey`] (`(sort, filter)`) changes — the shared [`OrderMemo`]
    /// (R780 lift).
    order: RefCell<OrderMemo<GridOrderKey>>,
}

impl GridSortState {
    /// Construct over a `col_count`-wide grid whose source row `r` has cells
    /// `cells[r]`. Starts unsorted. A row shorter than `col_count` (ragged
    /// data) reads `""` for the missing cells — the same blank-cell tolerance
    /// the paint layer applies.
    #[must_use]
    pub fn new(col_count: usize, cells: Vec<Vec<String>>) -> Self {
        Self {
            tag: None,
            cells,
            col_count,
            sort: Signal::new(None),
            filter: Signal::new(None),
            order: RefCell::new(OrderMemo::new()),
        }
    }

    /// As [`new`](Self::new) but records the `use_grid_sort` cache key, for
    /// symmetry with [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, col_count: usize, cells: Vec<Vec<String>>) -> Self {
        Self { tag: Some(key), ..Self::new(col_count, cells) }
    }

    /// The `use_grid_sort` cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Source row count.
    #[must_use]
    pub fn count(&self) -> usize {
        self.cells.len()
    }

    /// Column count (the sortable column bound).
    #[must_use]
    pub fn col_count(&self) -> usize {
        self.col_count
    }

    /// The cell text at `(row, col)`, or `""` out of range.
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> &str {
        self.cells.get(row).and_then(|r| r.get(col)).map_or("", String::as_str)
    }

    /// Active sort key `(col, ascending)`, or `None` when unsorted.
    /// Subscribes when read inside a view-fn.
    #[must_use]
    pub fn sort(&self) -> Option<(usize, bool)> {
        self.sort.get()
    }

    /// R783 §5.40 — active column filter, or `None` when unfiltered.
    /// Subscribes when read inside a view-fn.
    #[must_use]
    pub fn filter(&self) -> Option<GridFilter> {
        self.filter.get()
    }

    /// Whether source row `row` passes the active filter (always `true` when
    /// unfiltered): its `col` cell text equals the facet value exactly.
    #[must_use]
    fn passes_filter(&self, row: usize, filter: Option<&GridFilter>) -> bool {
        filter.is_none_or(|f| self.cell(row, f.col) == f.value)
    }

    /// The visual → source permutation for the current `(sort, filter)`,
    /// recomputed only when either changes (memoized). Subscribes to the
    /// `sort` + `filter` `Signal`s, so a view that calls this repaints on a
    /// sort *or* filter change. Cheap `Rc` clone on a cache hit. Ordered
    /// through the [`grid_order_by`] / [`cell_cmp`] SSOT (filter-then-sort: the
    /// filter shrinks the row set, the survivors sort by the numeric-aware
    /// comparison the eager [`Table::order`](crate::widgets::table::Table::order) uses).
    #[must_use]
    pub fn order(&self) -> Rc<Vec<usize>> {
        let sort = self.sort.get();
        let filter = self.filter.get();
        let filter_ref = filter.clone();
        self.order.borrow_mut().get((sort, filter), || {
            grid_order_by(
                self.cells.len(),
                sort,
                |col, a, b| cell_cmp(self.cell(a, col), self.cell(b, col)),
                |row| self.passes_filter(row, filter_ref.as_ref()),
            )
        })
    }

    /// Number of rows in the current view (rows passing the filter); equals
    /// [`count`](Self::count) when unfiltered, shrinks under an active filter.
    #[must_use]
    pub fn view_len(&self) -> usize {
        self.order().len()
    }

    /// Source data index painted at visual position `view_pos`, or `None` when
    /// out of range.
    #[must_use]
    pub fn source_at(&self, view_pos: usize) -> Option<usize> {
        self.order().get(view_pos).copied()
    }

    /// Set the sort directly (admin / restore). A `Signal` write repaints
    /// every view that read [`sort`](Self::sort) / [`order`](Self::order). An
    /// out-of-range column clamps to `None` (unsorted) so a malformed restore
    /// never points the glyph at a phantom column.
    pub fn set_sort(&self, sort: Option<(usize, bool)>) {
        let sort = sort.filter(|&(c, _)| c < self.col_count);
        self.sort.set(sort);
    }

    /// Cycle the sort on `col` the way a clicked column header does
    /// (unsorted → asc → desc → unsorted; a different column jumps to it
    /// ascending) via the [`cycle_col_sort`] SSOT. Out-of-range `col` is a
    /// silent no-op.
    pub fn cycle_sort(&self, col: usize) {
        self.sort.set(cycle_col_sort(self.sort.get(), col, self.col_count));
    }

    /// R783 §5.40 — set the column filter directly (`None` clears). A
    /// `Signal` write repaints every view that read [`filter`](Self::filter)
    /// / [`order`](Self::order). An out-of-range column clamps to `None`
    /// (unfiltered) so a malformed restore never filters on a phantom column.
    /// Returns the resulting [`view_len`](Self::view_len) (rows passing the
    /// filter) so the AI-first `set_filter` path reports the new view size in
    /// one round-trip (mirrors
    /// [`ViewOrderState::set_filter`](crate::widgets::view_order::ViewOrderState::set_filter)).
    pub fn set_filter(&self, filter: Option<GridFilter>) -> usize {
        let filter = filter.filter(|f| f.col < self.col_count);
        self.filter.set(filter);
        self.view_len()
    }
}

/// R778 §5.27 §5.40 — resolve the shared [`GridSortState`] for `key`,
/// building it once via `data` (the column count + source cells). Mirrors
/// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) /
/// [`use_view_order`](crate::widgets::view_order::use_view_order): the
/// `External` and the view both call this with the same `key` and receive the
/// same `Rc`, so the order is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_grid_sort(
    key: &'static str,
    data: impl FnOnce() -> (usize, Vec<Vec<String>>),
) -> Rc<GridSortState> {
    Owner::current()
        .expect("use_grid_sort requires an active Owner scope")
        .cache(key, || {
            let (col_count, cells) = data();
            GridSortState::with_tag(key, col_count, cells)
        })
}

/// R779 §5.40 §5.52 — a reversible column-sort change of a shared
/// [`GridSortState`]: the **third** [`UndoCommand`] consumer.
///
/// The bespoke peer of [`SortFilterEdit`](crate::widgets::view_order::SortFilterEdit):
/// both are `{ Rc<holder> + before/after value + label }` whose `redo`/`undo`
/// restore the captured value through the holder's setter. They are kept as
/// **separate typed commands** (not folded into one generic closure-edit)
/// deliberately — the local convention, set by [`SignalEdit`](crate::undo::SignalEdit)
/// (single `Signal`) vs `SortFilterEdit` (compound `ViewOrderState`), is one
/// typed `UndoCommand` per holder (the `QUndoCommand`-per-command model): the
/// reversal body is two trivial lines, so a generic closure-edit would erase
/// the typed holder and add `Rc<dyn Fn>` indirection for no real saving. This
/// command restores the single `Option<(col, bool)>` sort key via
/// [`GridSortState::set_sort`].
pub struct GridSortEdit {
    state: Rc<GridSortState>,
    before: Option<(usize, bool)>,
    after: Option<(usize, bool)>,
    label: Cow<'static, str>,
}

impl GridSortEdit {
    /// Capture the current sort as the `before` snapshot; the edit moves the
    /// state to `after` when recorded onto an [`UndoStack`]
    /// ([`UndoStack::record`] applies it via [`redo`](UndoCommand::redo), so
    /// the stack is the single mutation path).
    #[must_use]
    pub fn capture(
        state: &Rc<GridSortState>,
        after: Option<(usize, bool)>,
        label: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self { state: Rc::clone(state), before: state.sort(), after, label: label.into() }
    }
}

impl UndoCommand for GridSortEdit {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.state.set_sort(self.after);
    }

    fn undo(&self) {
        self.state.set_sort(self.before);
    }
}

/// R778 §5.27 §5.40 — the **RPC / pointer adapter** over a shared
/// [`GridSortState`] (the grid peer of
/// [`ViewSortFilterExternal`](crate::widgets::view_order::ViewSortFilterExternal)).
///
/// All state lives in the shared [`GridSortState`] — the external holds only
/// the `Rc`, so the view paints the same `order` this external reports. A
/// clicked column header routes its pointer arc here as `"h<col>:<EventName>"`
/// (the [`GridSendKey::Header`](crate::composite_tag::GridSendKey) wire the
/// R777 selection coordinator deliberately ignores), and the activation edge
/// cycles that column's sort. R779: attach an [`UndoStack`] with
/// [`with_undo`](Self::with_undo) to make every sort change reversible.
#[derive(Clone)]
pub struct GridSortExternal {
    state: Rc<GridSortState>,
    /// When attached, every sort mutation is recorded as a reversible
    /// [`GridSortEdit`] instead of mutating the state directly (R779 §5.52).
    undo: Option<Rc<UndoStack>>,
}

impl core::fmt::Debug for GridSortExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GridSortExternal")
            .field("sort", &grid_sort_str(self.state.sort()))
            .field("filter", &grid_filter_str(self.state.filter().as_ref()))
            .field("count", &self.state.count())
            .finish_non_exhaustive()
    }
}

impl GridSortExternal {
    /// Wrap the shared [`GridSortState`] (from [`use_grid_sort`]). Sort changes
    /// mutate the state directly; attach an [`UndoStack`] with
    /// [`with_undo`](Self::with_undo) to make them reversible.
    #[must_use]
    pub fn new(state: Rc<GridSortState>) -> Self {
        Self { state, undo: None }
    }

    /// Route every sort mutation through `stack` as a reversible
    /// [`GridSortEdit`] (R779 §5.52), so `invoke "undo"` / `"redo"` on the
    /// stack step the column sort back and forth.
    #[must_use]
    pub fn with_undo(mut self, stack: Rc<UndoStack>) -> Self {
        self.undo = Some(stack);
        self
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_grid_sort`]).
    #[must_use]
    pub fn state(&self) -> &Rc<GridSortState> {
        &self.state
    }

    /// Apply a sort key, recording it as a reversible [`GridSortEdit`] when an
    /// undo stack is attached, else mutating the shared state directly. The
    /// single mutation path for every sort change (header click, `invoke`,
    /// `intervene`) so the undo timeline captures them all.
    fn apply_sort(&self, after: Option<(usize, bool)>) {
        if let Some(stack) = &self.undo {
            stack.record(GridSortEdit::capture(&self.state, after, format!("Sort {}", grid_sort_str(after))));
        } else {
            self.state.set_sort(after);
        }
    }

    /// Cycle the sort on `col` (recorded when an undo stack is attached) via
    /// the [`cycle_col_sort`] SSOT.
    fn apply_cycle_sort(&self, col: usize) {
        let after = cycle_col_sort(self.state.sort(), col, self.state.col_count());
        self.apply_sort(after);
    }

    /// Drive the composite pointer channel: a clicked column header routes the
    /// pointer arc here. Only a [`GridSendKey::Header`](crate::composite_tag::GridSendKey)
    /// key on the activation edge (`PointerUp` / `KeyboardActivate`) cycles
    /// that column's sort; a cell key or any non-activation arc is a harmless
    /// no-op (sort ⊥ selection — cells are the selection coordinator's axis).
    fn handle_send(&self, payload: &str) {
        let Some((key, event_name)) = payload.split_once(':') else {
            return;
        };
        if event_name.is_empty() {
            return;
        }
        if let Some(crate::composite_tag::GridSendKey::Header { col }) =
            crate::composite_tag::GridSendKey::parse(key)
        {
            if crate::input::is_activation_event(event_name) {
                self.apply_cycle_sort(col);
            }
        }
    }

    /// The active sort column as an `IntrospectValue` (`Int`, or `-1` when
    /// unsorted) — the uniform "no column" sentinel the table external uses
    /// for `selected_row`.
    fn sort_col_value(&self) -> IntrospectValue {
        let col = self.state.sort().and_then(|(c, _)| i64::try_from(c).ok());
        IntrospectValue::Int(col.unwrap_or(-1))
    }

    /// R783 §5.40 — apply a column filter directly (the single mutation path
    /// for the header-less filter axis). Unlike the sort, the filter is *not*
    /// recorded on the undo stack: undoable filter is the same optionally-
    /// undoable-mutation axis deferred for the 1-D proxy (R779.1 carry), kept
    /// out of this slice. Returns the resulting view length.
    fn apply_set_filter(&self, filter: Option<GridFilter>) -> usize {
        self.state.set_filter(filter)
    }
}

// The shared display-config-proxy `External` skeleton (R847 SSOT): no §5.20
// intent — the sort `Signal` write already repaints every subscribed view, so
// the widget is never independently dirty.
query_proxy_external_impl!(GridSortExternal);

impl ExternalIntrospect for GridSortExternal {
    fn schema(&self) -> IntrospectSchema {
        // `sort`      — compound "<col>:<dir>" / "none" (query + intervene).
        // `sort_col`  — active column id, or -1 (query only).
        // `sort_dir`  — none/ascending/descending (query only).
        // `filter`    — compound "<col>=<value>" / "none" (query + intervene).
        // `view_len`  — rows passing the filter (query only).
        // `count`     — source row count (query only).
        // `cols`      — column count (query only).
        // `source_at` — `source_at.<pos>` visual→source map (query only).
        // `cycle_sort`/`set_filter`/`send` — invoke channels.
        IntrospectSchema::new(&[
            ("sort", "string"),
            ("sort_col", "int"),
            ("sort_dir", "string"),
            ("filter", "string"),
            ("view_len", "int"),
            ("count", "int"),
            ("cols", "int"),
            ("source_at", "int"),
            ("cycle_sort", "int"),
            ("set_filter", "string"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // `source_at.<pos>` resolves the visual→source map; an out-of-range
        // position reports Null (present-but-empty), never absence.
        if let Some(rest) = path.strip_prefix("source_at.") {
            return Some(source_at_value(rest, |p| self.state.source_at(p)));
        }
        match path {
            "sort" => Some(IntrospectValue::Text(grid_sort_str(self.state.sort()))),
            "sort_col" => Some(self.sort_col_value()),
            "sort_dir" => Some(IntrospectValue::Text(
                sort_dir_str(self.state.sort().map(|(_, dir)| dir)).into(),
            )),
            "filter" => {
                Some(IntrospectValue::Text(grid_filter_str(self.state.filter().as_ref())))
            }
            "view_len" => Some(IntrospectValue::Int(
                i64::try_from(self.state.view_len()).unwrap_or(i64::MAX),
            )),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.state.count()).unwrap_or(i64::MAX),
            )),
            "cols" => Some(IntrospectValue::Int(
                i64::try_from(self.state.col_count()).unwrap_or(i64::MAX),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: set the whole sort from its compound string
            // (recorded onto the undo timeline when a stack is attached).
            "sort" => match value {
                IntrospectValue::Text(ref s) => {
                    self.apply_sort(grid_sort_from_str(s));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // Admin / restore: set the column filter from its compound string,
            // or clear it with Null (the header-less filter axis).
            "filter" => match value {
                IntrospectValue::Text(ref s) => {
                    self.apply_set_filter(grid_filter_from_str(s));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.apply_set_filter(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "sort_col" | "sort_dir" | "view_len" | "count" | "cols" | "source_at" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first sort cycle on a column — returns the resulting compound
            // sort string in one round-trip.
            "cycle_sort" => match args {
                IntrospectValue::Int(i) => {
                    let col = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    self.apply_cycle_sort(col);
                    Ok(IntrospectValue::Text(grid_sort_str(self.state.sort())))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // AI-first column filter: a `"<col>=<value>"` payload filters,
            // Null clears. Returns the resulting view length (rows passing the
            // filter) in one round-trip, so an AI client learns the new view
            // size without a follow-up query.
            "set_filter" => {
                let view_len = match args {
                    IntrospectValue::Text(ref s) => self.apply_set_filter(grid_filter_from_str(s)),
                    IntrospectValue::Null => self.apply_set_filter(None),
                    _ => return Err(InvokeError::TypeMismatch),
                };
                Ok(IntrospectValue::Int(i64::try_from(view_len).unwrap_or(i64::MAX)))
            }
            // R51.42 §5.35 composite pointer channel: a clicked column header
            // routes the pointer arc here as `<key>:<EventName>`.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(IntrospectValue::Text(grid_sort_str(self.state.sort())))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PointerWireEvent;
    use crate::reactive::Owner;

    // A 4-row × 2-column grid: column 0 is text, column 1 is numeric (so the
    // numeric-vs-lexicographic distinction is observable: "9" < "12").
    fn cells() -> Vec<Vec<String>> {
        vec![
            vec!["Menu".to_string(), "12".to_string()],
            vec!["Tabs".to_string(), "9".to_string()],
            vec!["Table".to_string(), "707".to_string()],
            vec!["Grid".to_string(), "775".to_string()],
        ]
    }

    fn state() -> GridSortState {
        GridSortState::new(2, cells())
    }

    #[test]
    fn grid_sort_str_round_trips() {
        for s in [None, Some((0, true)), Some((1, false)), Some((2, true))] {
            assert_eq!(grid_sort_from_str(&grid_sort_str(s)), s);
        }
        assert_eq!(grid_sort_str(None), "none");
        assert_eq!(grid_sort_str(Some((1, true))), "1:ascending");
        assert_eq!(grid_sort_from_str("garbage"), None);
        assert_eq!(grid_sort_from_str("x:ascending"), None);
    }

    #[test]
    fn unsorted_order_is_identity() {
        let s = state();
        assert_eq!(*s.order(), vec![0, 1, 2, 3]);
        assert_eq!(s.sort(), None);
    }

    #[test]
    fn cycle_sort_text_column_lexicographic() {
        let s = state();
        s.cycle_sort(0); // ascending by name: Grid, Menu, Table, Tabs
        assert_eq!(s.sort(), Some((0, true)));
        // data ids: Grid=3, Menu=0, Table=2, Tabs=1
        assert_eq!(*s.order(), vec![3, 0, 2, 1]);
        s.cycle_sort(0); // descending
        assert_eq!(s.sort(), Some((0, false)));
        assert_eq!(*s.order(), vec![1, 2, 0, 3]);
        s.cycle_sort(0); // back to source order
        assert_eq!(s.sort(), None);
        assert_eq!(*s.order(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn cycle_sort_numeric_column_is_numeric_aware() {
        let s = state();
        s.cycle_sort(1); // ascending by number: 9 (id1), 12 (id0), 707 (id2), 775 (id3)
        // A lexicographic sort would put "12" before "9"; numeric must not.
        assert_eq!(*s.order(), vec![1, 0, 2, 3]);
    }

    #[test]
    fn clicking_a_different_column_jumps_to_it_ascending() {
        let s = state();
        s.cycle_sort(0);
        assert_eq!(s.sort(), Some((0, true)));
        s.cycle_sort(1); // different column -> ascending on it
        assert_eq!(s.sort(), Some((1, true)));
    }

    #[test]
    fn out_of_range_column_is_a_noop() {
        let s = state();
        s.cycle_sort(9); // col_count = 2
        assert_eq!(s.sort(), None);
    }

    #[test]
    fn set_sort_clamps_out_of_range_column() {
        let s = state();
        s.set_sort(Some((9, true)));
        assert_eq!(s.sort(), None, "phantom column clamps to unsorted");
        s.set_sort(Some((1, false)));
        assert_eq!(s.sort(), Some((1, false)));
    }

    #[test]
    fn source_at_tracks_the_permutation() {
        let s = state();
        s.cycle_sort(0); // order = [3, 0, 2, 1]
        assert_eq!(s.source_at(0), Some(3));
        assert_eq!(s.source_at(3), Some(1));
        assert_eq!(s.source_at(4), None);
    }

    #[test]
    fn external_send_header_cycles_sort() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            // h1:PointerUp -> sort column 1 ascending.
            let payload = format!("h1:{}", PointerWireEvent::Up.as_wire_name());
            ext.invoke("send", IntrospectValue::Text(payload)).unwrap();
            assert_eq!(st.sort(), Some((1, true)));
            // A cell key is ignored (selection coordinator's axis).
            ext.invoke(
                "send",
                IntrospectValue::Text(format!("0_0:{}", PointerWireEvent::Up.as_wire_name())),
            )
            .unwrap();
            assert_eq!(st.sort(), Some((1, true)), "cell send does not touch sort");
        });
    }

    #[test]
    fn external_invoke_cycle_sort_returns_compound_string() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            let out = ext.invoke("cycle_sort", IntrospectValue::Int(0)).unwrap();
            assert_eq!(out, IntrospectValue::Text("0:ascending".into()));
            assert_eq!(ext.query("sort_col"), Some(IntrospectValue::Int(0)));
            assert_eq!(ext.query("sort"), Some(IntrospectValue::Text("0:ascending".into())));
        });
    }

    #[test]
    fn external_intervene_restores_whole_sort() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            ext.intervene("sort", IntrospectValue::Text("1:descending".into())).unwrap();
            assert_eq!(st.sort(), Some((1, false)));
            ext.intervene("sort", IntrospectValue::Text("none".into())).unwrap();
            assert_eq!(st.sort(), None);
            assert_eq!(ext.intervene("count", IntrospectValue::Int(0)), Err(InterveneError::ReadOnly));
        });
    }

    #[test]
    fn grid_sort_edit_redo_undo_restore_the_sort() {
        // The bare command (3rd UndoCommand consumer): redo applies `after`,
        // undo restores the captured `before`.
        let st = Rc::new(state());
        st.set_sort(Some((0, true)));
        let edit = GridSortEdit::capture(&st, Some((1, false)), "Sort 1:descending");
        edit.redo();
        assert_eq!(st.sort(), Some((1, false)), "redo applies the after sort");
        edit.undo();
        assert_eq!(st.sort(), Some((0, true)), "undo restores the captured before sort");
        assert_eq!(edit.label(), "Sort 1:descending");
    }

    #[test]
    fn external_with_undo_records_every_sort_path() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let stack = Rc::new(UndoStack::new());
            let mut ext = GridSortExternal::new(Rc::clone(&st)).with_undo(Rc::clone(&stack));

            // Header click cycles + records.
            ext.invoke("send", IntrospectValue::Text(format!("h0:{}", PointerWireEvent::Up.as_wire_name())))
                .unwrap();
            assert_eq!(st.sort(), Some((0, true)));
            assert!(stack.can_undo(), "the header-click sort is on the undo timeline");

            // invoke + intervene also record onto the same timeline.
            ext.invoke("cycle_sort", IntrospectValue::Int(0)).unwrap(); // 0:desc
            ext.intervene("sort", IntrospectValue::Text("1:ascending".into())).unwrap();
            assert_eq!(st.sort(), Some((1, true)));

            // Unwind the whole timeline: 1:asc -> 0:desc -> 0:asc -> none.
            stack.undo();
            assert_eq!(st.sort(), Some((0, false)), "undo the intervene");
            stack.undo();
            assert_eq!(st.sort(), Some((0, true)), "undo the cycle");
            stack.undo();
            assert_eq!(st.sort(), None, "undo the header click → unsorted");
            // Redo re-applies forward.
            stack.redo();
            assert_eq!(st.sort(), Some((0, true)), "redo re-applies the header click");
        });
    }

    #[test]
    fn external_without_undo_mutates_directly() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            ext.invoke("cycle_sort", IntrospectValue::Int(1)).unwrap();
            assert_eq!(st.sort(), Some((1, true)), "no undo stack → direct mutation (unchanged behaviour)");
        });
    }

    // ── R783 §5.40 column filter ──────────────────────────────────────

    // A 6-row × 2-column grid: column 0 is a category facet with repeats
    // (so a filter keeps a *subset*), column 1 is numeric (so filter-then-sort
    // composition is observable on the survivors).
    fn cat_cells() -> Vec<Vec<String>> {
        vec![
            vec!["A".to_string(), "30".to_string()], // 0
            vec!["B".to_string(), "10".to_string()], // 1
            vec!["A".to_string(), "20".to_string()], // 2
            vec!["B".to_string(), "5".to_string()],  // 3
            vec!["A".to_string(), "9".to_string()],  // 4
            vec!["C".to_string(), "1".to_string()],  // 5
        ]
    }

    #[test]
    fn grid_filter_str_round_trips() {
        let cases = [None, Some(GridFilter { col: 0, value: "Active".into() })];
        for f in cases {
            assert_eq!(grid_filter_from_str(&grid_filter_str(f.as_ref())), f);
        }
        assert_eq!(grid_filter_str(None), "none");
        assert_eq!(grid_filter_str(Some(&GridFilter { col: 2, value: "Done".into() })), "2=Done");
        assert_eq!(grid_filter_from_str("none"), None, "no '=' → unfiltered");
        assert_eq!(grid_filter_from_str("x=Done"), None, "non-numeric column → unfiltered");
        // The value half is verbatim, even if it contains '='.
        assert_eq!(
            grid_filter_from_str("1=a=b"),
            Some(GridFilter { col: 1, value: "a=b".into() }),
        );
    }

    #[test]
    fn filter_keeps_only_matching_rows_in_source_order() {
        let s = GridSortState::new(2, cat_cells());
        assert_eq!(s.view_len(), 6, "unfiltered view is the full dataset");
        assert_eq!(s.set_filter(Some(GridFilter { col: 0, value: "A".into() })), 3);
        assert_eq!(*s.order(), vec![0, 2, 4], "only category-A rows, in source order");
        assert_eq!(s.set_filter(None), 6, "clearing restores the full view");
        assert_eq!(*s.order(), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn filter_then_sort_composes() {
        let s = GridSortState::new(2, cat_cells());
        s.set_filter(Some(GridFilter { col: 0, value: "A".into() }));
        // Survivors are ids 0,2,4 with numeric col-1 values 30,20,9.
        s.cycle_sort(1); // ascending numeric over the FILTERED set
        assert_eq!(*s.order(), vec![4, 2, 0], "filter shrinks, then survivors sort: 9 < 20 < 30");
        assert_eq!(s.view_len(), 3, "the sort does not change the view size");
    }

    #[test]
    fn set_filter_clamps_phantom_column() {
        let s = GridSortState::new(2, cat_cells());
        assert_eq!(
            s.set_filter(Some(GridFilter { col: 9, value: "A".into() })),
            6,
            "a filter on a phantom column clamps to unfiltered (full view)",
        );
        assert_eq!(s.filter(), None);
    }

    #[test]
    fn external_filter_query_intervene_invoke() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(GridSortState::new(2, cat_cells()));
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            // Boot: unfiltered, full view.
            assert_eq!(ext.query("filter"), Some(IntrospectValue::Text("none".into())));
            assert_eq!(ext.query("view_len"), Some(IntrospectValue::Int(6)));
            // invoke set_filter returns the resulting view length in one trip.
            assert_eq!(
                ext.invoke("set_filter", IntrospectValue::Text("0=A".into())),
                Ok(IntrospectValue::Int(3)),
            );
            assert_eq!(ext.query("filter"), Some(IntrospectValue::Text("0=A".into())));
            assert_eq!(ext.query("view_len"), Some(IntrospectValue::Int(3)));
            // Null clears via invoke.
            assert_eq!(ext.invoke("set_filter", IntrospectValue::Null), Ok(IntrospectValue::Int(6)));
            assert_eq!(st.filter(), None);
            // intervene sets + clears (admin / restore path).
            ext.intervene("filter", IntrospectValue::Text("0=B".into())).unwrap();
            assert_eq!(st.filter(), Some(GridFilter { col: 0, value: "B".into() }));
            ext.intervene("filter", IntrospectValue::Null).unwrap();
            assert_eq!(st.filter(), None);
            // view_len is read-only.
            assert!(ext.intervene("view_len", IntrospectValue::Int(1)).is_err());
        });
    }
}
