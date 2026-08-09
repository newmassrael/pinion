//! R746 §5.27 / §5.38 — **selection coordinator for a virtualized list**.
//!
//! R744/R745 land *display-only* virtualization: only the visible window
//! of an N-row dataset ever exists in the scene tree. The natural next
//! Model/View slice is **selection** — but the existing
//! [`selection`](crate::widgets::selection) substrate (R735.1) is
//! fundamentally a *leaf-based* model: it operates on a `&mut [L]` slice of
//! materialized leaves, each carrying its own selection bit (a `Radio`, a
//! `ListBoxItem`). That is exactly what a virtualized list **cannot**
//! provide — the whole point is that the 9 995 off-window leaves do not
//! exist. Reusing it would require materializing all N leaves, defeating
//! virtualization.
//!
//! So selection on a virtualized collection is held the way every real data
//! grid holds it: as a **selected data index**, owned by a coordinator and
//! decoupled from materialization. Selecting row 4 200 and scrolling away does
//! not drop the selection (no leaf to lose it on); the view paints `selected == index` for the
//! handful of *visible* rows. This is the canonical virtualized-selection
//! model (the toolkit item selection model over a abstract item model, another
//! retained-mode toolkit `ListView` + a selection controller, web `aria-activedescendant` over windowed
//! rows).
//!
//! Like [`SpinButtonExternal`](crate::widgets::spin_button) and
//! [`ProgressBarExternal`](crate::widgets::progress_bar) this widget owns
//! **no interaction statechart** — there is no per-row hover/press SCXML at
//! the list level (the rows are plain windowed `Scene` nodes, not
//! externals). It is a plain index holder: *operability* is "set the
//! selected index", driven by the R51.42 §5.35 composite pointer channel
//! (`vlist#<i>` → `invoke("send", "<i>:PointerUp")`) and the AI-first
//! `invoke("select", <i>)` path. Single-select this slice (the listbox /
//! data-grid default); multi-select by index is a later additive axis when
//! a consumer needs it.
//!
//! a11y: the binding lowers `selected == index` to
//! `AccessNode::with_selected`
//! (`aria-selected`) on each *rendered* `ListItem`, on a single-select
//! `List` (no `aria-multiselectable`) — exactly the windowed-AT model the
//! R744/R745 lists already use for `aria-setsize` / `aria-posinset`.

use crate::Scene;
use crate::composite_tag::GridSendKey;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::input::Modifiers;
use crate::intent::Intent;
use crate::widgets::IntentEmitter;
use crate::widgets::cell_selection::{CellSelection, ColumnSpan, SelectionBehavior};
use crate::widgets::index_runs::IndexRuns;
use crate::widgets::scroll::ScrollState;

/// R780 §5.40 — selection cardinality policy for [`VirtualSelect`], the
/// item selection model `SelectionMode` analogue.
///
/// `Single` (the default and every pre-R780 consumer) holds at most one
/// row; `Multi` holds an arbitrary set with an `anchor` for range
/// extension. The mode is fixed at construction — a list is built either
/// single- or multi-select, exactly as `setSelectionMode`
/// is a property of the view, not a per-interaction flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectionMode {
    /// At most one selected row (the pre-R780 behaviour). Range/toggle
    /// operations collapse to a plain replace.
    Single,
    /// An arbitrary set of selected rows with an `anchor` origin for
    /// `Shift`-range extension and per-row `Ctrl`-toggle.
    Multi,
}

/// R1563 §5.27 — what a press on a **horizontal header section** does, beyond
/// whatever else the binding has wired to it.
///
/// The toolkit reaches this through two independent connections to one header
/// view: `sectionPressed` drives the view's column selection and `sectionClicked` drives `sortByColumn` when `setSortingEnabled(true)`.
/// Nothing declares which a given header has, and a header can silently have
/// both — so "does clicking this header select the column?" is a question
/// about signal wiring rather than about the view.
///
/// Here it is one declared value. [`Inert`](Self::Inert) is the default and
/// every pre-R1563 grid: those headers are the sort control (R778), and a round
/// that adds column selection must not quietly make every sortable header
/// select as well. The mirror of R1562's `CornerAction`, on the other axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SectionPress {
    /// The section press selects nothing — it is the sort / reorder control, or
    /// nothing at all.
    #[default]
    Inert,
    /// The section press selects the column through it, with the same
    /// `Ctrl` / `Shift` chord vocabulary a cell press has (the toolkit
    /// `sectionPressed` → `selectColumn`).
    Select,
}

/// R1562 §5.40 — how much of the model a selection covers: the tri-state a
/// select-all control shows, and the value the toolkit's corner button does
/// not have.
///
/// The three states of an HTML `<input type=checkbox>` — unchecked, `indeterminate`, checked — and of WAI-ARIA
/// `aria-checked` (`false` / `"mixed"` / `true`). The toolkit's table corner button has **none** of
/// them: it is a abstract button that always runs `selectAll()`, so it cannot report
/// what is selected and cannot take a full selection back.
///
/// Answered in O(1) from [`VirtualSelect::selected_count`] (a sum over runs
/// since R1561) against the item count. The same question of a
/// item selection model is `selectedRows().size() == model->rowCount()`, which
/// builds one model index per selected row to compare two integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectionExtent {
    /// Nothing is selected — and the answer for an **empty model** too, which
    /// is the reading that keeps "pressing select-all selects something" true:
    /// a model with no rows has nothing to select, so calling it `All` would
    /// name a control that can only be a no-op.
    Empty,
    /// Some rows are selected and some are not (`aria-checked="mixed"`).
    Partial,
    /// Every row of a non-empty model is selected.
    All,
}

impl SelectionExtent {
    /// The rule, in one place: how `selected` of `total` rows reads as an
    /// extent.
    ///
    /// `pub` because the paint side has to answer the same question from a
    /// **projection** rather than from the model — a `WidgetCore::State` is
    /// `Copy`, so a binding hands its view fn a snapshot and cannot call
    /// [`VirtualSelect::extent`] from inside it. Restating `0 -> Empty,
    /// total -> All, else Partial` there would be a second implementation of an
    /// opinion-free rule, and the two could then disagree about the edge that
    /// matters (an empty model).
    #[must_use]
    pub fn of(selected: usize, total: usize) -> Self {
        if selected == 0 {
            Self::Empty
        } else if selected >= total {
            Self::All
        } else {
            Self::Partial
        }
    }
}

/// R746 §5.27 §5.38 / R780 §5.40 — the pure index-set selection model.
///
/// Pure selection-by-index state, no interaction statechart and no §5.20
/// queue of its own — the [`IntentEmitter`] wrapper owns the pending
/// intents (exactly as [`RadioGroup`](crate::widgets::radio_group) is the
/// plain widget inside `IntentEmitter<RadioGroup>`). Holding the selection
/// as **data indices** (not per-leaf bits) is what decouples it from
/// materialization; `item_count` bounds every mutation so a malformed wire
/// payload can never select a non-existent row.
///
/// Two embeddings (R922): [`VirtualSelectExternal`] wraps it with the §5.20
/// intent channel + the standalone-list RPC wire (a list / grid whose only
/// state *is* the selection); a coordinator that owns a **richer** model —
/// the inspector's object list, where the same External also carries the
/// selected objects' shared property panel — embeds the model directly as
/// its selection axis (`Rc<Signal<VirtualSelect>>`), reusing the
/// anchor-range-extend logic without the standalone wrapper. The flat,
/// stable data index keys this model; a collapse/expand tree (whose flat
/// index shifts) needs its own stable-id model instead (R902's `TreeSelect`).
///
/// The set generalises the pre-R780 `selected: Option<usize>`: a `Single`
/// model keeps the set at cardinality ≤ 1, so [`cursor`](Self::cursor) (the
/// active row, the `query("selected")` value) coincides with the sole
/// member and every old assertion holds. `Multi` lets the set grow, with
/// `anchor` recording where a `Shift`-range began.
///
/// `Serialize`/`Deserialize`/`PartialEq` (R922) so a coordinator can hold the
/// model in a `Signal` (the §2 #7 scene-as-data bound every reactive holder
/// carries); `VirtualSelectExternal` wraps it in an `IntentEmitter` instead and
/// does not need them, but they are harmless there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VirtualSelect {
    /// The selected **cells**, held as the bands they are made of (R1563).
    /// In `Single` mode this is kept at one row.
    ///
    /// Was a `BTreeSet<usize>` until R1561, which is one node per selected **row**: `Ctrl+A` over
    /// the 10 000-row `hello-multi-select` binding allocated ten thousand of them and answered
    /// `query("selection")` with 58 890 bytes of JSON in 14.3 ms, for a fact whose statement is
    /// `[[0, 9999]]`. R1561 made it [`IndexRuns`]; R1563 gave it the column axis the toolkit has
    /// had all along, without giving the run form up — see [`CellSelection`] for the
    /// normal form and why a rectangle list could not have one.
    cells: CellSelection,
    /// Range-extension origin — the row a `Shift`-extend grows from,
    /// unchanged while extending. Set by every plain move / toggle.
    anchor: Option<usize>,
    /// R1563 — the column half of the extension origin, so a `Shift`-press
    /// under [`SelectionBehavior::SelectItems`] grows a **rectangle** from where the
    /// last plain press landed. `None` while the selection has no column axis
    /// in play, which is every `Rows` grid.
    #[serde(default)]
    anchor_column: Option<usize>,
    /// The active row (WAI-ARIA active descendant / item selection model
    /// `currentIndex`): the keyboard-navigation reference and the
    /// `query("selected")` value. Coincides with the sole selected row in
    /// `Single` mode.
    cursor: Option<usize>,
    /// R1563 — the column half of the active cell. Together with
    /// [`cursor`](Self::cursor) this is the toolkit's `currentIndex()`, which is a model index
    /// and has always had both.
    #[serde(default)]
    cursor_column: Option<usize>,
    /// Total dataset size — the validity bound for any selection.
    item_count: usize,
    /// R1563 — the model's width, and the gate on the whole column axis.
    ///
    /// `None` means *this grid has no column axis*: every cell- and
    /// column-addressed operation refuses rather than guessing a width, and
    /// `query("column_count")` answers `null` so the refusal is discoverable
    /// without provoking one. Every pre-R1563 consumer is this, which is why it
    /// is the default rather than a number that would have to be right.
    #[serde(default)]
    column_count: Option<usize>,
    /// Single- vs multi-select cardinality policy.
    mode: SelectionMode,
    /// R1563 — what a press selects (the toolkit `setSelectionBehavior`).
    #[serde(default)]
    behavior: SelectionBehavior,
}

impl VirtualSelect {
    /// Construct an empty selection over an `item_count`-row dataset with the
    /// given cardinality policy. `pub` so a coordinator can embed the model
    /// directly (R922); a standalone list uses [`VirtualSelectExternal::new`]
    /// / [`new_multi`](VirtualSelectExternal::new_multi) instead.
    #[must_use]
    pub fn new(item_count: usize, mode: SelectionMode) -> Self {
        Self {
            cells: CellSelection::new(),
            anchor: None,
            anchor_column: None,
            cursor: None,
            cursor_column: None,
            item_count,
            column_count: None,
            mode,
            behavior: SelectionBehavior::default(),
        }
    }

    /// R1563 — declare the model's width, opening the column axis.
    ///
    /// Without it every cell- and column-addressed operation refuses: a grid
    /// that never said how wide it is cannot be asked to select its third
    /// column, and guessing a width from whatever the paint happens to be
    /// showing would make the answer depend on the scroll position.
    #[must_use]
    pub fn with_columns(mut self, column_count: usize) -> Self {
        self.column_count = Some(column_count);
        self
    }

    /// R1563 — declare what a press selects (the toolkit `setSelectionBehavior`).
    #[must_use]
    pub const fn with_behavior(mut self, behavior: SelectionBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    /// The rows selected in **every** column, as the runs they are made of
    /// (ascending, canonical) — the toolkit `selectedRows()`, and
    /// the whole of the selection in a `Rows` grid. Cardinality ≤ 1 in a
    /// `Single` model.
    ///
    /// R1563 — count-independently so: this is the
    /// [`ColumnSpan::All`] band, read in O(1), so a row it names is a row
    /// selected *as a record* and stays one when a column is added. See
    /// [`CellSelection::rows_all_columns`], and
    /// [`cells`](Self::cells) for the selection's other axis.
    #[must_use]
    pub fn selection(&self) -> &IndexRuns {
        self.cells.rows_all_columns()
    }

    /// R1563 — the selection with both its axes.
    #[must_use]
    pub fn cells(&self) -> &CellSelection {
        &self.cells
    }

    /// R1563 — the model's width, or `None` when this grid has no column axis.
    #[must_use]
    pub fn column_count(&self) -> Option<usize> {
        self.column_count
    }

    /// R1563 — what a press selects.
    #[must_use]
    pub fn behavior(&self) -> SelectionBehavior {
        self.behavior
    }

    /// R1563 — the column half of the active cell; together with
    /// [`cursor`](Self::cursor) this is the toolkit's `currentIndex()`.
    #[must_use]
    pub fn cursor_column(&self) -> Option<usize> {
        self.cursor_column
    }

    /// R1563 — the column half of the range-extension origin.
    #[must_use]
    pub fn anchor_column(&self) -> Option<usize> {
        self.anchor_column
    }

    /// R1563 — whether the cell at `(row, col)` is selected.
    #[must_use]
    pub fn is_cell_selected(&self, row: usize, col: usize) -> bool {
        self.cells.contains(row, col)
    }

    /// R1561 — how many rows are selected: the toolkit's missing
    /// `count()`.
    ///
    /// item selection model offers `hasSelection()` and nothing between that
    /// bool and `selectedRows().size()`, which builds one model index per
    /// selected row purely to read the list's length. Here it is a sum over
    /// [`IndexRuns::run_count`] runs, so a whole-model selection answers from
    /// one addition.
    #[must_use]
    pub fn selected_count(&self) -> usize {
        self.selection().len()
    }

    /// The active row (the `query("selected")` value, the WAI-ARIA active
    /// descendant): the sole selected row in a `Single` model, the
    /// keyboard-navigation reference in a `Multi` one. `None` when empty.
    #[must_use]
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// Whether `index` is selected as a **record** — every column of it.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.selection().contains(index)
    }

