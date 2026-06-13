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
//! ## Column grouping (R892 — the editable fold of the group axis)
//!
//! A settable group-by column (`invoke "set_group" "<col>"` / `Null`) flattens
//! the filtered+sorted rows into group runs through the cross-grid
//! [`group_rows`] free-fn SSOT (the read-only `hello-grouped-grid` shares it):
//! a [`group_header_row`] per group (label = the column's displayed value,
//! detail = member count, click toggles collapse) over its member data rows.
//! The wire mirrors the `group_order` vocabulary read/write-symmetrically
//! (`query "group"` / `group_count` / `visible_len` / `kind_at` / `label_at` /
//! `collapsed.<g>`; `invoke "set_group"` / `toggle_group` / `collapse_all` /
//! `expand_all`; group headers route a click via [`GridSendKey::Group`]).
//!
//! **Edit-while-grouped** is the fold's payoff (the group analog of R886
//! edit-while-sorted / R891 edit-while-filtered): every grid state stays
//! SOURCE-keyed, so a committed edit of the group-key cell moves its row to
//! another group on the next paint, and collapsing the cursor's group (or an
//! edit that hides it) re-anchors the source-keyed cursor through the shared
//! [`reanchor_cursor`] SSOT (now generalised over filter + group + collapse).
//! Collapse is keyed on the group LABEL (the displayed value), not its
//! positional id (R893 audit fix), so a sort / filter / edit that reorders
//! rows OR changes the distinct-value set keeps a group's collapse tied to its
//! identity — an edit that empties a group can never re-target a different
//! group's collapse; changing the group column clears it. The
//! [`GroupOrderState`](pinion_core::widgets::group_order::GroupOrderState)
//! coordinator is deliberately NOT reused (the R778 family ruling — the shared
//! parts are the `group_rows` free fn + wire vocab, not the coordinator, which
//! assumes a `VirtualSelectExternal` selection + a String dataset).
//!
//! ## Column validation (R894 — the DCC-inspector clamp axis)
//!
//! Each numeric column carries an optional [`ColRange`]; a committed or
//! programmatic value outside it is **clamped** to the nearest bound through
//! the one [`clamp_for_col`] gate both `commit_edit` (keyboard) and
//! `intervene "value"` (AI) run — so neither path can store an out-of-range
//! value (the bounded-spinbox / property-inspector contract). The constraint
//! is AI-readable (`query "col_range.<col>"` → `"0..1000"` / `"none"`), and the
//! clamp surfaces as the read-back value (the [[setter-wire-returns-read-outcome]]
//! discipline: an `intervene` of `999` reads back as the clamped bound).
//!
//! ## Known gaps (honest carry, shared with R836)
//!
//! - Native checkbox / textbox cell roles (per-cell a11y role) — additive.
//! - Multi-facet / substring column filter — one fixed-string column facet
//!   here (the cross-grid `GridFilter` shape); multi-facet is a later
//!   additive axis, exactly as the read-only `GridSortState` filter defers it.
//! - Frozen panes on the *editable* grid are a no-op at this size (the columns
//!   fit the window, so there is no horizontal scroll to pin against) —
//!   deferred until a wide editable grid needs it.

use std::cell::Cell;
use std::rc::Rc;

use std::collections::BTreeSet;

use pinion_a11y::{
    grid_table_nodes, grouped_grid_access_nodes, AccessNode, GridCell, GridColumn, GridRow,
    GroupedGridSelection, GroupedGridSpec, SortDirection, WidgetA11y,
};
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::composite_tag::GridSendKey;
use pinion_core::external::{
    int_of, Backend, BackendFallback, BackendSupport, CaptureNormalize, External,
    ExternalIntrospect, IntrospectSchema, IntrospectValue, InterveneError, InvokeError,
    RepaintOwner, ThreadOwnership,
};
use pinion_core::input::DragCalibration;
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
use pinion_core::widgets::group_order::{group_rows, GroupRow};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::table::{cycle_col_sort, grid_order_by};
use pinion_core::widgets::virtual_list::{compute_visible_range, content_height, VisibleWindow};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::checkbox::{view_checkbox_box, CheckboxStyle};
use pinion_widget_paint::group_header::group_header_row;
use pinion_widget_paint::table::{view_virtual_grid_body, GridScroll};
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
/// R896 — the horizontal `ScrollState` cache key shared by the header + rows:
/// `scene/set_scroll_offset` on this tag slides the columns sideways once
/// their total width outgrows the grid viewport (the R784 single-axis scroll,
/// the read-only grids' [`h_scrolled_column`] wrap reused on the editable grid).
const H_SCROLL_KEY: &str = "data_grid_hscroll";
/// R896.1 / R897 — the vertical body `ScrollState` cache key. The rows window
/// against this state's measured viewport and scroll under the pinned header;
/// R897 made the body **virtualized** (only the visible window is built, via
/// [`view_virtual_grid_body`]), so a 10k-row asset table renders a constant
/// handful of rows. It is the inner axis of the R784 nested single-axis pair.
const V_SCROLL_KEY: &str = "data_grid_vscroll";

/// R897 — rows built beyond the strict visible window on each side, so a
/// partial scroll never flashes an unbuilt gap (the read-only grids' default).
const OVERSCAN: usize = 4;
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

/// Per-column paint width (logical px). Text columns are wider. R896 — the
/// columns are deliberately wider than the grid viewport ([`GRID_VIEWPORT_W`])
/// so their `570 px` total outgrows the visible band and the R784 horizontal
/// scroll ([`h_scrolled_column`]) engages — the "wide asset table" a DCC
/// browser scrolls sideways.
const COL_W: [u32; NCOLS] = [160, 110, 100, 100, 100];

/// R896 — the grid's visible width. Narrower than the `570 px` column total
/// ([`COL_W`]) so the columns scroll sideways under the pinned header; wide
/// enough to show the leading Asset / Type / Count columns at rest (offset 0).
const GRID_VIEWPORT_W: u32 = 370;
/// R896 — the grid's visible height (header + the rows band). Tall enough for
/// the seeded rows + a grouped split; rows beyond it would scroll vertically
/// once the body is virtualized (a later round).
const GRID_VIEWPORT_H: u32 = 268;

/// R914 — float cell-scrub sensitivity: value units per pixel of horizontal
/// drag (100 px ⇒ +1.0), the Blender / Unreal "drag the number field" gesture.
/// Per-widget feel (the scrub *mechanism* is the shared [`DragCalibration`]; the
/// sensitivity is the caller's tuning, like the property grid's own constant).
const SCRUB_FLOAT_PER_PX: f64 = 0.01;
/// R914 — int cell-scrub sensitivity: pixels of horizontal drag per integer
/// step (8 px ⇒ +1), so an int scrubs in whole units without runaway.
const SCRUB_INT_PX_PER_STEP: f64 = 8.0;

// ─── per-column validation (R894 — the DCC-inspector clamp axis) ──

/// R894 — a numeric column's valid range; a committed / programmatic value
/// outside it is clamped to the nearest bound (the bounded-spinbox / property-
/// inspector contract — you cannot enter `999` into a `0..100` field). Typed
/// per kind so the clamp needs no cross-kind cast.
#[derive(Copy, Clone, Debug, PartialEq)]
enum ColRange {
    Int(i64, i64),
}

impl ColRange {
    /// The AI-readable wire form (`"0..1000"`); the kind is read separately via
    /// `col_kind`.
    fn wire(self) -> String {
        match self {
            ColRange::Int(lo, hi) => format!("{lo}..{hi}"),
        }
    }
}

/// Per-column clamp range, `None` for an unbounded column. Count (Int) is
/// `0..1000`; all other columns (Asset / Type / Scale / Active) are unbounded.
const COL_RANGE: [Option<ColRange>; NCOLS] =
    [None, None, Some(ColRange::Int(0, 1000)), None, None];

/// R894 — clamp a typed value to column `col`'s [`ColRange`] (identity for an
/// unbounded column, a non-numeric value, or a kind / range mismatch). The one
/// validation gate both the keyboard `commit_edit` and the programmatic
/// `intervene "value"` write run, so an AI set and a typed commit clamp alike.
fn clamp_for_col(value: CellValue, col: usize) -> CellValue {
    match (&value, COL_RANGE.get(col).and_then(|r| r.as_ref())) {
        (CellValue::Int(i), Some(ColRange::Int(lo, hi))) => CellValue::Int((*i).clamp(*lo, *hi)),
        _ => value,
    }
}

/// `(row, col)` → flat model index.
fn idx(row: usize, col: usize) -> usize {
    row * NCOLS + col
}

// ─── paint / a11y tag SSOT (byte-match guard, R886.1) ─────────────
// A node's a11y tag MUST equal its painted tag for `rect_for_tag` bounds +
// pointer routing; these helpers are the one place the grammar lives, so the
// paint producer and the a11y builder cannot drift.

/// Cell click target — `data_grid#<row>_<col>` (the `GridSendKey::Cell` wire).
fn cell_tag(row: usize, col: usize) -> String {
    format!("{GRID_TAG}#{}", GridSendKey::Cell { row, col }.encode())
}

/// Column-header click target — `data_grid#h<col>` (`GridSendKey::Header`).
fn col_header_tag(col: usize) -> String {
    format!("{GRID_TAG}#{}", GridSendKey::Header { col }.encode())
}

