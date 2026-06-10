//! R707 §5.38 — `Table` widget: an interactive data grid with
//! single-row selection and 2-D keyboard navigation.
//!
//! A table presents tabular data as a grid of rows and columns (the
//! WAI-ARIA `grid` role with `row` / `columnheader` / `gridcell`
//! children — the **second consumer** of the R704 grid-role family,
//! and the first to exercise the [`Row`](pinion_a11y::AriaRole::Row)
//! role, which the date picker's flat calendar grid does not). Exactly
//! one row may be selected at a time; activating any cell in a row
//! selects that row (single-row exclusion — framework-owned in the
//! coordinator, like a [`RadioGroup`](crate::widgets::radio_group::RadioGroup)'s
//! sibling-deselect, with one [`Radio`] leaf per row).
//!
//! `Table` is a single coordinator (mirroring `RadioGroup` /
//! [`DatePicker`](crate::widgets::datepicker::DatePicker) — a "select 1
//! of N" model). It owns the column headers, the immutable cell text,
//! per-row interaction state, the selected row, and a 2-D roving
//! *active descendant* `(row, col)` cursor independent of the selection
//! (the data-grid analog of the date picker's `focused_day`).
//!
//! Visual scene placement is the application's responsibility (same
//! contract as `RadioGroup` / `DatePicker`): the binding composes the
//! header row + body rows via [`pinion_widget_paint::table`] and queries
//! the coordinator for per-row state.
//!
//! The [`TableExternal`] adapter exposes the table on the §5.12 RPC
//! surface:
//!
//! * `query "rows"` / `query "cols"` → [`IntrospectValue::Int`] — row /
//!   column counts
//! * `query "selected"` → [`IntrospectValue::Bool`] — any row selected?
//! * `query "selected_row"` → [`IntrospectValue::Int`] (`-1` when none)
//! * `query "focused_row"` / `query "focused_col"` →
//!   [`IntrospectValue::Int`] — the 2-D active descendant (`focused_row`
//!   is `-1` before any navigation)
//! * `query "header.<col>"` → [`IntrospectValue::Text`] — column header
//!   label
//! * `query "cell.<row>.<col>"` → [`IntrospectValue::Text`] — cell text
//! * `query "state.<row>"` / `query "selected.<row>"` — per-row
//!   interaction-state name / selected bit
//! * `invoke "send" → "<row>_<col>:<EventName>"` — drive one cell (the
//!   composite click funnel); activation selects the cell's row
//!
//! On selection-change transitions, the §5.20 channel emits a
//! `"selected"` intent carrying the new selected row index as
//! [`IntrospectValue::Int`].