    /// Total dataset size — the validity bound for any selection.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// R1563 — the width every subtraction resolves
    /// [`ColumnSpan::All`] against. Zero when the grid has no column axis,
    /// which is sound because the only spans such a grid holds are `All`, and
    /// `All` minus `All` is empty at any width.
    fn columns(&self) -> usize {
        self.column_count.unwrap_or(0)
    }

    /// Move the active cell and reset the extension origin to it — the shared
    /// tail of every plain (unmodified) press, on whichever axis it landed.
    fn anchor_at(&mut self, row: Option<usize>, col: Option<usize>) {
        self.anchor = row;
        self.anchor_column = col;
        self.cursor = row;
        self.cursor_column = col;
    }

    /// Move the active row to `index` and replace the selection with just
    /// it (a plain click / unmodified arrow). Out-of-range indices are
    /// ignored. Sets the `anchor` (a later `Shift`-extend grows from here).
    /// Returns `true` if anything changed.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.item_count {
            return false;
        }
        let moved = self.cursor != Some(index) || self.cursor_column.is_some();
        let changed = self
            .cells
            .replace(&IndexRuns::run(index, index), &ColumnSpan::All);
        self.anchor_at(Some(index), None);
        changed || moved
    }

    /// `Ctrl`-toggle `index`: flip its membership, leaving the rest of the
    /// selection intact, and make it the active row + `anchor`. In `Single`
    /// mode this collapses to a plain [`select`](Self::select) (a single
    /// model cannot hold two rows). Out-of-range indices are ignored.
    /// Returns `true` if anything changed.
    pub fn toggle(&mut self, index: usize) -> bool {
        if index >= self.item_count {
            return false;
        }
        if self.mode == SelectionMode::Single {
            return self.select(index);
        }
        let row = IndexRuns::run(index, index);
        // R1563 — a row that is only *partly* selected is not selected as a
        // record, so toggling it takes the whole record rather than dropping
        // the cells it already had. The alternative reading — "it has
        // something, so clear it" — makes one `Ctrl`-click undo a selection the
        // user cannot see the boundary of.
        if self.selection().contains(index) {
            self.cells.remove(&row, &ColumnSpan::All, self.columns());
        } else {
            self.cells.add(&row, &ColumnSpan::All);
        }
        self.anchor_at(Some(index), None);
        true
    }

    /// `Shift`-extend to `index`: replace the selection with the inclusive
    /// range from the `anchor` to `index`, moving the active row to
    /// `index` while leaving the `anchor` put. With no anchor yet, behaves
    /// as a plain [`select`](Self::select). In `Single` mode it also
    /// collapses to a plain select. Out-of-range indices are ignored.
    /// Returns `true` if anything changed.
    pub fn extend_to(&mut self, index: usize) -> bool {
        if index >= self.item_count {
            return false;
        }
        if self.mode == SelectionMode::Single {
            return self.select(index);
        }
        let Some(anchor) = self.anchor else {
            return self.select(index);
        };
        let (lo, hi) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        // R1561 — the range is the unit, so extending across a million rows
        // writes one run rather than collecting a million indices. This is the
        // gesture that made the old representation's cost visible: `Shift+End`
        // on a large model is the *cheapest* thing a user can ask for and was
        // the most expensive thing the model could do.
        let moved = self.cursor != Some(index) || self.cursor_column.is_some();
        let changed = self
            .cells
            .replace(&IndexRuns::run(lo, hi), &ColumnSpan::All);
        self.cursor = Some(index);
        self.cursor_column = None;
        changed || moved
    }

    /// Select every row (`Ctrl+A`). A no-op in `Single` mode or on an empty
    /// dataset. Leaves the active row and `anchor` put. Returns `true` if
    /// the selection grew.
    ///
    /// R1561 — O(1) in the model's size: "everything is selected" is one run,
    /// whatever `item_count` is.
    pub fn select_all(&mut self) -> bool {
        if self.mode == SelectionMode::Single || self.item_count == 0 {
            return false;
        }
        self.cells
            .replace(&IndexRuns::run(0, self.item_count - 1), &ColumnSpan::All)
    }

    pub fn clear(&mut self) -> bool {
        let had = self.cells.clear();
        self.anchor_at(None, None);
        had
    }

    /// R1562 §5.40 — how much of the model the selection covers, in O(1).
    ///
    /// See [`SelectionExtent`] for why the empty model answers
    /// [`Empty`](SelectionExtent::Empty) rather than
    /// [`All`](SelectionExtent::All) (`0 == 0` is the reading this rejects).
    #[must_use]
    pub fn extent(&self) -> SelectionExtent {
        SelectionExtent::of(self.selected_count(), self.item_count)
    }

    /// R1562 §5.40 — the corner control's action: select every row, or clear
    /// when every row is already selected. Returns `true` if it changed
    /// anything.
    ///
    /// The toolkit's corner button is one-way — table view's documented
    /// behaviour is that clicking it "selects all cells in the view", with no
    /// second press that takes it back — so a toolkit user who select-alls by
    /// mistake clears by clicking a cell, which also *selects that cell*. The
    /// toggle is what every modern table's header checkbox does, and it is
    /// expressible here for one reason: [`extent`](Self::extent) is O(1), so "is
    /// everything selected" is a question the control can afford to ask on
    /// every press.
    ///
    /// In a `Single` model [`select_all`](Self::select_all) refuses, and the
    /// extent can only reach [`All`](SelectionExtent::All) on a one-row model,
    /// so this is a no-op — the same nothing `selectAll`
    /// does under `SingleSelection`. A grid states whether it offers the
    /// control at all when it declares its band, so an inert one is a decision
    /// rather than a press that quietly achieves nothing.
    pub fn toggle_all(&mut self) -> bool {
        if self.extent() == SelectionExtent::All {
            self.clear()
        } else {
            self.select_all()
        }
    }

    /// Admin replace to a single selection (or clear) — the persisted /
    /// form-default channel. Mirrors the pre-R780 single-index restore.
    fn set_selected(&mut self, index: Option<usize>) -> bool {
        let next = match index {
            Some(i) if i < self.item_count => Some(i),
            _ => None,
        };
        let want = next.map_or_else(IndexRuns::new, |i| IndexRuns::run(i, i));
        let moved = self.cursor != next || self.cursor_column.is_some();
        let changed = self.cells.replace(&want, &ColumnSpan::All);
        self.anchor_at(next, None);
        changed || moved
    }

    /// Admin replace to an arbitrary set (the multi-select restore channel).
    /// Indices at or beyond `item_count` are dropped. The active row / anchor
    /// become the greatest selected index (or `None` when empty). Returns
    /// `true` if the set changed.
    ///
    /// R1561 — the validity bound is applied per **run**
    /// ([`IndexRuns::clamped_below`]) rather than per index, so restoring a
    /// whole-model selection costs one comparison instead of one per row.
    pub fn set_selection(&mut self, indices: &IndexRuns) -> bool {
        let next = indices.clamped_below(self.item_count);
        let last = next.last();
        let moved = self.cursor != last || self.cursor_column.is_some();
        let changed = self.cells.replace(&next, &ColumnSpan::All);
        self.anchor_at(last, None);
        changed || moved
    }

    /// R1563 — admin replace of the **two-axis** selection (the persisted /
    /// restore channel for a grid that selects cells or columns).
    ///
    /// Clamped to the model on both axes, so a restored snapshot taken against
    /// a wider table cannot name a column that no longer exists. The active
    /// cell becomes the last selected row, as the row-axis restore does.
    pub fn set_cells(&mut self, cells: &CellSelection) -> bool {
        let next = cells.clamped(self.item_count, self.columns());
        let last = next.last_row();
        let moved = self.cursor != last || self.cursor_column.is_some();
        let changed = next != self.cells;
        self.cells = next;
        self.anchor_at(last, None);
        changed || moved
    }

    /// R1563 — plain press on the cell `(row, col)`: replace the selection with
    /// just that cell and make it the active cell + extension origin.
    ///
    /// Refuses (returns `false`, changing nothing) when the grid declared no
    /// column axis — see [`with_columns`](Self::with_columns) — or when either
    /// coordinate is outside the model, matching the row axis, where an
    /// out-of-range index has always been ignored rather than clamped.
    pub fn select_cell(&mut self, row: usize, col: usize) -> bool {
        if !self.cell_exists(row, col) {
            return false;
        }
        let moved = self.cursor != Some(row) || self.cursor_column != Some(col);
        let changed = self
            .cells
            .replace(&IndexRuns::run(row, row), &ColumnSpan::column(col));
        self.anchor_at(Some(row), Some(col));
        changed || moved
    }

    /// R1563 — `Ctrl`-press on the cell `(row, col)`: flip that one cell,
    /// leaving the rest of the selection alone. Collapses to
    /// [`select_cell`](Self::select_cell) in a `Single` model.
    pub fn toggle_cell(&mut self, row: usize, col: usize) -> bool {
        if !self.cell_exists(row, col) {
            return false;
        }
        if self.mode == SelectionMode::Single {
            return self.select_cell(row, col);
        }
        let rows = IndexRuns::run(row, row);
        let span = ColumnSpan::column(col);
        if self.cells.contains(row, col) {
            self.cells.remove(&rows, &span, self.columns());
        } else {
            self.cells.add(&rows, &span);
        }
        self.anchor_at(Some(row), Some(col));
        true
    }

    /// R1563 — `Shift`-press on the cell `(row, col)`: replace the selection
    /// with the **rectangle** from the extension origin to it, leaving the
    /// origin put. The toolkit's `item selection range(anchor, current)`.
    ///
    /// With no origin on both axes yet — the state a grid is in before its
    /// first cell press — it behaves as a plain
    /// [`select_cell`](Self::select_cell), the row axis's rule for a
    /// `Shift`-press with no anchor.
    pub fn extend_to_cell(&mut self, row: usize, col: usize) -> bool {
        if !self.cell_exists(row, col) {
            return false;
        }
        if self.mode == SelectionMode::Single {
            return self.select_cell(row, col);
        }
        let (Some(anchor_row), Some(anchor_col)) = (self.anchor, self.anchor_column) else {
            return self.select_cell(row, col);
        };
        let rows = span_between(anchor_row, row);
        let cols = span_between(anchor_col, col);
        let moved = self.cursor != Some(row) || self.cursor_column != Some(col);
        let changed = self.cells.replace(&rows, &ColumnSpan::Runs(cols));
        self.cursor = Some(row);
        self.cursor_column = Some(col);
        changed || moved
    }

    /// R1563 — plain press on the header section for `col`: replace the
    /// selection with that whole column. The toolkit `selectColumn`.
    ///
    /// The rows are named as a run against the model's **current** height, and
    /// deliberately so: the row axis is the one this framework windows, and a
    /// selection that silently claimed rows streaming in later is not what
    /// selecting a column means. The column axis is the one that outlives its
    /// count — see [`CellSelection`].
    pub fn select_column(&mut self, col: usize) -> bool {
        if !self.column_exists(col) || self.item_count == 0 {
            return false;
        }
        let moved = self.cursor != Some(0) || self.cursor_column != Some(col);
        let changed = self
            .cells
            .replace(&self.all_rows(), &ColumnSpan::column(col));
        self.anchor_at(Some(0), Some(col));
        changed || moved
    }

    /// R1563 — `Ctrl`-press on the header section for `col`: add the column to
    /// the selection, or take it back when it is already selected in every row.
    /// Collapses to [`select_column`](Self::select_column) in a `Single` model.
    pub fn toggle_column(&mut self, col: usize) -> bool {
        if !self.column_exists(col) || self.item_count == 0 {
            return false;
        }
        if self.mode == SelectionMode::Single {
            return self.select_column(col);
        }
        let rows = self.all_rows();
        let span = ColumnSpan::column(col);
        if self.cells.column_extent(col, self.item_count) == SelectionExtent::All {
            self.cells.remove(&rows, &span, self.columns());
        } else {
            self.cells.add(&rows, &span);
        }
        self.anchor_at(Some(0), Some(col));
        true
    }

    /// R1563 — `Shift`-press on the header section for `col`: select every
    /// column from the extension origin's column to it. With no column origin
    /// yet, behaves as a plain [`select_column`](Self::select_column).
    pub fn extend_to_column(&mut self, col: usize) -> bool {
        if !self.column_exists(col) || self.item_count == 0 {
            return false;
        }
        if self.mode == SelectionMode::Single {
            return self.select_column(col);
        }
        let Some(anchor_col) = self.anchor_column else {
            return self.select_column(col);
        };
        let moved = self.cursor_column != Some(col);
        let changed = self.cells.replace(
            &self.all_rows(),
            &ColumnSpan::Runs(span_between(anchor_col, col)),
        );
        self.cursor = Some(0);
        self.cursor_column = Some(col);
        changed || moved
    }

    /// R1563 — how much of `row` is selected, the tri-state the vertical band
    /// shows. [`SelectionExtent::Empty`] for every row of a grid with no column
    /// axis that has nothing selected, [`All`](SelectionExtent::All) for one it
    /// holds as a record.
    #[must_use]
    pub fn row_extent(&self, row: usize) -> SelectionExtent {
        match self.column_count {
            // With no column axis every span is `All`, so a row is selected or
            // it is not — asking `CellSelection` with a width of zero would
            // answer `Empty` for a row that *is* selected.
            None => {
                if self.selection().contains(row) {
                    SelectionExtent::All
                } else {
                    SelectionExtent::Empty
                }
            }
            Some(columns) => self.cells.row_extent(row, columns),
        }
    }

    /// R1563 — how much of `col` is selected, the tri-state the horizontal band
    /// shows.
    #[must_use]
    pub fn column_extent(&self, col: usize) -> SelectionExtent {
        self.cells.column_extent(col, self.item_count)
    }

    /// The toolkit `selectedColumns()` — the columns selected in
    /// every row. Empty when the grid has no column axis.
    #[must_use]
    pub fn column_selection(&self) -> IndexRuns {
        self.cells
            .columns_covering_all_rows(self.item_count, self.columns())
    }

    /// The whole row axis as one run — the rows a column selection covers.
    fn all_rows(&self) -> IndexRuns {
        IndexRuns::run(0, self.item_count.saturating_sub(1))
    }

    /// Whether `col` is a column of this model, and whether it has columns at
    /// all.
    fn column_exists(&self, col: usize) -> bool {
        self.column_count.is_some_and(|count| col < count)
    }

    /// Whether `(row, col)` is a cell of this model.
    fn cell_exists(&self, row: usize, col: usize) -> bool {
        row < self.item_count && self.column_exists(col)
    }
}

