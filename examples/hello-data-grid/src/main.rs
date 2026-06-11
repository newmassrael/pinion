// R837 §5.38 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (WAI-ARIA, DataGridExternal, TextFieldExternal, gridcell, …).
#![allow(clippy::doc_markdown)]

//! `hello-data-grid` — R837 §5.38 §5.40 §5.50 **editable data grid**: a
//! 2-D table where every cell is editable in place by a type-appropriate
//! control (the spreadsheet / DCC data-table form factor). It is the 2-D
//! generalisation of the R836 property grid: the property grid is the 1
//! column-of-values special case, this is N typed columns × M rows.
//!
//! ## 2nd consumer of two SSOTs (the lift these consumers justify)
//!
//! * **`pinion_core::cell_value`** — the typed value model (`CellValue` /
//!   `CellKind` + kind dispatch / display / parse / the keystroke gate / the
//!   introspect read+write). R836 minted it locally as the 1st consumer; the
//!   data grid is the 2nd consumer, so it was lifted to a framework crate at
//!   this round (the `[[abstraction-needs-second-consumer]]` "lift the
//!   pure-logic model at the 2nd consumer" discipline — a divergence between
//!   the two grids' typed-value logic would be a bug, not a style choice).
//! * **`pinion_a11y::grid_table_nodes`** — the WAI-ARIA `grid` a11y skeleton
//!   (hello-table / hello-table-multi are the 1st / 2nd consumers; this is
//!   the 3rd).
//!
//! ## Architecture — two externals, the R836 edit-in-cell shape at 2-D
//!
//! * **`DataGridExternal`** (`data_grid`, primary) — the grid coordinator.
//!   Owns the flat `Signal<Vec<CellValue>>` cell model (`row * NCOLS + col`),
//!   the 2-D roving cursor (`focused_row` / `focused_col` Signals), and the
//!   edit latch (`Signal<Option<(row, col)>>`). Each column carries a fixed
//!   [`CellKind`] ([`COL_KINDS`]). Exposes the whole grid for AI-first
//!   introspection: `query value.<r>.<c>` / `col_name.<c>` / `col_kind.<c>` /
//!   `focused_row` / `focused_col` / `editing_row` / `editing_col`,
//!   `intervene value.<r>.<c>` (the deterministic typed-set path), `invoke
//!   toggle` / `begin` / `send`.
//! * **`TextFieldExternal`** (`data_grid_edit`, extra) — ONE shared inline
//!   editor reused across every editable cell (the R836 single-editor
//!   pattern; scales to any cell count). Paints only inside the cell being
//!   edited.
//!
//! ## Keyboard model (WAI-ARIA editable data grid)
//!
//! Single Tab stop with a 2-D roving cursor: `ArrowUp` / `ArrowDown` move
//! the row, `ArrowLeft` / `ArrowRight` the column, `Home` / `End` jump to the
//! first / last column (all clamped — a grid has ends). `Space` toggles a
//! bool cell; `Enter` / `F2` toggles a bool or enters edit mode on a text /
//! int / float cell (focus moves into the shared inline field). While
//! editing: `Enter` commits (parse → write back), `Escape` cancels, and int /
//! float columns gate non-numeric keystrokes. A click-away commit-on-blur
//! rides the field's `with_blur_intent`.
//!
//! ## a11y (R837 §5.40)
//!
//! A WAI-ARIA `grid` (the [`grid_table_nodes`] SSOT): a header row of column
//! names over one data `row` per record, each a row of `gridcell`s. The
//! focused cell carries the roving `focused` flag (`aria-activedescendant`);
//! the typed value is encoded in the cell name (`"Count: 24"`).
//!
//! ## Column sort (R886 — the editable fold)
//!
//! A clicked column header cycles unsorted → asc → desc → unsorted through
//! the [`cycle_col_sort`] / [`grid_order_by`] / [`cell_cmp`] SSOT every
//! read-only grid sorts by; the wire speaks the cross-grid
//! [`grid_sort_str`] vocabulary (`query "sort"` / `intervene "sort"` /
//! `invoke "cycle_sort"` / `query "source_at.<pos>"`). The fold's one design
//! decision: ALL grid state stays **source-keyed** (cursor, edit latch,
//! cell tags, `value.<row>.<col>` addressing) and only the paint / a11y row
//! sequence + arrow navigation consult the derived visual order — so a
//! committed edit that changes the active sort key moves its row on the
//! very next paint while the cursor and the in-flight editor follow the
//! source row (the Excel / Qt `QSortFilterProxyModel` behaviour). The
//! [`GridSortState`] coordinator is deliberately NOT reused here: it owns a
//! static materialized `String` dataset (right for its read-only 10k-row
//! consumers), while this grid's typed `Signal` model is the SSOT — per the
//! R778 family ruling the shared parts are exactly the free-fn SSOT +
//! wire vocabulary, not the coordinator struct.
//!
//! ## Column filter (R891 — the editable fold of the filter axis)
//!
//! An AI-first column filter (no clickable chip — driven by `invoke
//! "set_filter" "<col>=<value>"`, exactly as `hello-grid-filter` /
//! `hello-virtual-sort` drive theirs) shrinks the painted rows to the
//! matching set, composing orthogonally with the sort (filter-then-sort
//! through the same [`grid_order_by`] permutation SSOT). The wire speaks the
//! cross-grid [`grid_filter_str`] vocabulary (`query "filter"` /
//! `intervene "filter"` / `invoke "set_filter"` returning the new `view_len`
//! / `query "view_len"`), so an AI client reads and restores the whole filter
//! in one round-trip — read/write symmetric with the read-only proxies.
//!
//! Because the typed model is the SSOT, the match is by the cell's typed
//! VALUE through [`CellValue::matches_filter`] (the value-not-label peer of
//! `sort_cmp`), not its display string. **Edit-while-filtered** is the
//! fold's payoff invariant (Excel / Qt `QSortFilterProxyModel`): every grid
//! state stays SOURCE-keyed, so committing an edit that flips a row out of
//! the filter drops the row on the next paint AND re-anchors the now-hidden
//! source-keyed cursor to the visible row that takes its screen slot (the one
//! [`reanchor_cursor`] SSOT, shared by the `set_filter` / `intervene` writes
//! and the keyboard commit) — never the silent navigation teleport the R886
//! sort fold left as a documented note.
//!
//! ## Known gaps (honest carry, shared with R836)
//!
//! - Native checkbox / textbox cell roles (per-cell a11y role) — additive.
//! - Per-column validation / clamp ranges — additive.
//! - Multi-facet / substring column filter — one fixed-string column facet
//!   here (the cross-grid `GridFilter` shape); multi-facet is a later
//!   additive axis, exactly as the read-only `GridSortState` filter defers it.
//! - Column grouping / frozen panes on the *editable* grid — remaining fold
//!   rounds (sort landed R886, filter R891; the read-only catalog has
//!   grouping / frozen as separate substrate).

use std::rc::Rc;

use pinion_a11y::{
    grid_table_nodes, AccessNode, GridCell, GridColumn, GridRow, SortDirection, WidgetA11y,
};
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::composite_tag::GridSendKey;
use pinion_core::external::{
    int_of, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    IntrospectSchema, IntrospectValue, InterveneError, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::checkbox::CheckboxState;
use pinion_core::widgets::grid_sort::{
    col_sort_dir, grid_filter_from_str, grid_filter_str, grid_sort_from_str, grid_sort_str,
    GridFilter,
};
use pinion_core::widgets::table::{cycle_col_sort, grid_order_by};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::checkbox::{view_checkbox_box, CheckboxStyle};
use pinion_widget_paint::text_field as tf_paint;

use pinion_widget_paint::state_layer::HOVER;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDataGridRenderer, HelloDataGridRendererError);

// ─── window + layout constants ─────────────────────────────────────

const WIN_W: u32 = 460;
const WIN_H: u32 = 348;
const THEME_TAG: &str = "app";

const TITLE_PX: u32 = 22;
const HEADER_PX: u32 = 13;
const CELL_PX: u32 = 15;

const ROW_H: u32 = 36;
const CELL_PAD: u32 = 8;
const CHECKBOX_SIZE: u32 = 18;
const PANEL_PAD: u32 = 20;
const ROW_GAP: u32 = 1;