use crate::external::{
    int_of, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::input::PointerWireEvent;
use crate::intent::Intent;
use crate::widgets::radio::{Radio, RadioEvent, RadioState};
use crate::widgets::selection;
use crate::widgets::{IntentEmitter, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// Logical data grid with framework-owned single-row selection. See
/// module docs for the design rationale.
///
/// Reuses the [`Radio`] leaf statechart per row (the same "select 1 of
/// N" leaf [`RadioGroup`] composes), so rows inherit the canonical
/// `{Idle, Hover, Pressed, Disabled}` interaction model + activate edge.
/// The cell text is immutable for the table's lifetime (row insert /
/// delete / edit + virtualization are deferred axes — see the binding
/// docs), so the per-row [`Radio`] vector is sized once at construction.
pub struct Table {
    /// Column header labels; `headers.len()` is the column count.
    headers: Vec<String>,
    /// Row-major cell text. Every inner row has `headers.len()` cells
    /// (callers construct rectangular data; ragged rows clamp on query).
    rows: Vec<Vec<String>>,
    /// One [`Radio`] leaf per row, indexed by row number, carrying the
    /// row's selection + interaction state.
    row_radios: Vec<Radio>,
    /// The selected row (0-based), retained until another row activates.
    selected_row: Option<usize>,
    /// R707 §5.40 — the WAI-ARIA roving active-descendant **row** (the
    /// row of the keyboard cursor's cell), or `None` before any
    /// navigation. Independent of `selected_row` — arrow keys move it
    /// without committing a selection (the data-grid mirror of the date
    /// picker's `focused_day`). Activation syncs it to the activated
    /// cell per the WAI-ARIA "activation moves focus" rule.
    focused_row: Option<usize>,
    /// R707 §5.40 — the roving active-descendant **column** (0-based).
    /// Always valid (defaults to column 0); paired with `focused_row` to
    /// address the active-descendant cell.
    focused_col: usize,
    /// R730 §5.40 — the current sort key `(col, ascending)`, or `None`
    /// when unsorted (display = data order). Purely a **display order**
    /// concern: [`Self::order`] derives the visual→data permutation from
    /// it, while selection / interaction state stay indexed by **data
    /// row** (so a sorted view needs no remap — a selected data row
    /// simply paints at its new visual position). Defaulting to `None`
    /// keeps the unsorted boot identical to the pre-R730 table.
    sort: Option<(usize, bool)>,
    /// R735 §5.38 — selection cardinality mode. `false` (via
    /// [`Self::new`]) is the R707 single-row-exclusion model
    /// (`aria-multiselectable` absent): activating a row deselects every
    /// other. `true` (via [`Self::with_multiselect`]) is the WAI-ARIA
    /// `aria-multiselectable="true"` model (the 2-D grid analog of
    /// [`ListBox::with_multiselect`](crate::widgets::listbox::ListBox::with_multiselect)):
    /// activating a row toggles only that row's selection bit, siblings
    /// untouched, and [`Self::selected_row`] has no single meaningful
    /// value (callers use [`Self::selected_rows`]). Mode is immutable
    /// after construction — the row [`Radio`] vector is sized once.
    multiselect: bool,
}

impl Table {
    /// Construct a table over `headers` columns and `rows` of cell text.
    /// All rows start idle and unselected, with no active descendant.
    #[must_use]
    pub fn new(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        let row_radios = (0..rows.len()).map(|_| Radio::new()).collect();
        Self {
            headers,
            rows,
            row_radios,
            selected_row: None,
            focused_row: None,
            focused_col: 0,
            sort: None,
            multiselect: false,
        }
    }

    /// R735 §5.38 — construct a **multi-select** table (the WAI-ARIA
    /// `aria-multiselectable="true"` model). Activating a row toggles only
    /// that row's selection bit; siblings are untouched. The 2-D grid
    /// analog of [`ListBox::with_multiselect`](crate::widgets::listbox::ListBox::with_multiselect)
    /// — same [`Radio`] leaf per row, but no sibling-deselect. All rows
    /// start idle and unselected, with no active descendant.
    #[must_use]
    pub fn with_multiselect(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        let mut t = Self::new(headers, rows);
        t.multiselect = true;
        t
    }

    /// R735 §5.38 — `true` if this table was constructed via
    /// [`Self::with_multiselect`].
    #[must_use]
    pub fn is_multiselect(&self) -> bool {
        self.multiselect
    }

    /// The number of rows.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The number of columns.
    #[must_use]
    pub fn col_count(&self) -> usize {
        self.headers.len()
    }

    /// The column header label for `col` (0-based), or `""` out of range.
    #[must_use]
    pub fn header(&self, col: usize) -> &str {
        self.headers.get(col).map_or("", String::as_str)
    }

    /// The cell text at `(row, col)` (0-based), or `""` out of range.
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> &str {
        self.rows
            .get(row)
            .and_then(|r| r.get(col))
            .map_or("", String::as_str)
    }

    /// The selected row (0-based), or `None`. **Single-select only** —
    /// see [`Self::selected_rows`] for the multi-select-aware query.
    ///
    /// # Panics
    /// Panics if [`Self::is_multiselect`] is `true`: a multi-select table
    /// has no single meaningful selected row (zero or many rows may be
    /// selected); multi-mode callers must use [`Self::selected_rows`].
    #[must_use]
    pub fn selected_row(&self) -> Option<usize> {
        assert!(
            !self.multiselect,
            "Table::selected_row() is single-select-only; \
             use selected_rows() in multi-select mode"
        );
        self.selected_row
    }

    /// R735 §5.38 — every currently-selected row, ascending. Works in
    /// both modes (single-select returns 0 or 1 rows; multi-select returns
    /// `0..row_count`). The canonical query for multi-mode and the
    /// mode-agnostic query for tooling — mirror of
    /// [`ListBox::selected_indices`](crate::widgets::listbox::ListBox::selected_indices).
    #[must_use]
    pub fn selected_rows(&self) -> Vec<usize> {
        self.row_radios
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.is_selected().then_some(i))
            .collect()
    }

    /// R735 §5.38 — slot-assignment setter that works in both modes
    /// (persisted-preference restore / programmatic clear). Replaces the
    /// entire selection set with `rows`. Silent on the §5.20 intent
    /// channel — restoration is admin, not interaction (mirror of
    /// [`ListBox::set_selected_indices`](crate::widgets::listbox::ListBox::set_selected_indices)).
    ///
    /// # Panics
    /// * Panics if any index in `rows` is `>= row_count()`.
    /// * In single-select mode, panics if `rows.len() > 1`.
    pub fn set_selected_rows(&mut self, rows: &[usize]) {
        // R735.1 §5.38 — shared slot-assignment core (index validation +
        // single-select cardinality cap); the returned first index
        // mirrors into the single-select cursor.
        let first = selection::replace_selection(&mut self.row_radios, rows, self.multiselect);
        if !self.multiselect {
            self.selected_row = first;
        }
    }

    /// Drive `event` to the cell at `(row, col)`.
    ///
    /// **Single-select mode** ([`Self::new`]) — if the event activates
    /// that row (`false → true` selected), every other row is deselected
    /// and `selected_row` snaps to `row`.
    ///
    /// **Multi-select mode** ([`Self::with_multiselect`]) — R735 §5.38 —
    /// activation toggles the addressed row only (already-selected →
    /// deselected; unselected → selected); siblings stay untouched and
    /// `selected_row` is not maintained. Toggle-off is detected off the
    /// row [`Radio`]'s state-machine activation edge (the leaf's value is
    /// set-not-flip, so the composite forces the new `false` here) — the
    /// 2-D grid mirror of [`ListBox::send`](crate::widgets::listbox::ListBox::send).
    ///
    /// In both modes the `PointerUp` edge of a click syncs the active
    /// descendant to `(row, col)` — WAI-ARIA "clicking a cell moves focus
    /// to it", whether or not the selection changed.
    ///
    /// Out-of-range `(row, col)` is a silent no-op (the router rejects
    /// bad composite sub-indices upstream; this guards the model path).
    pub fn send_cell(&mut self, row: usize, col: usize, event: RadioEvent) {
        if row >= self.rows.len() || col >= self.headers.len() {
            return;
        }
        let pre_state = self.row_radios[row].state();
        let was_selected = self.row_radios[row].is_selected();
        self.row_radios[row].send(event);
        let post_state = self.row_radios[row].state();
        if self.multiselect {
            // R735.1 §5.38 — multi-select toggle. The activation predicate
            // is leaf-typed (RadioState / RadioEvent), so it stays inline;
            // the set-not-flip toggle-off completion is the shared
            // substrate (selection::toggle_off_if_reselected).
            let activation = (matches!(pre_state, RadioState::Pressed)
                && matches!(post_state, RadioState::Hover))
                || (matches!(event, RadioEvent::KeyboardActivate)
                    && !matches!(pre_state, RadioState::Disabled));
            selection::toggle_off_if_reselected(
                &mut self.row_radios[row],
                was_selected,
                activation,
            );
        } else if selection::select_exclusive(&mut self.row_radios, row, was_selected) {
            // R707 §5.40 — single-row exclusion landed; store the cursor.
            self.selected_row = Some(row);
        }
        // R707 §5.40 — the active descendant follows the click's
        // `PointerUp` regardless of whether the selection changed (the
        // 2-D data-grid refinement of `DatePicker`'s selection-coupled
        // focus sync, which only ever re-targets the same day).
        if matches!(event, RadioEvent::PointerUp) {
            self.focused_row = Some(row);
            self.focused_col = col;
        }
    }

    /// R707 §5.40 — the roving active-descendant row (0-based), or `None`
    /// before any navigation. See [`Self::focused_row`].
    #[must_use]
    pub fn focused_row(&self) -> Option<usize> {
        self.focused_row
    }

    /// R707 §5.40 — the roving active-descendant column (0-based).
    #[must_use]
    pub fn focused_col(&self) -> usize {
        self.focused_col
    }

    /// R707 §5.40 — set the roving active-descendant row. `None` clears
    /// it; `Some(r)` is stored as-is (callers validate against
    /// [`Self::row_count`]). Independent of selection — this neither
    /// activates the row nor fires the `"selected"` intent (the
    /// data-grid mirror of `DatePicker::set_focused_day`).
    pub fn set_focused_row(&mut self, row: Option<usize>) {
        self.focused_row = row;
    }

    /// R707 §5.40 — set the roving active-descendant column. Stored
    /// as-is (callers validate against [`Self::col_count`]).
    pub fn set_focused_col(&mut self, col: usize) {
        self.focused_col = col;
    }

    /// Interaction state of `row` (0-based), or [`RadioState::Idle`] for
    /// an out-of-range row.
    #[must_use]
    pub fn state(&self, row: usize) -> RadioState {
        self.row_radios
            .get(row)
            .map_or(RadioState::Idle, Radio::state)
    }

    /// Whether `row` (0-based) is selected. `false` out of range.
    #[must_use]
    pub fn is_selected(&self, row: usize) -> bool {
        self.row_radios.get(row).is_some_and(Radio::is_selected)
    }

    /// R730 §5.40 — the current sort key `(col, ascending)`, or `None`
    /// when unsorted.
    #[must_use]
    pub fn sort_state(&self) -> Option<(usize, bool)> {
        self.sort
    }

    /// R730 §5.40 — cycle the sort on `col` the way a clicked column
    /// header does: unsorted → ascending → descending → unsorted. Clicking
    /// a *different* column jumps straight to that column ascending.
    /// Out-of-range `col` is a silent no-op.
    pub fn cycle_sort(&mut self, col: usize) {
        // R778 — the cycle transition is the shared `cycle_col_sort` SSOT
        // (the virtualized grid is its second consumer).
        self.sort = cycle_col_sort(self.sort, col, self.headers.len());
    }

    /// R730 §5.40 — the visual→data row permutation for the current sort.
    /// `order()[visual]` is the data-row index painted at visual position
    /// `visual`. Identity (`0..row_count`) when unsorted. The comparison
    /// is **numeric-aware** (cells that both parse as numbers compare
    /// numerically, else lexicographically) and **stable** (ties keep
    /// their data order), so re-sorting is deterministic.
    #[must_use]
    pub fn order(&self) -> Vec<usize> {
        // R778 — delegate to the shared `grid_order_by` SSOT; this struct
        // supplies the cell source, the free fn owns the direction +
        // stable-tie-break policy (shared with the virtualized grid).
        grid_order_by(
            self.rows.len(),
            self.sort,
            |col, a, b| cell_cmp(self.cell(a, col), self.cell(b, col)),
            |_| true, // the eager table has no filter axis (sort only)
        )
    }
}

/// R730 — numeric-aware cell comparison: two cells that both parse as
/// `f64` compare numerically (so `"9" < "12"`); otherwise lexicographic
/// (so `"Active" < "Done"`). A data grid that sorted a numeric column
/// lexicographically (`"12" < "9"`) would be visibly wrong.
///
/// (R778 `pub` — the numeric comparator is the SSOT for both the eager
/// [`Table::order`] and the virtualized data-grid sort coordinator
/// [`GridSortState`](crate::widgets::grid_sort::GridSortState); they must
/// agree on numeric awareness, so the comparison lives in one place.)
#[must_use]
pub fn cell_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(core::cmp::Ordering::Equal),
        _ => a.cmp(b),
    }
}