/// The inclusive run between two endpoints, in either order — the shared half
/// of every `Shift`-extend, on either axis.
fn span_between(anchor: usize, to: usize) -> IndexRuns {
    if anchor <= to {
        IndexRuns::run(anchor, to)
    } else {
        IndexRuns::run(to, anchor)
    }
}

/// R1544 §5.27 — the [`VirtualSelectExternal::on_grid_gesture`] observer's
/// signature: `(decoded sub-key, event name, modifiers)`, named so the boxed
/// field reads as one concept rather than as a nested type.
///
/// R1555 — the first argument is the decoded
/// [`GridSendKey`] rather than a
/// [`CellIndex`](crate::model_index::CellIndex). It was a `CellIndex` while a
/// cell was the only sub-key with a
/// view-level behaviour behind it, and that shape is not extensible by
/// construction: R1555's step affordance addresses `EditorStep { row, col, up }`,
/// which no `CellIndex` can carry, so the alternative was a sibling hook per arm
/// of a grammar that already has a type. Taking the grammar's own type means a
/// later arm needs no hook at all.
type GridGestureFn = dyn Fn(GridSendKey, &str, Modifiers);

/// R746 §5.27 §5.38 — single-select-by-index coordinator for a virtualized
/// list.
///
/// Holds the selected **data index** (not a per-leaf bit), so selection is
/// independent of which rows are currently materialized. `item_count`
/// bounds every mutation: an out-of-range index is rejected (a malformed
/// wire payload can never select a non-existent row).
///
/// Like every selection coordinator in the catalogue
/// ([`RadioGroup`](crate::widgets::radio_group) /
/// [`ListBox`](crate::widgets::listbox) / [`Table`](crate::widgets::table))
/// it emits a §5.20 `"selected"` intent (the new index as
/// [`IntrospectValue::Int`]) on the *interaction* path so AI / automation
/// observe the selection on the intent channel — not only by polling
/// `query("selected")`. The admin restore path
/// ([`set_selected`](Self::set_selected) / [`clear`](Self::clear)) is
/// silent, exactly as [`selection::replace_selection`](crate::widgets::selection::replace_selection)
/// is (restoration is not interaction).
///
/// The §5.20 pending queue is owned by the shared
/// [`IntentEmitter`] wrapper — the same one
/// `RadioGroupExternal` / `ListBoxExternal` / `TableExternal` use — rather
/// than a hand-rolled `pending: Vec<Intent>` field (the pre-R51.5
/// anti-pattern that `IntentEmitter` exists to eliminate; R746.3 brought
/// this lone outlier back into that SSOT). This widget is a plain holder,
/// so it does *not* implement [`WidgetTransition`](crate::widgets::WidgetTransition)
/// auto-dispatch — it pushes the intent explicitly on the interaction edge.
pub struct VirtualSelectExternal {
    em: IntentEmitter<VirtualSelect>,
    /// R1544 §5.27 — an optional observer of decoded grid gestures, for the
    /// view-level behaviours a press drives beyond selection. `None` for every
    /// consumer that only selects.
    grid_gesture: Option<Box<GridGestureFn>>,
    /// R1563 — what a press on a horizontal header section does.
    section_press: SectionPress,
}

/// R1563 — which axis a decoded press resolved to, so one chord vocabulary can
/// reach all three. See [`VirtualSelectExternal::press`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PressAxis {
    /// A whole record — a cell press under
    /// [`SelectionBehavior::SelectRows`], a row-header press on any grid, or a bare
    /// list item.
    Row(usize),
    /// A whole column — a header-section press under
    /// [`SectionPress::Select`], or a cell press under
    /// [`SelectionBehavior::SelectColumns`].
    Column(usize),
    /// One cell — a cell press under [`SelectionBehavior::SelectItems`].
    Cell(usize, usize),
}

impl core::fmt::Debug for VirtualSelectExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtualSelectExternal")
            .field("selected", &self.selected())
            .field("item_count", &self.item_count())
            .finish()
    }
}

impl VirtualSelectExternal {
    /// Construct a **single-select** coordinator over an `item_count`-row
    /// dataset, nothing selected (the pre-R780 default — every existing
    /// consumer keeps its exact behaviour).
    #[must_use]
    pub fn new(item_count: usize) -> Self {
        Self {
            em: IntentEmitter::new(VirtualSelect::new(item_count, SelectionMode::Single)),
            grid_gesture: None,
            section_press: SectionPress::Inert,
        }
    }

    /// R1544 §5.27 — observe every decoded grid gesture with its event name and
    /// modifiers, *before* this coordinator applies its own selection
    /// transition.
    ///
    /// # Why the selection coordinator carries it
    ///
    /// The toolkit splits these responsibilities across two objects: item
    /// selection model decides what a click selects, and abstract item view
    /// decides what a click *else* does — `edit(index, DoubleClicked, event)`. Here the pointer wire has
    /// exactly one destination per paint tag, and for a windowed grid that
    /// destination is this coordinator, so the view-level hook is offered from
    /// here rather than invented as a second External the cells could not
    /// address.
    ///
    /// # Why before the selection transition
    ///
    /// The toolkit's `SelectedClicked` trigger means *a click on a cell that was already
    /// selected* — the slow-rename gesture. Running the observer after the
    /// transition would make every plain click look "already selected", which
    /// is the difference between a rename gesture and an unusable one.
    ///
    /// R1555 — **every** decoded sub-key is offered, not only a cell: a header
    /// (`h<col>`), a group (`g<n>`) and a step affordance
    /// (`su<row>_<col>` / `sd<row>_<col>`) all reach the observer, which matches
    /// the arm it cares about. A bare list-item key has no grid structure and is
    /// not decodable into one, so it is not offered.
    #[must_use]
    pub fn on_grid_gesture(mut self, f: impl Fn(GridSendKey, &str, Modifiers) + 'static) -> Self {
        self.grid_gesture = Some(Box::new(f));
        self
    }

    /// Construct a **multi-select** coordinator (R780): `Shift`-range,
    /// `Ctrl`-toggle, and `Ctrl+A` select-all hold an arbitrary index set
    /// with an `anchor` origin. The item selection model `ExtendedSelection`
    /// analogue.
    #[must_use]
    pub fn new_multi(item_count: usize) -> Self {
        Self {
            em: IntentEmitter::new(VirtualSelect::new(item_count, SelectionMode::Multi)),
            grid_gesture: None,
            section_press: SectionPress::Inert,
        }
    }

    /// R1563 — declare the model's width, opening the column axis
    /// ([`VirtualSelect::with_columns`]). Without it every cell- and
    /// column-addressed path refuses, and `query("column_count")` answers
    /// `null` so the refusal is discoverable without provoking one.
    #[must_use]
    pub fn with_columns(mut self, column_count: usize) -> Self {
        self.em.inner.column_count = Some(column_count);
        self
    }

    /// R1563 — declare what a press selects (the toolkit `setSelectionBehavior`).
    #[must_use]
    pub fn with_behavior(mut self, behavior: SelectionBehavior) -> Self {
        self.em.inner.behavior = behavior;
        self
    }

    /// R1563 — declare what a press on a horizontal header section does.
    ///
    /// [`SectionPress::Inert`] (the default) is every grid whose headers are
    /// the sort control. See [`SectionPress`] for why this is declared rather
    /// than inferred from the selection behaviour.
    #[must_use]
    pub const fn with_section_press(mut self, press: SectionPress) -> Self {
        self.section_press = press;
        self
    }