/// Group-header click target — `data_grid#g<group>` (`GridSendKey::Group`).
fn group_header_tag(group: usize) -> String {
    format!("{GRID_TAG}#{}", GridSendKey::Group { group }.encode())
}

/// Data-row container tag — `dg_row<source>` (the AT row-bounds anchor).
fn data_row_tag(source: usize) -> String {
    format!("dg_row{source}")
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

/// R892 — the active group-by column (`None` = ungrouped, flat). A SOURCE-keyed
/// axis like [`use_sort`] / [`use_filter`]: changing it repaints the rows into
/// group runs on the next paint. The cross-grid group-by facet (the read-only
/// `hello-grouped-grid` groups by a fixed column; here it is settable).
#[must_use]
fn use_group_col() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_group_col requires an active Owner scope");
    owner.cache("data_grid.group_col", || Signal::new(None))
}

/// R892 / R893 — the collapsed group LABELS (the displayed group-key values; a
/// `BTreeSet<String>` for a deterministic, cheap membership test). Keyed on the
/// VALUE, not the positional [`group_table`] id (R893 audit fix): a sort /
/// filter / edit that reorders rows OR changes the distinct-value set keeps a
/// group's collapse tied to its identity — an edit that empties a group leaves
/// a harmless stale label, never re-targeting a different group (the read-only
/// `GroupOrderState` keys collapse on positions, sound only for its STATIC
/// dataset; an editable grid's value set shifts). A group-by-column CHANGE
/// clears it (the labels are a different column's values).
#[must_use]
fn use_collapsed() -> Rc<Signal<BTreeSet<String>>> {
    let owner = Owner::current().expect("use_collapsed requires an active Owner scope");
    owner.cache("data_grid.collapsed", || Signal::new(BTreeSet::new()))
}

// ─── column grouping (R892 — the editable fold of the group axis) ──

/// R892 — the group-by label table: the distinct display values of column
/// `col` in SOURCE-order first appearance, so a group's id is STABLE across
/// sort / filter / edits (the collapse set keys on it). `labels[id]` is the
/// header's display name; [`group_of`] maps a source row to its id.
fn group_table(model: &[CellValue], col: usize) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for row in 0..NROWS {
        if let Some(key) = model.get(idx(row, col)).map(CellValue::display) {
            if !labels.contains(&key) {
                labels.push(key);
            }
        }
    }
    labels
}

/// R892 — the STABLE group id of source `row` under group column `col`: its
/// position in the [`group_table`]. Same display value ⇒ same group (Excel
/// groups by the shown value; for a homogeneous typed column display equality
/// is value equality, so no `sort_cmp`-style typed key is needed).
fn group_of(model: &[CellValue], row: usize, col: usize, table: &[String]) -> usize {
    let key = model.get(idx(row, col)).map(CellValue::display).unwrap_or_default();
    table.iter().position(|l| *l == key).unwrap_or(0)
}

/// R892 — the visible row sequence: the filtered+sorted [`current_order`]
/// flattened into [`GroupRow`]s when a group column is active (group headers +
/// their members; a collapsed group omits its members), or one
/// [`GroupRow::Data`] per source row when ungrouped (no headers — identical to
/// the pre-R892 flat order). The unified SSOT the paint loop, the a11y tree,
/// the `source_at` / `kind_at` wire, and the nav / re-anchor all read.
fn visible_rows(
    model: &[CellValue],
    sort: Option<(usize, bool)>,
    filter: Option<&GridFilter>,
    group_col: Option<usize>,
    collapsed: &BTreeSet<String>,
) -> Vec<GroupRow> {
    let order = current_order(model, sort, filter);
    match group_col {
        None => order.into_iter().map(|source| GroupRow::Data { source }).collect(),
        Some(col) => {
            let table = group_table(model, col);
            group_rows(
                &order,
                |row| group_of(model, row, col, &table),
                // R893 — collapse is keyed on the group LABEL, so map the id
                // group_rows hands us back to its label before the lookup.
                |group| table.get(group).is_some_and(|label| collapsed.contains(label)),
            )
        }
    }
}

/// R892 — the visible DATA rows (source indices) in visual order, a collapsed
/// group's members excluded — what vertical navigation walks and the cursor
/// re-anchor clamps into. Ungrouped, this equals [`current_order`].
fn visible_data_order(
    model: &[CellValue],
    sort: Option<(usize, bool)>,
    filter: Option<&GridFilter>,
    group_col: Option<usize>,
    collapsed: &BTreeSet<String>,
) -> Vec<usize> {
    visible_rows(model, sort, filter, group_col, collapsed)
        .iter()
        .filter_map(GroupRow::source)
        .collect()
}

// ─── cursor re-anchor (R891 filter / R892 collapse — one SSOT) ─────

/// R891/R892 — the cursor's position among the `visible` DATA rows, captured
/// BEFORE a filter / group / collapse / edit mutation so [`reanchor_cursor`]
/// can land the cursor on the row that takes its screen slot. `0` when the
/// cursor is already off-view (callers re-anchor explicitly regardless).
fn cursor_visual_pos(visible: &[usize], cursor: usize) -> usize {
    visible.iter().position(|&s| s == cursor).unwrap_or(0)
}