// ─── tags + intents ───────────────────────────────────────────────

/// Primary External — the grid coordinator (the single keyboard Tab stop).
const GRID_TAG: &str = "data_grid";
/// Extra External — the one shared inline cell editor.
const EDIT_TF_TAG: &str = "data_grid_edit";
/// Commit-on-blur intent the inline field raises on a click-away (R793).
const EDIT_TF_BLUR_INTENT_TAG: &str = pinion_core::intent_tag!("data_grid_edit", "blur");


// ─── grid shape (an editable asset table) ─────────────────────────

const NROWS: usize = 4;
const NCOLS: usize = 5;

/// Column titles (the header row + the AT cell-name prefix).
const COL_NAMES: [&str; NCOLS] = ["Asset", "Type", "Count", "Scale", "Active"];

/// The per-column [`CellKind`] — every cell in a column shares its column's
/// kind (the editor dispatch, parse, keystroke gate, and intervene coercion
/// all read from here).
const COL_KINDS: [CellKind; NCOLS] = [
    CellKind::Text,
    CellKind::Text,
    CellKind::Int,
    CellKind::Float,
    CellKind::Bool,
];

/// Per-column paint width (logical px). Text columns are wider.
const COL_W: [u32; NCOLS] = [120, 90, 70, 70, 70];

/// `(row, col)` → flat model index.
fn idx(row: usize, col: usize) -> usize {
    row * NCOLS + col
}

// ─── column sort (R886 — the editable fold of the sort axis) ──────

/// R886 / R891 §5.40 — the visual → source row permutation for the live
/// typed model. The editable grid's peer of [`GridSortState::order`]: that
/// proxy owns a *static* materialized `String` dataset (its read-only
/// consumers never mutate), while here the typed [`Signal`] model IS the SSOT
/// and a committed edit must re-order / re-filter the very next paint — so the
/// order derives from the live model on read, through the [`grid_order_by`]
/// permutation SSOT (filter-then-sort) with the typed [`CellValue::sort_cmp`]
/// comparator (R886.1 — the typed model sorts by its VALUES: `Bool`
/// semantically, `Int` exactly, `Float` totally, `Text` via the numeric-aware
/// `cell_cmp` string SSOT; stringifying first would tie the order to display
/// labels). R891 — the `filter` axis (the cross-grid [`GridFilter`] column
/// facet) shrinks the row set FIRST, through the typed
/// [`CellValue::matches_filter`] equality (the value-not-label peer of
/// `sort_cmp`); a `None` filter passes every row (bit-identical to the
/// pre-R891 `|_| true`). At `NROWS = 4` the permutation is recomputed per
/// read; the memoized coordinator remains the scale path (`hello-grid-sort`,
/// 10 000 rows).
fn current_order(
    model: &[CellValue],
    sort: Option<(usize, bool)>,
    filter: Option<&GridFilter>,
) -> Vec<usize> {
    grid_order_by(
        NROWS,
        sort,
        |col, a, b| model[idx(a, col)].sort_cmp(&model[idx(b, col)]),
        |row| {
            filter.is_none_or(|f| {
                model.get(idx(row, f.col)).is_some_and(|c| c.matches_filter(&f.value))
            })
        },
    )
}

/// First-paint cell values (row-major). Each column's values match
/// [`COL_KINDS`].
fn default_cells() -> Vec<CellValue> {
    vec![
        CellValue::Text("Hero".to_owned()), CellValue::Text("sprite".to_owned()),
        CellValue::Int(1), CellValue::Float(1.0), CellValue::Bool(true),
        CellValue::Text("Tree".to_owned()), CellValue::Text("mesh".to_owned()),
        CellValue::Int(24), CellValue::Float(2.5), CellValue::Bool(true),
        CellValue::Text("Coin".to_owned()), CellValue::Text("sprite".to_owned()),
        CellValue::Int(99), CellValue::Float(0.5), CellValue::Bool(false),
        CellValue::Text("Boss".to_owned()), CellValue::Text("mesh".to_owned()),
        CellValue::Int(1), CellValue::Float(4.0), CellValue::Bool(true),
    ]
}

// ─── reactive holders (Owner::cache, shared view ↔ coordinator) ────

#[must_use]
fn use_data_model() -> Rc<Signal<Vec<CellValue>>> {
    let owner = Owner::current().expect("use_data_model requires an active Owner scope");
    owner.cache("data_grid.model", || Signal::new(default_cells()))
}

#[must_use]
fn use_focused_row() -> Rc<Signal<usize>> {
    let owner = Owner::current().expect("use_focused_row requires an active Owner scope");
    owner.cache("data_grid.focused_row", || Signal::new(0_usize))
}

#[must_use]
fn use_focused_col() -> Rc<Signal<usize>> {
    let owner = Owner::current().expect("use_focused_col requires an active Owner scope");
    owner.cache("data_grid.focused_col", || Signal::new(0_usize))
}

/// Edit-mode latch — `Some((row, col))` while that cell is being text-edited
/// (the todomvc `editing_id`, keyed by a 2-D cell). `None` = navigating.
#[must_use]
fn use_editing_cell() -> Rc<Signal<Option<(usize, usize)>>> {
    let owner = Owner::current().expect("use_editing_cell requires an active Owner scope");
    owner.cache("data_grid.editing_cell", || Signal::new(None))
}

/// R886 — active column sort `(col, ascending)`, `None` = source order.
/// The view + a11y tree subscribe by reading it, so a header-click cycle
/// repaints exactly like a model edit. Every other grid state here is
/// SOURCE-keyed (cursor, edit latch, cell addressing) — only the paint /
/// a11y row sequence and arrow navigation consult the derived order, the
/// [[virtualized-multiselect-state-window-independent]] discipline.
#[must_use]
fn use_sort() -> Rc<Signal<Option<(usize, bool)>>> {
    let owner = Owner::current().expect("use_sort requires an active Owner scope");
    owner.cache("data_grid.sort", || Signal::new(None))
}

/// R891 — active column filter `(col, value)`, `None` = unfiltered. A SOURCE-
/// keyed axis exactly like [`use_sort`]: the view + a11y tree subscribe by
/// reading it, so an `set_filter` shrinks the painted rows on the next paint
/// like a sort cycle re-orders them. The cross-grid [`GridFilter`] facet
/// (`hello-grid-filter` / `hello-virtual-sort` speak the same wire vocab).
#[must_use]
fn use_filter() -> Rc<Signal<Option<GridFilter>>> {
    let owner = Owner::current().expect("use_filter requires an active Owner scope");
    owner.cache("data_grid.filter", || Signal::new(None))
}

// ─── cursor re-anchor (the R891 edit-while-filtered SSOT) ─────────

/// R891 — the cursor's visual position in the current `(sort, filter)` order,
/// captured BEFORE a filter / edit mutation so [`reanchor_cursor`] can land
/// the cursor on the row that takes its screen slot. `0` when the cursor is
/// somehow already off-view (callers re-anchor explicitly regardless).
fn cursor_visual_pos(
    model: &Signal<Vec<CellValue>>,
    sort: &Signal<Option<(usize, bool)>>,
    filter: &Signal<Option<GridFilter>>,
    cursor: &Signal<usize>,
) -> usize {
    let order = current_order(&model.get(), sort.get(), filter.get().as_ref());
    let src = cursor.get();
    order.iter().position(|&s| s == src).unwrap_or(0)
}

/// R891 — re-anchor the SOURCE-keyed cursor into the filtered+sorted view
/// after a filter change or an edit filtered its row out (the R886.1 note
/// made good: an EXPLICIT re-anchor, never the silent `position().unwrap_or(0)`
/// teleport the navigation once relied on). A no-op when the cursor's row
/// still passes (still visible); else the cursor lands on the visible row now
/// at its prior visual slot `prior_vis` (clamped — Excel / Qt keep the
/// selection at its screen position); a no-op when the view is now empty (no
/// visible row to land on — the grid shows no active cell until a row
/// reappears). The single SSOT both the coordinator's `set_filter` /
/// `intervene` and the owner-scoped `commit_edit` call (one re-anchor policy,
/// not a per-call-site copy).
fn reanchor_cursor(
    model: &Signal<Vec<CellValue>>,
    sort: &Signal<Option<(usize, bool)>>,
    filter: &Signal<Option<GridFilter>>,
    cursor: &Signal<usize>,
    prior_vis: usize,
) {
    let order = current_order(&model.get(), sort.get(), filter.get().as_ref());
    if order.contains(&cursor.get()) {
        return;
    }
    if let Some(&row) = order.get(prior_vis.min(order.len().saturating_sub(1))) {
        cursor.set(row);
    }
}