/// R778 §5.40 — the clicked-column-header sort transition, lifted from
/// [`Table::cycle_sort`] when the virtualized data grid became its second
/// consumer (the cycle is a controller wiring whose divergence between the
/// eager and the scale grid would be a bug, not a style choice).
///
/// Clicking the **active** column cycles its direction
/// unsorted → ascending → descending → unsorted; clicking a **different**
/// column jumps straight to that column ascending. An out-of-range `col`
/// (`col >= col_count`) is a silent no-op (returns `prev` unchanged).
#[must_use]
pub fn cycle_col_sort(
    prev: Option<(usize, bool)>,
    col: usize,
    col_count: usize,
) -> Option<(usize, bool)> {
    if col >= col_count {
        return prev;
    }
    match prev {
        Some((c, true)) if c == col => Some((col, false)),
        Some((c, false)) if c == col => None,
        _ => Some((col, true)),
    }
}

/// R778 §5.40 — the visual→data row permutation for a multi-column grid
/// sort, lifted from [`Table::order`] for its second consumer (the
/// virtualized [`GridSortState`](crate::widgets::grid_sort::GridSortState)).
///
/// `order[visual]` is the data-row index painted at visual position
/// `visual`. Identity (`0..row_count`) when `sort` is `None`. `cell_cmp_at`
/// compares two data rows on a column — `(col, row_a, row_b)` — so the
/// caller supplies the data source (eager `Table` cells or the
/// coordinator's materialized cell grid) while this fn owns the
/// direction + stable-tie-break policy: the sort is **stable**, so equal
/// keys keep their data order in both directions (deterministic
/// re-sorting), and `ascending = false` reverses the per-pair comparison.
///
/// The peer of [`view_order::compute_order`](crate::widgets::view_order::compute_order)
/// for the grid: that one is single-key + generic [`Ord`] (the 1-D list);
/// this one is multi-column + numeric-aware ([`cell_cmp`]). They are
/// deliberately separate vocabularies, not one merged sorter.
///
/// R783 §5.40 — `pass` is the **filter** axis (the grid peer of
/// `compute_order`'s `pass`): a row is kept only when `pass(row)` is `true`,
/// applied **before** the sort so a filtered view shrinks naturally
/// (`order.len() ≤ row_count`) and the surviving rows are then ordered. A
/// display-only / unfiltered grid passes `|_| true`. Filter-then-sort, so the
/// two axes compose orthogonally (filter the dataset, sort the survivors).
#[must_use]
pub fn grid_order_by(
    row_count: usize,
    sort: Option<(usize, bool)>,
    cell_cmp_at: impl Fn(usize, usize, usize) -> core::cmp::Ordering,
    pass: impl Fn(usize) -> bool,
) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..row_count).filter(|&i| pass(i)).collect();
    let Some((col, ascending)) = sort else {
        return idx;
    };
    idx.sort_by(|&a, &b| {
        let ord = cell_cmp_at(col, a, b);
        if ascending {
            ord
        } else {
            ord.reverse()
        }
    });
    idx
}

/// R735 §5.38 — `Table` snapshot capturing the mode flag + the full
/// per-row selection bitmap, so the trait-level `detect` expresses both
/// single-select-replace and multi-select-toggle in one rule (mirror of
/// [`ListBoxSnapshot`](crate::widgets::listbox::ListBox)). The Vec
/// heap-allocates once per `IntentEmitter::dispatch` (i.e. once per user
/// input event); negligible at UI / AI dispatch frequencies.
/// `Table` transition contract (R51.12 substrate). The event pairs the
/// `(row, col)` cell with the underlying [`RadioEvent`]. R735 §5.38 — the
/// snapshot captures mode + the per-row selection bitmap so `detect`
/// handles both modes:
///
/// **Single-select** (`multiselect = false`) — emit `Int(i)` for the row
/// whose bit went `false → true` (siblings deselect silently):
///
/// * `None → Some(r)` — first selection ⇒ `vec![Int(r)]`
/// * `Some(a) → Some(b)` (`a != b`) — `a` lost, `b` gained ⇒ `vec![Int(b)]`
/// * `Some(a) → Some(a)` — bitmap unchanged ⇒ `Vec::new()`
///
/// **Multi-select** (`multiselect = true`) — emit `Int(i)` for *every*
/// row whose bit flipped in either direction (toggle-on + toggle-off both
/// carry information the AI client needs; the follow-up `selected.<i>`
/// query learns the new boolean). Unchanged bitmap ⇒ `Vec::new()`.
///
/// The rule is single-shape: emit `Int(i)` for every index `i` whose bit
/// changed, gated in single-select to the `false → true` direction only.
impl WidgetTransition for Table {
    type Event = (usize, usize, RadioEvent);
    type Snapshot = selection::SelectionSnapshot;

    fn snapshot(&self) -> Self::Snapshot {
        selection::capture(&self.row_radios, self.multiselect)
    }

    fn drive(&mut self, event: Self::Event) {
        let (row, col, ev) = event;
        self.send_cell(row, col, ev);
    }

    fn detect(
        before: Self::Snapshot,
        _event: Self::Event,
        after: Self::Snapshot,
    ) -> Vec<Intent> {
        // R735.1 §5.38 — shared multi-capable bitmap diff (single: gain
        // only; multi: every flip).
        selection::detect_intents(&before, &after)
    }
}