    /// The active row (the `query("selected")` value, the
    /// item selection model `currentIndex`): the sole selected row in a
    /// single-select model, the keyboard-navigation reference in a
    /// multi-select one. `None` when nothing is selected.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.em.inner.cursor
    }

    /// R1563 — the selection with both its axes
    /// ([`VirtualSelect::cells`]).
    #[must_use]
    pub fn cells(&self) -> &CellSelection {
        self.em.inner.cells()
    }

    /// R1563 — the model's width, or `None` when this grid has no column axis.
    #[must_use]
    pub fn column_count(&self) -> Option<usize> {
        self.em.inner.column_count()
    }

    /// R1563 — what a press selects.
    #[must_use]
    pub fn behavior(&self) -> SelectionBehavior {
        self.em.inner.behavior()
    }

    /// R1563 — what a press on a horizontal header section does.
    #[must_use]
    pub fn section_press(&self) -> SectionPress {
        self.section_press
    }

    /// R1563 — whether the cell at `(row, col)` is selected.
    #[must_use]
    pub fn is_cell_selected(&self, row: usize, col: usize) -> bool {
        self.em.inner.is_cell_selected(row, col)
    }

    /// R1563 — the column half of the active cell (the toolkit `currentIndex()`).
    #[must_use]
    pub fn cursor_column(&self) -> Option<usize> {
        self.em.inner.cursor_column()
    }

    /// R1563 — how much of `row` is selected: the tri-state the vertical band
    /// shows ([`VirtualSelect::row_extent`]).
    #[must_use]
    pub fn row_extent(&self, row: usize) -> SelectionExtent {
        self.em.inner.row_extent(row)
    }

    /// R1563 — how much of `col` is selected: the tri-state the horizontal band
    /// shows ([`VirtualSelect::column_extent`]).
    #[must_use]
    pub fn column_extent(&self, col: usize) -> SelectionExtent {
        self.em.inner.column_extent(col)
    }

    /// The toolkit `selectedColumns()` — the columns selected in
    /// every row.
    #[must_use]
    pub fn column_selection(&self) -> IndexRuns {
        self.em.inner.column_selection()
    }

    /// The full selection, as the runs it is made of. Cardinality ≤ 1 in a
    /// single-select model.
    #[must_use]
    pub fn selection(&self) -> &IndexRuns {
        self.em.inner.selection()
    }

    /// R1561 — how many rows are selected, without building the list the
    /// toolkit has to build to count them. See [`VirtualSelect::selected_count`].
    #[must_use]
    pub fn selected_count(&self) -> usize {
        self.em.inner.selected_count()
    }

    /// Whether `index` is selected.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.em.inner.is_selected(index)
    }

    /// The range-extension origin (`anchor`), or `None`.
    #[must_use]
    pub fn anchor(&self) -> Option<usize> {
        self.em.inner.anchor
    }

    /// The selection-cardinality policy this coordinator was built with.
    #[must_use]
    pub fn mode(&self) -> SelectionMode {
        self.em.inner.mode
    }

    /// Total dataset size.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.em.inner.item_count
    }

    /// Move the active row to `index` and replace the selection with just it
    /// (a plain click / unmodified arrow) — the **interaction** path. Sets
    /// the `anchor`. Out-of-range indices are ignored. On a real change,
    /// queues the §5.20 selection intent. Returns `true` if it changed.
    pub fn select(&mut self, index: usize) -> bool {
        if !self.em.inner.select(index) {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// `Ctrl`-toggle `index` (multi-select interaction): flip its membership,
    /// leaving the rest selected. Collapses to [`select`](Self::select) in a
    /// single-select model. On a real change, queues the selection intent.
    /// Returns `true` if it changed.
    pub fn toggle(&mut self, index: usize) -> bool {
        if !self.em.inner.toggle(index) {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// `Shift`-extend the selection to `index` (multi-select interaction):
    /// the inclusive range from the `anchor`. Collapses to
    /// [`select`](Self::select) in a single-select model (or with no anchor
    /// yet). On a real change, queues the selection intent. Returns `true`
    /// if it changed.
    pub fn extend_to(&mut self, index: usize) -> bool {
        if !self.em.inner.extend_to(index) {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// `Ctrl+A` select-all (multi-select interaction). A no-op in a
    /// single-select model. On a real change, queues the selection intent.
    /// Returns `true` if the selection grew.
    pub fn select_all(&mut self) -> bool {
        if !self.em.inner.select_all() {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// Clear the selection. Returns `true` if something was selected.
    pub fn clear(&mut self) -> bool {
        self.em.inner.clear()
    }

    /// R1562 §5.40 — how much of the model the selection covers
    /// ([`VirtualSelect::extent`]): what the corner control shows and what its
    /// next press will do.
    #[must_use]
    pub fn extent(&self) -> SelectionExtent {
        self.em.inner.extent()
    }

    /// R1562 §5.40 — the corner control's press ([`VirtualSelect::toggle_all`]):
    /// select every row, or clear when every row is already selected.
    ///
    /// An **interaction**, so it queues the §5.20 selection intent on both
    /// legs — unlike [`clear`](Self::clear), which is the silent admin /
    /// restore channel [`set_selection`](Self::set_selection) belongs to.
    /// Returns `true` if it changed anything.
    pub fn toggle_all(&mut self) -> bool {
        if !self.em.inner.toggle_all() {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// Replace the selection directly with a single index (the admin /
    /// persisted-restore / form-default channel — not an interaction).
    /// `None` or an out-of-range index clears. Returns `true` if it changed.
    pub fn set_selected(&mut self, index: Option<usize>) -> bool {
        self.em.inner.set_selected(index)
    }

    /// Replace the selection directly with an arbitrary set (the
    /// multi-select admin / persisted-restore channel — not an
    /// interaction). Out-of-range indices are dropped. Returns `true` if it
    /// changed.
    pub fn set_selection(&mut self, indices: &IndexRuns) -> bool {
        self.em.inner.set_selection(indices)
    }

    /// R1563 — replace the two-axis selection directly (the admin /
    /// persisted-restore channel, not an interaction). Clamped to the model on
    /// both axes. Returns `true` if it changed.
    pub fn set_cells(&mut self, cells: &CellSelection) -> bool {
        self.em.inner.set_cells(cells)
    }

    /// R1563 — plain press on the cell `(row, col)`
    /// ([`VirtualSelect::select_cell`]) — the **interaction** path. On a real
    /// change, queues the §5.20 selection intent.
    pub fn select_cell(&mut self, row: usize, col: usize) -> bool {
        self.interaction(|model| model.select_cell(row, col))
    }

    /// R1563 — `Ctrl`-press on the cell `(row, col)`
    /// ([`VirtualSelect::toggle_cell`]).
    pub fn toggle_cell(&mut self, row: usize, col: usize) -> bool {
        self.interaction(|model| model.toggle_cell(row, col))
    }

    /// R1563 — `Shift`-press on the cell `(row, col)`
    /// ([`VirtualSelect::extend_to_cell`]).
    pub fn extend_to_cell(&mut self, row: usize, col: usize) -> bool {
        self.interaction(|model| model.extend_to_cell(row, col))
    }

    /// R1563 — plain press on the header section for `col`
    /// ([`VirtualSelect::select_column`]).
    pub fn select_column(&mut self, col: usize) -> bool {
        self.interaction(|model| model.select_column(col))
    }

    /// R1563 — `Ctrl`-press on the header section for `col`
    /// ([`VirtualSelect::toggle_column`]).
    pub fn toggle_column(&mut self, col: usize) -> bool {
        self.interaction(|model| model.toggle_column(col))
    }

    /// R1563 — `Shift`-press on the header section for `col`
    /// ([`VirtualSelect::extend_to_column`]).
    pub fn extend_to_column(&mut self, col: usize) -> bool {
        self.interaction(|model| model.extend_to_column(col))
    }

    /// R1563 — run one model transition on the **interaction** path: apply it,
    /// and queue the §5.20 selection intent when it changed something.
    ///
    /// The six second-axis verbs share it rather than each repeating the
    /// `if !changed { return false }` / `push_selection_intent` pair the row
    /// verbs were written with one at a time — a shape that is right three
    /// times and a silent omission the fourth.
    fn interaction(&mut self, apply: impl FnOnce(&mut VirtualSelect) -> bool) -> bool {
        if !apply(&mut self.em.inner) {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// Queue the §5.20 selection intent for the **interaction** path,
    /// carrying the mode-appropriate payload: a single-select model emits
    /// `"selected"` with the active index (the pre-R780 wire, so every
    /// existing consumer is unchanged), a multi-select model emits
    /// `"selection"` with the full set as a JSON array of indices.
    fn push_selection_intent(&mut self) {
        match self.em.inner.mode {
            SelectionMode::Single => {
                let value = self.selected_value();
                self.em.push(Intent::new_static("selected", value));
            }
            SelectionMode::Multi => {
                let value = self.selection_value();
                self.em.push(Intent::new_static("selection", value));
            }
        }
    }

    /// Drive the composite pointer channel: on the activation edge
    /// (`PointerUp` or `KeyboardActivate`) select the addressed **row**.
    /// Every other pointer-arc event (`PointerEnter` / `PointerDown` /
    /// `PointerLeave`) the router replays is a harmless no-op — single
    /// selection has no hover/press feedback at the row level.
    ///
    /// The same coordinator serves both a virtualized **list** and a
    /// virtualized **grid** (R777), so the composite key is one of two
    /// shapes, both selecting the row:
    ///
    /// - **list item** `"<row>"` — the windowed `vlist#<row>` row.
    /// - **grid cell** `"<row>_<col>"` — the windowed `vtbl#<row>_<col>`
    ///   cell. Selecting any cell selects its row (the WAI-ARIA / the toolkit
    ///   item selection model `SelectRows` behaviour: the column is
    ///   irrelevant to a single-row selection). A grid column-header click
    ///   arrives as `"h<col>"`, which has no leading row index and is
    ///   ignored here (sort is a separate axis, not this coordinator's).
    /// - **row header** `"r<row>"` (R1562) — the vertical band's section, the toolkit
    ///   header view with `sectionsClickable`. It reaches the transition
    ///   above by *answering with its row*, so the chord vocabulary is one
    ///   implementation rather than the two the toolkit keeps
    ///   (`mousePressEvent` → `selectRow` beside
    ///   `selectionCommand`). This is the only part of a row
    ///   that is always on screen — the band is pinned against the horizontal
    ///   scroll (R1548) — so on a wide grid it is the address a row *has* when
    ///   none of its cells is painted.
    /// - **corner** `"c"` (R1562) — the toolkit's table corner button: addresses the
    ///   whole model, so it toggles the selection's extent instead of moving
    ///   to a row.
    ///
    /// The grid grammar is decoded by the shared
    /// [`GridSendKey`] SSOT (R777.1) — a
    /// cell `"<row>_<col>"` yields its row, a header `"h<col>"` yields
    /// `None` (ignored). A bare list-item key `"<row>"` has no grid
    /// structure, so it falls back to a plain integer parse: one
    /// coordinator, both collection shapes, one wire grammar.
    fn handle_send(&mut self, payload: &str) {
        let Some((key, event_name, modifiers)) = crate::composite_tag::split_send_payload(payload)
        else {
            return;
        };
        let parsed = crate::composite_tag::GridSendKey::parse(key);
        // R1544 — the view-level hook, offered before the selection moves so
        // "was this cell already selected" is still answerable (the toolkit's
        // `SelectedClicked`). R1555 — every decoded arm is offered; the observer matches the
        // one it handles.
        if let (Some(observe), Some(grid_key)) = (self.grid_gesture.as_ref(), parsed) {
            observe(grid_key, event_name, modifiers);
        }
        // R1562 — the corner addresses the whole model, so it has no row to
        // fall through to. Handled on the same activation edge every other
        // address is, through the same modifier-blind path: the toolkit's
        // corner button ignores modifiers too, and a chorded select-all has no
        // meaning (`Ctrl`-toggling *every* row is `toggle_all` already).
        if parsed == Some(crate::composite_tag::GridSendKey::Corner) {
            if crate::input::is_activation_event(event_name) {
                self.toggle_all();
            }
            return;
        }
        if !crate::input::is_activation_event(event_name) {
            return;
        }
        // R1563 — a horizontal section press, when the grid declared that its
        // sections select. Handled before the row fall-through because a header
        // has no row: `GridSendKey::row()` answers `None` for it, so without
        // this the press would be dropped exactly as it was before R1563.
        if let (Some(grid_key), SectionPress::Select) = (parsed, self.section_press)
            && grid_key.row().is_none()
            && let Some(col) = grid_key.col()
        {
            self.press(PressAxis::Column(col), modifiers);
            return;
        }
        let row = match parsed {
            Some(grid_key) => grid_key.row(),
            None => key.parse::<usize>().ok(),
        };
        let Some(index) = row else {
            return;
        };
        // R1563 — what a press on a *cell* selects is the grid's declared
        // behaviour (the toolkit `setSelectionBehavior`). A row header carries no column and
        // always selects its row: it is the address of a record, which is why
        // the band exists. A bare list-item key has no column either.
        let axis = match (self.behavior(), parsed.and_then(GridSendKey::col)) {
            (SelectionBehavior::SelectItems, Some(col)) => PressAxis::Cell(index, col),
            (SelectionBehavior::SelectColumns, Some(col)) => PressAxis::Column(col),
            _ => PressAxis::Row(index),
        };
        self.press(axis, modifiers);
    }

    /// R781 §5.35 §5.40 / R1563 — the modifier-aware press, the pointer peer of
    /// the [`nav_select_key`] keyboard ops, for whichever axis the address
    /// resolved to.
    ///
    /// The chord policy itself is decoded by the R880.1
    /// [`SelectionChord`](crate::input::SelectionChord) SSOT: `Ctrl` / `Cmd`
    /// toggles membership, `Shift` extends from the anchor (the ordered-model
    /// meaning of *extend*), a plain press moves and replaces. In a
    /// single-select model every toggle / extend collapses to a plain select,
    /// so a chorded click on a single-select list still just selects — exactly
    /// the pre-R781 behaviour.
    ///
    /// One function over the **product** of chord and axis, rather than three
    /// per-axis copies of the same three-arm match: adding an axis, or a fourth
    /// chord, then fails to compile until every combination is stated. Three
    /// copies would each still compile while one of them quietly kept the old
    /// vocabulary — which is the shape R1562 removed on the row axis and the
    /// reason a band press and a cell press cannot mean different things.
    fn press(&mut self, axis: PressAxis, modifiers: Modifiers) {
        use crate::input::SelectionChord as Chord;
        match (Chord::from_modifiers(modifiers), axis) {
            (Chord::Replace, PressAxis::Row(row)) => self.select(row),
            (Chord::Toggle, PressAxis::Row(row)) => self.toggle(row),
            (Chord::Extend, PressAxis::Row(row)) => self.extend_to(row),
            (Chord::Replace, PressAxis::Column(col)) => self.select_column(col),
            (Chord::Toggle, PressAxis::Column(col)) => self.toggle_column(col),
            (Chord::Extend, PressAxis::Column(col)) => self.extend_to_column(col),
            (Chord::Replace, PressAxis::Cell(row, col)) => self.select_cell(row, col),
            (Chord::Toggle, PressAxis::Cell(row, col)) => self.toggle_cell(row, col),
            (Chord::Extend, PressAxis::Cell(row, col)) => self.extend_to_cell(row, col),
        };
    }

    /// The active index as an `IntrospectValue` (`Int` or `Null`) — the
    /// uniform return for the single-select mutating `invoke` paths and the
    /// `"selected"` query. Delegates to the [`selected_to_value`] serialize
    /// SSOT (the encode peer of [`read_selected`]).
    fn selected_value(&self) -> IntrospectValue {
        selected_to_value(self.selected())
    }

    /// The full selection as a JSON array of indices — the uniform return
    /// for the multi-select mutating `invoke` paths and the `"selection"`
    /// query. Delegates to the [`selection_to_value`] serialize SSOT (the
    /// encode peer of [`read_selection`]).
    fn selection_value(&self) -> IntrospectValue {
        selection_to_value(self.em.inner.selection())
    }

    /// R1563 — the two-axis selection as its wire form, through the
    /// [`cells_to_value`] encode SSOT (the peer of [`read_cells`]).
    fn cells_value(&self) -> IntrospectValue {
        cells_to_value(self.cells())
    }

    /// R1563 — run a cell-addressed verb: decode the `[row, col]` argument,
    /// apply, and answer with the resulting two-axis selection.
    ///
    /// A grid with no column axis **refuses** (`Rejected`) rather than quietly
    /// doing nothing and answering with an unchanged selection — the caller
    /// asked for something this grid cannot do, and `Rejected` is the outcome
    /// that says so. The toolkit's `selectColumn` returns `void` and a call
    /// on a view with no model is simply lost. An out-of-range *index* is not
    /// that case: it is the same ignored-index contract the row verbs have had
    /// since R746, so it answers with the unchanged selection.
    fn cell_action(
        &mut self,
        args: &IntrospectValue,
        apply: fn(&mut Self, usize, usize) -> bool,
    ) -> Result<IntrospectValue, InvokeError> {
        let (row, col) = decode_cell_arg(args).ok_or(InvokeError::TypeMismatch)?;
        if self.column_count().is_none() {
            return Err(InvokeError::rejected(
                "this surface selects whole rows: it has no column extent, \
                 so a cell address names nothing",
            ));
        }
        apply(self, row, col);
        Ok(self.cells_value())
    }

    /// R1563 — run a column-addressed verb. Same refusal rule as
    /// [`cell_action`](Self::cell_action).
    fn column_action(
        &mut self,
        args: &IntrospectValue,
        apply: fn(&mut Self, usize) -> bool,
    ) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Int(col) = *args else {
            return Err(InvokeError::TypeMismatch);
        };
        let col = usize::try_from(col).map_err(|_| InvokeError::TypeMismatch)?;
        if self.column_count().is_none() {
            return Err(InvokeError::rejected(
                "this surface selects whole rows: it has no column extent, \
                 so a column address names nothing",
            ));
        }
        apply(self, col);
        Ok(self.cells_value())
    }

    /// The mode as its canonical wire string (`"single"` / `"multi"`).
    fn mode_value(&self) -> IntrospectValue {
        IntrospectValue::Text(
            match self.mode() {
                SelectionMode::Single => "single",
                SelectionMode::Multi => "multi",
            }
            .to_string(),
        )
    }
}

impl External for VirtualSelectExternal {
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

    /// Drain the queued §5.20 `"selected"` intents (one per interaction
    /// that changed the selection) — the same contract
    /// [`RadioGroup`](crate::widgets::radio_group) /
    /// [`ListBox`](crate::widgets::listbox) honour, so AI / automation see
    /// the selection on the intent channel.
    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        self.em.drain(sink);
    }

    /// Dirty exactly while a `"selected"` intent awaits draining; the
    /// selection value itself only changes through `invoke` / `intervene`,
    /// which the framework already follows with a repaint.
    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }
}

impl ExternalIntrospect for VirtualSelectExternal {
    fn schema(&self) -> IntrospectSchema {
        // `selected` — settable active index (query + intervene).
        // `selection` — settable selection as a JSON array of `[first, last]`
        //   runs (R780 multi-select, R1561 run form; query + intervene).
        // `selection_count` — how many ROWS those runs cover (R1561, query
        //   only). Derived, so it is not settable: writing it would be a
        //   second way to say what `selection` already says.
        // `anchor` — the range-extension origin (query only).
        // `mode` — `"single"` / `"multi"` cardinality policy (query only).
        // `item_count` — construction-fixed dataset size (query only).
        // `cells` — settable two-axis selection as a JSON array of bands
        //   (R1563; query + intervene). `selection` is its `All`-span band.
        // `column_selection` — the toolkit `selectedColumns()` (R1563, query only).
        // `column_count` — the model's width, `null` when this grid declared no
        //   column axis, in which case every cell / column path refuses. Query
        //   only: a width is what the grid IS, not a knob.
        // `behavior` / `section_press` — what a press selects, and what a
        //   header-section press does (R1563, query only; both are
        //   construction-fixed the way `mode` is).
        // `selected_column` / `anchor_column` — the column halves of the active
        //   cell and of the extension origin (R1563, query only).
        // `cell_count` / `band_count` — how many CELLS the selection covers and
        //   how many bands hold them: the extension and the size of the
        //   statement, which on this axis are different numbers.
        //
        // R1563 — the **verbs are declared too**, as `SchemaChannel::Invoke`.
        // Six of them were absent before this round (`select`, `toggle`,
        // `extend_to`, `select_all`, `toggle_all`, `clear`), so an agent reading
        // `$schema` for this surface was told what it could read and nothing
        // about what it could do. R1562's census found that and recorded the
        // cause as a type that could not say "action" — which was **wrong**:
        // `SchemaField::action` has existed since R1504. It was six missing
        // lines. `send` moves from a read to an action for the same reason: it
        // is called, never queried, and it was declared as a readable string.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("selected", "int"),
                    SchemaField::new("selected_column", "int"),
                    SchemaField::new("selection", "json"),
                    SchemaField::new("selection_count", "int"),
                    SchemaField::new("cells", "json"),
                    SchemaField::new("cell_count", "int"),
                    SchemaField::new("band_count", "int"),
                    SchemaField::new("column_selection", "json"),
                    SchemaField::new("column_count", "int"),
                    SchemaField::new("behavior", "string"),
                    SchemaField::new("section_press", "string"),
                    SchemaField::new("anchor", "int"),
                    SchemaField::new("anchor_column", "int"),
                    SchemaField::new("mode", "string"),
                    SchemaField::new("item_count", "int"),
                    SchemaField::action("select", "int"),
                    SchemaField::action("toggle", "json"),
                    SchemaField::action("extend_to", "json"),
                    SchemaField::action("select_all", "json"),
                    SchemaField::action("toggle_all", "json"),
                    SchemaField::action("clear", "int"),
                    SchemaField::action("select_cell", "json"),
                    SchemaField::action("toggle_cell", "json"),
                    SchemaField::action("extend_to_cell", "json"),
                    SchemaField::action("select_column", "json"),
                    SchemaField::action("toggle_column", "json"),
                    SchemaField::action("extend_to_column", "json"),
                    SchemaField::action("send", "int"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            // A schema-listed path must always return a value (the RPC
            // layer treats a `None` from a declared slot as
            // `UnknownIntrospectPath`); an empty selection reports
            // `Null` (present-but-empty) for the single index, an empty
            // JSON array for the set.
            "selected" => Some(self.selected_value()),
            "selection" => Some(self.selection_value()),
            // R1561 — the row count, answered from the runs. The toolkit has
            // no such accessor: `selectedRows().size()` builds one model index per selected row to
            // read the list's length.
            "selection_count" => Some(
                i64::try_from(self.selected_count())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            // R1563 — the two-axis selection, and the two numbers that describe
            // it: how many cells it covers, and how many bands say so. On the
            // row axis those collapsed into one question; here a select-all
            // over a million rows and two hundred columns is 200 000 000 cells
            // in **one** band, and an agent budgeting a read wants the second
            // number, not the first.
            "cells" => Some(self.cells_value()),
            "cell_count" => Some(usize_value(
                self.cells().cell_count(self.em.inner.columns()),
            )),
            "band_count" => Some(usize_value(self.cells().band_count())),
            "column_selection" => Some(selection_to_value(&self.column_selection())),
            // `null` rather than `0`: a grid with no column axis is not a grid
            // zero columns wide, and the difference is exactly what a caller
            // must know before addressing a cell.
            "column_count" => Some(
                self.column_count()
                    .and_then(|c| i64::try_from(c).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            "behavior" => Some(IntrospectValue::Text(
                match self.behavior() {
                    SelectionBehavior::SelectRows => "rows",
                    SelectionBehavior::SelectColumns => "columns",
                    SelectionBehavior::SelectItems => "items",
                }
                .to_string(),
            )),
            "section_press" => Some(IntrospectValue::Text(
                match self.section_press() {
                    SectionPress::Inert => "inert",
                    SectionPress::Select => "select",
                }
                .to_string(),
            )),
            "selected_column" => Some(
                self.cursor_column()
                    .and_then(|i| i64::try_from(i).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            "anchor_column" => Some(
                self.em
                    .inner
                    .anchor_column()
                    .and_then(|i| i64::try_from(i).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            "anchor" => Some(
                self.anchor()
                    .and_then(|i| i64::try_from(i).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            "mode" => Some(self.mode_value()),
            "item_count" => Some(
                i64::try_from(self.item_count())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The active index is a writable axis (admin / restore). `Int`
            // selects a single row (out-of-range clears); `Null` clears.
            "selected" => match value {
                IntrospectValue::Int(i) => {
                    let index = usize::try_from(i).ok();
                    self.set_selected(index);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_selected(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // The full set is a writable axis (multi-select admin / restore):
            // a JSON array of `[first, last]` runs replaces the selection
            // (rows at or beyond `item_count` dropped); `Null` clears.
            //
            // R1561 — the decode canonicalises
            // ([`IndexRuns`]'s `From<Vec<(usize, usize)>>`), so a client may
            // send its runs in any order, overlapping or abutting, and the
            // model reaches exactly the state the constructors build. A
            // malformed array is a type mismatch rather than a silently
            // partial selection: the pre-R1561 decode filtered non-integers
            // out one by one, so `[0, "x", 2]` selected two rows and said
            // nothing about the third.
            "selection" => match value {
                IntrospectValue::Json(json @ serde_json::Value::Array(_)) => {
                    let runs: IndexRuns =
                        serde_json::from_value(json).map_err(|_| InterveneError::TypeMismatch)?;
                    self.set_selection(&runs);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_selection(&IndexRuns::new());
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R1563 — the two-axis restore. Canonicalising and clamped on both
            // axes ([`CellSelection`]'s `From<Vec<SelectionBand>>` and
            // [`VirtualSelect::set_cells`]), so a payload whose bands overlap,
            // repeat a span or name a column the model does not have reaches
            // exactly the state the mutators build. A malformed band — a column
            // span that is neither `"all"` nor an array of runs — is a type
            // mismatch rather than a silently smaller selection.
            "cells" => match value {
                IntrospectValue::Json(json @ serde_json::Value::Array(_)) => {
                    let cells: CellSelection =
                        serde_json::from_value(json).map_err(|_| InterveneError::TypeMismatch)?;
                    self.set_cells(&cells);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_cells(&CellSelection::new());
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "selection_count" | "cell_count" | "band_count" | "column_selection"
            | "column_count" | "behavior" | "section_press" | "selected_column"
            | "anchor_column" | "anchor" | "mode" | "item_count" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first direct selection — returns the resulting selected
            // index (or Null) so the caller sees the outcome in one
            // round-trip.
            "select" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(index) = usize::try_from(i) {
                        self.select(index);
                    }
                    Ok(self.selected_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R780 multi-select interaction funnel — `Ctrl`-toggle a row,
            // `Shift`-extend the range to a row, or `Ctrl+A` select all.
            // Each returns the resulting full set as a JSON array so the
            // caller (keyboard controller / AI / future modifier-click wire)
            // sees the outcome in one round-trip. In a single-select model
            // `toggle` / `extend_to` collapse to a plain move and
            // `select_all` is a no-op.
            "toggle" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(index) = usize::try_from(i) {
                        self.toggle(index);
                    }
                    Ok(self.selection_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "extend_to" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(index) = usize::try_from(i) {
                        self.extend_to(index);
                    }
                    Ok(self.selection_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "select_all" => {
                self.select_all();
                Ok(self.selection_value())
            }
            // R1562 — the corner control's verb, so the select-all affordance
            // is drivable headlessly and not only by a click on its pixels (§2
            // #2: the RPC path is the primary one, not a mirror of the pointer
            // path). The toolkit has no such verb — `selectAll()` is one-way, and taking
            // a full selection back means calling `clearSelection()` after asking `selectedRows().size()` whether
            // it is full.
            "toggle_all" => {
                self.toggle_all();
                Ok(self.selection_value())
            }
            "clear" => {
                self.clear();
                Ok(self.selected_value())
            }
            // R1563 — the second axis's six verbs. Each answers with the
            // resulting two-axis selection, so a caller sees the outcome in one
            // round-trip exactly as the row verbs do — and it has to be `cells`
            // rather than `selection`, because a cell selection has no rows in
            // the `All` band and `selection` would answer `[]` after a press
            // that plainly selected something.
            //
            // A cell is addressed as a two-element array. Not `"<row>_<col>"`:
            // that spelling belongs to the *pointer* channel (`send`), where the
            // key is a paint tag's sub-region; here the argument is a pair of
            // numbers and JSON has a way to say so.
            "select_cell" => self.cell_action(&args, Self::select_cell),
            "toggle_cell" => self.cell_action(&args, Self::toggle_cell),
            "extend_to_cell" => self.cell_action(&args, Self::extend_to_cell),
            "select_column" => self.column_action(&args, Self::select_column),
            "toggle_column" => self.column_action(&args, Self::toggle_column),
            "extend_to_column" => self.column_action(&args, Self::extend_to_column),
            // R51.42 §5.35 composite pointer channel: the windowed
            // `vlist#<i>` rows route the full pointer arc here as
            // `invoke("send", "<i>:PointerEnter")` … `"<i>:PointerUp")`.
            // The `"<i>:<EventName>"` wire is split by the R660
            // [`composite_tag::parse_send_payload`] SSOT; the key type is
            // `usize` because the sub-region is a numeric data index (the
            // Table-cell model, not the named-region SpinButton model).
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(self.selected_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// R782 §5.40 — **deserialize peer** of the coordinator's `"selection"`
/// query (the [`selection_to_value`] encode): decode the multi-select index set
/// from any introspection surface that emits the canonical `"selection"` array
/// — a standalone [`VirtualSelectExternal`] in `Multi` mode or a coordinator
/// embedding the [`VirtualSelect`] model directly (the inspector's object list,
/// R922). The inverse of that encode, kept in the same module so a slot rename
/// can't silently break a binding's hand-decode (the R743.1 `read_reorder`
/// decode-of-encode rule; the `"selection"` array is read by `hello-multi-select`,
/// `hello-grid-multi-select`, and `hello-inspector`).
///
/// R1561 — returns the [`IndexRuns`] the slot now carries, canonical and
/// ascending; an empty array, an absent slot and a mistyped one all decode to
/// the empty set. A binding paints from it with [`IndexRuns::contains`], asked
/// once per **rendered** row — which is what the windowing already bounds.
/// The pre-R1561 shape returned a `Vec` of every selected index and each
/// binding then projected that into a per-row bitmap, so a select-all cost the
/// model's size twice on every frame that read it.
/// R1563 — a `usize` as an `IntrospectValue::Int`, or `Null` when it does not
/// fit. The three count slots on this surface all answer this way, and writing
/// the fallback three times is how one of them ends up answering `0` for a
/// number too large to send.
fn usize_value(count: usize) -> IntrospectValue {
    i64::try_from(count).map_or(IntrospectValue::Null, IntrospectValue::Int)
}

/// R1563 — decode a `[row, col]` cell argument.
///
/// Both coordinates must be present and be non-negative integers. A pair with a
/// negative or fractional member is a **type mismatch**, not a clamp: R1561
/// made the same call for a malformed run array, and the reason holds here —
/// `[-1, 3]` is a caller bug, and answering it with a selection of column 3 in
/// row 0 would hide it.
fn decode_cell_arg(args: &IntrospectValue) -> Option<(usize, usize)> {
    let IntrospectValue::Json(serde_json::Value::Array(pair)) = args else {
        return None;
    };
    let [row, col] = pair.as_slice() else {
        return None;
    };
    Some((
        usize::try_from(row.as_u64()?).ok()?,
        usize::try_from(col.as_u64()?).ok()?,
    ))
}

/// R1563 §5.40 — **serialize SSOT** for the two-axis selection: the bands, each
/// a `{"rows": [[first, last], …], "columns": "all" | [[first, last], …]}`.
///
/// The encode peer of [`read_cells`], kept beside it so a shape change cannot
/// break one direction silently (the R743.1 decode-of-encode rule).
#[must_use]
pub fn cells_to_value(cells: &CellSelection) -> IntrospectValue {
    serde_json::to_value(cells).map_or(IntrospectValue::Null, IntrospectValue::Json)
}

/// R1563 §5.40 — **deserialize peer** of the coordinator's `"cells"` query.
///
/// An empty array, an absent slot and a mistyped one all decode to the empty
/// selection, matching [`read_selection`]. A *malformed band* decodes to empty
/// too — this is the read path, where the caller asked what is selected and the
/// answer was unreadable; the **write** path
/// (`intervene("cells", …)`) refuses instead, because there a malformed payload
/// is a caller's statement and silently accepting a smaller one changes the
/// model.
#[must_use]
pub fn read_cells(intro: &dyn ExternalIntrospect) -> CellSelection {
    match intro.query("cells") {
        Some(IntrospectValue::Json(json)) => {
            serde_json::from_value(json).unwrap_or_else(|_| CellSelection::new())
        }
        _ => CellSelection::new(),
    }
}

#[must_use]
pub fn read_selection(intro: &dyn ExternalIntrospect) -> IndexRuns {
    read_selection_at(intro, "selection")
}

/// R1563 — the same decode for any slot that carries a run set, named by path.
///
/// The surface has two of them now — `selection` (rows selected as records) and `column_selection`
/// (the toolkit `selectedColumns()`) — encoded by the one [`selection_to_value`]. A second copy of this
/// four-line decode is how the two directions drift, which is the R743.1 rule
/// this pair exists under.
#[must_use]
pub fn read_selection_at(intro: &dyn ExternalIntrospect, path: &str) -> IndexRuns {
    match intro.query(path) {
        Some(IntrospectValue::Json(json)) => serde_json::from_value(json).unwrap_or_default(),
        _ => IndexRuns::new(),
    }
}

/// R783.1 §5.40 — **deserialize peer** of the coordinator's `"selected"`
/// query (the `selected_value` encode): decode the single active row index
/// from an introspection surface that delegates it to a
/// [`VirtualSelectExternal`]. The scalar sibling of [`read_selection`] (same
/// R743.1 decode-of-encode rule): the `"selected"` `Int` slot is read by every
/// single-select index-model binding (`hello-virtual-select` / `-nav`,
/// `hello-grid-nav` / `-sort` / `-filter`, `hello-virtual-sort`), so the
/// decode lives next to its encode and a slot rename can't silently break six
/// hand-decoders. An absent / `Null` / mistyped slot yields `None`.
#[must_use]
pub fn read_selected(intro: &dyn ExternalIntrospect) -> Option<usize> {
    match intro.query("selected") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// R922 §5.40 — **serialize peer** of [`read_selection`]: encode a multi-select
/// index set as the canonical `"selection"` JSON array (ascending). Kept beside
/// its decoder so the array shape has one definition for both producers — the
/// standalone [`VirtualSelectExternal`] and any coordinator that embeds the
/// [`VirtualSelect`] model directly (the inspector's object list, R922) — and a
/// shape drift can never silently break a [`read_selection`] hand-decoder
/// (the R743.1 decode-of-encode symmetry, now closed on the encode side too).
/// Always an array (empty `[]` when nothing is selected), never `Null`, so an
/// AI consumer treats the slot as a list unconditionally.
///
/// # R1561 — the array is of **runs**, not of indices
///
/// It was one integer per selected row until R1561, which made
/// `query("selection")` on a select-all cost the model: 58 890 bytes and
/// 10.9 ms for the 10 000-row `hello-multi-select` binding, ~5.9 MB and ~1.1 s
/// for the million-row model this axis is named for — **per read**. The slot
/// now carries `[[first, last], …]`, the same form
/// [`IndexRuns`] holds and serializes, so the wire
/// shape is not a projection of the model that could disagree with it.
///
/// A run pair is *more* informative than the indices it stands for, not less:
/// `[[0, 9999]]` says "a contiguous span of ten thousand", which the flat array
/// only implied and which an agent would have had to re-derive by scanning it.
#[must_use]
pub fn selection_to_value(selection: &IndexRuns) -> IntrospectValue {
    IntrospectValue::Json(
        serde_json::to_value(selection).unwrap_or(serde_json::Value::Array(Vec::new())),
    )
}

/// R922 §5.40 — **serialize peer** of [`read_selected`]: encode the active-row
/// cursor as the canonical `"selected"` slot (`Int`, or `Null` when nothing is
/// active). The scalar sibling of [`selection_to_value`].
#[must_use]
pub fn selected_to_value(cursor: Option<usize>) -> IntrospectValue {
    cursor
        .and_then(|i| i64::try_from(i).ok())
        .map_or(IntrospectValue::Null, IntrospectValue::Int)
}

/// R777 §5.27 — the standard **linear-clamp** keyboard navigation policy
/// for a finite virtualized collection: map a key to the next selected
/// index given the current selection and a `page` size (rows per measured
/// viewport-ful).
///
/// Single-select, **clamp** (no wrap) — a finite data list / grid has ends,
/// unlike the cyclic roving of a small `ListBox` / `RadioGroup` (those wrap
/// because every option is a peer tab stop). With no current selection,
/// every navigation key lands on the first row (the W3C "first key focuses
/// the first option" convention). Returns `None` for an unhandled key (or
/// an empty collection) so the caller falls through to the shell's
/// unrecognised-key swallow contract.
///
/// This is the policy half of [`nav_select_key`]; it is `pub` so a binding
/// that wants the same key→index mapping without the full controller (or a
/// different mechanism) can reuse it. A *cyclic* peer is a later additive
/// policy when a wrapping virtualized collection needs one.
#[must_use]
pub fn clamp_nav(
    current: Option<usize>,
    key: &str,
    item_count: usize,
    page: usize,
) -> Option<usize> {
    let last = item_count.checked_sub(1)?;
    let next = match key {
        "ArrowDown" => current.map_or(0, |i| (i + 1).min(last)),
        "ArrowUp" => current.map_or(0, |i| i.saturating_sub(1)),
        "Home" => 0,
        "End" => last,
        "PageDown" => current.map_or(0, |i| i.saturating_add(page).min(last)),
        "PageUp" => current.map_or(0, |i| i.saturating_sub(page)),
        _ => return None,
    };
    Some(next)
}

/// R780 §5.27 — the uniform-pitch geometry of a virtualized collection that
/// [`nav_select_key`] navigates: the full dataset size and the per-row
/// pitch the body windows against. Bundled into one `Copy` struct (the
/// R775 `VirtualTableData` precedent) so the controller stays within the
/// 7-argument budget after R780 added the `modifiers` axis.
#[derive(Debug, Clone, Copy)]
pub struct RowMetrics {
    /// Full dataset size — the navigation clamp bound.
    pub item_count: usize,
    /// Uniform per-row pitch the body windows against (the list row pitch /
    /// the grid data-row height).
    pub row_pitch: u32,
}

/// R777 §5.27 / R780 §5.40 — drive keyboard navigation **and multi-select
/// modifiers** for an index-model virtualized **collection** (a virtualized
/// list *or* grid) backed by a [`VirtualSelectExternal`] at `tag` and a
/// flex-viewport [`ScrollState`].
///
/// This is the shared `WidgetCore::apply_key` body behind `hello-virtual-nav`
/// (list), `hello-grid-nav` (grid), and `hello-multi-select` (multi list):
/// the wiring is byte-identical between them (only the tag, scroll state,
/// row pitch, and item count differ), and a divergence — selecting or
/// revealing differently in one than another — would be a bug, not a style
/// choice. So it lifts here (the R758 self-grep mandate) rather than living
/// thrice.
///
/// The `modifiers` come straight from `WidgetCore::apply_key` (already W3C
/// four-bit). They only change behaviour when the coordinator is a
/// **multi-select** model (`query("mode") == "multi"`); a single-select
/// coordinator ignores them, so the two existing consumers are unchanged:
///
/// - **`Ctrl+A`** — select every row (`invoke("select_all")`).
/// - **`Ctrl+Space`** — toggle the active row (`invoke("toggle", cursor)`).
/// - **`Shift`+nav** (`Arrow` / `Home` / `End` / `Page`) — extend the
///   contiguous range from the `anchor` to the navigated row
///   (`invoke("extend_to", target)`).
/// - **plain nav** — move the active row and replace the selection
///   (`invoke("select", target)`), exactly as R777.
///
/// Every selection mutation goes through the coordinator's AI-first `invoke`
/// funnel (the same wire a `scene/invoke` drives — keyboard and RPC
/// selection are one funnel), then the navigated row is scrolled into view
/// with [`reveal_row`](crate::widgets::virtual_list::reveal_row) (so
/// navigating to a never-materialized row scrolls there).
///
/// Returns `true` when the key was handled (the grid/list was focused and
/// the key is a navigation / multi-select key), `false` otherwise — the
/// exact bool `apply_key` must return. Keys only route when
/// `focused == Some(tag)` (single tab stop, no sibling aliasing).
///
/// `metrics` carries the dataset size and the uniform per-row pitch the
/// body windows against ([`RowMetrics`]).
pub fn nav_select_key(
    scene: &mut Scene,
    scroll: &ScrollState,
    tag: &str,
    focused: Option<&str>,
    key: &str,
    modifiers: crate::input::Modifiers,
    metrics: RowMetrics,
) -> bool {
    let RowMetrics {
        item_count,
        row_pitch,
    } = metrics;
    if focused != Some(tag) {
        return false;
    }
    let page = crate::widgets::virtual_list::page_rows(scroll, row_pitch);

    let Some(node) = scene.find_external_with_tag_mut(tag) else {
        return false;
    };
    let Some(intro) = node.handle.introspect() else {
        return false;
    };
    let multi = matches!(intro.query("mode"), Some(IntrospectValue::Text(ref m)) if m == "multi");
    let current = match intro.query("selected") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    };

    // Multi-select set ops that are *not* navigation: Ctrl+A select-all and
    // Ctrl+Space toggle-active. These handle the key (swallow it) without a
    // navigation target / scroll. R902.1 — the chord→op gate is the shared
    // [`MultiSelectKeyOp::classify`](crate::input::MultiSelectKeyOp::classify)
    // SSOT (so the list/grid and the tree-grid never diverge on which keys are
    // set-ops); only fires on a `multi` coordinator.
    if multi {
        match crate::input::MultiSelectKeyOp::classify(key, modifiers) {
            Some(crate::input::MultiSelectKeyOp::SelectAll) => {
                if let Some(intro) = node.handle.introspect_mut() {
                    let _ = intro.invoke("select_all", IntrospectValue::Null);
                }
                return true;
            }
            Some(crate::input::MultiSelectKeyOp::ToggleCursor) => {
                if let (Some(intro), Some(c)) = (node.handle.introspect_mut(), current) {
                    if let Ok(c) = i64::try_from(c) {
                        let _ = intro.invoke("toggle", IntrospectValue::Int(c));
                    }
                }
                return current.is_some();
            }
            None => {}
        }
    }

    let Some(target) = clamp_nav(current, key, item_count, page) else {
        return false;
    };
    // Shift+nav extends the range (multi only); everything else moves +
    // replaces. The coordinator collapses `extend_to` to a plain move in a
    // single-select model, so the branch is harmless there.
    let action = if multi && modifiers.shift {
        "extend_to"
    } else {
        "select"
    };
    if let (Some(intro), Ok(t)) = (node.handle.introspect_mut(), i64::try_from(target)) {
        let _ = intro.invoke(action, IntrospectValue::Int(t));
    }
    crate::widgets::virtual_list::reveal_row(scroll, target, row_pitch);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::SchemaChannel;

    #[test]
    fn new_starts_unselected() {
        let s = VirtualSelectExternal::new(100);
        assert_eq!(s.selected(), None);
        assert_eq!(s.item_count(), 100);
    }

    #[test]
    fn select_sets_and_reports_change() {
        let mut s = VirtualSelectExternal::new(100);
        assert!(s.select(42));
        assert_eq!(s.selected(), Some(42));
        // Re-selecting the same index is a no-op.
        assert!(!s.select(42));
        // Moving the selection is a change.
        assert!(s.select(7));
        assert_eq!(s.selected(), Some(7));
    }

    #[test]
    fn select_out_of_range_is_ignored() {
        let mut s = VirtualSelectExternal::new(10);
        assert!(!s.select(10), "index == count is out of range");
        assert!(!s.select(9999));
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn select_a_deep_index_works_without_materialization() {
        // The headline: selecting row 9 999 never requires the other
        // 9 999 leaves to exist (no leaf slice at all).
        let mut s = VirtualSelectExternal::new(10_000);
        assert!(s.select(9_999));
        assert_eq!(s.selected(), Some(9_999));
    }

    #[test]
    fn clear_resets() {
        let mut s = VirtualSelectExternal::new(10);
        s.select(3);
        assert!(s.clear());
        assert_eq!(s.selected(), None);
        assert!(!s.clear(), "clearing an empty selection is a no-op");
    }

    #[test]
    fn set_selected_admin_channel_validates() {
        let mut s = VirtualSelectExternal::new(10);
        assert!(s.set_selected(Some(5)));
        assert_eq!(s.selected(), Some(5));
        // Out-of-range clears.
        assert!(s.set_selected(Some(100)));
        assert_eq!(s.selected(), None);
        // None clears (and is a no-op when already empty).
        assert!(!s.set_selected(None));
    }

    #[test]
    fn composite_send_selects_only_on_activation_edge() {
        let mut s = VirtualSelectExternal::new(100);
        // The router replays the full arc; only PointerUp selects.
        s.handle_send("4:PointerEnter");
        assert_eq!(s.selected(), None, "hover does not select");
        s.handle_send("4:PointerDown");
        assert_eq!(s.selected(), None, "press alone does not select");
        s.handle_send("4:PointerUp");
        assert_eq!(s.selected(), Some(4), "release selects");
        // KeyboardActivate also selects.
        s.handle_send("9:KeyboardActivate");
        assert_eq!(s.selected(), Some(9));
        // Malformed / out-of-range payloads are harmless no-ops.
        s.handle_send("noseparator");
        s.handle_send("4:");
        s.handle_send("9999:PointerUp");
        assert_eq!(
            s.selected(),
            Some(9),
            "no-op payloads leave selection intact"
        );
    }

    #[test]
    fn grid_cell_send_selects_the_row_column_irrelevant() {
        // R777 — the same coordinator drives a virtualized grid: a cell
        // key `<row>_<col>` selects the ROW (SelectRows). Clicking any
        // column of row 4 selects row 4.
        let mut s = VirtualSelectExternal::new(100);
        s.handle_send("4_0:PointerEnter");
        assert_eq!(s.selected(), None, "hover does not select");
        s.handle_send("4_2:PointerUp");
        assert_eq!(s.selected(), Some(4), "cell in column 2 selects row 4");
        // A different column of a different row moves the selection.
        s.handle_send("9_1:PointerUp");
        assert_eq!(s.selected(), Some(9), "cell in column 1 selects row 9");
        // KeyboardActivate on a cell selects its row too.
        s.handle_send("3_0:KeyboardActivate");
        assert_eq!(s.selected(), Some(3));
    }

    #[test]
    fn grid_header_send_is_ignored() {
        // A column-header click `h<col>` has no leading row index — it is
        // the sort axis, not this coordinator's, and must be a no-op.
        let mut s = VirtualSelectExternal::new(100);
        s.select(5);
        s.handle_send("h2:PointerUp");
        assert_eq!(
            s.selected(),
            Some(5),
            "header click leaves the selection intact"
        );
    }

    #[test]
    fn query_reports_selected_and_count() {
        let mut s = VirtualSelectExternal::new(50);
        assert_eq!(
            s.query("selected"),
            Some(IntrospectValue::Null),
            "unset selection reports Null (present-but-empty), not absence",
        );
        assert_eq!(s.query("item_count"), Some(IntrospectValue::Int(50)));
        s.select(12);
        assert_eq!(s.query("selected"), Some(IntrospectValue::Int(12)));
        assert_eq!(
            s.query("nope"),
            None,
            "an undeclared path is genuinely absent"
        );
    }

    /// R1561 — the declared surface, pinned as an EXACT set, and every readable
    /// slot in it actually answering.
    ///
    /// This coordinator had no such test while a dozen sibling externals do
    /// (`external_schema_declares_three_slots` and peers), and R1561 added a
    /// field to it. `IntrospectSchema.fields` is a hand-written literal —
    /// R1501 measured ~104 of them and disproved the type's own claim that
    /// "mismatches surface as test failures" — and the workspace-wide dynamic
    /// audit (`r1353_declared_domains_hold_on_real_widgets`) names its widgets
    /// by hand and does not name this one. So nothing tied this declaration to
    /// its implementation: a slot could be declared and unreachable, or
    /// reachable and undeclared, in either direction and silently.
    ///
    /// R1563 — the carve-out for `send` is gone, and with it the reason the
    /// test had to name a path by hand: every non-reading path now **declares**
    /// itself an action ([`SchemaChannel::Invoke`]), so the split is read off
    /// the declaration instead of being a string this test remembers. Before
    /// this round `send` was the one action declared as a readable string and
    /// six real verbs were not declared at all.
    #[test]
    fn r1561_schema_is_exactly_what_query_answers() {
        let s = VirtualSelectExternal::new_multi(50);
        let declared: Vec<&str> = s.schema().fields.iter().map(|f| f.path).collect();
        assert_eq!(
            declared,
            [
                "selected",
                "selected_column",
                "selection",
                "selection_count",
                "cells",
                "cell_count",
                "band_count",
                "column_selection",
                "column_count",
                "behavior",
                "section_press",
                "anchor",
                "anchor_column",
                "mode",
                "item_count",
                "select",
                "toggle",
                "extend_to",
                "select_all",
                "toggle_all",
                "clear",
                "select_cell",
                "toggle_cell",
                "extend_to_cell",
                "select_column",
                "toggle_column",
                "extend_to_column",
                "send",
            ],
            "the declared surface is exact — a field added or dropped lands here",
        );
        for field in s.schema().fields {
            match field.channel {
                SchemaChannel::Read => assert!(
                    s.query(field.path).is_some(),
                    "declared slot {:?} must answer — a declaration nothing \
                     implements is a surface an agent cannot follow",
                    field.path,
                ),
                SchemaChannel::Invoke => assert_eq!(
                    s.query(field.path),
                    None,
                    "declared action {:?} must not also read: `SchemaChannel` \
                     is what tells an agent which call to make",
                    field.path,
                ),
            }
        }
        // Every verb the `invoke` match answers is declared. The reverse
        // direction of the same audit, and the one R1562's census found open:
        // `UnknownPath` is what an undeclared verb *should* return, so a verb
        // that works while being undeclared is invisible to it.
        for verb in [
            "select",
            "toggle",
            "extend_to",
            "select_all",
            "toggle_all",
            "clear",
            "select_cell",
            "toggle_cell",
            "extend_to_cell",
            "select_column",
            "toggle_column",
            "extend_to_column",
            "send",
        ] {
            assert!(
                s.schema()
                    .fields
                    .iter()
                    .any(|f| f.path == verb && f.channel == SchemaChannel::Invoke),
                "{verb:?} is answered by `invoke` and must be declared as an action",
            );
        }
    }

    #[test]
    fn intervene_selected_sets_clears_and_guards() {
        let mut s = VirtualSelectExternal::new(50);
        s.intervene("selected", IntrospectValue::Int(20))
            .expect("int selects");
        assert_eq!(s.selected(), Some(20));
        s.intervene("selected", IntrospectValue::Null)
            .expect("null clears");
        assert_eq!(s.selected(), None);
        assert_eq!(
            s.intervene("item_count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            s.intervene("selected", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
        assert_eq!(
            s.intervene("nope", IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath),
        );
    }

    fn drained(s: &mut VirtualSelectExternal) -> Vec<Intent> {
        let mut out = Vec::new();
        s.drain_intents(&mut |i| out.push(i));
        out
    }

    #[test]
    fn interaction_emits_selected_intent_admin_is_silent() {
        let mut s = VirtualSelectExternal::new(100);
        // Interaction (select) emits one "selected" intent with the index.
        assert!(s.select(7));
        let intents = drained(&mut s);
        assert_eq!(intents.len(), 1, "one selected intent per interaction");
        assert_eq!(
            intents[0],
            Intent::new_static("selected", IntrospectValue::Int(7))
        );
        assert!(
            drained(&mut s).is_empty(),
            "drain is idempotent (queue emptied)"
        );
        // A no-op re-select emits nothing.
        assert!(!s.select(7));
        assert!(
            drained(&mut s).is_empty(),
            "unchanged selection emits nothing"
        );
        // Composite send (the click wire) is also an interaction → emits.
        s.handle_send("9:PointerUp");
        assert_eq!(
            drained(&mut s).len(),
            1,
            "composite send emits on activation"
        );
        // Admin paths (intervene / set_selected / clear) are SILENT.
        s.intervene("selected", IntrospectValue::Int(3)).unwrap();
        s.set_selected(Some(5));
        s.clear();
        assert!(
            drained(&mut s).is_empty(),
            "admin restore/clear is silent on §5.20"
        );
    }

    #[test]
    fn is_dirty_tracks_pending_intent() {
        let mut s = VirtualSelectExternal::new(10);
        assert!(!s.is_dirty(), "clean at rest");
        s.select(2);
        assert!(s.is_dirty(), "dirty while a selected intent is queued");
        let _ = drained(&mut s);
        assert!(!s.is_dirty(), "clean after drain");
    }

    #[test]
    fn invoke_select_clear_send_return_outcome() {
        let mut s = VirtualSelectExternal::new(100);
        assert_eq!(
            s.invoke("select", IntrospectValue::Int(7)),
            Ok(IntrospectValue::Int(7))
        );
        assert_eq!(
            s.invoke("clear", IntrospectValue::Null),
            Ok(IntrospectValue::Null)
        );
        assert_eq!(
            s.invoke("send", IntrospectValue::Text("3:PointerUp".into())),
            Ok(IntrospectValue::Int(3)),
        );
        assert_eq!(
            s.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
        assert_eq!(
            s.invoke("select", IntrospectValue::Text("x".into())),
            Err(InvokeError::TypeMismatch),
        );
    }

    // ── R777 keyboard navigation policy + controller ────────────────

    #[test]
    fn clamp_nav_steps_clamps_and_pages() {
        // Arrows step one, clamped at both ends (no wrap).
        assert_eq!(clamp_nav(Some(5), "ArrowDown", 100, 12), Some(6));
        assert_eq!(clamp_nav(Some(5), "ArrowUp", 100, 12), Some(4));
        assert_eq!(
            clamp_nav(Some(0), "ArrowUp", 100, 12),
            Some(0),
            "top clamps, no wrap"
        );
        assert_eq!(
            clamp_nav(Some(99), "ArrowDown", 100, 12),
            Some(99),
            "bottom clamps"
        );
        // Home / End.
        assert_eq!(clamp_nav(Some(50), "Home", 100, 12), Some(0));
        assert_eq!(clamp_nav(Some(50), "End", 100, 12), Some(99));
        // Page steps by `page`, clamped.
        assert_eq!(clamp_nav(Some(50), "PageDown", 100, 12), Some(62));
        assert_eq!(clamp_nav(Some(50), "PageUp", 100, 12), Some(38));
        assert_eq!(clamp_nav(Some(5), "PageUp", 100, 12), Some(0));
        assert_eq!(clamp_nav(Some(95), "PageDown", 100, 12), Some(99));
    }

    #[test]
    fn clamp_nav_from_none_lands_on_first_and_rejects_unknown() {
        for key in ["ArrowDown", "ArrowUp", "PageDown", "PageUp"] {
            assert_eq!(
                clamp_nav(None, key, 100, 12),
                Some(0),
                "{key} from None -> 0"
            );
        }
        assert_eq!(
            clamp_nav(Some(3), "Tab", 100, 12),
            None,
            "unhandled key -> None"
        );
        assert_eq!(
            clamp_nav(Some(0), "ArrowDown", 0, 12),
            None,
            "empty collection -> None"
        );
    }

    use crate::input::Modifiers;

    const NONE: Modifiers = Modifiers::empty();
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };
    const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
        meta: false,
    };

    fn grid_scene(tag: &str) -> Scene {
        Scene::External(
            crate::scene::ExternalNode::new(Box::new(VirtualSelectExternal::new(10_000)))
                .with_tag(tag.to_string()),
        )
    }

    fn multi_scene(tag: &str) -> Scene {
        Scene::External(
            crate::scene::ExternalNode::new(Box::new(VirtualSelectExternal::new_multi(10_000)))
                .with_tag(tag.to_string()),
        )
    }

    fn selected_of(scene: &Scene, tag: &str) -> Option<usize> {
        scene
            .find_external_with_tag(tag)
            .and_then(|n| n.handle.introspect())
            .and_then(|i| match i.query("selected") {
                Some(IntrospectValue::Int(v)) => usize::try_from(v).ok(),
                _ => None,
            })
    }

    /// The selection an external in the scene reports.
    ///
    /// R1561 — routed through [`read_selection`], the production decoder, and
    /// not through a second `query("selection")` match of its own. It *was* the
    /// latter, so this module held two hand-decodes of one slot and the tests
    /// were checking the wire against a copy of the thing under test — which is
    /// how the run form landed with these five still asserting the index form.
    fn selection_of(scene: &Scene, tag: &str) -> IndexRuns {
        scene
            .find_external_with_tag(tag)
            .and_then(|n| n.handle.introspect())
            .map_or_else(IndexRuns::new, read_selection)
    }

    /// The selected rows of an external in the scene, one per row — the
    /// materialising sibling of [`selection_of`], for assertions that are about
    /// *which* rows.
    fn selection_rows_of(scene: &Scene, tag: &str) -> Vec<usize> {
        selection_of(scene, tag).iter().collect()
    }

    #[test]
    fn nav_select_key_unfocused_or_unknown_key_is_a_noop() {
        let mut scene = grid_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // Not focused → ignored.
        assert!(!nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("other"),
            "End",
            NONE,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(selected_of(&scene, "vlist"), None);
        // Focused but a non-nav key → ignored.
        assert!(!nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "Tab",
            NONE,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(selected_of(&scene, "vlist"), None);
    }

    #[test]
    fn nav_select_key_selects_and_reveals_a_deep_row() {
        let mut scene = grid_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // End selects the last row and scrolls the offset deep so the row
        // is revealed (a row never materialized at offset 0).
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "End",
            NONE,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(selected_of(&scene, "vlist"), Some(9_999));
        assert!(
            scroll.offset_y() > 300_000,
            "End scrolled deep, offset {}",
            scroll.offset_y()
        );
        // Home brings selection + scroll back to the top.
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "Home",
            NONE,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(selected_of(&scene, "vlist"), Some(0));
        assert_eq!(scroll.offset_y(), 0, "Home revealed the top");
    }

    // ── R780 multi-select model ─────────────────────────────────────

    /// R1561 — the selected rows, one per row, for assertions that are about
    /// **which** rows rather than about the representation. It goes through
    /// [`IndexRuns::iter`], the materialising accessor, so a test that pays the
    /// selection's size in indices says so at the call site exactly as a
    /// binding would have to.
    fn rows(s: &VirtualSelectExternal) -> Vec<usize> {
        s.selection().iter().collect()
    }

    #[test]
    fn single_mode_holds_at_most_one_and_keeps_the_old_wire() {
        let mut s = VirtualSelectExternal::new(100);
        assert_eq!(s.mode(), SelectionMode::Single);
        assert!(s.select(4));
        assert!(s.select(9));
        assert_eq!(rows(&s), vec![9], "single replaces, never accumulates");
        // toggle / extend / select_all all collapse to a plain move.
        assert!(s.toggle(3));
        assert_eq!(rows(&s), vec![3]);
        assert!(s.extend_to(7));
        assert_eq!(rows(&s), vec![7]);
        assert!(!s.select_all(), "select_all is a no-op in single mode");
        assert_eq!(rows(&s), vec![7]);
    }

    #[test]
    fn multi_toggle_accumulates_and_flips() {
        let mut s = VirtualSelectExternal::new_multi(100);
        assert_eq!(s.mode(), SelectionMode::Multi);
        assert!(s.toggle(2));
        assert!(s.toggle(5));
        assert!(s.toggle(9));
        assert_eq!(rows(&s), vec![2, 5, 9], "toggles accumulate");
        assert!(s.is_selected(5));
        assert!(s.toggle(5), "toggling a selected row removes it");
        assert_eq!(rows(&s), vec![2, 9]);
        assert_eq!(
            s.selected(),
            Some(5),
            "the toggled row stays the active row"
        );
        assert_eq!(s.anchor(), Some(5));
    }

    #[test]
    fn multi_modifier_click_toggles_extends_and_replaces() {
        // R781 — the composite pointer wire carries the held modifiers as a
        // third `:` segment; multi-select interprets them.
        let mut s = VirtualSelectExternal::new_multi(100);
        // Plain click moves + replaces (anchor = 4).
        s.handle_send("4:PointerUp");
        assert_eq!(rows(&s), vec![4]);
        // Ctrl-click toggles a second row in (Ctrl = "c").
        s.handle_send("9:PointerUp:c");
        assert_eq!(rows(&s), vec![4, 9], "Ctrl-click accumulates");
        // Ctrl-click an existing member removes it.
        s.handle_send("4:PointerUp:c");
        assert_eq!(rows(&s), vec![9], "Ctrl-click toggles off");
        // Shift-click extends the range from the anchor (now 4, the last
        // toggled row) to the clicked row.
        s.handle_send("6:PointerUp:s");
        assert_eq!(rows(&s), vec![4, 5, 6], "Shift-click extends from anchor 4");
    }

    #[test]
    fn read_selection_round_trips_the_encode() {
        // R782 §5.40 — `read_selection` is the exact inverse of the
        // coordinator's `"selection"` query encode (the R743.1 decode-of-encode
        // contract): selecting a set and decoding through the introspect
        // surface yields the same canonical run set, and an empty selection
        // decodes to the empty one.
        let mut s = VirtualSelectExternal::new_multi(10_000);
        s.toggle(9);
        s.toggle(2);
        s.toggle(5_000);
        let intro: &dyn ExternalIntrospect = &s;
        assert_eq!(
            read_selection(intro),
            [2usize, 9, 5_000].into_iter().collect::<IndexRuns>(),
            "decode is the inverse of the selection_value encode",
        );
        // R1561 — three scattered rows really are three runs, so the decode is
        // not accidentally right because everything collapses to one.
        assert_eq!(read_selection(intro).run_count(), 3);
        let empty = VirtualSelectExternal::new_multi(10);
        let empty_intro: &dyn ExternalIntrospect = &empty;
        assert_eq!(
            read_selection(empty_intro),
            IndexRuns::new(),
            "empty selection decodes to []"
        );
    }

    /// R1561 — the scale property the run representation exists for, asserted
    /// on the model rather than on a clock: selecting a million rows is one
    /// run, the count is exact, and the wire form is the runs.
    ///
    /// The negative control is the same assertions on a *scattered* selection,
    /// where the run count really does grow — without it the test would pass
    /// against an implementation that reported `1` unconditionally.
    #[test]
    fn r1561_whole_model_selection_costs_one_run_on_the_wire() {
        let mut s = VirtualSelectExternal::new_multi(1_000_000);
        assert!(s.select_all());
        assert_eq!(s.selection().run_count(), 1, "a million rows, one run");
        assert_eq!(s.selected_count(), 1_000_000, "and the count is exact");
        assert_eq!(
            s.query("selection"),
            Some(IntrospectValue::Json(serde_json::json!([[0, 999_999]]))),
            "the wire carries the run, not the rows",
        );
        assert_eq!(
            s.query("selection_count"),
            Some(IntrospectValue::Int(1_000_000)),
            "the count is answerable without materialising the selection",
        );
        // Shift-extend across the whole model is likewise one run.
        assert!(s.select(0));
        assert!(s.extend_to(999_999));
        assert_eq!(s.selection().run_count(), 1);
        assert_eq!(s.selected_count(), 1_000_000);
        // Negative control: punching a hole splits it, so `run_count` is
        // reporting the representation rather than a constant.
        assert!(s.toggle(500_000));
        assert_eq!(s.selection().run_count(), 2, "a hole makes two runs");
        assert_eq!(s.selected_count(), 999_999);
        assert_eq!(
            s.query("selection"),
            Some(IntrospectValue::Json(serde_json::json!([
                [0, 499_999],
                [500_001, 999_999]
            ]))),
        );
    }

    /// R1562 — a row-header press reaches the SAME transition a cell press
    /// reaches. The assertion is byte-equality of the resulting model state
    /// across the two addresses, chord for chord: if the band ever grew its own
    /// selection code, one of these three pairs would drift.
    #[test]
    fn r1562_a_band_press_and_a_cell_press_are_one_derivation() {
        let arcs: [(&str, &str); 3] = [
            // (band address, body address) — the same three chords each.
            ("r7:PointerUp", "7_0:PointerUp"),
            ("r7:PointerUp:c", "7_0:PointerUp:c"),
            ("r7:PointerUp:s", "7_0:PointerUp:s"),
        ];
        for (band, body) in arcs {
            let mut from_band = VirtualSelectExternal::new_multi(1_000);
            let mut from_body = VirtualSelectExternal::new_multi(1_000);
            for s in [&mut from_band, &mut from_body] {
                // A common prior selection, so `Ctrl` has something to toggle
                // against and `Shift` has an anchor to extend from.
                s.select(3);
                s.toggle(5);
            }
            from_band.handle_send(band);
            from_body.handle_send(body);
            assert_eq!(
                from_band.selection(),
                from_body.selection(),
                "the band and the body must agree for {band}",
            );
            assert_eq!(
                from_band.selected(),
                from_body.selected(),
                "and on the cursor"
            );
            assert_eq!(from_band.anchor(), from_body.anchor(), "and on the anchor");
        }
    }

    /// R1562 — the band's `Shift`-chord spans a run whose cost is the run's,
    /// not the span's: 897 rows selected from two presses, one run, and the
    /// wire form is the two endpoints. The gesture R1561's representation was
    /// built for, now reachable from the header.
    #[test]
    fn r1562_a_shift_press_in_the_band_is_one_run() {
        let mut s = VirtualSelectExternal::new_multi(10_000);
        s.handle_send("r4:PointerUp");
        assert_eq!(s.selection(), &IndexRuns::run(4, 4));
        s.handle_send("r900:PointerUp:s");
        assert_eq!(s.selection().run_count(), 1, "one run for 897 rows");
        assert_eq!(s.selected_count(), 897);
        assert_eq!(
            s.query("selection"),
            Some(IntrospectValue::Json(serde_json::json!([[4, 900]]))),
        );
    }

    /// R1562 — the corner control: what it shows, what it does, and that the
    /// two are the same fact. The toolkit's corner button has neither.
    #[test]
    fn r1562_the_corner_shows_what_it_will_do() {
        let mut s = VirtualSelectExternal::new_multi(10_000);
        assert_eq!(s.extent(), SelectionExtent::Empty);
        // Empty -> a press selects everything.
        s.handle_send("c:PointerUp");
        assert_eq!(s.extent(), SelectionExtent::All);
        assert_eq!(s.selection(), &IndexRuns::run(0, 9_999));
        assert_eq!(s.selection().run_count(), 1, "select-all is ONE run");
        // All -> a press takes it back. This is the leg the toolkit does not
        // have: `selectAll()` is one-way.
        s.handle_send("c:PointerUp");
        assert_eq!(s.extent(), SelectionExtent::Empty);
        assert!(s.selection().is_empty());
        // Partial -> a press completes it (not "toggles each row").
        s.select(7);
        assert_eq!(s.extent(), SelectionExtent::Partial);
        s.handle_send("c:PointerUp");
        assert_eq!(s.extent(), SelectionExtent::All);
        // Non-activation phases of the same arc are inert, exactly as they are
        // for a cell.
        s.handle_send("c:PointerEnter");
        s.handle_send("c:PointerDown");
        assert_eq!(
            s.extent(),
            SelectionExtent::All,
            "only the activate edge acts"
        );
    }

    /// R1562 — the corner is an interaction, so it reaches the §5.20 intent
    /// channel on BOTH legs. The clear leg is the one that could have been
    /// silent: `clear()` is the admin path and does not emit.
    #[test]
    fn r1562_the_corner_emits_its_intent_on_both_legs() {
        let mut s = VirtualSelectExternal::new_multi(100);
        let mut names = Vec::new();
        s.handle_send("c:PointerUp");
        s.drain_intents(&mut |i| names.push(i.tag.to_string()));
        assert_eq!(names, ["selection"], "select-all announced");
        names.clear();
        s.handle_send("c:PointerUp");
        s.drain_intents(&mut |i| names.push(i.tag.to_string()));
        assert_eq!(names, ["selection"], "and so is taking it back");
        // A press that changes nothing announces nothing.
        let mut single = VirtualSelectExternal::new(100);
        single.handle_send("c:PointerUp");
        let mut none = Vec::new();
        single.drain_intents(&mut |i| none.push(i.tag.to_string()));
        assert!(
            none.is_empty(),
            "a single-select model cannot select all, and says nothing",
        );
    }

    /// R1562 — the extent's edges. An empty model is `Empty`, not `All`: a
    /// control that reads "everything is selected" over nothing would then
    /// offer a press that clears nothing.
    #[test]
    fn r1562_an_empty_model_has_an_empty_extent() {
        let empty = VirtualSelect::new(0, SelectionMode::Multi);
        assert_eq!(empty.extent(), SelectionExtent::Empty);
        let mut one = VirtualSelect::new(1, SelectionMode::Multi);
        assert_eq!(one.extent(), SelectionExtent::Empty);
        assert!(one.select(0));
        assert_eq!(
            one.extent(),
            SelectionExtent::All,
            "one of one is all of it"
        );
    }

    #[test]
    fn read_selected_round_trips_the_scalar_encode() {
        // R783.1 — `read_selected` is the exact inverse of the coordinator's
        // `"selected"` query encode (the scalar sibling of `read_selection`):
        // a selected index decodes back to itself, and an empty selection
        // decodes to `None`.
        let mut s = VirtualSelectExternal::new(10_000);
        assert_eq!(
            read_selected(&s as &dyn ExternalIntrospect),
            None,
            "empty → None"
        );
        s.select(4_200);
        assert_eq!(
            read_selected(&s as &dyn ExternalIntrospect),
            Some(4_200),
            "decode is the inverse of the selected_value encode",
        );
    }

    #[test]
    fn single_modifier_click_still_just_selects() {
        // R781 — a Shift / Ctrl-click on a *single*-select coordinator
        // collapses to a plain move (no range, no accumulation): the
        // pre-R781 behaviour is preserved for every existing consumer.
        let mut s = VirtualSelectExternal::new(100);
        s.handle_send("4:PointerUp:s");
        assert_eq!(rows(&s), vec![4]);
        s.handle_send("9:PointerUp:c");
        assert_eq!(
            rows(&s),
            vec![9],
            "single-select replaces, never accumulates"
        );
    }

    #[test]
    fn multi_shift_extends_a_contiguous_range_from_anchor() {
        let mut s = VirtualSelectExternal::new_multi(100);
        s.select(10); // anchor = 10
        assert!(s.extend_to(13));
        assert_eq!(
            rows(&s),
            vec![10, 11, 12, 13],
            "range grows down from anchor"
        );
        // Re-extend the other side: anchor unchanged, range replaced.
        assert!(s.extend_to(8));
        assert_eq!(
            rows(&s),
            vec![8, 9, 10],
            "range grows up, anchor stays at 10"
        );
        assert_eq!(s.anchor(), Some(10));
        assert_eq!(s.selected(), Some(8), "the navigated end is the active row");
    }

    #[test]
    fn multi_select_all_and_clear() {
        let mut s = VirtualSelectExternal::new_multi(5);
        assert!(s.select_all());
        assert_eq!(rows(&s), vec![0, 1, 2, 3, 4]);
        assert!(!s.select_all(), "already-all is a no-op");
        assert!(s.clear());
        assert!(rows(&s).is_empty());
        assert_eq!(s.anchor(), None);
    }

    #[test]
    fn multi_select_intent_carries_the_full_set_admin_is_silent() {
        let mut s = VirtualSelectExternal::new_multi(100);
        assert!(s.toggle(2));
        assert!(s.toggle(7));
        let intents = drained(&mut s);
        assert_eq!(intents.len(), 2, "one selection intent per interaction");
        assert_eq!(
            intents[1],
            Intent::new_static(
                "selection",
                IntrospectValue::Json(serde_json::json!([[2, 2], [7, 7]])),
            ),
            "the intent carries the full set, not the single index",
        );
        // Admin restore is silent on §5.20.
        let restore: IndexRuns = [1, 4].into_iter().collect();
        s.set_selection(&restore);
        assert!(drained(&mut s).is_empty(), "admin set_selection is silent");
        assert_eq!(rows(&s), vec![1, 4]);
    }

    #[test]
    fn multi_query_and_intervene_round_trip_the_set() {
        let mut s = VirtualSelectExternal::new_multi(100);
        assert_eq!(s.query("mode"), Some(IntrospectValue::Text("multi".into())));
        assert_eq!(
            s.query("selection"),
            Some(IntrospectValue::Json(serde_json::json!([]))),
            "empty selection is an empty array, never Null",
        );
        // R1561 — runs in, canonical set out: the pairs arrive out of order and
        // abutting, and the model reaches the value the constructors build.
        s.intervene(
            "selection",
            IntrospectValue::Json(serde_json::json!([[8, 8], [90, 800], [3, 3]])),
        )
        .expect("array of runs replaces selection");
        assert_eq!(
            s.query("selection"),
            Some(IntrospectValue::Json(serde_json::json!([
                [3, 3],
                [8, 8],
                [90, 99]
            ]))),
            "the run straddling item_count is trimmed, not dropped",
        );
        assert_eq!(s.selected_count(), 12);
        assert_eq!(s.query("anchor"), Some(IntrospectValue::Int(99)));
        // A malformed payload is refused rather than partly applied.
        assert_eq!(
            s.intervene(
                "selection",
                IntrospectValue::Json(serde_json::json!([1, 2]))
            ),
            Err(InterveneError::TypeMismatch),
            "an array of bare indices is not an array of runs",
        );
        assert_eq!(
            s.selected_count(),
            12,
            "a refused write leaves the selection alone",
        );
        assert_eq!(
            s.intervene("selection_count", IntrospectValue::Int(3)),
            Err(InterveneError::ReadOnly),
            "the count is derived, so it is not a second way to set the selection",
        );
        // Null clears.
        s.intervene("selection", IntrospectValue::Null)
            .expect("null clears");
        assert!(rows(&s).is_empty());
        // mode / anchor / item_count are read-only.
        assert_eq!(
            s.intervene("mode", IntrospectValue::Text("single".into())),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            s.intervene("anchor", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn invoke_multi_paths_return_the_set() {
        let mut s = VirtualSelectExternal::new_multi(100);
        assert_eq!(
            s.invoke("toggle", IntrospectValue::Int(4)),
            Ok(IntrospectValue::Json(serde_json::json!([[4, 4]]))),
        );
        assert_eq!(
            s.invoke("toggle", IntrospectValue::Int(9)),
            Ok(IntrospectValue::Json(serde_json::json!([[4, 4], [9, 9]]))),
            "two apart rows are two runs",
        );
        // R1561 — closing the gap merges them: the returned value is a function
        // of the selection, not of the toggles that built it.
        assert_eq!(
            s.invoke("toggle", IntrospectValue::Int(5)),
            Ok(IntrospectValue::Json(serde_json::json!([[4, 5], [9, 9]]))),
        );
        // extend_to from anchor 5 (last toggle) up to 11.
        assert_eq!(
            s.invoke("extend_to", IntrospectValue::Int(11)),
            Ok(IntrospectValue::Json(serde_json::json!([[5, 11]]))),
        );
    }

    #[test]
    fn nav_select_key_shift_extends_in_multi_mode() {
        let mut scene = multi_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // Plain ArrowDown from nothing lands on row 0 (anchor = 0).
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "ArrowDown",
            NONE,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(selection_rows_of(&scene, "vlist"), vec![0]);
        // Shift+End extends the range to the last row.
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "ArrowDown",
            NONE,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(
            selected_of(&scene, "vlist"),
            Some(1),
            "plain move resets the anchor to 1"
        );
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "ArrowDown",
            SHIFT,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(
            selection_rows_of(&scene, "vlist"),
            vec![1, 2],
            "Shift extends from anchor 1"
        );
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "ArrowDown",
            SHIFT,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(selection_rows_of(&scene, "vlist"), vec![1, 2, 3]);
    }

    #[test]
    fn nav_select_key_ctrl_a_selects_all_and_ctrl_space_toggles() {
        let mut scene = multi_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // Move to a row so there is an active row to toggle.
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "ArrowDown",
            NONE,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        // Ctrl+Space toggles the active row OFF (it was selected by the move).
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "Space",
            CTRL,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert!(
            selection_of(&scene, "vlist").is_empty(),
            "Ctrl+Space toggled row 0 off"
        );
        // Ctrl+A selects every row.
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "a",
            CTRL,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        let all = selection_of(&scene, "vlist");
        assert_eq!(all.len(), 10_000, "Ctrl+A selects all");
        // R1561 — and says so in one run, over the wire. This is the assertion
        // that would have caught the old representation's cost: the same
        // `len()` held before, while the answer it was read out of was 58 890
        // bytes of JSON.
        assert_eq!(all.run_count(), 1, "and states it as a single run");
    }

    #[test]
    fn nav_select_key_single_mode_ignores_modifiers() {
        // Shift in a single-select model still just moves (no range).
        let mut scene = grid_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "ArrowDown",
            NONE,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert!(nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "ArrowDown",
            SHIFT,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(
            selection_rows_of(&scene, "vlist"),
            vec![1],
            "single mode never accumulates"
        );
        // Ctrl+A is inert in single mode.
        assert!(!nav_select_key(
            &mut scene,
            &scroll,
            "vlist",
            Some("vlist"),
            "a",
            CTRL,
            RowMetrics {
                item_count: 10_000,
                row_pitch: 32
            }
        ));
        assert_eq!(selection_rows_of(&scene, "vlist"), vec![1]);
    }
}
