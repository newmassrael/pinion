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
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::radio::{Radio, RadioEvent, RadioState};
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
        }
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

    /// The selected row (0-based), or `None`.
    #[must_use]
    pub fn selected_row(&self) -> Option<usize> {
        self.selected_row
    }

    /// Drive `event` to the cell at `(row, col)`. If the event activates
    /// that row (`false → true` selected), every other row is deselected
    /// and `selected_row` snaps to `row`. Independently, the `PointerUp`
    /// edge of a click syncs the active descendant to `(row, col)` —
    /// WAI-ARIA "clicking a cell moves focus to it", whether or not the
    /// selection changed (so clicking a different cell in the already-
    /// selected row still moves the keyboard cursor there).
    ///
    /// Out-of-range `(row, col)` is a silent no-op (the router rejects
    /// bad composite sub-indices upstream; this guards the model path).
    pub fn send_cell(&mut self, row: usize, col: usize, event: RadioEvent) {
        if row >= self.rows.len() || col >= self.headers.len() {
            return;
        }
        let was_selected = self.row_radios[row].is_selected();
        self.row_radios[row].send(event);
        let now_selected = self.row_radios[row].is_selected();
        if !was_selected && now_selected {
            for (j, r) in self.row_radios.iter_mut().enumerate() {
                if j != row {
                    r.set_selected(false);
                }
            }
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
        if col >= self.headers.len() {
            return;
        }
        self.sort = match self.sort {
            Some((c, true)) if c == col => Some((col, false)),
            Some((c, false)) if c == col => None,
            _ => Some((col, true)),
        };
    }

    /// R730 §5.40 — the visual→data row permutation for the current sort.
    /// `order()[visual]` is the data-row index painted at visual position
    /// `visual`. Identity (`0..row_count`) when unsorted. The comparison
    /// is **numeric-aware** (cells that both parse as numbers compare
    /// numerically, else lexicographically) and **stable** (ties keep
    /// their data order), so re-sorting is deterministic.
    #[must_use]
    pub fn order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.rows.len()).collect();
        let Some((col, ascending)) = self.sort else {
            return idx;
        };
        idx.sort_by(|&a, &b| {
            let ord = cell_cmp(self.cell(a, col), self.cell(b, col));
            if ascending { ord } else { ord.reverse() }
        });
        idx
    }
}

/// R730 — numeric-aware cell comparison: two cells that both parse as
/// `f64` compare numerically (so `"9" < "12"`); otherwise lexicographic
/// (so `"Active" < "Done"`). A data grid that sorted a numeric column
/// lexicographically (`"12" < "9"`) would be visibly wrong.
fn cell_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(core::cmp::Ordering::Equal),
        _ => a.cmp(b),
    }
}

/// `Table` transition contract (R51.12 substrate). The event pairs the
/// `(row, col)` cell with the underlying [`RadioEvent`]; the snapshot is
/// the selected row index; detect emits `"selected"` (the new row index
/// as [`IntrospectValue::Int`]) whenever the selection moves to a new
/// row.
///
/// Selection transitions that emit:
///
/// * `None → Some(r)` — first selection
/// * `Some(a) → Some(b)` where `a != b` — switch rows
///
/// Transitions that stay silent (idempotent):
///
/// * `Some(a) → Some(a)` — re-activate the same row
/// * `None → None` — non-activating event
impl WidgetTransition for Table {
    type Event = (usize, usize, RadioEvent);
    type Snapshot = Option<usize>;

    fn snapshot(&self) -> Self::Snapshot {
        self.selected_row
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
        if before != after {
            if let Some(row) = after {
                if let Ok(row_i) = i64::try_from(row) {
                    return vec![Intent::new_static(
                        "selected",
                        IntrospectValue::Int(row_i),
                    )];
                }
            }
        }
        Vec::new()
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
    /// Construct a table over `headers` columns and `rows` of cell text.
    #[must_use]
    pub fn new(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { em: IntentEmitter::new(Table::new(headers, rows)) }
    }

    /// Drive `event` to the cell at `(row, col)`. Queues a `"selected"`
    /// intent on selection-change transitions.
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

    /// The selected row (0-based), or `None`.
    #[must_use]
    pub fn selected_row(&self) -> Option<usize> {
        self.em.inner.selected_row()
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
        f.debug_struct("TableExternal")
            .field("rows", &self.row_count())
            .field("cols", &self.col_count())
            .field("selected_row", &self.selected_row())
            .finish()
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
            "selected" => Some(IntrospectValue::Bool(self.selected_row().is_some())),
            // The selected / focused row use `-1` until a value lands
            // (mirror of the date picker's `selected_day` / `focused_day`
            // `int` sentinel convention).
            "selected_row" => Some(IntrospectValue::Int(
                self.selected_row().map_or(-1, int_of),
            )),
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
                    let (key, event_name) =
                        s.split_once(':').ok_or(InvokeError::Rejected)?;
                    // R730 §5.40 — a column-header click arrives as
                    // `"h<col>:<EventName>"` (the paint tag `"<tag>#h<col>"`
                    // through the R51.42 `'#'`-split funnel). Cell keys are
                    // `"<row>_<col>"` (always an underscore), so the `h`
                    // prefix is unambiguous. The sort cycles on `PointerUp`
                    // (the activate edge), matching cell activation; other
                    // pointer phases are inert. Returns the new `sort_dir`.
                    if let Some(col_str) = key.strip_prefix('h') {
                        let col: usize =
                            col_str.parse().map_err(|_| InvokeError::Rejected)?;
                        if col >= self.col_count() {
                            return Err(InvokeError::Rejected);
                        }
                        if event_name == "PointerUp" {
                            self.cycle_sort(col);
                        }
                        return Ok(self
                            .query("sort_dir")
                            .unwrap_or(IntrospectValue::Null));
                    }
                    let (row_str, col_str) =
                        key.split_once('_').ok_or(InvokeError::Rejected)?;
                    let row: usize =
                        row_str.parse().map_err(|_| InvokeError::Rejected)?;
                    let col: usize =
                        col_str.parse().map_err(|_| InvokeError::Rejected)?;
                    if row >= self.row_count() || col >= self.col_count() {
                        return Err(InvokeError::Rejected);
                    }
                    let ev = RadioEvent::from_name(event_name)
                        .ok_or(InvokeError::Rejected)?;
                    self.send_cell(row, col, ev);
                    Ok(match self.selected_row() {
                        Some(r) => IntrospectValue::Int(int_of(r)),
                        None => IntrospectValue::Null,
                    })
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

/// Widen a `usize` index to the `i64` the introspect surface speaks,
/// saturating at the (practically unreachable) `i64::MAX` edge so the
/// conversion is total without a `cast_possible_truncation` lint.
fn int_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
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
}