/// `External` adapter wrapping a [`Table`]. Surfaces table state to the
/// §5.12 `scene/query` / `scene/rewind` / `scene/invoke` paths and emits
/// a `"selected"` intent (the new row index as [`IntrospectValue::Int`])
/// on selection-change transitions.
pub struct TableExternal {
    em: IntentEmitter<Table>,
}

impl TableExternal {
    /// Construct a single-select table over `headers` columns and `rows`
    /// of cell text.
    #[must_use]
    pub fn new(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { em: IntentEmitter::new(Table::new(headers, rows)) }
    }

    /// R735 §5.38 — construct a **multi-select** table (the WAI-ARIA
    /// `aria-multiselectable="true"` model). Activating a row toggles only
    /// that row; siblings are untouched.
    #[must_use]
    pub fn with_multiselect(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { em: IntentEmitter::new(Table::with_multiselect(headers, rows)) }
    }

    /// R735 §5.38 — `true` if this table was constructed via
    /// [`Self::with_multiselect`].
    #[must_use]
    pub fn is_multiselect(&self) -> bool {
        self.em.inner.is_multiselect()
    }

    /// Drive `event` to the cell at `(row, col)`. Queues a `"selected"`
    /// intent per row whose selection bit flipped (single: on gain;
    /// multi: on every flip).
    pub fn send_cell(&mut self, row: usize, col: usize, event: RadioEvent) {
        self.em.dispatch((row, col, event));
    }

    /// The number of rows.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.em.inner.row_count()
    }

    /// The number of columns.
    #[must_use]
    pub fn col_count(&self) -> usize {
        self.em.inner.col_count()
    }

    /// The selected row (0-based), or `None`. **Single-select only**.
    ///
    /// # Panics
    /// Panics if [`Self::is_multiselect`] is `true` (use
    /// [`Self::selected_rows`] in multi-select mode).
    #[must_use]
    pub fn selected_row(&self) -> Option<usize> {
        self.em.inner.selected_row()
    }

    /// R735 §5.38 — every currently-selected row, ascending. Works in
    /// both modes.
    #[must_use]
    pub fn selected_rows(&self) -> Vec<usize> {
        self.em.inner.selected_rows()
    }

    /// R735 §5.38 — slot-assignment setter (persisted-preference restore
    /// / boot seed / programmatic clear). Replaces the entire selection
    /// set with `rows`; silent on the §5.20 intent channel and leaves the
    /// active descendant untouched — restoration is admin, not
    /// interaction (mirror of [`Table::set_selected_rows`]).
    ///
    /// # Panics
    /// Panics on an out-of-range index, or `rows.len() > 1` in
    /// single-select mode (see [`Table::set_selected_rows`]).
    pub fn set_selected_rows(&mut self, rows: &[usize]) {
        self.em.inner.set_selected_rows(rows);
    }

    /// Interaction state of `row` (0-based).
    #[must_use]
    pub fn state(&self, row: usize) -> RadioState {
        self.em.inner.state(row)
    }

    /// Whether `row` (0-based) is selected.
    #[must_use]
    pub fn is_selected(&self, row: usize) -> bool {
        self.em.inner.is_selected(row)
    }

    /// R707 §5.40 — the roving active-descendant row (0-based), or `None`.
    #[must_use]
    pub fn focused_row(&self) -> Option<usize> {
        self.em.inner.focused_row()
    }

    /// R707 §5.40 — the roving active-descendant column (0-based).
    #[must_use]
    pub fn focused_col(&self) -> usize {
        self.em.inner.focused_col()
    }

    /// R730 §5.40 — the current sort key `(col, ascending)`, or `None`.
    #[must_use]
    pub fn sort_state(&self) -> Option<(usize, bool)> {
        self.em.inner.sort_state()
    }

    /// R730 §5.40 — cycle the sort on `col` (unsorted → asc → desc →
    /// unsorted; a new column jumps to ascending). The column-header
    /// click + the `invoke "sort"` RPC path both land here.
    pub fn cycle_sort(&mut self, col: usize) {
        self.em.inner.cycle_sort(col);
    }

    /// R730 §5.40 — the visual→data row permutation for the current sort
    /// ([`Table::order`]).
    #[must_use]
    pub fn order(&self) -> Vec<usize> {
        self.em.inner.order()
    }

    /// Validate an intervene `row` index against the row count.
    fn resolve_row_intervene(&self, i: i64) -> Result<usize, InterveneError> {
        let row = usize::try_from(i).map_err(|_| InterveneError::OutOfRange)?;
        if row >= self.row_count() {
            return Err(InterveneError::OutOfRange);
        }
        Ok(row)
    }

    /// Validate an intervene `col` index against the column count.
    fn resolve_col_intervene(&self, i: i64) -> Result<usize, InterveneError> {
        let col = usize::try_from(i).map_err(|_| InterveneError::OutOfRange)?;
        if col >= self.col_count() {
            return Err(InterveneError::OutOfRange);
        }
        Ok(col)
    }
}

impl Default for TableExternal {
    /// Default constructs an empty table (no columns, no rows).
    fn default() -> Self {
        Self::new(Vec::new(), Vec::new())
    }
}

impl core::fmt::Debug for TableExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut dbg = f.debug_struct("TableExternal");
        dbg.field("rows", &self.row_count())
            .field("cols", &self.col_count())
            .field("multiselect", &self.is_multiselect());
        // R735 §5.38 — `selected_row` panics in multi-mode, so the Debug
        // output substitutes the mode-agnostic `selected_rows` there.
        if self.is_multiselect() {
            dbg.field("selected_rows", &self.selected_rows());
        } else {
            dbg.field("selected_row", &self.selected_row());
        }
        dbg.finish()
    }
}

impl External for TableExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        self.em.drain(sink);
    }

    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }
}