// ─── grid coordinator External ────────────────────────────────────

/// The data-grid coordinator. Holds `Rc` clones of the reactive holders
/// (same instances the view fn reads) + the shared editor's
/// [`TextEditState`] for [`Self::begin_edit`] seeding. Mutations write the
/// Signals directly — the R836 `PropertyGridExternal` shape at 2-D.
struct DataGridExternal {
    model: Rc<Signal<Vec<CellValue>>>,
    focused_row: Rc<Signal<usize>>,
    focused_col: Rc<Signal<usize>>,
    editing_cell: Rc<Signal<Option<(usize, usize)>>>,
    editor: Rc<TextEditState>,
    /// R886 — the shared column-sort signal (`use_sort`).
    sort: Rc<Signal<Option<(usize, bool)>>>,
    /// R891 — the shared column-filter signal (`use_filter`).
    filter: Rc<Signal<Option<GridFilter>>>,
}

impl DataGridExternal {
    fn new(
        model: Rc<Signal<Vec<CellValue>>>,
        focused_row: Rc<Signal<usize>>,
        focused_col: Rc<Signal<usize>>,
        editing_cell: Rc<Signal<Option<(usize, usize)>>>,
        editor: Rc<TextEditState>,
        sort: Rc<Signal<Option<(usize, bool)>>>,
        filter: Rc<Signal<Option<GridFilter>>>,
    ) -> Self {
        Self { model, focused_row, focused_col, editing_cell, editor, sort, filter }
    }

    /// R891 — rows passing the active filter (`NROWS` when unfiltered), the
    /// derived view length the AI-first `set_filter` reports in one round-trip.
    fn view_len(&self) -> usize {
        current_order(&self.model.get(), self.sort.get(), self.filter.get().as_ref()).len()
    }

    /// R891 — apply a column filter (`None` clears) and re-anchor the cursor
    /// into the resulting view. An out-of-range column clamps to unfiltered
    /// (mirrors [`GridSortState::set_filter`]). Returns the resulting
    /// [`view_len`](Self::view_len). The one mutation path the wire's
    /// `intervene "filter"` and `invoke "set_filter"` share.
    fn set_filter(&self, filter: Option<GridFilter>) -> usize {
        let filter = filter.filter(|f| f.col < NCOLS);
        let prior_vis =
            cursor_visual_pos(&self.model, &self.sort, &self.filter, &self.focused_row);
        self.filter.set(filter);
        reanchor_cursor(&self.model, &self.sort, &self.filter, &self.focused_row, prior_vis);
        self.view_len()
    }

    /// Toggle the bool at `(row, col)`; no-op (returns `false`) unless the
    /// column is a bool. The checkbox affordance behind `Space` + click.
    fn toggle(&self, row: usize, col: usize) -> bool {
        if col >= NCOLS || COL_KINDS[col] != CellKind::Bool {
            return false;
        }
        let mut toggled = false;
        self.model.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(CellValue::Bool(b)) = next.get_mut(idx(row, col)) {
                *b = !*b;
                toggled = true;
            }
            next
        });
        toggled
    }

    /// Enter edit mode on `(row, col)`: latch the cell, seed the shared
    /// editor with the formatted value (caret parked at the trailing edge),
    /// and request focus into the field. Returns `false` for a bool column
    /// (bools toggle) or an out-of-range cell.
    fn begin_edit(&self, row: usize, col: usize) -> bool {
        if row >= NROWS || col >= NCOLS || !COL_KINDS[col].is_text_editable() {
            return false;
        }
        let model = self.model.get();
        let Some(value) = model.get(idx(row, col)) else {
            return false;
        };
        self.editing_cell.set(Some((row, col)));
        // R878 — `seed` = set_text + caret-at-end (the lifted pair).
        self.editor.seed(value.edit_text());
        pinion_core::focus_request::request(EDIT_TF_TAG);
        true
    }

    fn set_focused_row_clamped(&self, row: usize) {
        self.focused_row.set(row.min(NROWS - 1));
    }

    fn set_focused_col_clamped(&self, col: usize) {
        self.focused_col.set(col.min(NCOLS - 1));
    }
}

impl core::fmt::Debug for DataGridExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DataGridExternal")
            .field("focused_row", &self.focused_row.get())
            .field("focused_col", &self.focused_col.get())
            .field("editing_cell", &self.editing_cell.get())
            .field("sort", &self.sort.get())
            .field("filter", &self.filter.get())
            .finish_non_exhaustive()
    }
}

impl External for DataGridExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Tui, Backend::Rpc], BackendFallback::Skip)
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
}