/// R891/R892 — re-anchor the SOURCE-keyed cursor into the visible DATA rows
/// after a filter / group-by / collapse change or an edit hid its row (the
/// R886.1 note made good: an EXPLICIT re-anchor, never the silent
/// `position().unwrap_or(0)` teleport navigation once relied on). A no-op when
/// the cursor's row is still visible; else the cursor lands on the visible row
/// now at its prior slot `prior_vis` (clamped — Excel / Qt keep the selection
/// at its screen position); a no-op when no data row is visible (every group
/// collapsed / filter excludes all — the grid shows no active cell until one
/// reappears). The single SSOT the coordinator writes and `commit_edit` share.
fn reanchor_cursor(visible: &[usize], cursor: &Signal<usize>, prior_vis: usize) {
    if visible.contains(&cursor.get()) {
        return;
    }
    if let Some(&row) = visible.get(prior_vis.min(visible.len().saturating_sub(1))) {
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
    /// R892 — the shared group-by column (`use_group_col`).
    group_col: Rc<Signal<Option<usize>>>,
    /// R892 — the shared collapsed-group set (`use_collapsed`).
    collapsed: Rc<Signal<BTreeSet<String>>>,
    /// R914 — the numeric cell armed by a `PointerDown` over an Int / Float
    /// column, before the first capture `pointer_move` calibrates the scrub.
    /// `None` for a press on a non-numeric column (which never scrubs — text
    /// cells edit, bool cells toggle).
    scrub_armed: Cell<Option<(usize, usize)>>,
    /// R914 — the live cell-scrub calibration ([`DragCalibration`]); active
    /// between the first `pointer_move` and the release. Its activity at
    /// `PointerUp` distinguishes a scrub (commit live, suppress the click) from
    /// a click.
    scrub_cal: DragCalibration<ScrubCell>,
}

/// R914 — the per-drag payload the cell scrub's [`DragCalibration`] snapshots on
/// the first capture `pointer_move`: the dragged cell, its column's
/// [`CellKind`], and its value at press. The cursor's anchor fraction lives in
/// the [`DragCalibration`]; each later move applies `base + travel_px ·
/// sensitivity`. `Copy` so it rides in the calibration's `Cell`.
#[derive(Clone, Copy)]
struct ScrubCell {
    row: usize,
    col: usize,
    kind: CellKind,
    base: f64,
}

impl DataGridExternal {
    #[allow(clippy::too_many_arguments)] // one Rc per shared reactive axis
    fn new(
        model: Rc<Signal<Vec<CellValue>>>,
        focused_row: Rc<Signal<usize>>,
        focused_col: Rc<Signal<usize>>,
        editing_cell: Rc<Signal<Option<(usize, usize)>>>,
        editor: Rc<TextEditState>,
        sort: Rc<Signal<Option<(usize, bool)>>>,
        filter: Rc<Signal<Option<GridFilter>>>,
        group_col: Rc<Signal<Option<usize>>>,
        collapsed: Rc<Signal<BTreeSet<String>>>,
    ) -> Self {
        Self {
            model,
            focused_row,
            focused_col,
            editing_cell,
            editor,
            sort,
            filter,
            group_col,
            collapsed,
            scrub_armed: Cell::new(None),
            scrub_cal: DragCalibration::new(),
        }
    }

    /// R891 — rows passing the active filter (`NROWS` when unfiltered), the
    /// derived data-row count the AI-first `set_filter` reports in one
    /// round-trip. Independent of grouping / collapse (the logical filtered
    /// count, not the rendered row count — that is [`visible_len`](Self::visible_len)).
    fn view_len(&self) -> usize {
        current_order(&self.model.get(), self.sort.get(), self.filter.get().as_ref()).len()
    }

    /// R892 — the visible row sequence (group headers + uncollapsed data rows
    /// when grouped; one data row per source when ungrouped) — the SSOT the
    /// `source_at` / `kind_at` wire and the a11y tree index.
    fn rows(&self) -> Vec<GroupRow> {
        visible_rows(
            &self.model.get(),
            self.sort.get(),
            self.filter.get().as_ref(),
            self.group_col.get(),
            &self.collapsed.get(),
        )
    }

    /// R892 — the rendered row count (headers + uncollapsed data rows);
    /// `view_len` when ungrouped. What `source_at.<pos>` / `kind_at.<pos>` index.
    fn visible_len(&self) -> usize {
        self.rows().len()
    }

    /// R892 — distinct group count under the active group column, `0` ungrouped.
    fn group_count(&self) -> usize {
        match self.group_col.get() {
            Some(col) => group_table(&self.model.get(), col).len(),
            None => 0,
        }
    }

    /// R892 — the visible DATA rows (source order, collapsed members excluded)
    /// — the cursor re-anchor / nav window.
    fn cur_visible(&self) -> Vec<usize> {
        visible_data_order(
            &self.model.get(),
            self.sort.get(),
            self.filter.get().as_ref(),
            self.group_col.get(),
            &self.collapsed.get(),
        )
    }

    /// R892 — capture the cursor's visual slot BEFORE a mutation (re-anchor input).
    fn cursor_prior_vis(&self) -> usize {
        cursor_visual_pos(&self.cur_visible(), self.focused_row.get())
    }

    /// R892 — re-anchor the cursor into the post-mutation visible rows.
    fn reanchor(&self, prior_vis: usize) {
        reanchor_cursor(&self.cur_visible(), &self.focused_row, prior_vis);
    }

    /// R891 — apply a column filter (`None` clears) and re-anchor the cursor
    /// into the resulting view. An out-of-range column clamps to unfiltered
    /// (mirrors [`GridSortState::set_filter`]). Returns the resulting
    /// [`view_len`](Self::view_len). The one mutation path the wire's
    /// `intervene "filter"` and `invoke "set_filter"` share.
    fn set_filter(&self, filter: Option<GridFilter>) -> usize {
        let filter = filter.filter(|f| f.col < NCOLS);
        let prior_vis = self.cursor_prior_vis();
        self.filter.set(filter);
        self.reanchor(prior_vis);
        self.view_len()
    }

    /// R892 — set the group-by column (`None` ungroups). An out-of-range column
    /// clamps to ungrouped. Re-grouping (a CHANGE of column) clears the collapse
    /// set (its labels are a different column's values). Re-anchors the cursor and
    /// returns the resulting [`group_count`](Self::group_count). The mutation
    /// path the wire's `intervene "group"` and `invoke "set_group"` share.
    fn set_group(&self, col: Option<usize>) -> usize {
        let col = col.filter(|&c| c < NCOLS);
        let prior_vis = self.cursor_prior_vis();
        if col != self.group_col.get() {
            self.collapsed.set(BTreeSet::new());
        }
        self.group_col.set(col);
        self.reanchor(prior_vis);
        self.group_count()
    }

    /// R893 — the LABEL of group id `group` under the active group column, or
    /// `None` when ungrouped or out of range. The id→label map the label-keyed
    /// collapse set goes through; an out-of-range id resolves to `None`, so the
    /// collapse wire naturally rejects a phantom group (no hand-rolled guard).
    fn group_label_at(&self, group: usize) -> Option<String> {
        self.group_col.get().and_then(|col| group_table(&self.model.get(), col).get(group).cloned())
    }

    /// R892 — toggle group `group`'s collapse (a clicked group header / the
    /// `toggle_group` wire). Collapsing the cursor's group hides its rows, so
    /// the cursor re-anchors. Returns the resulting collapsed flag; an
    /// out-of-range group is a no-op returning `false` (R893 — the label map
    /// guards it).
    fn toggle_group(&self, group: usize) -> bool {
        let Some(label) = self.group_label_at(group) else {
            return false;
        };
        let prior_vis = self.cursor_prior_vis();
        let mut next = self.collapsed.get();
        let now_collapsed = if next.remove(&label) {
            false
        } else {
            next.insert(label);
            true
        };
        self.collapsed.set(next);
        self.reanchor(prior_vis);
        now_collapsed
    }

    /// R892 — collapse every group / expand every group (the `collapse_all` /
    /// `expand_all` wire). Re-anchors the cursor (collapse-all hides every data
    /// row — the cursor stays put, no visible row to land on, until re-expanded).
    fn set_all_collapsed(&self, collapse: bool) {
        let prior_vis = self.cursor_prior_vis();
        let set: BTreeSet<String> = if collapse {
            self.group_col
                .get()
                .map(|col| group_table(&self.model.get(), col).into_iter().collect())
                .unwrap_or_default()
        } else {
            BTreeSet::new()
        };
        self.collapsed.set(set);
        self.reanchor(prior_vis);
    }

    /// Toggle the bool at `(row, col)`; no-op (returns `false`) unless the
    /// column is a bool. The checkbox affordance behind `Space` + click. R893 —
    /// this is a committed edit, so it re-anchors the cursor exactly like
    /// `commit_edit` / `intervene "value"`: toggling the cell out of an active
    /// filter (or into a collapsed group) on the group/filter column moves its
    /// row, and the source-keyed cursor follows into the visible set.
    fn toggle(&self, row: usize, col: usize) -> bool {
        if col >= NCOLS || COL_KINDS[col] != CellKind::Bool {
            return false;
        }
        let prior_vis = self.cursor_prior_vis();
        let mut toggled = false;
        self.model.set_with(|prev| {
            let mut next = prev.clone();
            if let Some(CellValue::Bool(b)) = next.get_mut(idx(row, col)) {
                *b = !*b;
                toggled = true;
            }
            next
        });
        self.reanchor(prior_vis);
        toggled
    }

    /// R894 / R914 — write a typed value into cell `(row, col)`, clamped to the
    /// column's [`ColRange`] and re-anchoring the cursor. The one funnel the AI
    /// `value.<row>.<col>` intervene write and the live numeric scrub both
    /// commit through, so a drag cannot exceed a bound a programmatic set
    /// cannot (the R894 keyboard / RPC symmetry, now extended to the scrub). A
    /// no-op for an out-of-range cell.
    fn set_cell(&self, row: usize, col: usize, value: CellValue) {
        if row >= NROWS || col >= NCOLS {
            return;
        }
        let new_value = clamp_for_col(value, col);
        // R891/R892 — a write that flips the cursor's row out of an active
        // filter (or into a collapsed group) re-anchors the cursor (a no-op
        // when the write leaves the cursor's row visible).
        let prior_vis = self.cursor_prior_vis();
        self.model.set_with(move |prev| {
            let mut next = prev.clone();
            next[idx(row, col)] = new_value.clone();
            next
        });
        self.reanchor(prior_vis);
    }

    /// R914 — arm a numeric cell scrub: a `PointerDown` over an Int / Float
    /// column records the cell so the first capture `pointer_move` calibrates.
    /// A press on a non-numeric (or out-of-range) cell leaves the arm clear (it
    /// never scrubs — text cells edit, bool cells toggle).
    fn arm_scrub(&self, row: usize, col: usize) {
        let numeric = col < NCOLS && matches!(COL_KINDS[col], CellKind::Int | CellKind::Float);
        self.scrub_armed.set((numeric && row < NROWS).then_some((row, col)));
    }

    /// R914 — drive the live cell scrub from the captured cursor's horizontal
    /// fraction `x_rel` across the grid (`GRID_TAG`, a stable `GRID_VIEWPORT_W`
    /// basis) through the [`DragCalibration`] substrate. The first move
    /// calibrates: `seed` snapshots the armed cell's kind + base value
    /// (declining if nothing is armed or the cell is no longer numeric), and
    /// the move mutates nothing. Each later move yields the fraction delta,
    /// which `· GRID_VIEWPORT_W` recovers as pixel travel; the scrub writes
    /// `base + travel_px · sensitivity` through the shared clamped
    /// [`set_cell`](Self::set_cell) funnel. An int scrub steps in whole units; a
    /// float scrub is continuous.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "scrub values are small game-object cell magnitudes (count / \
                  scale), nowhere near f64's 2^53 exact-int limit or i64's range; \
                  the f64→i64 step is an intentional round-to-unit"
    )]
    fn scrub_to(&self, x_rel: f64) {
        let Some((cell, delta)) = self.scrub_cal.drive(x_rel, || {
            let (row, col) = self.scrub_armed.get()?;
            let model = self.model.get();
            match model.get(idx(row, col)) {
                Some(CellValue::Int(i)) => {
                    Some(ScrubCell { row, col, kind: CellKind::Int, base: *i as f64 })
                }
                Some(CellValue::Float(f)) => {
                    Some(ScrubCell { row, col, kind: CellKind::Float, base: *f })
                }
                // Nothing armed, or the armed cell is no longer numeric.
                _ => None,
            }
        }) else {
            return;
        };
        let travel_px = delta * f64::from(GRID_VIEWPORT_W);
        let next = match cell.kind {
            CellKind::Int => {
                let steps = (travel_px / SCRUB_INT_PX_PER_STEP).round() as i64;
                CellValue::Int(cell.base as i64 + steps)
            }
            _ => CellValue::Float(cell.base + travel_px * SCRUB_FLOAT_PER_PX),
        };
        self.set_cell(cell.row, cell.col, next);
    }

    /// R914 — tear the scrub down at release. Returns whether a drag was in
    /// flight (a real scrub committed live), so `PointerUp` can suppress the
    /// click action: a scrub must not also focus / toggle the cell as a plain
    /// click would.
    fn end_scrub(&self) -> bool {
        self.scrub_armed.set(None);
        self.scrub_cal.end()
    }

    /// R914 — whether a numeric cell scrub is live (the AI-first `scrubbing`
    /// query slot).
    fn is_scrubbing(&self) -> bool {
        self.scrub_cal.is_active()
    }

    /// R837 / R914 — route a composite cell `send` event to the cell at
    /// `(row, col)`. `PointerDown` arms a numeric scrub (the first capture
    /// `pointer_move` calibrates it); `PointerUp` ends the scrub and, if no
    /// drag ran, focuses the cell (and toggles a bool); `PointerLeave` /
    /// `PointerCancel` tear a strayed-off scrub down; `DoubleClick` edits an
    /// editable cell.
    fn handle_cell_send(
        &self,
        row: usize,
        col: usize,
        event_name: &str,
    ) -> Result<IntrospectValue, InvokeError> {
        if row >= NROWS || col >= NCOLS {
            return Err(InvokeError::Rejected);
        }
        match event_name {
            "PointerDown" => {
                self.arm_scrub(row, col);
                Ok(IntrospectValue::Null)
            }
            "PointerUp" => {
                // R914 — a scrub committed its value live during the drag; its
                // release must NOT also fire the click action (focus / toggle).
                if self.end_scrub() {
                    return Ok(IntrospectValue::Null);
                }
                self.focused_row.set(row);
                self.focused_col.set(col);
                self.toggle(row, col);
                Ok(IntrospectValue::Null)
            }
            // R914 — the capture lock lets the cursor stray off the cell; a
            // release there arrives as PointerLeave / PointerCancel. Tear the
            // scrub down (the value is already committed).
            "PointerLeave" | "PointerCancel" => {
                self.end_scrub();
                Ok(IntrospectValue::Null)
            }
            "DoubleClick" => Ok(IntrospectValue::Bool(self.begin_edit(row, col))),
            _ => Ok(IntrospectValue::Null),
        }
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

    /// R914 — opt into the R51.34 capture lock so a numeric cell scrub survives
    /// the cursor straying off the cell (the property-grid / slider stance). A
    /// press that never moves is still a click — the release dispatches
    /// `PointerUp` with no scrub calibrated, so the existing focus / toggle /
    /// edit path runs unchanged.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R914 — normalize the captured cursor against the grid container
    /// (`GRID_TAG`), a stable `GRID_VIEWPORT_W`-wide rect, so the cursor-fraction
    /// delta recovers true pixel travel for the scrub (the scrubbed cell never
    /// resizes the viewport, so the whole grid is a fine basis — the
    /// column-resize stable-basis rule).
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Tag(GRID_TAG)
    }

    /// R914 — drive the live numeric cell scrub from the captured cursor's
    /// horizontal fraction; `y_rel` is ignored (scrub is the X axis only).
    fn pointer_move(&mut self, x_rel: f32, _y_rel: f32) {
        self.scrub_to(f64::from(x_rel));
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
            ("col_range.<col>", "string"),
            ("value.<row>.<col>", "json"),
            ("sort", "string"),
            ("filter", "string"),
            ("view_len", "int"),
            ("group", "string"),
            ("group_count", "int"),
            ("visible_len", "int"),
            ("source_at.<pos>", "int"),
            ("kind_at.<pos>", "string"),
            ("label_at.<pos>", "string"),
            ("collapsed.<group>", "bool"),
            ("scrubbing", "bool"),
            ("send", "string"),
            ("toggle", "json"),
            ("begin", "json"),
            ("cycle_sort", "json"),
            ("set_filter", "string"),
            ("set_group", "string"),
            ("toggle_group", "int"),
            ("collapse_all", "json"),
            ("expand_all", "json"),
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
            // R892 — the group-by column ("none" / "<col>"), the read side of
            // `set_group` (decode = inverse in `intervene "group"`).
            "group" => Some(IntrospectValue::Text(match self.group_col.get() {
                Some(col) => col.to_string(),
                None => "none".to_owned(),
            })),
            // R892 — distinct group count (0 ungrouped) + rendered row count
            // (headers + uncollapsed data rows; `view_len` ungrouped).
            "group_count" => Some(IntrospectValue::Int(int_of(self.group_count()))),
            "visible_len" => Some(IntrospectValue::Int(int_of(self.visible_len()))),
            // R914 — whether a live numeric cell scrub is in flight (the
            // AI-first read peer of the capture-drag scrub gesture).
            "scrubbing" => Some(IntrospectValue::Bool(self.is_scrubbing())),
            _ => {
                // R886 — `source_at.<pos>`: the source row painted at
                // visual position `pos` under the active sort (identity
                // when unsorted) — the AI-side order introspection.
                if let Some(pos_str) = path.strip_prefix("source_at.") {
                    // R886.1/R892 — the shared `source_at.` projection SSOT over
                    // the visible row sequence: a data row reports its source, a
                    // group header or out-of-range position reports Null
                    // (present-but-empty), never absence — the family contract
                    // every sort / group proxy speaks. Ungrouped, the sequence
                    // is the flat filtered+sorted order (bit-identical to R886).
                    let rows = self.rows();
                    return Some(pinion_core::widgets::order_memo::source_at_value(
                        pos_str,
                        |p| rows.get(p).and_then(GroupRow::source),
                    ));
                }
                // R892 — the visible-row discriminator (`"header"` / `"data"`,
                // Null out of range) — disambiguates a `source_at` Null (header
                // vs out-of-range).
                if let Some(pos_str) = path.strip_prefix("kind_at.") {
                    let pos: usize = pos_str.parse().ok()?;
                    return Some(self.rows().get(pos).map_or(IntrospectValue::Null, |r| {
                        IntrospectValue::Text(r.kind_str().to_owned())
                    }));
                }
                // R892 — the group label of a header position (Null for a data
                // row or out of range): the displayed group key.
                if let Some(pos_str) = path.strip_prefix("label_at.") {
                    let pos: usize = pos_str.parse().ok()?;
                    let rows = self.rows();
                    let label = match (self.group_col.get(), rows.get(pos)) {
                        (Some(col), Some(GroupRow::Header { group, .. })) => {
                            group_table(&self.model.get(), col).get(*group).cloned()
                        }
                        _ => None,
                    };
                    return Some(label.map_or(IntrospectValue::Null, IntrospectValue::Text));
                }
                // R892 / R893 — whether group `<group>` is collapsed (the read
                // side of `toggle_group` / `intervene "collapsed.<group>"`).
                // Resolves the id to its label; an out-of-range group reports
                // Null (present-but-empty), the §5.12 convention the
                // `GroupOrderExternal` SSOT speaks.
                if let Some(g_str) = path.strip_prefix("collapsed.") {
                    let group: usize = g_str.parse().ok()?;
                    return Some(match self.group_label_at(group) {
                        Some(label) => IntrospectValue::Bool(self.collapsed.get().contains(&label)),
                        None => IntrospectValue::Null,
                    });
                }
                if let Some(col_str) = path.strip_prefix("col_name.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_NAMES.get(col).map(|n| IntrospectValue::Text((*n).to_owned()));
                }
                if let Some(col_str) = path.strip_prefix("col_kind.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_KINDS.get(col).map(|k| IntrospectValue::Text(k.name().to_owned()));
                }
                // R894 — the column's clamp range ("<min>..<max>" / "none"); an
                // out-of-range column is `None` (an unknown path), an unbounded
                // one is the text "none" (present-but-unconstrained).
                if let Some(col_str) = path.strip_prefix("col_range.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_RANGE.get(col).map(|range| {
                        IntrospectValue::Text(range.map_or_else(|| "none".to_owned(), ColRange::wire))
                    });
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
            "row_count" | "col_count" | "editing_row" | "editing_col" | "view_len"
            | "group_count" | "visible_len" => Err(InterveneError::ReadOnly),
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
            // R892 — admin / restore write of the group-by column: "<col>" /
            // "none" / Null (decode = inverse of `query "group"`). An
            // out-of-range / unparseable column ungroups (`set_group` clamps).
            "group" => match value {
                IntrospectValue::Text(ref s) => {
                    self.set_group(if s == "none" { None } else { s.parse::<usize>().ok() });
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_group(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => {
                // R895 — read-only metadata / projection paths reject as
                // `ReadOnly` (the honest "exists but you can't write it"), not
                // `UnknownPath` (the "doesn't exist" lie) — error-class honesty
                // for the AI-first introspection contract.
                if ["col_name.", "col_kind.", "col_range.", "source_at.", "kind_at.", "label_at."]
                    .iter()
                    .any(|prefix| path.starts_with(prefix))
                {
                    return Err(InterveneError::ReadOnly);
                }
                // R892 / R893 — set group `<group>`'s collapse directly
                // (idempotent; re-anchors via `toggle_group` when it actually
                // changes). The id resolves to its label; an out-of-range group
                // is an unknown path (mirrors the `query` Null).
                if let Some(g_str) = path.strip_prefix("collapsed.") {
                    let group: usize =
                        g_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                    let IntrospectValue::Bool(want) = value else {
                        return Err(InterveneError::TypeMismatch);
                    };
                    let Some(label) = self.group_label_at(group) else {
                        return Err(InterveneError::UnknownPath);
                    };
                    if self.collapsed.get().contains(&label) != want {
                        self.toggle_group(group);
                    }
                    return Ok(());
                }
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
                // R894 / R914 — coerce the wire value to the column's kind and
                // commit through the shared clamped [`set_cell`] funnel, the
                // same path the live scrub commits through (an AI write cannot
                // exceed the bounds a keyboard edit / a drag cannot, and the
                // cursor re-anchors identically).
                self.set_cell(row, col, COL_KINDS[col].coerce(value)?);
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
                        GridSendKey::Cell { row, col } => self.handle_cell_send(row, col, event_name),
                        // R892 — a clicked group header toggles that group's
                        // collapse (the `GridSendKey::Group` wire, parallel to
                        // the column-header sort cycle).
                        GridSendKey::Group { group } => {
                            if group >= self.group_count() {
                                return Err(InvokeError::Rejected);
                            }
                            if event_name == "PointerUp" {
                                self.toggle_group(group);
                            }
                            Ok(IntrospectValue::Null)
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
            // R892 — the AI-first group-by: "<col>" / "none" / Null sets the
            // group column (an out-of-range column ungroups). Returns the
            // resulting group_count in one round-trip.
            "set_group" => {
                let count = match args {
                    IntrospectValue::Text(ref s) => {
                        self.set_group(if s == "none" { None } else { s.parse::<usize>().ok() })
                    }
                    IntrospectValue::Null => self.set_group(None),
                    _ => return Err(InvokeError::TypeMismatch),
                };
                Ok(IntrospectValue::Int(int_of(count)))
            }
            // R892 — toggle a group's collapse; returns the resulting flag.
            "toggle_group" => match args {
                IntrospectValue::Int(i) => {
                    let group = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.toggle_group(group)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R892 — collapse / expand every group at once.
            "collapse_all" => {
                self.set_all_collapsed(true);
                Ok(IntrospectValue::Null)
            }
            "expand_all" => {
                self.set_all_collapsed(false);
                Ok(IntrospectValue::Null)
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
    let cursor = use_focused_row();
    let text = use_text_edit_state(EDIT_TF_TAG).text();
    // R892 — the cursor's visible-data window (filter + group + collapse), used
    // both to capture the prior slot and to re-anchor after the write.
    let visible = || {
        visible_data_order(
            &model.get(),
            use_sort().get(),
            use_filter().get().as_ref(),
            use_group_col().get(),
            &use_collapsed().get(),
        )
    };
    // R891/R892 — capture the edited row's visual slot BEFORE the write so a
    // commit that filters the row out (or moves it into a collapsed group)
    // re-anchors the cursor to the row that takes its screen position (the
    // cursor IS the edited row — editing never moves the grid cursor).
    let prior_vis = cursor_visual_pos(&visible(), cursor.get());
    if col < NCOLS {
        if let Some(parsed) = COL_KINDS[col].parse(&text) {
            // R894 — clamp the committed value to the column's range (the
            // bounded-spinbox contract; an out-of-range edit lands on the bound).
            let parsed = clamp_for_col(parsed, col);
            model.set_with(move |prev| {
                let mut next = prev.clone();
                next[idx(row, col)] = parsed.clone();
                next
            });
        }
    }
    end_edit_mode(restore_focus);
    // R891/R892 — if the committed value hid the row, re-anchor the now-hidden
    // cursor (no-op when the row stays visible).
    reanchor_cursor(&visible(), &cursor, prior_vis);
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
            // R892 — walk the visible DATA rows (collapsed-group members
            // excluded), so ArrowDown/Up skip group headers and hidden rows.
            let order = visible_data_order(
                &use_data_model().get(),
                use_sort().get(),
                use_filter().get().as_ref(),
                use_group_col().get(),
                &use_collapsed().get(),
            );
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
            .with_tag(cell_tag(row, col))
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
                .with_tag(col_header_tag(col))
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

/// R892 — a group header row (the [`group_header_row`] SSOT, R871): the group
/// label + member count + collapse chevron, tagged `data_grid#g<group>` so a
/// click routes to the coordinator's collapse toggle. Spans the full grid width.
fn view_group_header(
    group: usize,
    label: &str,
    member_count: usize,
    collapsed: bool,
    theme: &Theme,
) -> Scene {
    group_header_row(
        group_header_tag(group),
        label,
        &member_count.to_string(),
        collapsed,
        theme,
        COL_W.iter().sum::<u32>(),
        ROW_H,
    )
}

/// R886.1 — one data row: a flex row of [`view_cell`]s, tagged `dg_row<src>`
/// (the same tag its a11y `row` node uses, so AT bounds attach). The cursor /
/// edit latch are SOURCE-keyed, so this paints by source index regardless of
/// the visual order.
fn view_data_row(
    row: usize,
    model: &[CellValue],
    focus: (usize, usize),
    editing: Option<(usize, usize)>,
    theme: &Theme,
    edit_field: (TextFieldState, u32),
) -> Scene {
    let (focused_row, focused_col) = focus;
    let cells: Vec<Scene> = (0..NCOLS)
        .map(|col| {
            let value = &model[idx(row, col)];
            let focused = row == focused_row && col == focused_col;
            let edit_active = editing == Some((row, col)) && COL_KINDS[col].is_text_editable();
            view_cell(row, col, value, focused, edit_active, theme, edit_field)
        })
        .collect();
    Scene::Container(
        ContainerNode::new(cells).with_tag(data_row_tag(row)).with_layout(
            LayoutStyle::new().flex(FlexDirection::Row).with_align_items(AlignItems::Center),
        ),
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
    // R886 / R891 / R892 — paint rows in the filtered+sorted+grouped visual
    // sequence; every data cell keeps its SOURCE identity (tags, cursor, edit
    // latch), so a committed edit that changes the sort key moves its row — a
    // filter drops it — a group-key edit re-groups it — on this very repaint
    // while the cursor and any in-flight editor follow the source row.
    let sort = use_sort().get();
    let filter = use_filter().get();
    let group_col = use_group_col().get();
    let collapsed = use_collapsed().get();
    let vis_rows = visible_rows(&model, sort, filter.as_ref(), group_col, &collapsed);
    let group_labels = group_col.map(|col| group_table(&model, col));
    // The status "showing" count stays the FILTER readout (data rows passing,
    // independent of collapse) — the R891 semantics, so its demo is unaffected.
    let view_len = current_order(&model, sort, filter.as_ref()).len();

    let title = Scene::Text(TextNode::styled(
        "Asset table",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    // R897 — VIRTUALIZE the body: window over the visible rows and build only
    // those, through the read-only grids' `view_virtual_grid_body` SSOT. The
    // body's measured viewport height drives the window (the R774 AutoSizer
    // feedback), so a 10k-row asset table renders a constant handful of rows —
    // the real Model/View-at-scale fix, replacing the R896.1 eager clip-scroll.
    // Group headers + data rows share a uniform ROW_H pitch, so the windowing is
    // the same `uniform_slots` math; each visible position resolves to a
    // SOURCE-keyed group header or data row, so a sorted/filtered/grouped row
    // still paints by its identity. The header rides outside the inner vertical
    // body (pinned) inside the outer horizontal scroll (R784 nested pair).
    let v_scroll = use_scroll_state(V_SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let total_w: u32 = COL_W.iter().sum();
    let (_, measured_h) = v_scroll.measured_viewport();
    let window =
        compute_visible_range(v_scroll.offset_y(), measured_h, vis_rows.len(), ROW_H, OVERSCAN);
    let total_h = content_height(vis_rows.len(), ROW_H);
    let scrolled = view_virtual_grid_body(
        GridScroll { body: &v_scroll, horizontal: &h_scroll },
        &window,
        total_w,
        total_h,
        ROW_H,
        view_header(&theme, sort),
        |view_pos| match vis_rows[view_pos] {
            // R892 — a group header spanning the grid (label + member count +
            // collapse chevron; a click toggles collapse).
            GroupRow::Header { group, member_count, collapsed: is_collapsed } => {
                let label =
                    group_labels.as_ref().and_then(|t| t.get(group)).map_or("", String::as_str);
                view_group_header(group, label, member_count, is_collapsed, &theme)
            }
            GroupRow::Data { source } => view_data_row(
                source,
                &model,
                (focused_row, focused_col),
                editing,
                &theme,
                (edit_state, edit_caret),
            ),
        },
    );
    let grid = Scene::Container(
        ContainerNode::new(vec![scrolled])
            .with_tag(GRID_TAG)
            .with_aria_label("Asset table")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)).with_border(
                Border::new(theme.resolve(ColorRole::Outline), 1),
            ))
            // The fixed viewport bounds both scroll axes. The default
            // `AlignItems::Stretch` makes `scrolled` claim the full
            // GRID_VIEWPORT_W width, so the horizontal scroll has a viewport
            // narrower than the 570px columns to scroll against (R896.1 —
            // stating the cross-axis contract that was implicit before).
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(GRID_VIEWPORT_W, GRID_VIEWPORT_H)),
            ),
    );

    // R891 / R892 — a scene-as-data readout of the active filter + resulting
    // view size (`filter 1=mesh \u{00B7} showing 2 of 4`), plus the group-by
    // column when grouped — the `hello-grid-filter` status-bar pattern at the
    // editable grid's scale. Tagged for AI-first introspection. The ungrouped
    // text is byte-identical to R891 (the group suffix is appended only when
    // grouped), so the R891 status assertions stand.
    let group_suffix = match group_col {
        Some(col) => format!(" \u{00B7} grouped by {}", COL_NAMES[col]),
        None => String::new(),
    };
    let status = Scene::Text(
        TextNode::styled(
            format!(
                "filter {} \u{00B7} showing {view_len} of {NROWS}{group_suffix}",
                grid_filter_str(filter.as_ref()),
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
        let group_col = use_group_col();
        let collapsed = use_collapsed();
        Box::new(DataGridExternal::new(
            model, focused_row, focused_col, editing, editor, sort, filter, group_col, collapsed,
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

/// R895 — the a11y tag of a visible [`GroupRow`] (the painted-tag SSOT: a
/// header is `group_header_tag`, a data row `data_row_tag`). This is the
/// `row_tag` closure the editable grid hands to [`grouped_grid_access_nodes`]
/// — the single-`External` `data_grid#g<g>` / `dg_row<src>` scheme the
/// composite-tag default cannot express, supplied by the consumer (the
/// substrate owns the topology, the consumer owns the tags).
fn group_row_a11y_tag(vrow: &GroupRow) -> String {
    match *vrow {
        GroupRow::Header { group, .. } => group_header_tag(group),
        GroupRow::Data { source } => data_row_tag(source),
    }
}

impl WidgetA11y for DataGridView {
    /// R837 / R886 / R891 / R892 / R895 — ungrouped, a flat WAI-ARIA `grid`
    /// (the [`grid_table_nodes`] SSOT) over the filtered+sorted rows, the
    /// focused cell as `aria-activedescendant`; grouped, a `treegrid` via the
    /// [`grouped_grid_access_nodes`] SSOT (R895 — the editable grid is the
    /// substrate's cell-focus + bespoke-row-tag consumer, replacing the
    /// R892 hand-roll). The columns (with `aria-sort`) are shared.
    fn access_node(_state: &RootState, _focused: Option<&str>) -> Vec<AccessNode> {
        let model = use_data_model().get();
        let focused_row = use_focused_row().get();
        let focused_col = use_focused_col().get();
        let sort = use_sort().get();
        let filter = use_filter().get();
        let group_col = use_group_col().get();
        let collapsed = use_collapsed().get();
        // R886.1 — the a11y columnheader tag IS the painted clickable header
        // tag, so `rect_for_tag` bounds attach and an AT activation routes to
        // the sort wire. The active column announces `aria-sort` (WAI-ARIA 1.2
        // §6.6.2).
        let columns: Vec<GridColumn> = COL_NAMES
            .iter()
            .enumerate()
            .map(|(col, label)| GridColumn {
                tag: col_header_tag(col),
                label: (*label).to_owned(),
                sort: col_sort_dir(sort, col).map(SortDirection::from_ascending),
            })
            .collect();

        let Some(gcol) = group_col else {
            // R886/R891 — ungrouped flat grid; the rows are the filtered+sorted
            // order, so AT linear navigation matches what sighted users see.
            let order = current_order(&model, sort, filter.as_ref());
            let rows: Vec<GridRow> = order
                .iter()
                .map(|&row| GridRow {
                    tag: data_row_tag(row),
                    selected: false,
                    state: RadioState::Idle,
                    cells: (0..NCOLS)
                        .map(|col| GridCell {
                            tag: cell_tag(row, col),
                            name: format!("{}: {}", COL_NAMES[col], model[idx(row, col)].display()),
                            focused: row == focused_row && col == focused_col,
                        })
                        .collect(),
                })
                .collect();
            return grid_table_nodes(GRID_TAG, "Asset table", false, "dg_header", &columns, &rows);
        };

        // R895 — grouped treegrid via the substrate SSOT. The editable grid is
        // its cell-focus consumer (`focused_cell = Some(col)` puts the
        // activedescendant on the focused gridcell, not the row) and its
        // bespoke-row-tag consumer (`group_row_a11y_tag` supplies the
        // `data_grid#g<g>` / `dg_row<src>` scheme the composite default can't).
        let rows = visible_rows(&model, sort, filter.as_ref(), Some(gcol), &collapsed);
        let labels = group_table(&model, gcol);
        let focused_view_pos = rows.iter().position(|r| r.source() == Some(focused_row));
        let spec = GroupedGridSpec {
            grid_tag: GRID_TAG,
            name: Some("Asset table"),
            header_row_tag: "dg_header",
            columns: &columns,
            selection: GroupedGridSelection::Display,
            focused_view_pos,
            focused_cell: Some(focused_col),
        };
        grouped_grid_access_nodes(
            &spec,
            &rows,
            VisibleWindow { first: 0, count: rows.len() },
            |g| labels.get(g).cloned().unwrap_or_default(),
            cell_tag,
            |source, col| {
                format!("{}: {}", COL_NAMES[col], model.get(idx(source, col)).map(CellValue::display).unwrap_or_default())
            },
            group_row_a11y_tag,
        )
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
            // R897 — the body virtualizes against the measured viewport, so a
            // unit test (no shell layout pass) must seed a viewport height or
            // the window is empty and no rows build (the read-only grid tests'
            // convention). GRID_VIEWPORT_H windows all four seeded rows.
            use_scroll_state(V_SCROLL_KEY).set_measured_viewport(GRID_VIEWPORT_W, GRID_VIEWPORT_H);
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

    // ─── R892 — the editable fold of the GROUP axis ──────────────────

    // Type column (col 1) source values: sprite, mesh, sprite, mesh.
    // Grouping by Type: group 0 = sprite (rows 0, 2), group 1 = mesh (1, 3).
    // Unsorted visible sequence = [H0, D0, D2, H1, D1, D3].

    #[test]
    fn r892_group_by_flattens_and_indexes_the_sequence() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(intro.query("group"), Some(IntrospectValue::Text("none".to_owned())));
            assert_eq!(intro.query("group_count"), Some(IntrospectValue::Int(0)), "ungrouped");
            // set_group returns the new group count in one round-trip.
            assert_eq!(
                intro.invoke("set_group", IntrospectValue::Text("1".to_owned())),
                Ok(IntrospectValue::Int(2)),
                "Type has two distinct values",
            );
            assert_eq!(intro.query("group"), Some(IntrospectValue::Text("1".to_owned())));
            assert_eq!(intro.query("visible_len"), Some(IntrospectValue::Int(6)), "2 headers + 4 data");
            // kind_at disambiguates header vs data positions.
            assert_eq!(intro.query("kind_at.0"), Some(IntrospectValue::Text("header".to_owned())));
            assert_eq!(intro.query("kind_at.1"), Some(IntrospectValue::Text("data".to_owned())));
            // source_at: headers report Null, data rows their source.
            assert_eq!(intro.query("source_at.0"), Some(IntrospectValue::Null), "header");
            assert_eq!(intro.query("source_at.1"), Some(IntrospectValue::Int(0)), "sprite: Hero");
            assert_eq!(intro.query("source_at.2"), Some(IntrospectValue::Int(2)), "sprite: Coin");
            assert_eq!(intro.query("source_at.4"), Some(IntrospectValue::Int(1)), "mesh: Tree");
            // label_at gives a header's group label.
            assert_eq!(intro.query("label_at.0"), Some(IntrospectValue::Text("sprite".to_owned())));
            assert_eq!(intro.query("label_at.3"), Some(IntrospectValue::Text("mesh".to_owned())));
            assert_eq!(intro.query("label_at.1"), Some(IntrospectValue::Null), "data row has no label");
        });
    }

    #[test]
    fn r892_collapse_hides_members_and_reanchors_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0)); // Hero, sprite group
            // Collapse the sprite group (group 0); its members (0, 2) vanish.
            assert_eq!(intro.invoke("toggle_group", IntrospectValue::Int(0)), Ok(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("collapsed.0"), Some(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("visible_len"), Some(IntrospectValue::Int(4)), "2 headers + 2 mesh");
            assert_eq!(intro.query("source_at.2"), Some(IntrospectValue::Int(1)), "first mesh row");
            // The cursor was on the now-hidden Hero (row 0) → re-anchors into
            // the visible set (the first mesh row, source 1).
            assert_eq!(
                intro.query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "cursor re-anchored out of the collapsed group",
            );
        });
    }

    #[test]
    fn r892_group_wire_round_trips_read_write() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // intervene decode = inverse of query encode.
            assert_eq!(intro.intervene("group", IntrospectValue::Text("1".to_owned())), Ok(()));
            assert_eq!(intro.query("group"), Some(IntrospectValue::Text("1".to_owned())));
            // collapse_all / expand_all bound the rendered rows.
            let _ = intro.invoke("collapse_all", IntrospectValue::Null);
            assert_eq!(intro.query("visible_len"), Some(IntrospectValue::Int(2)), "two headers only");
            let _ = intro.invoke("expand_all", IntrospectValue::Null);
            assert_eq!(intro.query("visible_len"), Some(IntrospectValue::Int(6)), "all members back");
            // collapsed.<g> is a writable bool axis.
            assert_eq!(intro.intervene("collapsed.1", IntrospectValue::Bool(true)), Ok(()));
            assert_eq!(intro.query("collapsed.1"), Some(IntrospectValue::Bool(true)));
            // Null clears the group (decode), reported group_count drops to 0.
            assert_eq!(intro.intervene("group", IntrospectValue::Null), Ok(()));
            assert_eq!(intro.query("group"), Some(IntrospectValue::Text("none".to_owned())));
            assert_eq!(intro.query("group_count"), Some(IntrospectValue::Int(0)));
        });
    }

    #[test]
    fn r892_edit_group_key_regroups_live() {
        // The fold's payoff: editing a row's group-key cell moves it to another
        // group on the same commit (the live derivation), the group analog of
        // edit-while-sorted / edit-while-filtered.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            // Before: sprite group [0, 2] leads, mesh [1, 3] follows.
            assert_eq!(intro.query("source_at.1"), Some(IntrospectValue::Int(0)));
            assert_eq!(intro.query("source_at.2"), Some(IntrospectValue::Int(2)));
            // Edit Hero's Type sprite -> mesh: it joins the mesh group live.
            assert_eq!(intro.intervene("value.0.1", IntrospectValue::Text("mesh".to_owned())), Ok(()));
            assert_eq!(intro.query("group_count"), Some(IntrospectValue::Int(2)), "still two values");
            // Now mesh [0, 1, 3] leads (first appearance), sprite [2] follows:
            // visible = [H(mesh), D0, D1, D3, H(sprite), D2].
            assert_eq!(intro.query("label_at.0"), Some(IntrospectValue::Text("mesh".to_owned())));
            assert_eq!(intro.query("source_at.1"), Some(IntrospectValue::Int(0)), "Hero now in mesh");
            assert_eq!(intro.query("source_at.3"), Some(IntrospectValue::Int(3)), "mesh has 3 members");
            assert_eq!(intro.query("label_at.4"), Some(IntrospectValue::Text("sprite".to_owned())));
            assert_eq!(intro.query("source_at.5"), Some(IntrospectValue::Int(2)), "sprite: only Coin");
        });
    }

    #[test]
    fn r892_arrow_nav_skips_headers_and_walks_data() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
                let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            }
            // Visible data order = [0, 2, 1, 3]; ArrowDown walks it, skipping
            // the group-header rows between source 2 and source 1.
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", m));
            assert_eq!(grid_intro(&scene).query("focused_row"), Some(IntrospectValue::Int(2)), "sprite: Coin");
            assert!(DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", m));
            assert_eq!(grid_intro(&scene).query("focused_row"), Some(IntrospectValue::Int(1)), "into mesh: Tree");
        });
    }

    #[test]
    fn r892_header_click_toggles_collapse() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            // A click on the group-0 header (the GridSendKey::Group wire) toggles.
            let _ = intro.invoke("send", IntrospectValue::Text("g0:PointerUp".to_owned()));
            assert_eq!(intro.query("collapsed.0"), Some(IntrospectValue::Bool(true)));
            let _ = intro.invoke("send", IntrospectValue::Text("g0:PointerUp".to_owned()));
            assert_eq!(intro.query("collapsed.0"), Some(IntrospectValue::Bool(false)));
        });
    }

    #[test]
    fn r892_grouped_a11y_is_treegrid_with_cell_focus() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            }
            use_focused_row().set(2); // Coin (sprite group)
            use_focused_col().set(2); // Count
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            assert_eq!(nodes[0].role, pinion_a11y::AriaRole::TreeGrid, "grouped grid is a treegrid");
            let header = nodes
                .iter()
                .find(|n| n.tag == group_header_tag(0))
                .expect("sprite group header present (painted-tag parity)");
            assert_eq!(header.role, pinion_a11y::AriaRole::Row);
            assert_eq!(header.level, Some(1), "group header is aria-level 1");
            assert_eq!(header.expanded, Some(true));
            // Cell-focus: the focused gridcell is the activedescendant, not the row.
            let cell = nodes.iter().find(|n| n.tag == cell_tag(2, 2)).expect("focused cell");
            assert!(cell.state.focused, "the focused cell carries activedescendant");
            let row = nodes.iter().find(|n| n.tag == data_row_tag(2)).expect("data row 2");
            assert!(!row.state.focused, "the data row does not (cell focus, not row focus)");
        });
    }

    #[test]
    fn r892_ungrouped_a11y_stays_a_flat_grid() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            assert_eq!(nodes[0].role, pinion_a11y::AriaRole::Grid, "ungrouped stays a flat grid");
        });
    }

    // ─── R893 — session-review audit remediation ─────────────────────

    #[test]
    fn r893_bool_toggle_under_filter_reanchors_cursor() {
        // A bool toggle is a committed edit: toggling the cursor's Active cell
        // out of an `Active=true` filter must drop the row AND re-anchor the
        // source-keyed cursor (R893 — the toggle path was the one model
        // mutation missing the re-anchor R891 wired into commit_edit/intervene).
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Active (col 4) = [true, true, false, true]; filter keeps 0, 1, 3.
            let _ = intro.invoke("set_filter", IntrospectValue::Text("4=true".to_owned()));
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(4));
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(3)));
            // Toggle Hero's Active true -> false: it leaves the filter.
            let _ = intro.invoke("toggle", IntrospectValue::Null);
            assert_eq!(intro.query("value.0.4"), Some(IntrospectValue::Bool(false)), "toggled");
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(2)), "Hero dropped");
            assert_eq!(
                intro.query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "cursor re-anchored out of the now-hidden row",
            );
        });
    }

    #[test]
    fn r893_collapse_keyed_on_label_survives_value_removing_edit() {
        // Group by Type [sprite, mesh, sprite, mesh]; collapse the sprite
        // group; then edit BOTH sprite rows to mesh so "sprite" vanishes. The
        // collapse keys on the LABEL, so it must NOT re-target the mesh group
        // (the positional-id bug would collapse whatever now sits at index 0).
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            assert_eq!(intro.invoke("toggle_group", IntrospectValue::Int(0)), Ok(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("visible_len"), Some(IntrospectValue::Int(4)), "sprite collapsed");
            // Both sprite rows (0, 2) -> mesh; "sprite" no longer exists.
            let _ = intro.intervene("value.0.1", IntrospectValue::Text("mesh".to_owned()));
            let _ = intro.intervene("value.2.1", IntrospectValue::Text("mesh".to_owned()));
            assert_eq!(intro.query("group_count"), Some(IntrospectValue::Int(1)), "only mesh remains");
            assert_eq!(
                intro.query("collapsed.0"),
                Some(IntrospectValue::Bool(false)),
                "mesh is NOT collapsed (the stale 'sprite' label does not match it)",
            );
            assert_eq!(
                intro.query("visible_len"),
                Some(IntrospectValue::Int(5)),
                "one mesh header + four data rows, all shown",
            );
        });
    }

    #[test]
    fn r893_collapsed_out_of_range_group_is_null_and_noop() {
        // R893 — the label map guards an out-of-range group id without a
        // hand-rolled bound: query is Null (present-but-empty), toggle is a
        // no-op returning false, intervene is UnknownPath; the set stays clean.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned())); // 2 groups
            assert_eq!(intro.query("collapsed.9"), Some(IntrospectValue::Null), "OOR group -> Null");
            assert_eq!(
                intro.invoke("toggle_group", IntrospectValue::Int(9)),
                Ok(IntrospectValue::Bool(false)),
                "OOR toggle is a no-op",
            );
            assert_eq!(
                intro.intervene("collapsed.9", IntrospectValue::Bool(true)),
                Err(InterveneError::UnknownPath),
                "OOR collapse write is UnknownPath",
            );
            // No dead id leaked: the real groups stay expanded.
            assert_eq!(intro.query("collapsed.0"), Some(IntrospectValue::Bool(false)));
            assert_eq!(intro.query("visible_len"), Some(IntrospectValue::Int(6)), "all expanded");
        });
    }

    // ─── R894 — per-column validation (clamp) ────────────────────────

    #[test]
    fn r894_commit_clamps_to_column_range() {
        // Count (col 2) range 0..1000. A committed edit above the bound lands
        // on the max; in-range commits unchanged. Scale (col 3) is unbounded
        // so a negative commit stores verbatim (no clamp applied).
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let commit = |scene: &mut Scene, col: i64, text: &str| {
                {
                    let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
                    let intro = node.handle.introspect_mut().expect("introspectable");
                    let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
                    let _ = intro.intervene("focused_col", IntrospectValue::Int(col));
                    let _ = intro.invoke("begin", IntrospectValue::Null);
                }
                use_text_edit_state(EDIT_TF_TAG).set_text(text.to_owned());
                commit_edit(true);
            };
            commit(&mut scene, 2, "5000");
            assert_eq!(grid_intro(&scene).query("value.0.2"), Some(IntrospectValue::Int(1000)), "clamp to max");
            commit(&mut scene, 3, "-5");
            assert_eq!(grid_intro(&scene).query("value.0.3"), Some(IntrospectValue::Float(-5.0)), "unbounded — stores as-is");
            commit(&mut scene, 2, "42");
            assert_eq!(grid_intro(&scene).query("value.0.2"), Some(IntrospectValue::Int(42)), "in-range unchanged");
        });
    }

    #[test]
    fn r894_intervene_clamps_to_column_range() {
        // The AI write path runs the same clamp gate (an `intervene` cannot
        // exceed a bound a keyboard edit cannot); the read-back is the clamped
        // value (the setter-returns-read-outcome discipline). Scale (col 3) is
        // unbounded so a negative intervene stores verbatim.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(intro.intervene("value.0.2", IntrospectValue::Int(5000)).is_ok());
            assert_eq!(intro.query("value.0.2"), Some(IntrospectValue::Int(1000)), "clamp to max");
            assert!(intro.intervene("value.0.3", IntrospectValue::Float(-5.0)).is_ok());
            assert_eq!(intro.query("value.0.3"), Some(IntrospectValue::Float(-5.0)), "unbounded — stores as-is");
            assert!(intro.intervene("value.1.3", IntrospectValue::Float(5.0)).is_ok());
            assert_eq!(intro.query("value.1.3"), Some(IntrospectValue::Float(5.0)), "positive float kept");
        });
    }

    #[test]
    fn r894_col_range_wire_query() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("col_range.2"), Some(IntrospectValue::Text("0..1000".to_owned())));
            assert_eq!(intro.query("col_range.3"), Some(IntrospectValue::Text("none".to_owned())), "Scale unbounded");
            assert_eq!(intro.query("col_range.0"), Some(IntrospectValue::Text("none".to_owned())), "Asset unbounded");
            assert_eq!(intro.query("col_range.9"), None, "out-of-range column -> None");
        });
    }

    #[test]
    fn r894_unbounded_column_is_unclamped() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Asset (col 0, Text, no range): a long value is stored verbatim.
            assert!(intro.intervene("value.0.0", IntrospectValue::Text("VeryLongAssetName".to_owned())).is_ok());
            assert_eq!(
                intro.query("value.0.0"),
                Some(IntrospectValue::Text("VeryLongAssetName".to_owned())),
            );
        });
    }

    // ─── R914 cell scrub (DragCalibration 3rd consumer) ───────────────

    /// R914 — fire one captured `pointer_move` at cursor fraction `x` on the
    /// grid External (the runtime capture-lock path `scene/drag` drives at
    /// runtime; the unit test feeds the same arc directly).
    fn grid_pointer_move(scene: &mut Scene, x: f32) {
        let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
        node.handle.pointer_move(x, 0.0);
    }

    /// R914 — send a composite cell event (`<row>_<col>:<Event>`) to the grid:
    /// `PointerDown` arms, `PointerUp` releases / clicks.
    fn grid_send(scene: &mut Scene, payload: &str) {
        let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        intro.invoke("send", IntrospectValue::Text(payload.to_owned())).expect("send accepted");
    }

    fn cell_int(scene: &Scene, path: &str) -> i64 {
        match grid_intro(scene).query(path) {
            Some(IntrospectValue::Int(i)) => i,
            other => panic!("expected int at {path}, got {other:?}"),
        }
    }

    fn cell_float(scene: &Scene, path: &str) -> f64 {
        match grid_intro(scene).query(path) {
            Some(IntrospectValue::Float(f)) => f,
            other => panic!("expected float at {path}, got {other:?}"),
        }
    }

    fn scrubbing(scene: &Scene) -> bool {
        matches!(grid_intro(scene).query("scrubbing"), Some(IntrospectValue::Bool(true)))
    }

    #[test]
    fn r914_float_cell_scrub_tracks_cursor() {
        // Scale (col 3, Float, unbounded) boots at 1.0 in row 0. A press arms
        // the cell; the first captured move calibrates (no mutation); each later
        // move scrubs `base + travel_px · 0.01`, travel_px = delta·GRID_VIEWPORT_W.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            grid_send(&mut scene, "0_3:PointerDown");
            assert!(!scrubbing(&scene), "armed but not yet calibrated => not scrubbing");
            grid_pointer_move(&mut scene, 0.5); // calibrate (no mutation)
            assert!(scrubbing(&scene), "the first move calibrates => scrubbing");
            assert!(
                (cell_float(&scene, "value.0.3") - 1.0).abs() < f64::EPSILON,
                "the calibration frame does not mutate",
            );
            grid_pointer_move(&mut scene, 0.75); // +0.25 fraction
            let expected = 1.0 + 0.25 * f64::from(GRID_VIEWPORT_W) * SCRUB_FLOAT_PER_PX;
            let got = cell_float(&scene, "value.0.3");
            assert!((got - expected).abs() < 1e-6, "Scale scrubbed to ~{expected}, got {got}");
            assert!(got > 1.0, "a rightward drag increases the value");
            // A leftward drag is signed — back below the press value.
            grid_pointer_move(&mut scene, 0.25); // -0.25 fraction from the press
            assert!(cell_float(&scene, "value.0.3") < 1.0, "a leftward drag decreases the value");
            grid_send(&mut scene, "0_3:PointerUp");
            assert!(!scrubbing(&scene), "release tears the scrub down");
        });
    }

    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the scrub step count is a small whole number; this mirrors the \
                  production round-to-unit cast in scrub_to"
    )]
    fn r914_int_cell_scrub_steps_in_whole_units() {
        // Count (col 2, Int, 0..1000) boots at 1 in row 0. An int scrub steps in
        // whole units (8px/step) and stays an int.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            grid_send(&mut scene, "0_2:PointerDown");
            grid_pointer_move(&mut scene, 0.5); // calibrate
            grid_pointer_move(&mut scene, 0.75); // +0.25·370 = 92.5px
            let steps = (0.25 * f64::from(GRID_VIEWPORT_W) / SCRUB_INT_PX_PER_STEP).round() as i64;
            assert_eq!(cell_int(&scene, "value.0.2"), 1 + steps, "Count steps +{steps} in whole units");
            grid_send(&mut scene, "0_2:PointerUp");
            assert!(!scrubbing(&scene));
        });
    }

    #[test]
    fn r914_scrub_clamps_to_column_range() {
        // R894 / R914 — the scrub commits through the SAME clamped `set_cell`
        // funnel as the AI `value` write, so a drag cannot exceed a bound a
        // keyboard / RPC edit cannot. Count (col 2) is 0..1000.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // A huge rightward drag clamps at the column maximum.
            grid_send(&mut scene, "0_2:PointerDown");
            grid_pointer_move(&mut scene, 0.5); // calibrate
            grid_pointer_move(&mut scene, 50.0); // far right => > 1000 before clamp
            assert_eq!(cell_int(&scene, "value.0.2"), 1000, "rightward scrub clamps to the max");
            grid_send(&mut scene, "0_2:PointerUp");
            // A huge leftward drag clamps at the column minimum.
            grid_send(&mut scene, "0_2:PointerDown");
            grid_pointer_move(&mut scene, 0.5); // recalibrate (base now 1000)
            grid_pointer_move(&mut scene, -50.0); // far left => < 0 before clamp
            assert_eq!(cell_int(&scene, "value.0.2"), 0, "leftward scrub clamps to the min");
            grid_send(&mut scene, "0_2:PointerUp");
        });
    }

    #[test]
    fn r914_numeric_click_is_absorbed_and_non_numeric_click_acts() {
        // The R51.34 capture lock + R51.35 click-to-position forward calibrate a
        // zero-travel scrub on the press of a numeric cell (the framework
        // forwards the press cursor as the first `pointer_move`), so the release
        // suppresses the click: a click on a numeric cell neither scrubs nor
        // focuses (matching the property grid). A non-numeric cell never arms,
        // so its click falls through to the focus / toggle action.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // (a) numeric cell: press arms + the press-time forward calibrates;
            //     the release is absorbed (no focus change, value unchanged).
            grid_send(&mut scene, "0_2:PointerDown");
            grid_pointer_move(&mut scene, 0.5); // the R51.35 press-time forward
            assert!(scrubbing(&scene), "the press-time forward calibrates a (zero-travel) scrub");
            grid_send(&mut scene, "0_2:PointerUp");
            assert!(!scrubbing(&scene), "the release tears the scrub down");
            assert_eq!(cell_int(&scene, "value.0.2"), 1, "Count unchanged by the absorbed click");
            assert_eq!(grid_intro(&scene).query("focused_row"), Some(IntrospectValue::Int(0)));
            assert_eq!(grid_intro(&scene).query("focused_col"), Some(IntrospectValue::Int(0)),
                "the absorbed click did not move the cursor onto the numeric cell");

            // (b) the Active bool (col 4, row 2 = false) never arms; even a real
            //     cursor march never scrubs, and the release toggles the bool.
            assert_eq!(grid_intro(&scene).query("value.2.4"), Some(IntrospectValue::Bool(false)));
            grid_send(&mut scene, "2_4:PointerDown");
            grid_pointer_move(&mut scene, 0.5);
            grid_pointer_move(&mut scene, 0.8); // a real cursor march, but col 4 is not numeric
            assert!(!scrubbing(&scene), "a non-numeric press never calibrates a scrub");
            grid_send(&mut scene, "2_4:PointerUp");
            assert_eq!(
                grid_intro(&scene).query("value.2.4"),
                Some(IntrospectValue::Bool(true)),
                "the bool toggles on release (the press did not scrub)",
            );
            assert_eq!(grid_intro(&scene).query("focused_row"), Some(IntrospectValue::Int(2)),
                "the non-numeric click focuses its cell");

            // (c) isolation: none of the above touched a neighbouring numeric cell.
            assert!((cell_float(&scene, "value.0.3") - 1.0).abs() < f64::EPSILON, "Scale untouched");
        });
    }
}