impl ExternalIntrospect for TableExternal {
    fn schema(&self) -> IntrospectSchema {
        // The per-cell / per-row paths advertise their `<row>` / `<col>`
        // placeholders the same way `send` documents its
        // `"<row>_<col>:<EventName>"` wire format — discovery metadata
        // for AI clients (`scene/schema` RPC), not a static enumeration.
        IntrospectSchema::new(&[
            ("rows", "int"),
            ("cols", "int"),
            // R735 §5.38 — `multiselect` (bool) is read-only mode
            // metadata. `selected.<row>` becomes write-enabled in
            // multi-select mode (the canonical per-row admin path);
            // `selected_row` returns `null` in multi-mode and rejects
            // intervene.
            ("multiselect", "bool"),
            ("selected", "bool"),
            ("selected_row", "int"),
            ("focused_row", "int"),
            ("focused_col", "int"),
            ("header.<col>", "string"),
            ("cell.<row>.<col>", "string"),
            ("state.<row>", "string"),
            ("selected.<row>", "bool"),
            ("send", "string"),
            // R730 §5.40 — sort surface. `sort_col` is the sort key column
            // (`-1` when unsorted); `sort_dir` is "none"/"ascending"/
            // "descending"; `order.<visual>` is the data-row index painted
            // at visual position `<visual>`; `sort` (invoke) cycles a
            // column's sort the way a header click does.
            ("sort_col", "int"),
            ("sort_dir", "string"),
            ("order.<visual>", "int"),
            ("sort", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "rows" => Some(IntrospectValue::Int(int_of(self.row_count()))),
            "cols" => Some(IntrospectValue::Int(int_of(self.col_count()))),
            // R735 §5.38 — mode metadata so a mode-agnostic introspector
            // picks the right path (`selected.<i>` per-row vs
            // `selected_row` single).
            "multiselect" => Some(IntrospectValue::Bool(self.is_multiselect())),
            // `selected` (any row selected?) is mode-agnostic — reads the
            // selection set, never the single-only `selected_row`.
            "selected" => Some(IntrospectValue::Bool(!self.selected_rows().is_empty())),
            // The selected / focused row use `-1` until a value lands
            // (mirror of the date picker's `selected_day` / `focused_day`
            // `int` sentinel convention). R735 §5.38 — multi-mode has no
            // single selected row, so it returns `-1` (AI clients use the
            // per-row `selected.<i>` slots).
            "selected_row" => Some(IntrospectValue::Int(if self.is_multiselect() {
                -1
            } else {
                self.selected_row().map_or(-1, int_of)
            })),
            "focused_row" => Some(IntrospectValue::Int(
                self.focused_row().map_or(-1, int_of),
            )),
            "focused_col" => Some(IntrospectValue::Int(int_of(self.focused_col()))),
            // R730 §5.40 — sort key column (`-1` when unsorted) + the
            // WAI-ARIA `aria-sort` token the active header carries.
            "sort_col" => Some(IntrospectValue::Int(
                self.sort_state().map_or(-1, |(c, _)| int_of(c)),
            )),
            "sort_dir" => Some(IntrospectValue::Text(
                match self.sort_state() {
                    None => "none",
                    Some((_, true)) => "ascending",
                    Some((_, false)) => "descending",
                }
                .to_string(),
            )),
            _ => {
                // Per-column header text: `header.<col>`.
                if let Some(col_str) = path.strip_prefix("header.") {
                    let col: usize = col_str.parse().ok()?;
                    if col >= self.col_count() {
                        return None;
                    }
                    return Some(IntrospectValue::Text(
                        self.em.inner.header(col).to_string(),
                    ));
                }
                // Per-cell text: `cell.<row>.<col>`.
                if let Some(rest) = path.strip_prefix("cell.") {
                    let (row_str, col_str) = rest.split_once('.')?;
                    let row: usize = row_str.parse().ok()?;
                    let col: usize = col_str.parse().ok()?;
                    if row >= self.row_count() || col >= self.col_count() {
                        return None;
                    }
                    return Some(IntrospectValue::Text(
                        self.em.inner.cell(row, col).to_string(),
                    ));
                }
                // Per-row interaction state: `state.<row>`.
                if let Some(row_str) = path.strip_prefix("state.") {
                    let row: usize = row_str.parse().ok()?;
                    if row >= self.row_count() {
                        return None;
                    }
                    return Some(IntrospectValue::Text(
                        self.state(row).as_name().to_string(),
                    ));
                }
                // Per-row selected bit: `selected.<row>`.
                if let Some(row_str) = path.strip_prefix("selected.") {
                    let row: usize = row_str.parse().ok()?;
                    if row >= self.row_count() {
                        return None;
                    }
                    return Some(IntrospectValue::Bool(self.is_selected(row)));
                }
                // R730 §5.40 — visual→data permutation: `order.<visual>`
                // is the data-row index painted at visual position.
                if let Some(v_str) = path.strip_prefix("order.") {
                    let visual: usize = v_str.parse().ok()?;
                    let order = self.order();
                    return order.get(visual).map(|&d| IntrospectValue::Int(int_of(d)));
                }
                None
            }
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            // R707 §5.40 — the 2-D roving active descendant is the
            // writable surface: AT `Focus` actions + the binding's
            // arrow-key roving land here (mirror of the date picker's
            // `focused_day` intervene). It moves the cursor only — no
            // activation, no `"selected"` intent. `Null` clears the row
            // (returns to "no active descendant"); `Int(r)` validates
            // against the row count.
            "focused_row" => match value {
                IntrospectValue::Int(i) => {
                    let row = self.resolve_row_intervene(i)?;
                    self.em.inner.set_focused_row(Some(row));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.set_focused_row(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "focused_col" => match value {
                IntrospectValue::Int(i) => {
                    let col = self.resolve_col_intervene(i)?;
                    self.em.inner.set_focused_col(col);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R735 §5.38 — per-row selection write, **only in
            // multi-select mode** (slot-assignment admin path: persisted
            // restore / programmatic toggle). Single-select keeps the
            // selection driven solely through the cell activate wire so
            // the mutual-exclusion invariant cannot be violated via the
            // RPC surface (mirror of `ListBoxExternal`'s `selected.<i>`).
            other if other.starts_with("selected.") => {
                if !self.is_multiselect() {
                    return Err(InterveneError::ReadOnly);
                }
                let row_str = other.strip_prefix("selected.").unwrap_or("");
                let row: usize =
                    row_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                if row >= self.row_count() {
                    return Err(InterveneError::OutOfRange);
                }
                match value {
                    IntrospectValue::Bool(b) => {
                        let mut next: Vec<usize> = self
                            .selected_rows()
                            .into_iter()
                            .filter(|&j| j != row)
                            .collect();
                        if b {
                            next.push(row);
                            next.sort_unstable();
                        }
                        self.em.inner.set_selected_rows(&next);
                        Ok(())
                    }
                    _ => Err(InterveneError::TypeMismatch),
                }
            }
            // The remaining slots are read-only: the selection is driven
            // through the cell activate wire (`invoke "send"`), never by
            // direct slot assignment; the data is immutable. This mirrors
            // the `RadioGroup` / `DatePicker` convention where the
            // commit-class paths fire the `"selected"` intent.
            "rows" | "cols" | "selected" | "selected_row" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Wire format: "<row>_<col>:<EventName>" drives one cell. A
            // click on the paint `"<tag>#<row>_<col>"` cell arrives here
            // as `"<row>_<col>:<EventName>"` (the R51.42 `'#'`-split
            // funnel). Returns the new selected row (or `Null`).
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    // R880.1 — the `split_send_payload` `:` grammar SSOT
                    // strips a held-modifier third segment (a hand-rolled
                    // split_once read "PointerUp:c" as the event name and
                    // a Ctrl+click on a cell/header was silently rejected).
                    let (key, event_name, _mods) =
                        crate::composite_tag::split_send_payload(s)
                            .ok_or(InvokeError::Rejected)?;
                    // R730 §5.40 / R777.1 — the `'#'`-split sub-key is
                    // decoded by the shared `GridSendKey` SSOT (the same
                    // grammar the paint producer encodes and
                    // `VirtualSelectExternal` decodes): a header click
                    // `"h<col>"` cycles the sort on `PointerUp` (the
                    // activate edge; other phases inert), a cell click
                    // `"<row>_<col>"` drives that cell's radio.
                    match crate::composite_tag::GridSendKey::parse(key)
                        .ok_or(InvokeError::Rejected)?
                    {
                        crate::composite_tag::GridSendKey::Header { col } => {
                            if col >= self.col_count() {
                                return Err(InvokeError::Rejected);
                            }
                            if event_name == PointerWireEvent::Up.as_wire_name() {
                                self.cycle_sort(col);
                            }
                            Ok(self.query("sort_dir").unwrap_or(IntrospectValue::Null))
                        }
                        crate::composite_tag::GridSendKey::Cell { row, col } => {
                            if row >= self.row_count() || col >= self.col_count() {
                                return Err(InvokeError::Rejected);
                            }
                            let ev = RadioEvent::from_name(event_name)
                                .ok_or(InvokeError::Rejected)?;
                            self.send_cell(row, col, ev);
                            // R735 §5.38 — single returns the (possibly new)
                            // `selected_row`; multi returns Null (no single
                            // row). AI clients follow up with `selected.<i>`
                            // / `selected_rows()` for the new full set.
                            Ok(if self.is_multiselect() {
                                IntrospectValue::Null
                            } else {
                                match self.selected_row() {
                                    Some(r) => IntrospectValue::Int(int_of(r)),
                                    None => IntrospectValue::Null,
                                }
                            })
                        }
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R730 §5.40 — direct sort cycle for AI clients: `invoke
            // "sort" Int(col)` cycles that column's sort the same way a
            // header click does, without synthesising a pointer event.
            // Returns the resulting `sort_dir` token.
            "sort" => match args {
                IntrospectValue::Int(i) => {
                    let col = usize::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    if col >= self.col_count() {
                        return Err(InvokeError::Rejected);
                    }
                    self.cycle_sort(col);
                    Ok(self.query("sort_dir").unwrap_or(IntrospectValue::Null))
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

    fn sample() -> Table {
        Table::new(
            vec!["Name".to_string(), "Round".to_string()],
            vec![
                vec!["Tabs".to_string(), "R690".to_string()],
                vec!["Menu".to_string(), "R691".to_string()],
                vec!["Table".to_string(), "R707".to_string()],
            ],
        )
    }

    /// Drive the full pointer click cycle on cell `(row, col)` — the
    /// sequence the `InputRouter` produces for a click.
    fn activate(t: &mut Table, row: usize, col: usize) {
        t.send_cell(row, col, RadioEvent::PointerEnter);
        t.send_cell(row, col, RadioEvent::PointerDown);
        t.send_cell(row, col, RadioEvent::PointerUp);
        t.send_cell(row, col, RadioEvent::PointerLeave);
    }

    #[test]
    fn dimensions_match_construction() {
        let t = sample();
        assert_eq!(t.row_count(), 3);
        assert_eq!(t.col_count(), 2);
        assert_eq!(t.header(0), "Name");
        assert_eq!(t.header(1), "Round");
        assert_eq!(t.cell(2, 0), "Table");
        assert_eq!(t.cell(2, 1), "R707");
        // Out-of-range access is a graceful empty string.
        assert_eq!(t.cell(9, 9), "");
        assert_eq!(t.header(9), "");
    }

    #[test]
    fn no_initial_selection_or_focus() {
        let t = sample();
        assert_eq!(t.selected_row(), None);
        assert_eq!(t.focused_row(), None);
        assert_eq!(t.focused_col(), 0);
    }

    #[test]
    fn activating_a_cell_selects_its_row_exclusively() {
        let mut t = sample();
        activate(&mut t, 1, 0);
        assert_eq!(t.selected_row(), Some(1));
        assert!(t.is_selected(1));
        assert!(!t.is_selected(0));
        assert!(!t.is_selected(2));
        // Switching rows deselects the previous one.
        activate(&mut t, 2, 1);
        assert_eq!(t.selected_row(), Some(2));
        assert!(t.is_selected(2));
        assert!(!t.is_selected(1));
    }

    #[test]
    fn activation_moves_active_descendant() {
        let mut t = sample();
        activate(&mut t, 2, 1);
        // WAI-ARIA "activation moves focus": the cursor lands on the
        // activated cell.
        assert_eq!(t.focused_row(), Some(2));
        assert_eq!(t.focused_col(), 1);
    }

    #[test]
    fn click_moves_cursor_within_already_selected_row() {
        let mut t = sample();
        activate(&mut t, 1, 0);
        assert_eq!(t.selected_row(), Some(1));
        assert_eq!((t.focused_row(), t.focused_col()), (Some(1), 0));
        // Click a different cell in the SAME (already-selected) row: the
        // selection is unchanged but the active descendant must follow.
        activate(&mut t, 1, 1);
        assert_eq!(t.selected_row(), Some(1), "selection unchanged");
        assert_eq!(
            (t.focused_row(), t.focused_col()),
            (Some(1), 1),
            "cursor follows the click within the selected row",
        );
    }

    #[test]
    fn roving_is_independent_of_selection() {
        let mut t = sample();
        t.set_focused_row(Some(1));
        t.set_focused_col(0);
        // Moving the cursor selects nothing.
        assert_eq!(t.focused_row(), Some(1));
        assert_eq!(t.selected_row(), None);
    }

    #[test]
    fn out_of_range_send_is_a_noop() {
        let mut t = sample();
        activate(&mut t, 9, 9);
        assert_eq!(t.selected_row(), None);
    }

    #[test]
    fn external_emits_selected_intent_on_change() {
        let mut ext = TableExternal::new(
            vec!["A".to_string()],
            vec![vec!["x".to_string()], vec!["y".to_string()]],
        );
        ext.send_cell(1, 0, RadioEvent::PointerEnter);
        ext.send_cell(1, 0, RadioEvent::PointerDown);
        ext.send_cell(1, 0, RadioEvent::PointerUp);
        let mut intents = Vec::new();
        ext.drain_intents(&mut |i| intents.push(i));
        assert!(
            intents.iter().any(|i| i.tag_str() == "selected"),
            "selection change emits a `selected` intent",
        );
    }

    #[test]
    fn introspect_query_round_trip() {
        let ext = TableExternal::new(
            vec!["Name".to_string(), "Round".to_string()],
            vec![vec!["Table".to_string(), "R707".to_string()]],
        );
        assert_eq!(ext.query("rows"), Some(IntrospectValue::Int(1)));
        assert_eq!(ext.query("cols"), Some(IntrospectValue::Int(2)));
        assert_eq!(ext.query("selected"), Some(IntrospectValue::Bool(false)));
        assert_eq!(ext.query("selected_row"), Some(IntrospectValue::Int(-1)));
        assert_eq!(ext.query("focused_row"), Some(IntrospectValue::Int(-1)));
        assert_eq!(ext.query("focused_col"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            ext.query("header.1"),
            Some(IntrospectValue::Text("Round".to_string())),
        );
        assert_eq!(
            ext.query("cell.0.0"),
            Some(IntrospectValue::Text("Table".to_string())),
        );
        // Out-of-range and unknown slots return `None` (the RPC layer
        // turns this into a clean error, never a silent default).
        assert_eq!(ext.query("cell.0.9"), None);
        assert_eq!(ext.query("no_such_slot"), None);
    }

    #[test]
    fn intervene_focus_then_query() {
        let mut ext = TableExternal::new(
            vec!["A".to_string(), "B".to_string()],
            vec![vec!["1".to_string(), "2".to_string()], vec![
                "3".to_string(),
                "4".to_string(),
            ]],
        );
        assert!(ext.intervene("focused_row", IntrospectValue::Int(1)).is_ok());
        assert!(ext.intervene("focused_col", IntrospectValue::Int(1)).is_ok());
        assert_eq!(ext.query("focused_row"), Some(IntrospectValue::Int(1)));
        assert_eq!(ext.query("focused_col"), Some(IntrospectValue::Int(1)));
        // Out-of-range rejected.
        assert_eq!(
            ext.intervene("focused_row", IntrospectValue::Int(9)),
            Err(InterveneError::OutOfRange),
        );
        // Read-only slot rejected.
        assert_eq!(
            ext.intervene("selected_row", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly),
        );
        // `Null` clears the active descendant row.
        assert!(ext.intervene("focused_row", IntrospectValue::Null).is_ok());
        assert_eq!(ext.query("focused_row"), Some(IntrospectValue::Int(-1)));
    }

    #[test]
    fn invoke_send_wire_selects_row() {
        let mut ext = TableExternal::new(
            vec!["A".to_string(), "B".to_string()],
            vec![vec!["1".to_string(), "2".to_string()], vec![
                "3".to_string(),
                "4".to_string(),
            ]],
        );
        for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
            let _ = ext.invoke("send", IntrospectValue::Text(format!("1_1:{ev}")));
        }
        assert_eq!(ext.selected_row(), Some(1));
        assert_eq!(ext.query("selected_row"), Some(IntrospectValue::Int(1)));
        // Malformed and out-of-range wires reject.
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text("nope".to_string())),
            Err(InvokeError::Rejected),
        );
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text("9_9:PointerUp".to_string())),
            Err(InvokeError::Rejected),
        );
    }

    // ----- R730 §5.40 sort -----

    fn sort_sample() -> Table {
        // A numeric "Round" column to exercise numeric-aware compare.
        Table::new(
            vec!["Name".to_string(), "Round".to_string()],
            vec![
                vec!["Menu".to_string(), "12".to_string()],
                vec!["Tabs".to_string(), "9".to_string()],
                vec!["Table".to_string(), "707".to_string()],
            ],
        )
    }

    #[test]
    fn boot_is_unsorted_identity_order() {
        let t = sort_sample();
        assert_eq!(t.sort_state(), None);
        assert_eq!(t.order(), vec![0, 1, 2], "unsorted = identity (R707 boot)");
    }

    #[test]
    fn cycle_sort_three_states_then_clears() {
        let mut t = sort_sample();
        t.cycle_sort(0);
        assert_eq!(t.sort_state(), Some((0, true)), "1st click ascending");
        t.cycle_sort(0);
        assert_eq!(t.sort_state(), Some((0, false)), "2nd click descending");
        t.cycle_sort(0);
        assert_eq!(t.sort_state(), None, "3rd click clears");
    }

    #[test]
    fn cycle_a_different_column_jumps_to_ascending() {
        let mut t = sort_sample();
        t.cycle_sort(0);
        t.cycle_sort(1);
        assert_eq!(t.sort_state(), Some((1, true)), "new column -> ascending");
    }

    #[test]
    fn order_sorts_text_column_lexicographically() {
        let mut t = sort_sample();
        t.cycle_sort(0); // Name ascending: Menu, Table, Tabs
        assert_eq!(t.order(), vec![0, 2, 1]);
        t.cycle_sort(0); // descending
        assert_eq!(t.order(), vec![1, 2, 0]);
    }

    #[test]
    fn order_sorts_numeric_column_numerically_not_lexically() {
        let mut t = sort_sample();
        t.cycle_sort(1); // Round ascending: 9 (row1), 12 (row0), 707 (row2)
        assert_eq!(t.order(), vec![1, 0, 2], "9 < 12 < 707 numerically");
        // A lexicographic sort would wrongly give "12" < "707" < "9".
    }

    #[test]
    fn selection_is_data_indexed_so_it_survives_sort() {
        // Select data row 0 ("Menu"), then sort by Name: it moves to
        // visual position 0 here, but the *data row* stays selected with
        // no remap — is_selected is by data index.
        let mut ext = TableExternal::new(
            vec!["Name".to_string(), "Round".to_string()],
            vec![
                vec!["Menu".to_string(), "12".to_string()],
                vec!["Tabs".to_string(), "9".to_string()],
                vec!["Table".to_string(), "707".to_string()],
            ],
        );
        for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
            let _ = ext.invoke("send", IntrospectValue::Text(format!("0_0:{ev}")));
        }
        assert_eq!(ext.selected_row(), Some(0));
        ext.cycle_sort(1); // sort by Round ascending -> order [1, 0, 2]
        assert_eq!(ext.order(), vec![1, 0, 2]);
        assert!(ext.is_selected(0), "data row 0 still selected after sort");
        assert_eq!(ext.selected_row(), Some(0), "no remap needed");
    }

    #[test]
    fn header_click_wire_cycles_sort_on_pointer_up_only() {
        let mut ext = TableExternal::new(
            vec!["Name".to_string(), "Round".to_string()],
            vec![vec!["Menu".to_string(), "12".to_string()]],
        );
        // Enter / Down are inert; only PointerUp activates the sort.
        let _ = ext.invoke("send", IntrospectValue::Text("h0:PointerEnter".to_string()));
        assert_eq!(ext.sort_state(), None, "hover does not sort");
        let r = ext.invoke("send", IntrospectValue::Text("h0:PointerUp".to_string()));
        assert_eq!(r, Ok(IntrospectValue::Text("ascending".to_string())));
        assert_eq!(ext.sort_state(), Some((0, true)));
    }

    #[test]
    fn invoke_sort_action_is_the_ai_path() {
        let mut ext = sort_sample_ext();
        assert_eq!(
            ext.invoke("sort", IntrospectValue::Int(1)),
            Ok(IntrospectValue::Text("ascending".to_string())),
        );
        assert_eq!(ext.query("sort_col"), Some(IntrospectValue::Int(1)));
        assert_eq!(ext.query("sort_dir"), Some(IntrospectValue::Text("ascending".to_string())));
        // Out-of-range column rejects.
        assert_eq!(
            ext.invoke("sort", IntrospectValue::Int(9)),
            Err(InvokeError::Rejected),
        );
    }

    #[test]
    fn order_query_reports_visual_to_data_mapping() {
        let mut ext = sort_sample_ext();
        let _ = ext.invoke("sort", IntrospectValue::Int(1)); // Round asc -> [1,0,2]
        assert_eq!(ext.query("order.0"), Some(IntrospectValue::Int(1)));
        assert_eq!(ext.query("order.1"), Some(IntrospectValue::Int(0)));
        assert_eq!(ext.query("order.2"), Some(IntrospectValue::Int(2)));
        assert_eq!(ext.query("order.9"), None, "out of range visual is None");
    }

    fn sort_sample_ext() -> TableExternal {
        TableExternal::new(
            vec!["Name".to_string(), "Round".to_string()],
            vec![
                vec!["Menu".to_string(), "12".to_string()],
                vec!["Tabs".to_string(), "9".to_string()],
                vec!["Table".to_string(), "707".to_string()],
            ],
        )
    }

    // ----- R735 §5.38 multi-select -----

    fn multi_sample() -> Table {
        Table::with_multiselect(
            vec!["Name".to_string(), "Round".to_string()],
            vec![
                vec!["Tabs".to_string(), "R690".to_string()],
                vec!["Menu".to_string(), "R691".to_string()],
                vec!["Table".to_string(), "R707".to_string()],
            ],
        )
    }

    #[test]
    fn multiselect_flag_is_set_by_constructor() {
        assert!(!sample().is_multiselect(), "new() is single-select");
        assert!(multi_sample().is_multiselect(), "with_multiselect() is multi");
    }

    #[test]
    fn multi_activation_toggles_only_the_addressed_row() {
        let mut t = multi_sample();
        activate(&mut t, 0, 0);
        activate(&mut t, 2, 1);
        // Both rows are selected — no sibling-deselect (the multi-select
        // contract that distinguishes it from R707 single-select).
        assert_eq!(t.selected_rows(), vec![0, 2]);
        assert!(t.is_selected(0) && t.is_selected(2));
        assert!(!t.is_selected(1));
        // Re-activating row 0 toggles it back off (siblings untouched).
        activate(&mut t, 0, 0);
        assert_eq!(t.selected_rows(), vec![2], "re-activate toggles row 0 off");
    }

    #[test]
    fn multi_keyboard_activate_toggles() {
        let mut t = multi_sample();
        t.send_cell(1, 0, RadioEvent::KeyboardActivate);
        assert_eq!(t.selected_rows(), vec![1]);
        t.send_cell(1, 0, RadioEvent::KeyboardActivate);
        assert_eq!(t.selected_rows(), Vec::<usize>::new(), "2nd KeyboardActivate toggles off");
    }

    #[test]
    fn set_selected_rows_replaces_the_whole_set() {
        let mut t = multi_sample();
        t.set_selected_rows(&[0, 2]);
        assert_eq!(t.selected_rows(), vec![0, 2]);
        t.set_selected_rows(&[1]);
        assert_eq!(t.selected_rows(), vec![1], "set replaces, not merges");
        t.set_selected_rows(&[]);
        assert_eq!(t.selected_rows(), Vec::<usize>::new(), "empty clears all");
    }

    #[test]
    #[should_panic(expected = "single-select-only")]
    fn selected_row_panics_in_multi_mode() {
        let _ = multi_sample().selected_row();
    }

    #[test]
    fn multi_external_emits_per_flip_intent() {
        let mut ext = TableExternal::with_multiselect(
            vec!["A".to_string()],
            vec![vec!["x".to_string()], vec!["y".to_string()]],
        );
        // Toggle row 0 on, then off: two flips => two intents.
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let _ = ext.invoke("send", IntrospectValue::Text(format!("0_0:{ev}")));
        }
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let _ = ext.invoke("send", IntrospectValue::Text(format!("0_0:{ev}")));
        }
        let mut intents = Vec::new();
        ext.drain_intents(&mut |i| intents.push(i));
        let selected: Vec<_> =
            intents.iter().filter(|i| i.tag_str() == "selected").collect();
        assert_eq!(selected.len(), 2, "toggle-on + toggle-off each emit an intent");
    }

    #[test]
    fn multi_external_query_surface() {
        let mut ext = TableExternal::with_multiselect(
            vec!["A".to_string(), "B".to_string()],
            vec![vec!["1".to_string(), "2".to_string()], vec![
                "3".to_string(),
                "4".to_string(),
            ]],
        );
        assert_eq!(ext.query("multiselect"), Some(IntrospectValue::Bool(true)));
        // No selection yet.
        assert_eq!(ext.query("selected"), Some(IntrospectValue::Bool(false)));
        // selected_row is the -1 sentinel in multi-mode (no single row).
        assert_eq!(ext.query("selected_row"), Some(IntrospectValue::Int(-1)));
        // Toggle row 1 on through the wire.
        for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
            let _ = ext.invoke("send", IntrospectValue::Text(format!("1_0:{ev}")));
        }
        assert_eq!(ext.query("selected"), Some(IntrospectValue::Bool(true)));
        assert_eq!(ext.query("selected.1"), Some(IntrospectValue::Bool(true)));
        assert_eq!(ext.query("selected.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(ext.selected_rows(), vec![1]);
        // invoke send returns Null in multi-mode (no single selected row).
        let r = ext.invoke("send", IntrospectValue::Text("0_0:PointerUp".to_string()));
        assert_eq!(r, Ok(IntrospectValue::Null));
    }

    #[test]
    fn r880_1_cell_click_with_modifier_segment_still_selects() {
        // "1_0:PointerUp:c" (the R781 modifier segment) must still drive
        // the cell — the pre-R880.1 hand-rolled split read "PointerUp:c"
        // as the event name and rejected the activation.
        let mut ext = TableExternal::with_multiselect(
            vec!["A".to_string()],
            vec![vec!["1".to_string()], vec!["2".to_string()]],
        );
        for ev in ["PointerEnter", "PointerDown", "PointerUp:c"] {
            let r = ext.invoke("send", IntrospectValue::Text(format!("1_0:{ev}")));
            assert_eq!(r, Ok(IntrospectValue::Null), "{ev} accepted");
        }
        assert_eq!(ext.selected_rows(), vec![1], "Ctrl+click still toggles the row");
    }

    #[test]
    fn multi_intervene_per_row_selected_bit() {
        let mut ext = TableExternal::with_multiselect(
            vec!["A".to_string()],
            vec![vec!["1".to_string()], vec!["2".to_string()], vec!["3".to_string()]],
        );
        // Per-row write is enabled in multi-mode (persisted restore path).
        assert!(ext.intervene("selected.0", IntrospectValue::Bool(true)).is_ok());
        assert!(ext.intervene("selected.2", IntrospectValue::Bool(true)).is_ok());
        assert_eq!(ext.selected_rows(), vec![0, 2]);
        // Clearing a bit removes only that row.
        assert!(ext.intervene("selected.0", IntrospectValue::Bool(false)).is_ok());
        assert_eq!(ext.selected_rows(), vec![2]);
        // Out-of-range row rejects.
        assert_eq!(
            ext.intervene("selected.9", IntrospectValue::Bool(true)),
            Err(InterveneError::OutOfRange),
        );
    }

    #[test]
    fn single_mode_rejects_per_row_selected_write() {
        let mut ext = TableExternal::new(
            vec!["A".to_string()],
            vec![vec!["1".to_string()], vec!["2".to_string()]],
        );
        // Single-select keeps `selected.<row>` read-only so the
        // mutual-exclusion invariant cannot be violated via the RPC surface.
        assert_eq!(
            ext.intervene("selected.0", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
        );
    }
}
