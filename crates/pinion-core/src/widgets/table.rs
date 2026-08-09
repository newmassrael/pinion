//! R707 §5.38 — `Table` widget: an interactive data grid with
//! single-row selection and 2-D keyboard navigation.
//!
//! A table presents tabular data as a grid of rows and columns (the
//! WAI-ARIA `grid` role with `row` / `columnheader` / `gridcell`
//! children — the **second consumer** of the R704 grid-role family,
//! and the first to exercise the `Row`
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
//! header row + body rows via `pinion_widget_paint::table` and queries
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

use crate::WidgetStateName;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership, int_of,
};
use crate::input::PointerWireEvent;
use crate::intent::Intent;
use crate::widgets::radio::{Radio, RadioEvent, RadioState};
use crate::widgets::selection;
use crate::widgets::{IntentEmitter, WidgetTransition};

/// R954 §5.38 §5.40 — re-exported where the eager [`Table`] names it.
///
/// R1563 moved the definition to [`cell_selection`](crate::widgets::cell_selection),
/// because a second coordinator
/// ([`VirtualSelect`](crate::widgets::virtual_select::VirtualSelect)) acquired the same
/// axis and "what does a press select" is **one** concept — the toolkit spells
/// it with one enum on abstract item view, and two types here would be two
/// vocabularies for one question that could then disagree about what `SelectItems`
/// means. The re-export keeps every R954 consumer's spelling.
pub use crate::widgets::cell_selection::SelectionBehavior;

/// Logical data grid with framework-owned single-row selection. See
/// module docs for the design rationale.
///
/// Reuses the [`Radio`] leaf statechart per row (the same "select 1 of
/// N" leaf [`RadioGroup`](crate::widgets::radio_group::RadioGroup) composes), so rows inherit the canonical
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
    /// R952 §5.38 §5.40 — the pinned corner of a **cell range selection** (`(row, col)`
    /// in **data** coords), or `None` when no rectangular cell selection is
    /// active. The opposite corner is the roving cursor (`focused_row` / `focused_col`), so the
    /// selected rectangle is the bounding box of `cell_anchor` and the cursor — the
    /// spreadsheet / the toolkit table view `SelectItems` model (cell range), distinct
    /// from the `row_radios` row selection (`SelectRows`). A plain cursor move collapses it
    /// (anchor follows the cursor → a single cell); a `Shift`-move extends it
    /// (anchor pinned). Data-indexed like every other selection here (R730),
    /// so a sort repositions the selected cells without remapping the model.
    cell_anchor: Option<(usize, usize)>,
    /// R954 §5.38 — what a pointer click selects (rows vs cells). `SelectRows`
    /// (via [`Self::new`] / [`Self::with_multiselect`]) washes the clicked
    /// cell's row; `SelectItems` (via [`Self::with_select_items`]) selects the
    /// clicked cell, `Shift`+click extending the rectangle. Immutable after
    /// construction (a grid is one behavior, never both — the R953 no-mode
    /// smell). Only the pointer `send` path reads it; the keyboard / RPC
    /// `select-cell` / `extend-cell` wire is behavior-agnostic.
    selection_behavior: SelectionBehavior,
}

/// R1223 §5.38 — replace embedded tab / newline in a cell with a space so a
/// [`Table::selected_tsv`] block's row / column shape always matches the
/// selection rectangle. TSV has no in-band delimiter escaping, so a raw tab
/// would split a column and a raw newline a row (silently disagreeing with the
/// selected bounds). Structure-preserving over content-faithful; the grid's
/// typical numeric / single-line cells never contain a delimiter, so this is a
/// no-op there.
fn tsv_sanitize(cell: &str) -> String {
    cell.replace(['\t', '\n'], " ")
}