impl ExternalIntrospect for DataGridExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("row_count", "int"),
            ("col_count", "int"),
            ("focused_row", "int"),
            ("focused_col", "int"),
            ("editing_row", "int"),
            ("editing_col", "int"),
            ("col_name.<col>", "string"),
            ("col_kind.<col>", "string"),
            ("value.<row>.<col>", "json"),
            ("sort", "string"),
            ("filter", "string"),
            ("view_len", "int"),
            ("source_at.<pos>", "int"),
            ("send", "string"),
            ("toggle", "json"),
            ("begin", "json"),
            ("cycle_sort", "json"),
            ("set_filter", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "row_count" => Some(IntrospectValue::Int(int_of(NROWS))),
            "col_count" => Some(IntrospectValue::Int(int_of(NCOLS))),
            "focused_row" => Some(IntrospectValue::Int(int_of(self.focused_row.get()))),
            "focused_col" => Some(IntrospectValue::Int(int_of(self.focused_col.get()))),
            "editing_row" => Some(match self.editing_cell.get() {
                Some((row, _)) => IntrospectValue::Int(int_of(row)),
                None => IntrospectValue::Null,
            }),
            "editing_col" => Some(match self.editing_cell.get() {
                Some((_, col)) => IntrospectValue::Int(int_of(col)),
                None => IntrospectValue::Null,
            }),
            // R886 — the wire form is the cross-grid `grid_sort_str`
            // vocabulary ("<col>:asc" / "<col>:desc" / "" = unsorted),
            // byte-identical to the read-only sort proxies.
            "sort" => Some(IntrospectValue::Text(grid_sort_str(self.sort.get()))),
            // R891 — the cross-grid `grid_filter_str` vocabulary
            // ("none" / "<col>=<value>"), byte-identical to the read-only
            // `GridSortExternal` filter facet.
            "filter" => {
                Some(IntrospectValue::Text(grid_filter_str(self.filter.get().as_ref())))
            }
            // R891 — rows passing the active filter (the read side of the
            // `set_filter` outcome; `NROWS` when unfiltered).
            "view_len" => Some(IntrospectValue::Int(int_of(self.view_len()))),
            _ => {
                // R886 — `source_at.<pos>`: the source row painted at
                // visual position `pos` under the active sort (identity
                // when unsorted) — the AI-side order introspection.
                if let Some(pos_str) = path.strip_prefix("source_at.") {
                    // R886.1 — the shared `source_at.` projection SSOT:
                    // out-of-range / unparseable reports Null
                    // (present-but-empty), never absence — the family
                    // contract every sort proxy speaks.
                    let model = self.model.get();
                    let order = current_order(&model, self.sort.get(), self.filter.get().as_ref());
                    return Some(pinion_core::widgets::order_memo::source_at_value(
                        pos_str,
                        |p| order.get(p).copied(),
                    ));
                }
                if let Some(col_str) = path.strip_prefix("col_name.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_NAMES.get(col).map(|n| IntrospectValue::Text((*n).to_owned()));
                }
                if let Some(col_str) = path.strip_prefix("col_kind.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_KINDS.get(col).map(|k| IntrospectValue::Text(k.name().to_owned()));
                }
                if let Some(rest) = path.strip_prefix("value.") {
                    let (row_str, col_str) = rest.split_once('.')?;
                    let row: usize = row_str.parse().ok()?;
                    let col: usize = col_str.parse().ok()?;
                    let model = self.model.get();
                    return model.get(idx(row, col)).map(CellValue::to_introspect);
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "row_count" | "col_count" | "editing_row" | "editing_col" | "view_len" => {
                Err(InterveneError::ReadOnly)
            }
            "focused_row" => match value {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    self.set_focused_row_clamped(row);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "focused_col" => match value {
                IntrospectValue::Int(i) => {
                    let col = usize::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    self.set_focused_col_clamped(col);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R886 — admin / restore write of the sort key, in the same
            // `grid_sort_from_str` vocabulary `query "sort"` emits
            // (decode = inverse of encode). An out-of-range column clamps
            // to unsorted, mirroring `GridSortState::set_sort`.
            "sort" => match value {
                IntrospectValue::Text(ref s) => {
                    let sort = grid_sort_from_str(s).filter(|&(c, _)| c < NCOLS);
                    self.sort.set(sort);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R891 — admin / restore write of the column filter, in the same
            // `grid_filter_from_str` vocabulary `query "filter"` emits
            // (decode = inverse of encode); `Null` clears. The cursor
            // re-anchors into the new view (`set_filter` SSOT), mirroring
            // `GridSortExternal`'s `intervene "filter"`.
            "filter" => match value {
                IntrospectValue::Text(ref s) => {
                    self.set_filter(grid_filter_from_str(s));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_filter(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => {
                let Some(rest) = path.strip_prefix("value.") else {
                    return Err(InterveneError::UnknownPath);
                };
                let (row_str, col_str) =
                    rest.split_once('.').ok_or(InterveneError::UnknownPath)?;
                let row: usize = row_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                let col: usize = col_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                if row >= NROWS || col >= NCOLS {
                    return Err(InterveneError::UnknownPath);
                }
                let new_value = COL_KINDS[col].coerce(value)?;
                // R891 — a typed-set that flips the cursor's row out of an
                // active filter re-anchors the cursor (no-op when the write
                // leaves the cursor's row visible), so the AI write path keeps
                // the same cursor-stays-visible invariant the edit commit does.
                let prior_vis =
                    cursor_visual_pos(&self.model, &self.sort, &self.filter, &self.focused_row);
                self.model.set_with(move |prev| {
                    let mut next = prev.clone();
                    next[idx(row, col)] = new_value.clone();
                    next
                });
                reanchor_cursor(&self.model, &self.sort, &self.filter, &self.focused_row, prior_vis);
                Ok(())
            }
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Composite wire `"<row>_<col>:<EventName>"` (the shared
            // `GridSendKey` SSOT, the same grammar hello-table encodes /
            // decodes). PointerUp focuses the cell (and toggles a bool);
            // DoubleClick enters edit mode on an editable cell.
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    // R880.1 — the `split_send_payload` `:` grammar SSOT
                    // strips a held-modifier third segment (the hand-rolled
                    // split_once read "PointerUp:c" as the event name and a
                    // Ctrl+click on a cell was silently rejected).
                    let (key, event_name, _mods) =
                        pinion_core::composite_tag::split_send_payload(s)
                            .ok_or(InvokeError::Rejected)?;
                    match GridSendKey::parse(key).ok_or(InvokeError::Rejected)? {
                        // R886 — a clicked column header cycles that
                        // column's sort through the `cycle_col_sort` SSOT
                        // (unsorted → asc → desc → unsorted; a different
                        // column jumps to it ascending), exactly the
                        // read-only grids' header behaviour.
                        GridSendKey::Header { col } => {
                            if col >= NCOLS {
                                return Err(InvokeError::Rejected);
                            }
                            if event_name == "PointerUp" {
                                self.sort.set(cycle_col_sort(self.sort.get(), col, NCOLS));
                            }
                            Ok(IntrospectValue::Null)
                        }
                        GridSendKey::Cell { row, col } => {
                            if row >= NROWS || col >= NCOLS {
                                return Err(InvokeError::Rejected);
                            }
                            match event_name {
                                "PointerUp" => {
                                    self.focused_row.set(row);
                                    self.focused_col.set(col);
                                    self.toggle(row, col);
                                    Ok(IntrospectValue::Null)
                                }
                                "DoubleClick" => {
                                    Ok(IntrospectValue::Bool(self.begin_edit(row, col)))
                                }
                                _ => Ok(IntrospectValue::Null),
                            }
                        }
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Toggle the focused bool cell (the `Space` keyboard path + RPC).
            "toggle" => {
                let toggled = self.toggle(self.focused_row.get(), self.focused_col.get());
                Ok(IntrospectValue::Bool(toggled))
            }
            // Enter edit mode on the focused cell (the `Enter` / `F2` path).
            "begin" => {
                let started = self.begin_edit(self.focused_row.get(), self.focused_col.get());
                Ok(IntrospectValue::Bool(started))
            }
            // R886 — the RPC shortcut for a header click: cycle `col`'s
            // sort. R886.1 — out-of-range `col` is a silent no-op
            // returning the unchanged key, matching the
            // `GridSortExternal::cycle_sort` / `cycle_col_sort` family
            // contract it mirrors (one wire name, one edge semantics).
            "cycle_sort" => match args {
                IntrospectValue::Int(i) => {
                    let col = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    self.sort.set(cycle_col_sort(self.sort.get(), col, NCOLS));
                    Ok(IntrospectValue::Text(grid_sort_str(self.sort.get())))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R891 — the AI-first column filter: a `"<col>=<value>"` payload
            // filters, `Null` clears. Returns the resulting `view_len` (rows
            // passing the filter) in one round-trip, byte-identical to
            // `GridSortExternal::set_filter`. The cursor re-anchors into the
            // new view inside `Self::set_filter`.
            "set_filter" => {
                let view_len = match args {
                    IntrospectValue::Text(ref s) => self.set_filter(grid_filter_from_str(s)),
                    IntrospectValue::Null => self.set_filter(None),
                    _ => return Err(InvokeError::TypeMismatch),
                };
                Ok(IntrospectValue::Int(int_of(view_len)))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ─── inline-editor commit / cancel (keyboard, owner-scoped) ───────

/// Commit the in-flight edit: parse the editor text by the editing column's
/// kind and write it back to the cell. A malformed numeric commit keeps the
/// prior value (no data loss). Mirrors `todomvc::commit_edit`.
fn commit_edit(restore_focus: bool) {
    let editing = use_editing_cell();
    let Some((row, col)) = editing.get() else {
        return;
    };
    let model = use_data_model();
    let sort = use_sort();
    let filter = use_filter();
    let cursor = use_focused_row();
    let text = use_text_edit_state(EDIT_TF_TAG).text();
    // R891 — capture the edited row's visual slot BEFORE the write so a
    // commit that filters the row out can re-anchor the cursor to the row
    // that takes its screen position (the cursor IS the edited row — editing
    // never moves the grid cursor).
    let prior_vis = cursor_visual_pos(&model, &sort, &filter, &cursor);
    if col < NCOLS {
        if let Some(parsed) = COL_KINDS[col].parse(&text) {
            model.set_with(move |prev| {
                let mut next = prev.clone();
                next[idx(row, col)] = parsed.clone();
                next
            });
        }
    }
    end_edit_mode(restore_focus);
    // R891 — if the committed value flipped the row out of an active filter,
    // re-anchor the now-hidden cursor (no-op when the row still passes).
    reanchor_cursor(&model, &sort, &filter, &cursor, prior_vis);
}

fn cancel_edit() {
    end_edit_mode(true);
}

fn end_edit_mode(restore_focus: bool) {
    use_editing_cell().set(None);
    use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
    if restore_focus {
        pinion_core::focus_request::request(GRID_TAG);
    }
}

/// The kind of the column currently being edited (`None` when not editing) —
/// drives the int / float keystroke gate.
fn editing_col_kind() -> Option<CellKind> {
    let (_, col) = use_editing_cell().get()?;
    COL_KINDS.get(col).copied()
}

// ─── keyboard ─────────────────────────────────────────────────────

/// Grid-focused keymap: 2-D roving navigation + activate.
fn apply_key_grid(scene: &mut Scene, key: &str) -> bool {
    let row_sig = use_focused_row();
    let col_sig = use_focused_col();
    let col = col_sig.get().min(NCOLS - 1);
    match key {
        // R886 / R891 — vertical navigation walks the filtered+sorted VISUAL
        // sequence while the cursor itself stays SOURCE-keyed: resolve the
        // cursor's visual position in the current order, step there, store the
        // source row found at the destination. Identity mapping when unsorted
        // + unfiltered (the pre-R886 behaviour, bit-identical). R891 — the
        // cursor is kept visible by the re-anchor invariant, so its visual
        // position is present; a cursor off the view (defensive) or an empty
        // view (a filter excluded every row) has no row to step to, so the
        // vertical arms no-op rather than the old silent `unwrap_or(0)`
        // teleport (the R886.1 note made good).
        "ArrowDown" | "ArrowUp" => {
            let order =
                current_order(&use_data_model().get(), use_sort().get(), use_filter().get().as_ref());
            let row = row_sig.get().min(NROWS - 1);
            let Some(vis) = order.iter().position(|&s| s == row) else {
                return false;
            };
            let dest = if key == "ArrowDown" {
                (vis + 1).min(order.len() - 1)
            } else {
                vis.saturating_sub(1)
            };
            row_sig.set(order[dest]);
            true
        }
        "ArrowRight" => {
            col_sig.set((col + 1).min(NCOLS - 1));
            true
        }
        "ArrowLeft" => {
            col_sig.set(col.saturating_sub(1));
            true
        }
        "Home" => {
            col_sig.set(0);
            true
        }
        "End" => {
            col_sig.set(NCOLS - 1);
            true
        }
        "Space" => activate_focused(scene, col, false),
        "Enter" | "F2" => activate_focused(scene, col, true),
        _ => false,
    }
}

/// Activate the focused cell: toggle a bool, or (when `allow_edit`) enter
/// edit mode on a text / int / float cell. Routes through the coordinator's
/// `invoke` so toggle / begin live in one place (the RPC path).
fn activate_focused(scene: &mut Scene, col: usize, allow_edit: bool) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(GRID_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    if COL_KINDS.get(col).copied() == Some(CellKind::Bool) {
        intro.invoke("toggle", IntrospectValue::Null).is_ok()
    } else if allow_edit {
        intro.invoke("begin", IntrospectValue::Null).is_ok()
    } else {
        false
    }
}

/// Edit-mode keymap over the shared inline field — the lifted
/// [`pinion_core::edit_field_keymap`] SSOT (R878; this binding carried one
/// of the two pre-lift copies). Commit / cancel stay binding policy; a
/// defensive "no cell is editing" resolves to [`CellKind::Bool`] (accepts
/// no keystroke), so only commit / cancel / caret keys remain meaningful.
fn apply_key_edit(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    let kind = editing_col_kind().unwrap_or(CellKind::Bool);
    pinion_core::edit_field_keymap(
        scene,
        EDIT_TF_TAG,
        key,
        modifiers,
        kind,
        || commit_edit(true),
        cancel_edit,
    )
}

// ─── paint ────────────────────────────────────────────────────────

/// Focused-cell background = the M3 `OnSurface` state-layer over the surface.
fn cell_fill(theme: &Theme, focused: bool) -> Color {
    if focused {
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::OnSurface), HOVER)
    } else {
        Color::TRANSPARENT
    }
}

/// Cell-sized M3 checkbox-box style. The bool cell renders the lifted
/// `view_checkbox_box` SSOT non-interactively (the grid coordinator owns the
/// toggle, so there is no per-cell `CheckboxExternal`) — one M3 checkbox
/// rendering across the catalog instead of a hand-rolled copy.
fn cell_checkbox_style() -> CheckboxStyle {
    CheckboxStyle { box_size: CHECKBOX_SIZE, glyph_size_px: 14, ..CheckboxStyle::m3_filled() }
}

/// One cell: tagged `data_grid#<row>_<col>` (the `GridSendKey` encoding) so a
/// click routes to the coordinator. Paints the shared inline field while
/// editing, else a checkbox (bool) or the value text.
fn view_cell(
    row: usize,
    col: usize,
    value: &CellValue,
    focused: bool,
    edit_active: bool,
    theme: &Theme,
    edit_field: (TextFieldState, u32),
) -> Scene {
    let inner = if edit_active {
        let style = tf_paint::TextFieldStyle {
            field_w: COL_W[col] - CELL_PAD,
            field_h: ROW_H - 6,
            ..tf_paint::TextFieldStyle::m3_filled()
        };
        tf_paint::view_field(EDIT_TF_TAG, edit_field.0, edit_field.1, theme, &style, "")
    } else if COL_KINDS[col] == CellKind::Bool {
        let checked = matches!(value, CellValue::Bool(true));
        view_checkbox_box(checked, CheckboxState::Idle, theme, &cell_checkbox_style())
    } else {
        Scene::Text(TextNode::styled(
            value.display(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CELL_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))
    };
    Scene::Container(
        ContainerNode::new(vec![inner])
            .with_tag(format!("{GRID_TAG}#{}", GridSendKey::Cell { row, col }.encode()))
            .with_style(BoxStyle::filled(cell_fill(theme, focused)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                    .with_size(Size::px(COL_W[col], ROW_H)),
            ),
    )
}

/// The column-header row.
fn view_header(theme: &Theme, sort: Option<(usize, bool)>) -> Scene {
    let cells: Vec<Scene> = COL_NAMES
        .iter()
        .enumerate()
        .map(|(col, label)| {
            // R886 — the active sort column appends the direction glyph;
            // the header cell carries the composite `Header` send tag so a
            // click routes to the coordinator's sort cycle (the same
            // `h<col>` sub-key grammar the read-only grids use).
            let glyph = pinion_widget_paint::glyph::sort_glyph(col_sort_dir(sort, col))
                .map(|g| format!(" {g}"))
                .unwrap_or_default();
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    format!("{label}{glyph}"),
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(HEADER_PX)
                        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
                ))])
                .with_tag(format!("{GRID_TAG}#{}", GridSendKey::Header { col }.encode()))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                        .with_size(Size::px(COL_W[col], ROW_H)),
                ),
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(cells)
            .with_tag("dg_header")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_align_items(AlignItems::Center)),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RootState, _frame: &Frame) -> Scene {
    let (edit_state, edit_caret) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let model = use_data_model().get();
    let focused_row = use_focused_row().get();
    let focused_col = use_focused_col().get();
    let editing = use_editing_cell().get();
    // R886 / R891 — paint rows in the filtered+sorted visual order; every
    // cell keeps its SOURCE identity (tags, cursor, edit latch), so a
    // committed edit that changes the sort key visibly moves its row — or a
    // filter facet drops it — on this very repaint while the cursor and any
    // in-flight editor follow the source row.
    let sort = use_sort().get();
    let filter = use_filter().get();
    let order = current_order(&model, sort, filter.as_ref());

    let title = Scene::Text(TextNode::styled(
        "Asset table",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let mut rows: Vec<Scene> = Vec::with_capacity(NROWS + 1);
    rows.push(view_header(&theme, sort));
    for &row in &order {
        let cells: Vec<Scene> = (0..NCOLS)
            .map(|col| {
                let value = &model[idx(row, col)];
                let focused = row == focused_row && col == focused_col;
                let edit_active = editing == Some((row, col)) && COL_KINDS[col].is_text_editable();
                view_cell(row, col, value, focused, edit_active, &theme, (edit_state, edit_caret))
            })
            .collect();
        // R886.1 — the painted row carries the same `dg_row<src>` tag its
        // a11y `row` node uses, so AT row bounds attach (the columnheader
        // parity applied to the row axis; pre-R886.1 the tags matched no
        // painted node).
        rows.push(Scene::Container(
            ContainerNode::new(cells)
                .with_tag(format!("dg_row{row}"))
                .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_align_items(AlignItems::Center)),
        ));
    }
    let grid = Scene::Container(
        ContainerNode::new(rows)
            .with_tag(GRID_TAG)
            .with_aria_label("Asset table")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)).with_border(
                Border::new(theme.resolve(ColorRole::Outline), 1),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(ROW_GAP),
            ),
    );

    // R891 — a scene-as-data readout of the active filter + resulting view
    // size (`filter 1=mesh \u{00B7} showing 2 of 4`), the witness that the
    // row set shrank — the `hello-grid-filter` status-bar pattern at the
    // editable grid's scale. Tagged for AI-first introspection.
    let status = Scene::Text(
        TextNode::styled(
            format!(
                "filter {} \u{00B7} showing {} of {NROWS}",
                grid_filter_str(filter.as_ref()),
                order.len(),
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(HEADER_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("dg_status"),
    );

    Scene::Container(
        ContainerNode::new(vec![title, status, grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_padding(Rect::new(PANEL_PAD, PANEL_PAD, PANEL_PAD, PANEL_PAD))
                    .with_gap(ROW_GAP * 8)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

// ─── WidgetCore impl ──────────────────────────────────────────────

/// Cached paint posture — only the shared inline field's interaction state +
/// caret. The model / cursor / edit-mode are read reactively in the view fn.
type RootState = (TextFieldState, u32);

struct DataGridView;

impl WidgetCore for DataGridView {
    type State = RootState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let model = use_data_model();
        let focused_row = use_focused_row();
        let focused_col = use_focused_col();
        let editing = use_editing_cell();
        let editor = use_text_edit_state(EDIT_TF_TAG);
        let sort = use_sort();
        let filter = use_filter();
        Box::new(DataGridExternal::new(
            model, focused_row, focused_col, editing, editor, sort, filter,
        ))
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        let editor_state = use_text_edit_state(EDIT_TF_TAG);
        let blink = use_caret_blink(EDIT_TF_TAG);
        vec![ExtraExternal::new(
            EDIT_TF_TAG,
            Box::new(
                TextFieldExternal::new()
                    .attach_state(editor_state)
                    .attach_blink(blink)
                    .with_blur_intent(),
            ),
        )]
    }

    fn read_state(scene: &Scene) -> RootState {
        tf_paint::read_text_field_state(scene, EDIT_TF_TAG)
    }

    fn view(state: RootState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-data-grid (R837 §5.38 editable data grid)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn focusable_tags() -> Vec<&'static str> {
        vec![GRID_TAG, EDIT_TF_TAG]
    }

    /// R793 §5.38 — commit-on-blur: the inline editor lost focus (a click
    /// elsewhere) while editing → commit without restoring focus. The
    /// `editing_cell` gate makes the post-commit blur a no-op.
    fn update(_state: RootState, intent: &pinion_core::Intent) -> Vec<Command> {
        if intent.tag_str() == EDIT_TF_BLUR_INTENT_TAG && use_editing_cell().get().is_some() {
            commit_edit(false);
        }
        Vec::new()
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        match focused {
            Some(GRID_TAG) => apply_key_grid(scene, key),
            Some(EDIT_TF_TAG) => apply_key_edit(scene, key, modifiers),
            _ => false,
        }
    }

    /// Route IME composition to the inline editor while it owns focus —
    /// through the lifted R764.1 SSOT (R878 audit replaced a hand-rolled
    /// copy of the same reformat block).
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(EDIT_TF_TAG) {
            return false;
        }
        tf_paint::forward_composition_to_field(scene, EDIT_TF_TAG, event)
    }
}

impl WidgetA11y for DataGridView {
    /// R837 §5.40 — the grid lowers through the lifted [`grid_table_nodes`]
    /// SSOT (3rd consumer). A column-name header over one row per record; the
    /// focused cell carries the roving `focused` flag (`aria-activedescendant`).
    fn access_node(_state: &RootState, _focused: Option<&str>) -> Vec<AccessNode> {
        let model = use_data_model().get();
        let focused_row = use_focused_row().get();
        let focused_col = use_focused_col().get();
        // R886 — the active column announces `aria-sort` (WAI-ARIA 1.2
        // §6.6.2) and the rows are emitted in the sorted visual order, so
        // AT linear navigation matches what sighted users see. R891 — the
        // rows are also filtered, so AT sees exactly the visible set (the
        // grid `row` count tracks the filtered `view_len`).
        let sort = use_sort().get();
        let order = current_order(&model, sort, use_filter().get().as_ref());
        let columns: Vec<GridColumn> = COL_NAMES
            .iter()
            .enumerate()
            .map(|(col, label)| GridColumn {
                // R886.1 — the a11y columnheader tag IS the painted
                // clickable header tag, so `rect_for_tag` bounds attach
                // and an AT activation routes to the sort wire (the
                // hello-table / grouped-grid-sort parity; the R837-era
                // `dg_col<n>` tags matched no painted node).
                tag: format!("{GRID_TAG}#{}", GridSendKey::Header { col }.encode()),
                label: (*label).to_owned(),
                sort: col_sort_dir(sort, col).map(SortDirection::from_ascending),
            })
            .collect();
        let rows: Vec<GridRow> = order
            .iter()
            .map(|&row| GridRow {
                tag: format!("dg_row{row}"),
                selected: false,
                state: RadioState::Idle,
                cells: (0..NCOLS)
                    .map(|col| GridCell {
                        tag: format!("{GRID_TAG}#{}", GridSendKey::Cell { row, col }.encode()),
                        name: format!("{}: {}", COL_NAMES[col], model[idx(row, col)].display()),
                        focused: row == focused_row && col == focused_col,
                    })
                    .collect(),
            })
            .collect();
        grid_table_nodes(GRID_TAG, "Asset table", false, "dg_header", &columns, &rows)
    }
}

impl WidgetView for DataGridView {
    type Renderer = HelloDataGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<DataGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn boot_scene() -> Scene {
        let mut children = vec![Scene::External(
            ExternalNode::new(DataGridView::create_external()).with_tag(GRID_TAG),
        )];
        for extra in DataGridView::create_extra_externals() {
            children.push(Scene::External(ExternalNode::new(extra.handle).with_tag(extra.tag)));
        }
        Scene::Container(ContainerNode::new(children))
    }

    fn grid_intro(scene: &Scene) -> &dyn ExternalIntrospect {
        scene
            .find_external_with_tag(GRID_TAG)
            .and_then(|n| n.handle.introspect())
            .expect("grid external present")
    }

    #[test]
    fn r837_shape_and_defaults() {
        assert_eq!(default_cells().len(), NROWS * NCOLS);
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("row_count"), Some(IntrospectValue::Int(4)));
            assert_eq!(intro.query("col_count"), Some(IntrospectValue::Int(5)));
            assert_eq!(intro.query("col_name.0"), Some(IntrospectValue::Text("Asset".to_owned())));
            assert_eq!(intro.query("col_kind.2"), Some(IntrospectValue::Text("int".to_owned())));
            assert_eq!(intro.query("col_kind.4"), Some(IntrospectValue::Text("bool".to_owned())));
            assert_eq!(intro.query("value.1.0"), Some(IntrospectValue::Text("Tree".to_owned())));
            assert_eq!(intro.query("value.1.2"), Some(IntrospectValue::Int(24)));
            assert_eq!(intro.query("value.2.4"), Some(IntrospectValue::Bool(false)));
            assert_eq!(intro.query("value.9.9"), None, "out-of-range -> None");
            assert_eq!(intro.query("editing_row"), Some(IntrospectValue::Null));
        });
    }

    #[test]
    fn r837_intervene_typed_value_strict() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(intro.intervene("value.0.2", IntrospectValue::Int(7)).is_ok());
            assert_eq!(
                intro.intervene("value.0.2", IntrospectValue::Text("no".to_owned())),
                Err(InterveneError::TypeMismatch),
                "int column rejects text",
            );
            assert!(intro.intervene("value.3.3", IntrospectValue::Float(9.5)).is_ok());
            assert_eq!(intro.query("value.0.2"), Some(IntrospectValue::Int(7)));
            assert_eq!(intro.query("value.3.3"), Some(IntrospectValue::Float(9.5)));
        });
    }

    #[test]
    fn r837_intervene_focus_clamps_both_axes() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(intro.intervene("focused_row", IntrospectValue::Int(99)).is_ok());
            assert!(intro.intervene("focused_col", IntrospectValue::Int(99)).is_ok());
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("focused_col"), Some(IntrospectValue::Int(4)));
        });
    }

    #[test]
    fn r837_click_focuses_cell_and_toggles_bool() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Cell (2,4) is the Active bool = false.
            let _ = intro.invoke("send", IntrospectValue::Text("2_4:PointerUp".to_owned()));
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(2)));
            assert_eq!(intro.query("focused_col"), Some(IntrospectValue::Int(4)));
            assert_eq!(intro.query("value.2.4"), Some(IntrospectValue::Bool(true)), "toggled");
            // A click on a text cell focuses but does not toggle.
            let _ = intro.invoke("send", IntrospectValue::Text("0_0:PointerUp".to_owned()));
            assert_eq!(intro.query("value.0.0"), Some(IntrospectValue::Text("Hero".to_owned())));
        });
    }

    #[test]
    fn r837_double_click_begins_edit_on_editable_cell() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("send", IntrospectValue::Text("1_2:DoubleClick".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(intro.query("editing_row"), Some(IntrospectValue::Int(1)));
            assert_eq!(intro.query("editing_col"), Some(IntrospectValue::Int(2)));
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "24", "seeded with the int value");
            // Double-click on a bool cell does not edit.
            assert_eq!(
                intro.invoke("send", IntrospectValue::Text("0_4:DoubleClick".to_owned())),
                Ok(IntrospectValue::Bool(false)),
            );
        });
    }

    #[test]
    fn r837_begin_commit_writes_back_parsed_value() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(1));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(2));
            assert_eq!(intro.invoke("begin", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            use_text_edit_state(EDIT_TF_TAG).set_text("250".to_owned());
            commit_edit(true);
            assert_eq!(grid_intro(&scene).query("value.1.2"), Some(IntrospectValue::Int(250)));
            assert_eq!(grid_intro(&scene).query("editing_row"), Some(IntrospectValue::Null));
        });
    }

    #[test]
    fn r837_commit_malformed_reverts() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(3));
            let _ = intro.invoke("begin", IntrospectValue::Null);
            use_text_edit_state(EDIT_TF_TAG).set_text("xyz".to_owned());
            commit_edit(true);
            assert_eq!(grid_intro(&scene).query("value.0.3"), Some(IntrospectValue::Float(1.0)));
        });
    }

    #[test]
    fn r837_keyboard_roves_both_axes_and_clamps() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowRight", m));
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", m));
            assert_eq!(use_focused_row().get(), 1);
            assert_eq!(use_focused_col().get(), 1);
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "End", m));
            assert_eq!(use_focused_col().get(), NCOLS - 1);
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowRight", m));
            assert_eq!(use_focused_col().get(), NCOLS - 1, "clamps at the last column");
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "Home", m));
            assert_eq!(use_focused_col().get(), 0);
        });
    }

    #[test]
    fn r837_space_toggles_bool_enter_edits_number() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let m = Modifiers::empty();
            // Focus the Active bool of row 0 (col 4) and Space-toggle.
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| {
                    let _ = i.intervene("focused_row", IntrospectValue::Int(0));
                    i.intervene("focused_col", IntrospectValue::Int(4))
                });
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "Space", m));
            assert_eq!(grid_intro(&scene).query("value.0.4"), Some(IntrospectValue::Bool(false)));
            // Focus the Count int of row 0 (col 2) and Enter -> edit mode.
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| i.intervene("focused_col", IntrospectValue::Int(2)));
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "Enter", m));
            assert_eq!(grid_intro(&scene).query("editing_col"), Some(IntrospectValue::Int(2)));
        });
    }

    #[test]
    fn r837_edit_float_gate_allows_dot_drops_letter() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(3)); // Scale (float)
            let _ = intro.invoke("begin", IntrospectValue::Null);
            use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
            use_text_edit_state(EDIT_TF_TAG).set_caret(0);
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "2", m));
            assert!(DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), ".", m), "float accepts dot");
            assert!(DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "5", m));
            assert!(!DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "z", m), "letter dropped");
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "2.5");
        });
    }

    #[test]
    fn r837_access_node_emits_grid_with_active_cell() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            use_focused_row().set(2);
            use_focused_col().set(2);
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            // grid + header row + 5 columnheaders + 4 rows + 20 cells.
            assert_eq!(nodes.len(), 1 + 1 + NCOLS + NROWS + NROWS * NCOLS);
            assert_eq!(nodes[0].role, pinion_a11y::AriaRole::Grid);
            let active = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#2_2"))
                .expect("focused cell present");
            assert!(active.state.focused, "the focused cell is the active descendant");
            assert_eq!(active.name.as_deref(), Some("Count: 99"));
        });
    }

    #[test]
    fn r837_view_carries_grid_and_cell_tags() {
        Owner::new().run(|| {
            let _ = boot_scene();
            let scene = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(scene.contains_tag(GRID_TAG), "grid root painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#0_0")), "cell (0,0) painted");
            assert!(scene.contains_tag(&format!("{GRID_TAG}#3_4")), "cell (3,4) painted");
            assert!(!scene.contains_tag(EDIT_TF_TAG), "no inline field when not editing");
        });
    }

    #[test]
    fn r837_view_contains_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<DataGridView>(
            (TextFieldState::Idle, 0),
            &Frame::default(),
        );
    }

    #[test]
    fn r886_header_click_cycles_sort_and_orders_view() {
        // Count column (col 2) values: 1, 24, 99, 1 — asc keeps the equal
        // keys in source order (stable): [0, 3, 1, 2]; desc reverses the
        // comparison (not the slice): [2, 1, 0, 3].
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(intro.query("sort"), Some(IntrospectValue::Text("none".to_owned())));
            assert_eq!(intro.query("source_at.1"), Some(IntrospectValue::Int(1)), "identity");

            let _ = intro.invoke("send", IntrospectValue::Text("h2:PointerUp".to_owned()));
            assert_eq!(intro.query("sort"), Some(IntrospectValue::Text("2:ascending".to_owned())));
            for (pos, src) in [(0, 0), (1, 3), (2, 1), (3, 2)] {
                assert_eq!(
                    intro.query(&format!("source_at.{pos}")),
                    Some(IntrospectValue::Int(src)),
                    "stable ascending order",
                );
            }

            let _ = intro.invoke("send", IntrospectValue::Text("h2:PointerUp".to_owned()));
            assert_eq!(intro.query("sort"), Some(IntrospectValue::Text("2:descending".to_owned())));
            assert_eq!(intro.query("source_at.0"), Some(IntrospectValue::Int(2)));

            let _ = intro.invoke("send", IntrospectValue::Text("h2:PointerUp".to_owned()));
            assert_eq!(intro.query("sort"), Some(IntrospectValue::Text("none".to_owned())));
        });
    }

    #[test]
    fn r886_edit_while_sorted_reorders_and_cursor_follows_source() {
        // The fold's payoff invariant: with Count ascending, raising row 0's
        // Count from 1 to 500 moves that row to the visual bottom on the
        // SAME model write (the order derives from the live model), while
        // the source-keyed cursor stays on row 0 — Excel's "the cell I
        // edited is still my cell" behaviour.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("cycle_sort", IntrospectValue::Int(2));
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(2));
            assert_eq!(intro.invoke("begin", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            use_text_edit_state(EDIT_TF_TAG).set_text("500".to_owned());
            commit_edit(true);
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("value.0.2"), Some(IntrospectValue::Int(500)), "source write");
            assert_eq!(
                intro.query("source_at.3"),
                Some(IntrospectValue::Int(0)),
                "edited row re-sorted to the visual bottom",
            );
            assert_eq!(
                intro.query("focused_row"),
                Some(IntrospectValue::Int(0)),
                "cursor is source-keyed: it follows the moved row",
            );
        });
    }

    #[test]
    fn r886_arrow_nav_walks_visual_order_not_source() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("cycle_sort", IntrospectValue::Int(2));
            }
            // Ascending Count order = [0, 3, 1, 2]; from source row 0
            // (visual 0) ArrowDown must land on source row 3 (visual 1),
            // not source row 1.
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", m));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(3)),
                "ArrowDown steps the VISUAL sequence",
            );
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowUp", m));
            assert_eq!(grid_intro(&scene).query("focused_row"), Some(IntrospectValue::Int(0)));
        });
    }

    #[test]
    fn r886_sort_intervene_round_trips_and_clamps() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // decode = inverse of encode (the cross-grid wire vocabulary).
            assert_eq!(intro.intervene("sort", IntrospectValue::Text("3:descending".to_owned())), Ok(()));
            assert_eq!(intro.query("sort"), Some(IntrospectValue::Text("3:descending".to_owned())));
            // Out-of-range column clamps to unsorted (GridSortState mirror).
            assert_eq!(intro.intervene("sort", IntrospectValue::Text("9:ascending".to_owned())), Ok(()));
            assert_eq!(intro.query("sort"), Some(IntrospectValue::Text("none".to_owned())));
            // R886.1 — out-of-range cycle_sort is the family's silent
            // no-op returning the unchanged key (GridSortExternal
            // contract), not a rejection.
            let _ = intro.intervene("sort", IntrospectValue::Text("1:ascending".to_owned()));
            assert_eq!(
                intro.invoke("cycle_sort", IntrospectValue::Int(9)),
                Ok(IntrospectValue::Text("1:ascending".to_owned())),
            );
        });
    }

    #[test]
    fn r886_access_node_announces_aria_sort_in_visual_order() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("cycle_sort", IntrospectValue::Int(2));
            }
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), None);
            let header = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#h2"))
                .expect("Count columnheader present (painted-tag parity)");
            assert_eq!(header.sort, Some(SortDirection::Ascending), "aria-sort on the key col");
            // The rows follow the visual permutation [0, 3, 1, 2] so AT
            // linear navigation matches what sighted users see.
            let row_tags: Vec<&str> = nodes
                .iter()
                .filter(|n| n.tag.starts_with("dg_row"))
                .map(|n| n.tag.as_str())
                .collect();
            assert_eq!(row_tags, ["dg_row0", "dg_row3", "dg_row1", "dg_row2"]);
        });
    }

    // ─── R891 — the editable fold of the filter axis ─────────────────

    // Type column (col 1) source values: sprite, mesh, sprite, mesh.
    // `set_filter "1=mesh"` keeps rows 1 (Tree) and 3 (Boss).

    #[test]
    fn r891_set_filter_shrinks_view_and_reports_view_len() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(4)), "unfiltered = NROWS");
            assert_eq!(intro.query("filter"), Some(IntrospectValue::Text("none".to_owned())));
            // set_filter returns the new view_len in one round-trip.
            assert_eq!(
                intro.invoke("set_filter", IntrospectValue::Text("1=mesh".to_owned())),
                Ok(IntrospectValue::Int(2)),
                "two rows carry Type=mesh",
            );
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(2)));
            assert_eq!(intro.query("filter"), Some(IntrospectValue::Text("1=mesh".to_owned())));
            // The view holds only the matching source rows, in source order.
            assert_eq!(intro.query("source_at.0"), Some(IntrospectValue::Int(1)), "Tree");
            assert_eq!(intro.query("source_at.1"), Some(IntrospectValue::Int(3)), "Boss");
            assert_eq!(intro.query("source_at.2"), Some(IntrospectValue::Null), "view shrank");
            // Clearing restores the full grid.
            assert_eq!(
                intro.invoke("set_filter", IntrospectValue::Null),
                Ok(IntrospectValue::Int(4)),
            );
            assert_eq!(intro.query("filter"), Some(IntrospectValue::Text("none".to_owned())));
        });
    }

    #[test]
    fn r891_filter_wire_round_trips_read_write() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // intervene decode = inverse of query encode (the cross-grid vocab).
            assert_eq!(intro.intervene("filter", IntrospectValue::Text("1=sprite".to_owned())), Ok(()));
            assert_eq!(intro.query("filter"), Some(IntrospectValue::Text("1=sprite".to_owned())));
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(2)), "Hero + Coin");
            // Null clears (the header-less filter axis).
            assert_eq!(intro.intervene("filter", IntrospectValue::Null), Ok(()));
            assert_eq!(intro.query("filter"), Some(IntrospectValue::Text("none".to_owned())));
            // view_len is read-only; a non-text/non-null filter is a mismatch.
            assert_eq!(intro.intervene("view_len", IntrospectValue::Int(1)), Err(InterveneError::ReadOnly));
            assert_eq!(
                intro.intervene("filter", IntrospectValue::Int(1)),
                Err(InterveneError::TypeMismatch),
            );
        });
    }

    #[test]
    fn r891_set_filter_clamps_out_of_range_col() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // An out-of-range column clamps to unfiltered (GridSortState mirror).
            assert_eq!(
                intro.invoke("set_filter", IntrospectValue::Text("9=x".to_owned())),
                Ok(IntrospectValue::Int(4)),
            );
            assert_eq!(intro.query("filter"), Some(IntrospectValue::Text("none".to_owned())));
        });
    }

    #[test]
    fn r891_filter_composes_with_sort() {
        // filter Type=mesh keeps Tree (Count 24) + Boss (Count 1); sorting
        // Count ascending orders the survivors [3 (Boss, 1), 1 (Tree, 24)].
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_filter", IntrospectValue::Text("1=mesh".to_owned()));
            let _ = intro.invoke("cycle_sort", IntrospectValue::Int(2)); // Count asc
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(2)), "filter survives sort");
            assert_eq!(intro.query("source_at.0"), Some(IntrospectValue::Int(3)), "Boss (1) first");
            assert_eq!(intro.query("source_at.1"), Some(IntrospectValue::Int(1)), "Tree (24) second");
        });
    }

    #[test]
    fn r891_filter_change_reanchors_filtered_out_cursor() {
        // Cursor on row 0 (Hero, Type=sprite); applying Type=mesh excludes it,
        // so the cursor re-anchors to the visible row at its prior visual slot
        // (Tree, source row 1) — never the silent teleport the sort fold noted.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.invoke("set_filter", IntrospectValue::Text("1=mesh".to_owned()));
            assert_eq!(
                intro.query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "cursor re-anchored from hidden row 0 to visible row 1",
            );
        });
    }

    #[test]
    fn r891_edit_filters_row_out_reanchors_cursor() {
        // The fold's payoff invariant: with Type=mesh active (Tree, Boss),
        // editing Tree's Type to "sprite" drops Tree from the view on the same
        // commit, and the source-keyed cursor re-anchors to the row that takes
        // its screen slot (Boss) — Excel / Qt QSortFilterProxyModel behaviour.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("set_filter", IntrospectValue::Text("1=mesh".to_owned()));
                let _ = intro.intervene("focused_row", IntrospectValue::Int(1)); // Tree
                let _ = intro.intervene("focused_col", IntrospectValue::Int(1)); // Type
                assert_eq!(intro.invoke("begin", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            }
            use_text_edit_state(EDIT_TF_TAG).set_text("sprite".to_owned());
            commit_edit(true);
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("value.1.1"),
                Some(IntrospectValue::Text("sprite".to_owned())),
                "source write landed",
            );
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(1)), "Tree dropped from view");
            assert_eq!(intro.query("source_at.0"), Some(IntrospectValue::Int(3)), "only Boss remains");
            assert_eq!(
                intro.query("focused_row"),
                Some(IntrospectValue::Int(3)),
                "cursor re-anchored from the filtered-out row to Boss",
            );
        });
    }

    #[test]
    fn r891_arrow_nav_skips_filtered_rows() {
        // Type=sprite keeps Hero (0) + Coin (2); ArrowDown/Up walk only the
        // visible pair, clamping at the ends.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("set_filter", IntrospectValue::Text("1=sprite".to_owned()));
                let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            }
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", m));
            assert_eq!(grid_intro(&scene).query("focused_row"), Some(IntrospectValue::Int(2)), "Coin");
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", m));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(2)),
                "clamps at the last visible row",
            );
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowUp", m));
            assert_eq!(grid_intro(&scene).query("focused_row"), Some(IntrospectValue::Int(0)), "back to Hero");
        });
    }
}