/// R1372 §5.38 — serialize an already-extracted rectangle of cell display
/// strings to TSV (tab-separated columns, newline-separated rows), each cell
/// `tsv_sanitize`d so an embedded delimiter can never make the block's
/// row/column shape disagree with the rectangle. The shared clipboard-copy
/// serialization core of every grid's "copy the selection": the [`Table`]'s own
/// [`Table::selected_tsv`] (a data-ordered rectangle) and the `hello-data-grid`
/// editable grid's visible-ordered copy both funnel here — a divergence between
/// two grids' TSV shape would be a bug, not a style choice (the
/// `abstraction-needs-second-consumer` lift of the pure codec at the 2nd
/// consumer). `rows` is the caller's already-read rectangle, so each consumer
/// keeps its own data-order vs visible-order reading and this stays a pure
/// string function.
#[must_use]
pub fn rows_to_tsv(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|cell| tsv_sanitize(cell))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
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
            cell_anchor: None,
            selection_behavior: SelectionBehavior::SelectRows,
        }
    }

    /// R954 §5.38 — construct a **cell-range** (`SelectItems`) table: a pointer click
    /// selects the clicked cell (collapsing any rectangle) and `Shift`+click
    /// extends the rectangle from the anchor, the spreadsheet / the toolkit
    /// `SelectItems` model (`hello-cell-select`). The row [`Radio`] state is never washed; pointer selection
    /// drives [`Self::select_cell`] / [`Self::extend_cell`]. All rows start idle with no selection.
    #[must_use]
    pub fn with_select_items(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        let mut t = Self::new(headers, rows);
        t.selection_behavior = SelectionBehavior::SelectItems;
        t
    }

    /// R954 §5.38 — this table's pointer [`SelectionBehavior`] (`SelectRows`
    /// unless constructed via [`Self::with_select_items`]).
    #[must_use]
    pub fn selection_behavior(&self) -> SelectionBehavior {
        self.selection_behavior
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

    /// R952 §5.38 §5.40 — start a **cell range selection** at `(row, col)`:
    /// move the cursor there and pin the anchor to it, so the selection is
    /// the single cell (the spreadsheet click / plain-arrow model — a fresh
    /// selection that a later [`extend_cell`](Self::extend_cell) grows into a
    /// rectangle). Out-of-range `(row, col)` is a silent no-op (the model-path
    /// guard, mirroring [`send_cell`](Self::send_cell)).
    pub fn select_cell(&mut self, row: usize, col: usize) {
        if row >= self.rows.len() || col >= self.headers.len() {
            return;
        }
        self.focused_row = Some(row);
        self.focused_col = col;
        self.cell_anchor = Some((row, col));
    }

    /// R952 §5.38 §5.40 — extend the cell range selection to `(row, col)`:
    /// move the cursor there but keep the pinned anchor, so the selection is
    /// the bounding rectangle of the anchor and the new cursor (the
    /// `Shift`-arrow / `Shift`-click model). With no anchor yet (a `Shift`
    /// move before any selection), the current cursor becomes the anchor — or
    /// `(row, col)` itself when there is no cursor either — so the first
    /// extension is a single cell that subsequent extensions grow. Out-of-range
    /// `(row, col)` is a silent no-op.
    pub fn extend_cell(&mut self, row: usize, col: usize) {
        if row >= self.rows.len() || col >= self.headers.len() {
            return;
        }
        if self.cell_anchor.is_none() {
            self.cell_anchor = Some(
                self.focused_row
                    .map_or((row, col), |r| (r, self.focused_col)),
            );
        }
        self.focused_row = Some(row);
        self.focused_col = col;
    }

    /// R952 §5.38 §5.40 — the selected cell rectangle as
    /// `(row0, col0, row1, col1)` (inclusive, normalized so `row0 <= row1`
    /// and `col0 <= col1`), or `None` when no cell selection is active (no
    /// anchor, or no cursor row to pair it with). The bounding box of the
    /// anchor and the roving cursor — the AI-first read of "which cells are
    /// selected", a rectangle rather than a per-cell bitmap because the model
    /// is anchor+extent.
    #[must_use]
    pub fn cell_selection_bounds(&self) -> Option<(usize, usize, usize, usize)> {
        let (ar, ac) = self.cell_anchor?;
        let fr = self.focused_row?;
        let fc = self.focused_col;
        Some((ar.min(fr), ac.min(fc), ar.max(fr), ac.max(fc)))
    }

    /// R952 §5.38 §5.40 — the number of cells in the selected rectangle
    /// (`0` when no cell selection is active). The `(rows × cols)` area of
    /// [`cell_selection_bounds`](Self::cell_selection_bounds).
    #[must_use]
    pub fn cell_selection_count(&self) -> usize {
        self.cell_selection_bounds()
            .map_or(0, |(r0, c0, r1, c1)| (r1 - r0 + 1) * (c1 - c0 + 1))
    }

    /// R1222 §5.38 — the selected cell rectangle serialized as TSV
    /// (tab-separated columns, newline-separated rows) — the spreadsheet
    /// clipboard form (an spreadsheet / Sheets paste lands as a cell block).
    /// `None` when no cell range is selected. Row-major over the data-ordered
    /// [`cell_selection_bounds`](Self::cell_selection_bounds) reading each [`cell`](Self::cell): the
    /// AI-first "copy the selection" read (§2 #2) AND the payload a Ctrl+C
    /// writes to the platform clipboard.
    ///
    /// R1223 — this is DATA-ORDERED (it does not re-map through a sort); it
    /// mirrors the data-indexed selection *rectangle*, which under an active
    /// sort is NOT what the widget paints as selected (the paint suppresses
    /// the cell-range overlay while sorted). Any embedded tab / newline in a
    /// cell is replaced with a space (`tsv_sanitize`) so the TSV block's row/column shape
    /// ALWAYS matches the selection rectangle — a general-purpose grid may
    /// hold free-text cells, and raw joining would silently split a row/column
    /// (structure-preserving over content-faithful; full spreadsheet-style
    /// quoting is a later enhancement if a delimiter-bearing grid needs
    /// faithful content).
    #[must_use]
    pub fn selected_tsv(&self) -> Option<String> {
        let (r0, c0, r1, c1) = self.cell_selection_bounds()?;
        // R1372 — the rectangle is read here in DATA order (each caller owns its
        // reading), then the shared [`rows_to_tsv`] codec sanitizes + joins it.
        let rows: Vec<Vec<String>> = (r0..=r1)
            .map(|r| (c0..=c1).map(|c| self.cell(r, c).to_string()).collect())
            .collect();
        Some(rows_to_tsv(&rows))
    }

    /// R952 §5.38 §5.40 — drop the cell range selection (clear the anchor).
    /// The roving cursor is untouched — clearing a selection leaves the
    /// active descendant where it was (the cell stays navigable / editable).
    pub fn clear_cell_selection(&mut self) {
        self.cell_anchor = None;
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
/// `f64` compare numerically (so `"9" < "12"`); otherwise the spreadsheet
/// two-class order — numbers sort before non-numbers, non-numbers compare
/// lexicographically (so `"Active" < "Done"`). A data grid that sorted a
/// numeric column lexicographically (`"12" < "9"`) would be visibly wrong.
///
/// R886.1 — the comparison is a **total order** (`slice::sort_by` panics
/// on detected non-total comparators since Rust 1.81). The pre-R886.1
/// shape fell back to lexicographic whenever EITHER side failed to parse,
/// which is cyclic on mixed cells (`"9" < "10"` numeric, `"10" < "1z"`
/// lex, `"1z" < "9"` lex). Read-only consumers had curated homogeneous
/// columns so the cycle was unreachable; the R886 editable grid lets a
/// user type arbitrary text into a sortable column, making totality a
/// correctness requirement. `f64::total_cmp` keeps a literal `"NaN"`
/// cell from re-breaking it.
///
/// (R778 `pub` — the numeric comparator is the SSOT for both the eager
/// [`Table::order`] and the virtualized data-grid sort coordinator
/// [`GridSortState`](crate::widgets::grid_sort::GridSortState); they must
/// agree on numeric awareness, so the comparison lives in one place.)
/// R952 §5.38 — parse a `"row,col"` cell-address wire arg (the
/// `select-cell` / `extend-cell` invoke shape) into `(row, col)`. `None`
/// when the arg is not exactly two comma-separated unsigned integers — the
/// caller maps that to `TypeMismatch` (the index-range check is the caller's,
/// against the live row / column count). R1372 — `pub`, so the `hello-data-grid`
/// editable grid (a 2nd consumer of the same cell-selection wire, driving its
/// own bespoke coordinator) parses the identical arg through this one SSOT
/// rather than a divergent copy.
#[must_use]
pub fn parse_row_col(s: &str) -> Option<(usize, usize)> {
    let (r, c) = s.split_once(',')?;
    Some((r.trim().parse().ok()?, c.trim().parse().ok()?))
}

#[must_use]
pub fn cell_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.total_cmp(&y),
        (Ok(_), Err(_)) => core::cmp::Ordering::Less,
        (Err(_), Ok(_)) => core::cmp::Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

/// R955.1 §5.40 — **deserialize peer** of the [`TableExternal`] `focused_row`
/// query slot: the roving active-descendant row (`-1` / absent → `None`).
///
/// Lifted from the three table-family grid bindings (`hello-table`,
/// `hello-table-multi`, `hello-cell-select`) where this and its siblings were
/// byte-identical hand-decoders. The decode lives next to its `query` encode
/// (the R743.1 decode-of-encode rule, the [`read_selected`] sibling), so a
/// slot rename can't silently break three copies.
///
/// [`read_selected`]: crate::widgets::virtual_select::read_selected
#[must_use]
pub fn read_focused_row(intro: &dyn ExternalIntrospect) -> Option<usize> {
    match intro.query("focused_row") {
        Some(IntrospectValue::Int(r)) if r >= 0 => usize::try_from(r).ok(),
        _ => None,
    }
}

/// R955.1 §5.40 — deserialize peer of the `focused_col` slot: the roving
/// active-descendant column (defaults to `0`). See [`read_focused_row`].
#[must_use]
pub fn read_focused_col(intro: &dyn ExternalIntrospect) -> usize {
    match intro.query("focused_col") {
        Some(IntrospectValue::Int(c)) if c >= 0 => usize::try_from(c).unwrap_or(0),
        _ => 0,
    }
}

/// R955.1 §5.40 — deserialize peer of the `rows` slot: the row count (`0`
/// when unavailable). See [`read_focused_row`].
#[must_use]
pub fn read_rows(intro: &dyn ExternalIntrospect) -> usize {
    match intro.query("rows") {
        Some(IntrospectValue::Int(r)) => usize::try_from(r).unwrap_or(0),
        _ => 0,
    }
}

/// R955.1 §5.40 — deserialize peer of the `cols` slot: the column count (`0`
/// when unavailable). See [`read_focused_row`].
#[must_use]
pub fn read_cols(intro: &dyn ExternalIntrospect) -> usize {
    match intro.query("cols") {
        Some(IntrospectValue::Int(c)) => usize::try_from(c).unwrap_or(0),
        _ => 0,
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
        if ascending { ord } else { ord.reverse() }
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

    fn detect(before: Self::Snapshot, _event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
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
        Self {
            em: IntentEmitter::new(Table::new(headers, rows)),
        }
    }

    /// R735 §5.38 — construct a **multi-select** table (the WAI-ARIA
    /// `aria-multiselectable="true"` model). Activating a row toggles only
    /// that row; siblings are untouched.
    #[must_use]
    pub fn with_multiselect(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self {
            em: IntentEmitter::new(Table::with_multiselect(headers, rows)),
        }
    }

    /// R954 §5.38 — construct a **cell-range** (`SelectItems`) table: a
    /// pointer click selects the clicked cell and `Shift`+click extends the
    /// rectangle, the spreadsheet model (`hello-cell-select`). The row
    /// selection is never washed.
    #[must_use]
    pub fn with_select_items(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self {
            em: IntentEmitter::new(Table::with_select_items(headers, rows)),
        }
    }

    /// R735 §5.38 — `true` if this table was constructed via
    /// [`Self::with_multiselect`].
    #[must_use]
    pub fn is_multiselect(&self) -> bool {
        self.em.inner.is_multiselect()
    }

    /// R954 §5.38 — this table's pointer [`SelectionBehavior`] (`SelectRows`
    /// unless constructed via [`Self::with_select_items`]).
    #[must_use]
    pub fn selection_behavior(&self) -> SelectionBehavior {
        self.em.inner.selection_behavior()
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

    /// R952 §5.38 — the selected cell rectangle `(row0, col0, row1, col1)`
    /// (inclusive, data coords), or `None` when no cell selection is active
    /// ([`Table::cell_selection_bounds`]).
    #[must_use]
    pub fn cell_selection_bounds(&self) -> Option<(usize, usize, usize, usize)> {
        self.em.inner.cell_selection_bounds()
    }

    /// R952 §5.38 — the number of cells in the selected rectangle
    /// ([`Table::cell_selection_count`]).
    #[must_use]
    pub fn cell_selection_count(&self) -> usize {
        self.em.inner.cell_selection_count()
    }

    /// R1222 §5.38 — the selected cell rectangle as TSV
    /// ([`Table::selected_tsv`]); the `cell_selection_tsv` query and a Ctrl+C
    /// clipboard copy both read this.
    #[must_use]
    pub fn selected_tsv(&self) -> Option<String> {
        self.em.inner.selected_tsv()
    }

    /// R1564 §5.15 — the `send` arm's body, lifted out of
    /// [`invoke`](ExternalIntrospect::invoke) so that match stays inside the
    /// workspace line budget. Same split R952 made for `invoke_cell_select`;
    /// this round's reasons pushed the arm over it.
    ///
    /// # Errors
    ///
    /// [`InvokeError::Rejected`] naming the malformed payload, the
    /// unaddressable target or the axis this surface does not have.
    fn dispatch_send(&mut self, s: &str) -> Result<IntrospectValue, InvokeError> {
        // R880.1 — the `split_send_payload` `:` grammar SSOT
        // strips a held-modifier third segment (a hand-rolled
        // split_once read "PointerUp:c" as the event name and
        // a Ctrl+click on a cell/header was silently rejected).
        let crate::composite_tag::SendPayload {
            key,
            event: event_name,
            modifiers: mods,
            ..
        } = crate::composite_tag::require_send_payload("table.send", s)?;
        // R730 §5.40 / R777.1 — the `'#'`-split sub-key is
        // decoded by the shared `GridSendKey` SSOT (the same
        // grammar the paint producer encodes and
        // `VirtualSelectExternal` decodes): a header click
        // `"h<col>"` cycles the sort on `PointerUp` (the
        // activate edge; other phases inert), a cell click
        // `"<row>_<col>"` drives that cell's radio.
        match crate::composite_tag::GridSendKey::parse(key).ok_or_else(|| {
            InvokeError::rejected(format!(
                "table.send: {key:?} is not a grid address \
                     (expected \"h<col>\", \"r<row>\", \"<row>_<col>\" or \"c\")"
            ))
        })? {
            crate::composite_tag::GridSendKey::Header { col } => {
                if col >= self.col_count() {
                    return Err(InvokeError::rejected(format!(
                        "table.send: no column {col} in this table (it has {})",
                        self.col_count()
                    )));
                }
                if event_name == PointerWireEvent::Up.as_wire_name() {
                    self.cycle_sort(col);
                }
                Ok(self.query("sort_dir").unwrap_or(IntrospectValue::Null))
            }
            crate::composite_tag::GridSendKey::Cell { row, col } => {
                if row >= self.row_count() || col >= self.col_count() {
                    return Err(InvokeError::rejected(format!(
                        "table.send: no cell ({row}, {col}) in this table \
                             (it is {} x {})",
                        self.row_count(),
                        self.col_count()
                    )));
                }
                // R954 §5.38 — a `SelectItems` grid selects the
                // clicked *cell* on the activate edge (PointerUp):
                // a plain click collapses the rectangle to it, a
                // `Shift`-click extends it from the anchor. Other
                // pointer phases are inert — no row washing (the
                // R953 SelectRows-on-click smell). The keyboard /
                // RPC `select-cell` wire is behavior-agnostic.
                if self.selection_behavior() == SelectionBehavior::SelectItems {
                    if event_name == PointerWireEvent::Up.as_wire_name() {
                        if mods.shift {
                            self.em.inner.extend_cell(row, col);
                        } else {
                            self.em.inner.select_cell(row, col);
                        }
                        // R955.1 — setter-returns-read-outcome: echo
                        // the new cell rectangle so a pointer client
                        // learns the selection in one round-trip (the
                        // SelectRows branch's `selected_row` analog).
                        return Ok(self
                            .query("cell_selection")
                            .unwrap_or(IntrospectValue::Null));
                    }
                    return Ok(IntrospectValue::Null);
                }
                let ev = crate::widget_core::require_event::<RadioEvent>("table", event_name)?;
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
            // R1562 — a row-header press (`"r<row>"`) selects the
            // whole line, on the surface that paints a vertical band
            // as much as on the virtualized one. Reaching the SAME
            // model verbs a cell press reaches is the point: one
            // behaviour, addressed two ways.
            crate::composite_tag::GridSendKey::RowHeader { row } => {
                if row >= self.row_count() {
                    return Err(InvokeError::rejected(format!(
                        "table.send: no row {row} in this table (it has {})",
                        self.row_count()
                    )));
                }
                self.send_row_header(row, event_name)
            }
            // R1562 — the corner (`"c"`) addresses the whole model.
            crate::composite_tag::GridSendKey::Corner => {
                if event_name == PointerWireEvent::Up.as_wire_name() {
                    self.toggle_all_rows();
                }
                Ok(self.selection_outcome())
            }
            // R892 — the eager `Table` has no group axis; a
            // group-header key is not addressable here.
            //
            // R1555 — nor an editing axis. `GridEditState` is wired
            // by the virtualized grid path, so the eager table paints
            // no editor and therefore has no step affordance to
            // address. Rejected rather than ignored, so a binding
            // that sends one learns it went nowhere.
            crate::composite_tag::GridSendKey::Group { .. } => Err(InvokeError::rejected(
                "table.send: the eager table has no group axis, \
                         so a group-header address reaches nothing",
            )),
            crate::composite_tag::GridSendKey::EditorStep { .. } => Err(InvokeError::rejected(
                "table.send: the eager table paints no cell editor, \
                         so it has no step affordance to address",
            )),
        }
    }

    /// R952 §5.38 — the cell range selection `invoke` actions, split out of
    /// [`invoke`](ExternalIntrospect::invoke) for SRP (and to keep that
    /// dispatch under the line ceiling, like the find / fold helpers
    /// elsewhere). `select-cell` / `extend-cell` take a `Text` `"row,col"`
    /// (data coords) and start / grow the rectangle (a no-op + `Bool(false)`
    /// when out of range); `clear-cell-selection` takes `Null` and drops it.
    /// All return `Bool(true)` on success. `TypeMismatch` on a malformed arg.
    fn invoke_cell_select(
        &mut self,
        path: &str,
        args: &IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path == "clear-cell-selection" {
            return match args {
                IntrospectValue::Null => {
                    self.em.inner.clear_cell_selection();
                    Ok(IntrospectValue::Bool(true))
                }
                _ => Err(InvokeError::TypeMismatch),
            };
        }
        let IntrospectValue::Text(s) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let Some((row, col)) = parse_row_col(s) else {
            return Err(InvokeError::TypeMismatch);
        };
        if row >= self.row_count() || col >= self.col_count() {
            return Ok(IntrospectValue::Bool(false));
        }
        if path == "extend-cell" {
            self.em.inner.extend_cell(row, col);
        } else {
            self.em.inner.select_cell(row, col);
        }
        Ok(IntrospectValue::Bool(true))
    }

    /// R1562 §5.27 §5.40 — a **row-header** press: the toolkit header view's
    /// `sectionPressed` on a `Vertical` header, which selects the line the
    /// section names.
    ///
    /// Whichever [`SelectionBehavior`] is in force, it reaches the *same model verbs* a press
    /// in the row's body reaches — the row [`Radio`] arc under `SelectRows`, the cell
    /// rectangle under `SelectItems` — rather than a second selection implementation
    /// beside them. The toolkit keeps two (`mousePressEvent` → `selectRow`, next to `mousePressEvent` → `selectionCommand`),
    /// which is why a toolkit header press and a toolkit cell press can be
    /// made to disagree.
    ///
    /// Under `SelectItems` the line is the row's **whole rectangle** — anchor at
    /// its first column, cursor at its last — because a header names a row and
    /// not a cell of it. Under `SelectRows` the arc is forwarded to column 0, so
    /// the row's statechart sees the press exactly as it would from a cell (in
    /// multi-select that means the same toggle, which is that surface's
    /// documented cell behaviour and therefore the band's too).
    fn send_row_header(
        &mut self,
        row: usize,
        event_name: &str,
    ) -> Result<IntrospectValue, InvokeError> {
        if self.selection_behavior() == SelectionBehavior::SelectItems {
            if event_name == PointerWireEvent::Up.as_wire_name() {
                let last = self.col_count().saturating_sub(1);
                self.em.inner.select_cell(row, 0);
                self.em.inner.extend_cell(row, last);
                return Ok(self
                    .query("cell_selection")
                    .unwrap_or(IntrospectValue::Null));
            }
            return Ok(IntrospectValue::Null);
        }
        let ev = crate::widget_core::require_event::<RadioEvent>("table", event_name)?;
        self.send_cell(row, 0, ev);
        Ok(self.selection_outcome())
    }

    /// R1562 §5.27 §5.40 — the corner control's action (the toolkit's
    /// table corner button): select every row, or clear when every row is
    /// already selected.
    ///
    /// Driven through the ordinary activate edge per row rather than through
    /// [`set_selected_rows`](Self::set_selected_rows), which is the **silent**
    /// admin / restore channel: a control the user pressed must reach the §5.20
    /// intent channel, so AI and automation observe a select-all the same way
    /// they observe every other selection change. That costs one dispatch per
    /// row it flips, which this surface can afford by construction — the eager
    /// table builds every row anyway (the virtualized grid's corner goes through
    /// [`VirtualSelect::toggle_all`](crate::widgets::virtual_select::VirtualSelect::toggle_all),
    /// which is O(1) because R1561 holds the selection as runs).
    ///
    /// A single-select `SelectRows` table cannot hold every row, so this is the no-op
    /// `selectAll` is under `SingleSelection`.
    fn toggle_all_rows(&mut self) {
        let rows = self.row_count();
        if self.selection_behavior() == SelectionBehavior::SelectItems {
            let full = self.cell_selection_bounds()
                == Some((
                    0,
                    0,
                    rows.saturating_sub(1),
                    self.col_count().saturating_sub(1),
                ));
            if full || rows == 0 {
                self.em.inner.clear_cell_selection();
            } else {
                self.em.inner.select_cell(0, 0);
                self.em
                    .inner
                    .extend_cell(rows - 1, self.col_count().saturating_sub(1));
            }
            return;
        }
        if !self.is_multiselect() {
            return;
        }
        let want = self.selected_rows().len() < rows;
        for row in 0..rows {
            if self.em.inner.is_selected(row) != want {
                self.send_cell(row, 0, RadioEvent::KeyboardActivate);
            }
        }
    }

    /// R1562 — the outcome a row-addressed `send` echoes: the new
    /// `selected_row` in a single-select table, `Null` in a multi-select one
    /// (where no single row is the answer). The rule the cell arm already
    /// applies, named so the band's two arms cannot state it differently.
    fn selection_outcome(&self) -> IntrospectValue {
        if self.is_multiselect() {
            IntrospectValue::Null
        } else {
            match self.selected_row() {
                Some(r) => IntrospectValue::Int(int_of(r)),
                None => IntrospectValue::Null,
            }
        }
    }

    /// Validate an intervene `row` index against the row count.
    ///
    /// R1565 — routed through [`wire::resolve_index`](crate::widgets::wire),
    /// which is what these two were open-coded copies of. Reaching for the
    /// sentence is what made that visible: three copies of a bound could not
    /// disagree while they all answered one payload-free variant, and three
    /// copies composing a sentence can.
    fn resolve_row_intervene(&self, i: i64) -> Result<usize, InterveneError> {
        crate::widgets::wire::resolve_index("row", i, self.row_count())
    }

    /// Validate an intervene `col` index against the column count.
    fn resolve_col_intervene(&self, i: i64) -> Result<usize, InterveneError> {
        crate::widgets::wire::resolve_index("column", i, self.col_count())
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
            .field("multiselect", &self.is_multiselect())
            .field("selection_behavior", &self.selection_behavior());
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
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("rows", "int"),
                    SchemaField::new("cols", "int"),
                    // R735 §5.38 — `multiselect` (bool) is read-only mode
                    // metadata. `selected.<row>` becomes write-enabled in
                    // multi-select mode (the canonical per-row admin path);
                    // `selected_row` returns `null` in multi-mode and rejects
                    // intervene.
                    SchemaField::new("multiselect", "bool"),
                    // R955.1 §5.38 — what a click selects (rows vs cells); the AI-first
                    // peer of `with_select_items` (the `multiselect` introspection
                    // sibling): `"rows"` (SelectRows) or `"items"` (SelectItems).
                    SchemaField::new("selection_behavior", "string"),
                    SchemaField::new("selected", "bool"),
                    SchemaField::new("selected_row", "int"),
                    SchemaField::new("focused_row", "int"),
                    SchemaField::new("focused_col", "int"),
                    SchemaField::parametric(
                        "header.<col>",
                        "string",
                        const { &[SchemaArg::index("col", "cols")] },
                    ),
                    SchemaField::parametric(
                        "cell.<row>.<col>",
                        "string",
                        const {
                            &[
                                SchemaArg::index("row", "rows"),
                                SchemaArg::index("col", "cols"),
                            ]
                        },
                    ),
                    SchemaField::parametric(
                        "state.<row>",
                        "string",
                        const { &[SchemaArg::index("row", "rows")] },
                    ),
                    SchemaField::parametric(
                        "selected.<row>",
                        "bool",
                        const { &[SchemaArg::index("row", "rows")] },
                    ),
                    SchemaField::action("send", "string"),
                    // R730 §5.40 — sort surface. `sort_col` is the sort key column
                    // (`-1` when unsorted); `sort_dir` is "none"/"ascending"/
                    // "descending"; `order.<visual>` is the data-row index painted
                    // at visual position `<visual>`; `sort` (invoke) cycles a
                    // column's sort the way a header click does.
                    SchemaField::new("sort_col", "int"),
                    SchemaField::new("sort_dir", "string"),
                    SchemaField::parametric(
                        "order.<visual>",
                        "int",
                        const { &[SchemaArg::index("visual", "rows")] },
                    ),
                    SchemaField::action("sort", "int"),
                    // R952 §5.38 — cell range selection (the spreadsheet / the
                    // toolkit `SelectItems` model, distinct from the `selected.<row>` row selection).
                    // `cell_selection` reads the selected rectangle as
                    // "row0,col0,row1,col1" (data coords, inclusive) or `null`;
                    // `cell_selection_count` is its cell area. `select-cell` (arg "row,col") starts a
                    // single-cell selection at the cursor; `extend-cell` (arg "row,col")
                    // grows the rectangle to that cell; `clear-cell-selection` (arg `null`) drops
                    // it. The AI-first peer of click / Shift+click /
                    // Shift+Arrow.
                    SchemaField::new("cell_selection", "string"),
                    SchemaField::new("cell_selection_count", "int"),
                    SchemaField::new("cell_selection_tsv", "string"),
                    SchemaField::action("select-cell", "boolean"),
                    SchemaField::action("extend-cell", "boolean"),
                    SchemaField::action("clear-cell-selection", "boolean"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "rows" => Some(IntrospectValue::Int(int_of(self.row_count()))),
            "cols" => Some(IntrospectValue::Int(int_of(self.col_count()))),
            // R735 §5.38 — mode metadata so a mode-agnostic introspector
            // picks the right path (`selected.<i>` per-row vs
            // `selected_row` single).
            "multiselect" => Some(IntrospectValue::Bool(self.is_multiselect())),
            // R955.1 §5.38 — the pointer SelectionBehavior as a wire token, so
            // an AI client reads the grid's mode (rows vs cells) the same way
            // it reads `multiselect`.
            "selection_behavior" => Some(IntrospectValue::Text(
                match self.selection_behavior() {
                    SelectionBehavior::SelectRows => "rows",
                    SelectionBehavior::SelectItems => "items",
                    // R1563 — unreachable for an eager `Table`: its only
                    // setter is `with_select_items`, so the column arm is a
                    // value this coordinator's constructors cannot produce.
                    // Answered rather than `unreachable!` because a wire slot
                    // that panics is worse than one that tells the truth.
                    SelectionBehavior::SelectColumns => "columns",
                }
                .to_string(),
            )),
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
            "focused_row" => Some(IntrospectValue::Int(self.focused_row().map_or(-1, int_of))),
            "focused_col" => Some(IntrospectValue::Int(int_of(self.focused_col()))),
            // R952 §5.38 — the selected cell rectangle as "row0,col0,row1,col1"
            // (data coords, inclusive), or `Null` when no cell selection is
            // active. Text-encoded (the file's `sort_dir` / `send` idiom; no
            // serde_json dependency), parsed by the AI client on the comma.
            "cell_selection" => Some(match self.cell_selection_bounds() {
                Some((r0, c0, r1, c1)) => IntrospectValue::Text(format!("{r0},{c0},{r1},{c1}")),
                None => IntrospectValue::Null,
            }),
            "cell_selection_count" => {
                Some(IntrospectValue::Int(int_of(self.cell_selection_count())))
            }
            // R1222 §5.38 — the selected cell rectangle as TSV (the AI-first
            // "copy the selection" read; `Null` when nothing is selected).
            "cell_selection_tsv" => Some(match self.selected_tsv() {
                Some(tsv) => IntrospectValue::Text(tsv),
                None => IntrospectValue::Null,
            }),
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
                    return Some(IntrospectValue::Text(self.em.inner.header(col).to_string()));
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
                    return Some(IntrospectValue::Text(self.state(row).as_name().to_string()));
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

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
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
                let row: usize = row_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                if row >= self.row_count() {
                    return Err(InterveneError::out_of_range(format!(
                        "no row {row} here (it has {}, so 0..{})",
                        self.row_count(),
                        self.row_count()
                    )));
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
            "rows" | "cols" | "selected" | "selected_row" => Err(InterveneError::ReadOnly),
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
                IntrospectValue::Text(ref s) => self.dispatch_send(s),
                _ => Err(InvokeError::TypeMismatch),
            },
            // R730 §5.40 — direct sort cycle for AI clients: `invoke
            // "sort" Int(col)` cycles that column's sort the same way a
            // header click does, without synthesising a pointer event.
            // Returns the resulting `sort_dir` token.
            "sort" => match args {
                IntrospectValue::Int(i) => {
                    let col = usize::try_from(i).map_err(|_| {
                        InvokeError::rejected(format!("table.sort: {i} is not a column index"))
                    })?;
                    if col >= self.col_count() {
                        return Err(InvokeError::rejected(format!(
                            "table.sort: no column {col} in this table (it has {})",
                            self.col_count()
                        )));
                    }
                    self.cycle_sort(col);
                    Ok(self.query("sort_dir").unwrap_or(IntrospectValue::Null))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R952 §5.38 — cell range selection actions, split out (SRP, line
            // ceiling) like the other dispatch helpers.
            "select-cell" | "extend-cell" | "clear-cell-selection" => {
                self.invoke_cell_select(path, &args)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::assert_out_of_range_saying;
    use crate::test_fixtures::assert_refused_saying;

    #[test]
    fn r886_1_cell_cmp_is_a_total_order_on_mixed_cells() {
        // The pre-R886.1 cycle: "9" < "10" (numeric), "10" < "1z" (lex),
        // "1z" < "9" (lex) — sort_by panics on such comparators since
        // Rust 1.81. The total shape orders numbers before non-numbers,
        // so the cycle is broken and a full sort cannot panic.
        use core::cmp::Ordering;
        assert_eq!(cell_cmp("9", "10"), Ordering::Less, "numeric pair");
        assert_eq!(cell_cmp("10", "1z"), Ordering::Less, "number before text");
        assert_eq!(cell_cmp("9", "1z"), Ordering::Less, "cycle arm inverted");
        assert_eq!(cell_cmp("1z", "9"), Ordering::Greater);
        // Full sort over the adversarial mix must not panic and must
        // land numbers (numeric order) ahead of text (lexicographic).
        let mut cells = vec!["1z", "10", "abc", "9", "NaN", "2"];
        cells.sort_by(|a, b| cell_cmp(a, b));
        assert_eq!(cells, ["2", "9", "10", "NaN", "1z", "abc"]);
    }

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
            vec![
                vec!["1".to_string(), "2".to_string()],
                vec!["3".to_string(), "4".to_string()],
            ],
        );
        assert!(
            ext.intervene("focused_row", IntrospectValue::Int(1))
                .is_ok()
        );
        assert!(
            ext.intervene("focused_col", IntrospectValue::Int(1))
                .is_ok()
        );
        assert_eq!(ext.query("focused_row"), Some(IntrospectValue::Int(1)));
        assert_eq!(ext.query("focused_col"), Some(IntrospectValue::Int(1)));
        // Out-of-range rejected.
        assert_out_of_range_saying(
            &ext.intervene("focused_row", IntrospectValue::Int(9)),
            "no row 9 here",
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
            vec![
                vec!["1".to_string(), "2".to_string()],
                vec!["3".to_string(), "4".to_string()],
            ],
        );
        for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
            let _ = ext.invoke("send", IntrospectValue::Text(format!("1_1:{ev}")));
        }
        assert_eq!(ext.selected_row(), Some(1));
        assert_eq!(ext.query("selected_row"), Some(IntrospectValue::Int(1)));
        // Malformed and out-of-range wires reject.
        assert_refused_saying(
            &ext.invoke("send", IntrospectValue::Text("nope".to_string())),
            "malformed send payload \"nope\"",
        );
        assert_refused_saying(
            &ext.invoke("send", IntrospectValue::Text("9_9:PointerUp".to_string())),
            "no cell (9, 9) in this table (it is 2 x 2)",
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
        assert_eq!(
            ext.query("sort_dir"),
            Some(IntrospectValue::Text("ascending".to_string()))
        );
        // Out-of-range column rejects.
        assert_refused_saying(
            &ext.invoke("sort", IntrospectValue::Int(9)),
            "no column 9 in this table",
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
        assert!(
            multi_sample().is_multiselect(),
            "with_multiselect() is multi"
        );
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
        assert_eq!(
            t.selected_rows(),
            Vec::<usize>::new(),
            "2nd KeyboardActivate toggles off"
        );
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
        let selected: Vec<_> = intents
            .iter()
            .filter(|i| i.tag_str() == "selected")
            .collect();
        assert_eq!(
            selected.len(),
            2,
            "toggle-on + toggle-off each emit an intent"
        );
    }

    #[test]
    fn multi_external_query_surface() {
        let mut ext = TableExternal::with_multiselect(
            vec!["A".to_string(), "B".to_string()],
            vec![
                vec!["1".to_string(), "2".to_string()],
                vec!["3".to_string(), "4".to_string()],
            ],
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
        assert_eq!(
            ext.selected_rows(),
            vec![1],
            "Ctrl+click still toggles the row"
        );
    }

    #[test]
    fn multi_intervene_per_row_selected_bit() {
        let mut ext = TableExternal::with_multiselect(
            vec!["A".to_string()],
            vec![
                vec!["1".to_string()],
                vec!["2".to_string()],
                vec!["3".to_string()],
            ],
        );
        // Per-row write is enabled in multi-mode (persisted restore path).
        assert!(
            ext.intervene("selected.0", IntrospectValue::Bool(true))
                .is_ok()
        );
        assert!(
            ext.intervene("selected.2", IntrospectValue::Bool(true))
                .is_ok()
        );
        assert_eq!(ext.selected_rows(), vec![0, 2]);
        // Clearing a bit removes only that row.
        assert!(
            ext.intervene("selected.0", IntrospectValue::Bool(false))
                .is_ok()
        );
        assert_eq!(ext.selected_rows(), vec![2]);
        // Out-of-range row rejects.
        assert_out_of_range_saying(
            &ext.intervene("selected.9", IntrospectValue::Bool(true)),
            "no row 9 here",
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

    // ─────────────────────────────────────────────────────────────
    // R952 §5.38 — cell range selection (anchor + extent rectangle)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r952_select_cell_is_a_single_cell() {
        let mut t = sample(); // 3 rows x 2 cols
        t.select_cell(1, 1);
        // A single-cell rectangle: anchor == cursor == (1,1), so the bounds
        // collapse to that cell (the bounds are the public read of the model).
        assert_eq!(t.cell_selection_bounds(), Some((1, 1, 1, 1)));
        assert_eq!(t.cell_selection_count(), 1);
    }

    #[test]
    fn r952_extend_cell_grows_a_rectangle() {
        let mut t = sample();
        t.select_cell(0, 0);
        t.extend_cell(2, 1);
        // The bounds = bbox(anchor (0,0), cursor (2,1)); a `(0,0)` lower corner
        // proves the anchor stayed pinned while the cursor moved to the extent.
        assert_eq!(
            t.cell_selection_bounds(),
            Some((0, 0, 2, 1)),
            "anchor (0,0) -> cursor (2,1)"
        );
        assert_eq!(t.cell_selection_count(), 6, "3 rows x 2 cols");
        assert_eq!(
            t.focused_row(),
            Some(2),
            "the cursor moved to the extent (row)"
        );
        assert_eq!(t.focused_col(), 1, "the cursor moved to the extent (col)");
    }

    #[test]
    fn r1222_selected_tsv_serializes_the_rectangle() {
        let mut t = sample(); // Tabs/R690, Menu/R691, Table/R707
        assert_eq!(t.selected_tsv(), None, "no selection -> None");
        // The full 3x2 rectangle: row-major, tab columns, newline rows.
        t.select_cell(0, 0);
        t.extend_cell(2, 1);
        assert_eq!(
            t.selected_tsv().as_deref(),
            Some("Tabs\tR690\nMenu\tR691\nTable\tR707"),
        );
        // A single row (1x2), a single column (3x1), a single cell.
        t.select_cell(1, 0);
        t.extend_cell(1, 1);
        assert_eq!(t.selected_tsv().as_deref(), Some("Menu\tR691"), "one row");
        t.select_cell(0, 1);
        t.extend_cell(2, 1);
        assert_eq!(
            t.selected_tsv().as_deref(),
            Some("R690\nR691\nR707"),
            "one column"
        );
        t.select_cell(2, 0);
        assert_eq!(t.selected_tsv().as_deref(), Some("Table"), "one cell");
        // Clearing the range drops the TSV (the cursor is untouched).
        t.clear_cell_selection();
        assert_eq!(t.selected_tsv(), None, "cleared -> None");
    }

    #[test]
    fn r1222_cell_selection_tsv_query_and_schema() {
        let mut ext = TableExternal::with_select_items(
            vec!["A".to_string(), "B".to_string()],
            vec![
                vec!["1".to_string(), "2".to_string()],
                vec!["3".to_string(), "4".to_string()],
            ],
        );
        assert_eq!(
            ext.query("cell_selection_tsv"),
            Some(IntrospectValue::Null),
            "no selection -> Null",
        );
        ext.invoke("select-cell", IntrospectValue::Text("0,0".to_string()))
            .unwrap();
        ext.invoke("extend-cell", IntrospectValue::Text("1,1".to_string()))
            .unwrap();
        assert_eq!(
            ext.query("cell_selection_tsv"),
            Some(IntrospectValue::Text("1\t2\n3\t4".to_string())),
            "the query mirrors Table::selected_tsv over the wire",
        );
        let fields: Vec<&str> = ext.schema().fields.iter().map(|f| f.path).collect();
        assert!(
            fields.contains(&"cell_selection_tsv"),
            "cell_selection_tsv is schema-declared",
        );
    }

    #[test]
    fn r1223_selected_tsv_sanitizes_embedded_delimiters() {
        // A free-text grid whose cells contain a tab / newline: the TSV block's
        // shape must STILL match the 2x2 selection rectangle (delimiters → space),
        // never silently split a row/column.
        let mut t = Table::new(
            vec!["A".to_string(), "B".to_string()],
            vec![
                vec!["a\tb".to_string(), "line1\nline2".to_string()],
                vec!["x".to_string(), "y".to_string()],
            ],
        );
        t.select_cell(0, 0);
        t.extend_cell(1, 1);
        let tsv = t.selected_tsv().expect("selection");
        assert_eq!(
            tsv, "a b\tline1 line2\nx\ty",
            "embedded tab/newline replaced with space; 2 rows x 2 cols preserved",
        );
        // Structure invariant: exactly one row-separator per (rows-1), and every
        // row has exactly one column-separator (cols-1) — never more.
        assert_eq!(tsv.matches('\n').count(), 1, "exactly 2 rows");
        for row in tsv.split('\n') {
            assert_eq!(row.matches('\t').count(), 1, "exactly 2 columns per row");
        }
    }

    #[test]
    fn r1372_rows_to_tsv_is_the_shared_codec() {
        // The pure codec the Table's `selected_tsv` and the editable data grid's
        // copy both funnel through: row-major join, tab columns, newline rows,
        // embedded delimiters sanitized so the block shape matches the rectangle.
        assert_eq!(
            rows_to_tsv(&[]),
            "",
            "an empty rectangle is the empty string"
        );
        assert_eq!(
            rows_to_tsv(&[vec!["one".to_string()]]),
            "one",
            "a single cell has no separators",
        );
        assert_eq!(
            rows_to_tsv(&[
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string(), "d".to_string()],
            ]),
            "a\tb\nc\td",
            "2x2: tab columns, newline rows",
        );
        assert_eq!(
            rows_to_tsv(&[vec!["a\tb".to_string(), "l1\nl2".to_string()]]),
            "a b\tl1 l2",
            "embedded tab/newline -> space (structure-preserving)",
        );
    }

    #[test]
    fn r952_extend_cell_normalizes_up_left() {
        let mut t = sample();
        t.select_cell(2, 1);
        t.extend_cell(0, 0); // extend toward the top-left
        assert_eq!(
            t.cell_selection_bounds(),
            Some((0, 0, 2, 1)),
            "bounds normalize regardless of order"
        );
        assert_eq!(t.cell_selection_count(), 6);
    }

    #[test]
    fn r952_extend_with_no_anchor_pins_the_cursor() {
        let mut t = sample();
        // A Shift-move before any selection: the current cursor (none here)
        // makes the target itself the anchor — a single cell that later
        // extensions grow.
        t.extend_cell(1, 0);
        assert_eq!(
            t.cell_selection_bounds(),
            Some((1, 0, 1, 0)),
            "anchor pins to (1,0), a single cell"
        );
        // A pre-existing cursor (no anchor) becomes the anchor on first extend.
        let mut t2 = sample();
        t2.set_focused_row(Some(0));
        t2.set_focused_col(0);
        t2.extend_cell(1, 1);
        assert_eq!(
            t2.cell_selection_bounds(),
            Some((0, 0, 1, 1)),
            "old cursor (0,0) anchors the rect"
        );
    }

    #[test]
    fn r952_clear_cell_selection_drops_it_keeps_cursor() {
        let mut t = sample();
        t.select_cell(1, 1);
        t.clear_cell_selection();
        assert_eq!(t.cell_selection_bounds(), None);
        assert_eq!(t.cell_selection_count(), 0);
        assert_eq!(
            t.focused_row(),
            Some(1),
            "the cursor survives a selection clear"
        );
        assert_eq!(t.focused_col(), 1);
    }

    #[test]
    fn r952_out_of_range_cell_select_is_a_no_op() {
        let mut t = sample();
        t.select_cell(9, 9);
        assert_eq!(
            t.cell_selection_bounds(),
            None,
            "out-of-range select is ignored"
        );
        t.select_cell(0, 0);
        t.extend_cell(9, 0);
        assert_eq!(
            t.cell_selection_bounds(),
            Some((0, 0, 0, 0)),
            "out-of-range extend is ignored"
        );
    }

    fn cell_sample_ext() -> TableExternal {
        TableExternal::new(
            vec!["Name".to_string(), "Round".to_string()],
            vec![
                vec!["Tabs".to_string(), "R690".to_string()],
                vec!["Menu".to_string(), "R691".to_string()],
                vec!["Table".to_string(), "R707".to_string()],
            ],
        )
    }

    #[test]
    fn r952_rpc_cell_selection_round_trip() {
        let mut ext = cell_sample_ext();
        assert_eq!(
            ext.query("cell_selection"),
            Some(IntrospectValue::Null),
            "no selection at boot"
        );
        assert_eq!(
            ext.query("cell_selection_count"),
            Some(IntrospectValue::Int(0))
        );
        assert_eq!(
            ext.invoke("select-cell", IntrospectValue::Text("1,1".to_string())),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(
            ext.query("cell_selection"),
            Some(IntrospectValue::Text("1,1,1,1".to_string())),
        );
        assert_eq!(
            ext.query("cell_selection_count"),
            Some(IntrospectValue::Int(1))
        );
        assert_eq!(
            ext.invoke("extend-cell", IntrospectValue::Text("2,1".to_string())),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(
            ext.query("cell_selection"),
            Some(IntrospectValue::Text("1,1,2,1".to_string())),
            "the rectangle grew from (1,1) to (2,1)",
        );
        assert_eq!(
            ext.query("cell_selection_count"),
            Some(IntrospectValue::Int(2))
        );
        assert_eq!(
            ext.invoke("clear-cell-selection", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(
            ext.query("cell_selection"),
            Some(IntrospectValue::Null),
            "cleared"
        );
    }

    #[test]
    fn r952_rpc_cell_select_out_of_range_and_bad_arg() {
        let mut ext = cell_sample_ext();
        assert_eq!(
            ext.invoke("select-cell", IntrospectValue::Text("9,9".to_string())),
            Ok(IntrospectValue::Bool(false)),
            "out-of-range cell select is a no-op (false), not an error",
        );
        assert_eq!(ext.query("cell_selection"), Some(IntrospectValue::Null));
        assert!(matches!(
            ext.invoke("select-cell", IntrospectValue::Int(3)),
            Err(InvokeError::TypeMismatch),
        ));
        assert!(matches!(
            ext.invoke(
                "clear-cell-selection",
                IntrospectValue::Text("x".to_string())
            ),
            Err(InvokeError::TypeMismatch),
        ));
    }

    fn cell_select_items_ext() -> TableExternal {
        TableExternal::with_select_items(
            vec!["Name".to_string(), "Round".to_string()],
            vec![
                vec!["Tabs".to_string(), "R690".to_string()],
                vec!["Menu".to_string(), "R691".to_string()],
                vec!["Table".to_string(), "R707".to_string()],
            ],
        )
    }

    /// R1562 §5.27 — a **row-header** press on the eager surface selects the
    /// line, through the same `send_cell` arc a press in the row's body drives.
    /// The band and the body are compared state-for-state, so a second
    /// selection implementation behind the band would show up here.
    #[test]
    fn r1562_an_eager_band_press_selects_the_line() {
        use crate::composite_tag::compose_send_payload;
        use crate::input::{Modifiers, PointerButtons};
        let arc = |ext: &mut TableExternal, key: &str| {
            for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                let wire = compose_send_payload(
                    Some(key),
                    ev,
                    Modifiers::default(),
                    PointerButtons::empty(),
                );
                let _ = ext.invoke("send", IntrospectValue::Text(wire));
            }
        };
        let mut from_band = cell_sample_ext();
        let mut from_body = cell_sample_ext();
        arc(&mut from_band, "r1");
        arc(&mut from_body, "1_0");
        assert_eq!(
            from_band.query("selected_row"),
            Some(IntrospectValue::Int(1)),
            "the band press washed row 1",
        );
        assert_eq!(
            from_band.query("selected_row"),
            from_body.query("selected_row"),
        );
        assert_eq!(
            from_band.query("focused_row"),
            from_body.query("focused_row")
        );
        assert_eq!(
            from_band.query("focused_col"),
            from_body.query("focused_col")
        );
        // Out of range is refused rather than silently ignored.
        let bad = compose_send_payload(
            Some("r99"),
            "PointerUp",
            Modifiers::default(),
            PointerButtons::empty(),
        );
        assert_refused_saying(
            &from_band.invoke("send", IntrospectValue::Text(bad)),
            "no row 99 in this table",
        );
    }

    /// R1562 §5.27 — under `SelectItems` a header names a ROW, so the press selects that
    /// row's whole rectangle rather than one cell of it. The toolkit's `selectRow`
    /// does the same; the point is that it happens here through the
    /// cell-rectangle verbs this surface already had.
    #[test]
    fn r1562_an_eager_band_press_selects_the_whole_row_rectangle() {
        use crate::composite_tag::compose_send_payload;
        use crate::input::{Modifiers, PointerButtons};
        let mut ext = cell_select_items_ext();
        let wire = compose_send_payload(
            Some("r1"),
            "PointerUp",
            Modifiers::default(),
            PointerButtons::empty(),
        );
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text(wire)),
            Ok(IntrospectValue::Text("1,0,1,1".to_string())),
            "the rectangle spans the row's every column",
        );
        assert_eq!(
            ext.query("cell_selection_count"),
            Some(IntrospectValue::Int(2)),
        );
    }

    /// R1562 §5.27 §5.40 — the eager corner toggles the whole model, on both
    /// selection behaviours, and announces every change it makes.
    #[test]
    fn r1562_the_eager_corner_toggles_the_whole_model() {
        use crate::composite_tag::compose_send_payload;
        use crate::input::{Modifiers, PointerButtons};
        let press = || {
            compose_send_payload(
                Some("c"),
                "PointerUp",
                Modifiers::default(),
                PointerButtons::empty(),
            )
        };
        let mut multi = TableExternal::with_multiselect(
            vec!["Name".to_string(), "Round".to_string()],
            vec![
                vec!["Tabs".to_string(), "R690".to_string()],
                vec!["Menu".to_string(), "R691".to_string()],
                vec!["Table".to_string(), "R707".to_string()],
            ],
        );
        let _ = multi.invoke("send", IntrospectValue::Text(press()));
        assert_eq!(multi.selected_rows(), vec![0, 1, 2], "every row selected");
        let mut announced = 0;
        multi.drain_intents(&mut |_| announced += 1);
        assert_eq!(announced, 3, "one intent per row it flipped");
        let _ = multi.invoke("send", IntrospectValue::Text(press()));
        assert!(
            multi.selected_rows().is_empty(),
            "and a second press clears"
        );
        // A single-select table cannot hold every row: the no-op `selectAll`
        // is under `SingleSelection`.
        let mut single = cell_sample_ext();
        let _ = single.invoke("send", IntrospectValue::Text(press()));
        assert_eq!(single.query("selected_row"), Some(IntrospectValue::Int(-1)));
        // `SelectItems` spans the whole grid, then clears.
        let mut items = cell_select_items_ext();
        let _ = items.invoke("send", IntrospectValue::Text(press()));
        assert_eq!(
            items.query("cell_selection"),
            Some(IntrospectValue::Text("0,0,2,1".to_string())),
        );
        let _ = items.invoke("send", IntrospectValue::Text(press()));
        assert_eq!(items.query("cell_selection"), Some(IntrospectValue::Null));
    }

    /// R954 §5.38 — a plain pointer click on a `SelectItems` grid selects the
    /// clicked *cell* (collapse) and washes **no** row (the R953
    /// SelectRows-on-click smell is gone); the cursor follows the click.
    #[test]
    fn r954_select_items_click_selects_cell_not_row() {
        use crate::composite_tag::compose_send_payload;
        use crate::input::{Modifiers, PointerButtons};
        let mut ext = cell_select_items_ext();
        assert_eq!(ext.selection_behavior(), SelectionBehavior::SelectItems);
        let wire = compose_send_payload(
            Some("1_1"),
            "PointerUp",
            Modifiers::default(),
            PointerButtons::empty(),
        );
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text(wire)),
            Ok(IntrospectValue::Text("1,1,1,1".to_string())),
            "the click echoes the new cell rectangle (setter-returns-read-outcome)",
        );
        assert_eq!(
            ext.query("cell_selection"),
            Some(IntrospectValue::Text("1,1,1,1".to_string())),
            "the click selected the single cell (1,1)",
        );
        assert_eq!(
            ext.query("selected_row"),
            Some(IntrospectValue::Int(-1)),
            "no row was washed (SelectItems, not SelectRows)",
        );
        assert_eq!(
            ext.query("focused_row"),
            Some(IntrospectValue::Int(1)),
            "cursor followed click"
        );
        assert_eq!(ext.query("focused_col"), Some(IntrospectValue::Int(1)));
    }

    /// R954 §5.38 — a `Shift`+click extends the rectangle from the anchor (the
    /// held modifier rides the composite `send` wire's third segment).
    #[test]
    fn r954_select_items_shift_click_extends_rectangle() {
        use crate::composite_tag::compose_send_payload;
        use crate::input::{Modifiers, PointerButtons};
        let mut ext = cell_select_items_ext();
        let plain = compose_send_payload(
            Some("0_0"),
            "PointerUp",
            Modifiers::default(),
            PointerButtons::empty(),
        );
        let _ = ext.invoke("send", IntrospectValue::Text(plain));
        let shift = compose_send_payload(
            Some("2_1"),
            "PointerUp",
            Modifiers {
                shift: true,
                ..Default::default()
            },
            PointerButtons::empty(),
        );
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text(shift)),
            Ok(IntrospectValue::Text("0,0,2,1".to_string())),
            "the Shift+click echoes the grown rectangle",
        );
        assert_eq!(
            ext.query("cell_selection"),
            Some(IntrospectValue::Text("0,0,2,1".to_string())),
            "Shift+click grew the rectangle (0,0)->(2,1) from the pinned anchor",
        );
        assert_eq!(
            ext.query("cell_selection_count"),
            Some(IntrospectValue::Int(6))
        );
    }

    /// R954 §5.38 — only the activate edge (`PointerUp`) selects; the other
    /// pointer phases of a click are inert (no premature anchor / no row wash).
    #[test]
    fn r954_select_items_non_activate_phases_are_inert() {
        use crate::composite_tag::compose_send_payload;
        use crate::input::{Modifiers, PointerButtons};
        let mut ext = cell_select_items_ext();
        for ev in ["PointerEnter", "PointerDown", "PointerLeave"] {
            let wire = compose_send_payload(
                Some("1_1"),
                ev,
                Modifiers::default(),
                PointerButtons::empty(),
            );
            assert_eq!(
                ext.invoke("send", IntrospectValue::Text(wire)),
                Ok(IntrospectValue::Null)
            );
        }
        assert_eq!(
            ext.query("cell_selection"),
            Some(IntrospectValue::Null),
            "hover / press / leave selected nothing — only PointerUp activates",
        );
        assert_eq!(
            ext.query("selected_row"),
            Some(IntrospectValue::Int(-1)),
            "no row washed"
        );
    }

    /// R954 §5.38 — the default `SelectRows` behavior is unaffected: a click
    /// still washes the row (the `hello-table` model), so the new mode is
    /// strictly additive.
    #[test]
    fn r954_select_rows_default_click_still_washes_row() {
        use crate::composite_tag::compose_send_payload;
        use crate::input::{Modifiers, PointerButtons};
        let mut ext = cell_sample_ext(); // SelectRows (default)
        assert_eq!(ext.selection_behavior(), SelectionBehavior::SelectRows);
        // The full pointer cycle — the radio activate edge is Pressed->Hover on
        // the PointerUp, so a bare Up does not wash (unlike SelectItems, where
        // the Up *is* the cell activate edge).
        let mut up_ret = Ok(IntrospectValue::Null);
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let wire = compose_send_payload(
                Some("1_1"),
                ev,
                Modifiers::default(),
                PointerButtons::empty(),
            );
            let r = ext.invoke("send", IntrospectValue::Text(wire));
            if ev == "PointerUp" {
                up_ret = r;
            }
        }
        assert_eq!(
            up_ret,
            Ok(IntrospectValue::Int(1)),
            "SelectRows click washes + returns row 1"
        );
        assert_eq!(
            ext.query("selected_row"),
            Some(IntrospectValue::Int(1)),
            "row 1 washed"
        );
        assert_eq!(
            ext.query("cell_selection"),
            Some(IntrospectValue::Null),
            "no cell rectangle in SelectRows mode",
        );
    }

    /// R955.1 §5.38 — the pointer [`SelectionBehavior`] is introspectable over
    /// the wire (the AI-first peer of `multiselect`), so a client reads the
    /// grid's mode rather than knowing it out-of-band.
    #[test]
    fn r955_1_selection_behavior_is_introspectable() {
        assert_eq!(
            cell_select_items_ext().query("selection_behavior"),
            Some(IntrospectValue::Text("items".to_string())),
            "a SelectItems grid reports its mode",
        );
        assert_eq!(
            cell_sample_ext().query("selection_behavior"),
            Some(IntrospectValue::Text("rows".to_string())),
            "a SelectRows grid reports its mode",
        );
    }

    /// R955.1 §5.40 — the lifted [`read_focused_row`] / [`read_focused_col`] /
    /// [`read_rows`] / [`read_cols`] decode the cursor / dimension query slots
    /// (the table-family grid bindings' shared decoders).
    #[test]
    fn r955_1_lifted_readers_decode_the_query_slots() {
        let mut ext = cell_select_items_ext();
        assert_eq!(read_rows(&ext), 3);
        assert_eq!(read_cols(&ext), 2);
        assert_eq!(read_focused_row(&ext), None, "no cursor at boot");
        assert_eq!(read_focused_col(&ext), 0);
        let _ = ext.invoke("select-cell", IntrospectValue::Text("2,1".to_string()));
        assert_eq!(
            read_focused_row(&ext),
            Some(2),
            "reader reflects the moved cursor"
        );
        assert_eq!(read_focused_col(&ext), 1);
    }
}
