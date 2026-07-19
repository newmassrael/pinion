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
//!   [`CellKind`] ([`COL_KINDS`]). R930 — the grid is **dynamic-length**: the
//!   row count is `model.len() / NCOLS` ([`nrows`], not a const), so
//!   `invoke "add_row"` / `"remove_row"` grow / shrink the table at runtime
//!   and the very next paint re-derives the sort / filter / group order over
//!   the longer-or-shorter model (the model is the one SSOT — no separate
//!   row-count to keep in sync; a grid keeps >= 1 row). R932 — every edit is
//!   **undoable** on a shared `UndoStack`: cell value edits (`SetCellEdit`) and
//!   row add/remove (`RowEdit`) / row move (`MoveRowEdit`) as granular
//!   reversible commands, the AI-first `UndoStackExternal` and the keyboard
//!   `Ctrl+Z` driving one timeline (the node-graph's journaled-edit shape at 2-D).
//!   R1237 — `invoke "paste" "<tsv>"` writes a TSV block anchored at the cursor
//!   (following the active sort / filter / group), the whole block one undo step
//!   (a `begin_macro` transaction — the grid's first macro consumer).
//!   Exposes the whole grid
//!   for AI-first introspection: `query value.<r>.<c>` / `col_name.<c>` / `col_kind.<c>` /
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
//! the [`cycle_col_sort`] / [`grid_order_by`] / `cell_cmp` SSOT every
//! read-only grid sorts by; the wire speaks the cross-grid
//! [`grid_sort_str`] vocabulary (`query "sort"` / `intervene "sort"` /
//! `invoke "cycle_sort"` / `query "source_at.<pos>"`). The fold's one design
//! decision: ALL grid state stays **source-keyed** (cursor, edit latch,
//! cell tags, `value.<row>.<col>` addressing) and only the paint / a11y row
//! sequence + arrow navigation consult the derived visual order — so a
//! committed edit that changes the active sort key moves its row on the
//! very next paint while the cursor and the in-flight editor follow the
//! source row (the Excel / Qt `QSortFilterProxyModel` behaviour). The
//! `GridSortState` coordinator is deliberately NOT reused here: it owns a
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
//! - R997 — the cross-grid `GridFilter` is now a multi-facet conjunction with
//!   per-facet ops (`=`/`!=`/`~`/`<`/`<=`/`>`/`>=`); this typed grid honours
//!   every op against its TYPED cells via `CellValue::matches_facet` (the
//!   wire vocab is shared, the comparison is the consumer's policy). The
//!   reference multi-facet UI is `hello-grid-multifilter` (the at-scale grid).
//! - Frozen panes on the *editable* grid are a no-op at this size (the columns
//!   fit the window, so there is no horizontal scroll to pin against) —
//!   deferred until a wide editable grid needs it.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use std::collections::{BTreeMap, BTreeSet};

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, AriaRole, GridCell, GridColumn, GridRow,
    GroupedGridSelection, GroupedGridSpec, ListOption, SortDirection, WidgetA11y,
    attach_child_button, grid_table_nodes, grouped_grid_access_nodes, listbox_option_nodes,
};
use pinion_core::cell_value::{CellKind, CellValue};
use pinion_core::composite_tag::{GridSendKey, prefixed_index, split_subindex};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, DragPayload, DropPoint, External,
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    RepaintOwner, SchemaArg, SchemaField, ThreadOwnership, int_of,
};
use pinion_core::input::{DRAG_CLICK_THRESHOLD_PX, DragCalibration};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::undo::{
    UndoCommand, UndoStack, UndoStackExternal, undo_redo_verb, use_undo_stack,
};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::checkbox::CheckboxState;
use pinion_core::widgets::grid_sort::{
    GridFilter, col_sort_dir, grid_filter_from_str, grid_filter_str, grid_sort_from_str,
    grid_sort_str,
};
use pinion_core::widgets::group_order::{GroupRow, group_rows};
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::table::{cycle_col_sort, grid_order_by, parse_row_col, rows_to_tsv};
use pinion_core::widgets::text_edit::{TextEditState, use_text_edit_state};
use pinion_core::widgets::text_field::{TextFieldState, blur_committing_field_extra};
use pinion_core::widgets::virtual_list::{VisibleWindow, compute_visible_range, content_height};
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_platform_clipboard::use_app_clipboard;
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::checkbox::{CheckboxStyle, view_checkbox_box};
use pinion_widget_paint::group_header::group_header_row;
use pinion_widget_paint::listbox::{OptionRow, view_option};
use pinion_widget_paint::popup::popup_surface;
use pinion_widget_paint::table::{GridScroll, view_virtual_grid_body};
use pinion_widget_paint::text_field as tf_paint;

use pinion_widget_paint::state_layer::focus_fill;

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
/// R960 — side of the per-cell "modified, click to reset" accent dot. It sits
/// inside the cell's [`CELL_PAD`] content box (not the trailing edge) so it is
/// not clipped out of the scroll viewport's hit-test — a dot past the content
/// box would paint but route its click to the cell underneath.
const RESET_DOT: u32 = 8;
const CHECKBOX_SIZE: u32 = 18;
const PANEL_PAD: u32 = 20;
const ROW_GAP: u32 = 1;
/// R937 — the leading drag-handle column width (px). A narrow grip column that
/// arms a row drag-to-reorder; painted (with the grip glyph) only when reorder
/// is enabled (the plain source view), but its width is always reserved so the
/// data columns do not shift when a sort / filter / group engages.
const HANDLE_W: u32 = 22;
/// R937 — U+283F BRAILLE PATTERN DOTS-123456, the drag-handle grip glyph (the
/// six-dot "grip" convention; a named const per the non-ASCII-literal rule).
const GRIP_GLYPH: &str = "\u{283F}";
/// R937 — the drop insertion-line thickness (px).
const DROP_LINE_H: u32 = 2;

// ─── tags + intents ───────────────────────────────────────────────

/// Primary External — the grid coordinator (the single keyboard Tab stop).
const GRID_TAG: &str = "data_grid";
/// R960 — a cell's reset-to-default click target is `data_grid#reset<row>_<col>`
/// (this prefix + the shared [`GridSendKey::Cell`] `<row>_<col>` grammar). The
/// 3rd `reset`-send consumer after hello-property-grid and hello-inspector, but
/// the three diverge in key arity (2-D cell here, 1-D index / `ValueRef` there),
/// arrow node, and positioning, so the decode + paint stay per-binding — only
/// the [`CellValue::value_eq`] modified atom is shared (an audit-falsified lift).
const RESET_PREFIX: &str = "reset";
/// Extra External — the one shared inline cell editor.
const EDIT_TF_TAG: &str = "data_grid_edit";
/// R932 — Extra External tag of the AI-first undo-history surface
/// ([`UndoStackExternal`]); `scene/{query,invoke}` at `/data_grid_undo/external/…`
/// observe + drive the same [`UndoStack`] the coordinator records onto.
const UNDO_TAG: &str = "data_grid_undo";
/// R932 — the [`use_undo_stack`] cache key the coordinator (which records cell /
/// row edits), the keyboard `Ctrl+Z` path, the status-line label read, and the
/// [`UndoStackExternal`] all share — one history source of truth.
const UNDO_KEY: &str = "hello_data_grid::undo";
/// R896 — the horizontal `ScrollState` cache key shared by the header + rows:
/// `scene/set_scroll_offset` on this tag slides the columns sideways once
/// their total width outgrows the grid viewport (the R784 single-axis scroll,
/// the read-only grids' `h_scrolled_column` wrap reused on the editable grid).
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
/// R937 — the [`DragPayload::kind`] discriminator for a row drag-to-reorder (the
/// `hello-dnd` `"dnd-row"` peer): names what a drag session over this grid carries.
const REORDER_KIND: &str = "data-grid-row";

// ─── choice-popup tags + dimensions (R940) ────────────────────────
// The floating dropdown a `Choice` cell opens (the property-grid popup pattern,
// data-grid's 2nd consumer). The panel is anchored in GRID-LOCAL coordinates
// (a sibling of the scroll viewport inside the grid container), scroll-aware so
// it tracks the cell as the body scrolls (the Qt combobox-delegate behaviour).

/// The open dropdown's container tag (a standalone overlay tag, like the
/// inline editor's; not a composite send target).
const CHOICE_POPUP_TAG: &str = "data_grid_choice";
/// The light-dismiss barrier — a composite tag routing back to the grid
/// coordinator (`{GRID_TAG}#dismiss`), so a click outside the panel closes the
/// popup through the same `send` funnel the cells use (the property-grid shape).
const POPUP_DISMISS_TAG: &str = "data_grid#dismiss";
/// Each option's composite sub-key prefix (`{GRID_TAG}#opt<i>`): a click commits
/// option `i`, a `PointerEnter` / `PointerLeave` sets / clears the hover.
const CHOICE_OPT_PREFIX: &str = "opt";
/// The dropdown panel width — the `Type` column's width, so the panel aligns
/// under the cell it edits.
const POPUP_W: u32 = COL_W[TYPE_COL];
/// One option row's height + the panel's inner padding (the property-grid feel).
const POPUP_OPT_H: u32 = 30;
const POPUP_PAD: u32 = 6;

// ─── colour swatch popup (R943) ───────────────────────────────────
/// R943 — the colour cell's swatch-palette popup tag (the 2nd popup-kind after
/// R940's choice dropdown; the property-grid `COLOR_POPUP_TAG` shape). A click
/// on a `Color` cell opens it; a standalone overlay tag, not a send target.
const COLOR_POPUP_TAG: &str = "data_grid_color";
/// Each swatch chip's composite sub-key prefix (`{GRID_TAG}#sw<i>`): a click
/// commits swatch `i`, a `PointerEnter` / `PointerLeave` sets / clears the hover
/// (the `opt<i>` peer for the colour palette).
const COLOR_SW_PREFIX: &str = "sw";
/// Swatch chip size, palette column count, and inter-chip gap.
const SWATCH_SIZE: u32 = 30;
const SWATCH_COLS: usize = 4;
const SWATCH_GAP: u32 = 6;
/// R943 — the preset palette the colour popup offers (the AT label = the name,
/// mirroring the property-grid `COLOR_SWATCHES`). An arbitrary colour is set
/// through `intervene value` with a `#RRGGBB` hex string (the AI-first path); an
/// in-popup GUI hex-entry field is a documented follow-up (the property grid
/// staged its colour cell the same way — presets first, hex field later).
const COLOR_SWATCHES: [(Color, &str); 8] = [
    (Color::rgb(0xff, 0xff, 0xff), "White"),
    (Color::rgb(0x21, 0x21, 0x21), "Black"),
    (Color::rgb(0xe5, 0x39, 0x35), "Red"),
    (Color::rgb(0x43, 0xa0, 0x47), "Green"),
    (Color::rgb(0x1e, 0x88, 0xe5), "Blue"),
    (Color::rgb(0xfd, 0xd8, 0x35), "Yellow"),
    (Color::rgb(0x00, 0xac, 0xc1), "Cyan"),
    (Color::rgb(0x8e, 0x24, 0xaa), "Purple"),
];

// ─── grid shape (an editable asset table) ─────────────────────────

const NROWS: usize = 4;
const NCOLS: usize = 6;

/// Column titles (the header row + the AT cell-name prefix).
const COL_NAMES: [&str; NCOLS] = ["Asset", "Type", "Count", "Scale", "Active", "Tint"];

/// The per-column [`CellKind`] — every cell in a column shares its column's
/// kind (the editor dispatch, parse, keystroke gate, and intervene coercion
/// all read from here). R940 — the `Type` column (col 1) is a [`CellKind::Choice`]
/// enum (asset kinds are a closed set, not free text), edited through a floating
/// dropdown popup; the rest stay scalar.
const COL_KINDS: [CellKind; NCOLS] = [
    CellKind::Text,
    CellKind::Choice,
    CellKind::Int,
    CellKind::Float,
    CellKind::Bool,
    CellKind::Color,
];

/// R940 — the `Type` column index (the one [`CellKind::Choice`] column). The
/// SSOT the seed, [`default_row`], the popup paint / a11y, and the column-kind
/// dispatch all read so the choice column is named in exactly one place.
const TYPE_COL: usize = 1;

/// R940 — the `Type` column's closed enum (asset kinds). Index 0 = `sprite`,
/// 1 = `mesh` (the seed values, so the filter / group / sort tests read the
/// same display labels); the rest broaden the dropdown so a `Choice` edit
/// picks among real alternatives. A `Choice` cell stores its own option list
/// (the value-level options the property grid's cells carry), so this is the
/// per-column source those cells clone from.
const TYPE_OPTIONS: [&str; 5] = ["sprite", "mesh", "material", "audio", "script"];

/// Per-column paint width (logical px). Text columns are wider. R896 — the
/// columns are deliberately wider than the grid viewport ([`GRID_VIEWPORT_W`])
/// so their `690 px` total outgrows the visible band and the R784 horizontal
/// scroll (`h_scrolled_column`) engages — the "wide asset table" a DCC
/// browser scrolls sideways.
const COL_W: [u32; NCOLS] = [160, 110, 100, 100, 100, 120];

/// R896 — the grid's visible width. Narrower than the `690 px` column total
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
    [None, None, Some(ColRange::Int(0, 1000)), None, None, None];

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

/// R960 — a cell's reset-dot click target — `data_grid#reset<row>_<col>`
/// ([`RESET_PREFIX`] + the same [`GridSendKey::Cell`] `<row>_<col>` grammar, so
/// the cell address is not re-derived). The decoder (`dispatch_send`) strips
/// `reset` and reuses [`GridSendKey::parse`] on the remainder.
fn reset_cell_tag(row: usize, col: usize) -> String {
    format!(
        "{GRID_TAG}#{RESET_PREFIX}{}",
        GridSendKey::Cell { row, col }.encode()
    )
}

/// R966 — a column header's "reset this column" dot click target —
/// `data_grid#resetcol<col>` ([`RESET_PREFIX`] + a `col`-discriminated index).
/// The `col` / `row` LETTER prefixes never alias the DIGIT-leading `<row>_<col>`
/// cell form, so one `reset`-prefixed namespace carries all three reset
/// granularities (cell / row / column) decoded through [`ResetTarget`].
fn reset_col_tag(col: usize) -> String {
    format!("{GRID_TAG}#{RESET_PREFIX}col{col}")
}

/// R966 — a row's "reset this row" dot click target — `data_grid#resetrow<row>`
/// (the [`reset_col_tag`] row peer).
fn reset_row_tag(row: usize) -> String {
    format!("{GRID_TAG}#{RESET_PREFIX}row{row}")
}

/// R966 — a decoded reset-affordance target: which cells a `reset`-prefixed
/// pointer send / AT `Click` clears. The one grammar SSOT shared by the pointer
/// channel ([`DataGridExternal::dispatch_send`]) and the AT channel
/// ([`DataGridView::access_child_invoke`]) so the two cannot decode the same
/// `reset<…>` tag differently — a divergence-is-a-bug 2nd-consumer lift, where
/// the cell case reuses [`GridSendKey::Cell`] rather than re-deriving the
/// `<row>_<col>` address ([R960](`reset_cell_tag`) `reset`-first ordering).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ResetTarget {
    /// `reset<row>_<col>` — one cell (R960).
    Cell { row: usize, col: usize },
    /// `resetrow<row>` — every modified cell in the row (R966).
    Row { row: usize },
    /// `resetcol<col>` — every modified cell in the column (R966).
    Col { col: usize },
}

impl ResetTarget {
    /// Decode the remainder AFTER [`RESET_PREFIX`] is stripped: a `row` / `col`
    /// letter prefix names a 1-D bulk reset, otherwise the digit-leading
    /// remainder is a [`GridSendKey::Cell`] `<row>_<col>`. A malformed remainder
    /// (`"row"` with no index, an unknown shape) decodes to `None` (a no-op).
    fn parse(rest: &str) -> Option<Self> {
        // R1231 — `row<i>` / `col<i>` via the shared `composite_tag::prefixed_index`.
        if let Some(row) = prefixed_index(rest, "row") {
            return Some(ResetTarget::Row { row });
        }
        if let Some(col) = prefixed_index(rest, "col") {
            return Some(ResetTarget::Col { col });
        }
        match GridSendKey::parse(rest)? {
            GridSendKey::Cell { row, col } => Some(ResetTarget::Cell { row, col }),
            _ => None,
        }
    }
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

/// R937 — a row's drag-handle target — `data_grid#d<source>`. A LOCAL scheme
/// (the `'d'` prefix), kept OUT of the shared [`GridSendKey`] grammar: the handle
/// is a data-grid-reorder concept produced + consumed only here, where
/// [`GridSendKey`] is shared with the read-only grids + a11y (R773 — home a wire
/// vocabulary by who produces + consumes it). `GridSendKey::parse` returns `None`
/// for a `d<n>` key (no `'_'`, not `'h'`/`'g'`), so the send dispatch checks this
/// prefix BEFORE falling through to `GridSendKey::parse` with no collision.
fn handle_tag(source: usize) -> String {
    format!("{GRID_TAG}#d{source}")
}

/// R937 — the `'d'`-prefixed sub-key of a [`handle_tag`] decoded back to its
/// source row (the producer's inverse, the one place the handle grammar is read).
fn parse_handle_sub(sub: &str) -> Option<usize> {
    prefixed_index(sub, "d")
}

// ─── column sort (R886 — the editable fold of the sort axis) ──────

/// R886 / R891 §5.40 — the visual → source row permutation for the live
/// typed model. The editable grid's peer of `GridSortState::order`: that
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
        nrows(model),
        sort,
        |col, a, b| model[idx(a, col)].sort_cmp(&model[idx(b, col)]),
        |row| {
            filter.is_none_or(|f| {
                // R997 — every facet in the conjunction must hold, each
                // evaluated against this TYPED cell via the op-aware
                // `matches_facet` (the typed-grid policy; the at-scale
                // `GridSortState` evaluates the same wire facets as text).
                f.facets.iter().all(|facet| {
                    model
                        .get(idx(row, facet.col))
                        .is_some_and(|c| c.matches_facet(facet.op, &facet.value))
                })
            })
        },
    )
}

/// R930 — the live row count, derived from the flat model (the SSOT) rather
/// than the [`NROWS`] const: `model.len() / NCOLS`. The grid is now
/// *dynamic-length* (rows add / remove at runtime), so every former `NROWS`
/// read routes through this so the sort / filter / group order, the paint
/// loop, bounds checks, and the a11y row tree all track the live model
/// ([[constraint-change-grep-all-layers]]). `NROWS` survives only as the boot
/// seed length (`default_cells`).
fn nrows(model: &[CellValue]) -> usize {
    // R930.1 — fail loud on a corrupt model rather than let the integer
    // division silently truncate a partial row (add/remove always move whole
    // NCOLS-wide rows, so a non-multiple length is a bug, not a valid state).
    debug_assert!(
        model.len() % NCOLS == 0,
        "the cell model is a whole number of rows"
    );
    model.len() / NCOLS
}

/// R937.1 — whether the grid is in the PLAIN source view (no sort / filter /
/// group), where the visual order IS the source order, so a manual row reorder is
/// 1:1 meaningful (Qt / Excel disable reorder under a sort proxy). The ONE
/// predicate the coordinator's [`DataGridExternal::reorder_enabled`], the view
/// (grip + drop line), and the a11y reorder actions all share, so the three can
/// never disagree on when reorder is enabled — the R886.1 one-gate discipline
/// (the R937 session-review caught this triplicated inline).
fn plain_view(
    sort: Option<(usize, bool)>,
    filter: Option<&GridFilter>,
    group_col: Option<usize>,
) -> bool {
    sort.is_none() && filter.is_none() && group_col.is_none()
}

/// R940 — the [`CellKind::Choice`] option labels for column `col` (the `Type`
/// column's [`TYPE_OPTIONS`]; empty for a non-choice column). The per-column
/// option SSOT the seed + [`default_row`] clone into each `Choice` cell (the
/// option list lives on the value, the property-grid cell shape).
fn choice_options(col: usize) -> Vec<String> {
    match col {
        TYPE_COL => TYPE_OPTIONS.iter().map(|s| (*s).to_owned()).collect(),
        _ => Vec::new(),
    }
}

/// R940 — a `Choice` cell for column `col` at option `selected` (clamped into
/// range, falling back to 0). The one constructor the seed + [`default_row`]
/// build choice cells through, so a seeded and an added choice cell carry the
/// same option list.
fn choice_cell(col: usize, selected: usize) -> CellValue {
    let options = choice_options(col);
    let selected = if selected < options.len() {
        selected
    } else {
        0
    };
    CellValue::Choice { selected, options }
}

/// R943 — a [`CellValue::Color`] cell at preset swatch `i` (clamped, falling back
/// to the first swatch). The constructor the seed + [`default_row`] build colour
/// cells through, so a seeded and an added `Tint` cell start from the palette.
fn swatch_cell(i: usize) -> CellValue {
    let (color, _) = COLOR_SWATCHES[i.min(COLOR_SWATCHES.len() - 1)];
    CellValue::Color(color)
}

/// R930 — a fresh row's default cells, one per column keyed by [`COL_KINDS`]
/// (the typed empty value `add_row` appends). The typed peer of
/// [`default_cells`]'s seed, so an added row edits exactly like a seeded one.
/// R940 — column-aware: a choice column seeds its [`choice_cell`] (option 0)
/// so an added `Type` row carries the full dropdown, not an empty list.
fn default_row() -> Vec<CellValue> {
    (0..NCOLS).map(col_default).collect()
}

/// R960 — the default value of column `col` (what a fresh [`default_row`] cell
/// gets): the per-column reset target AND the modified-from-default baseline.
/// A cell is "modified" when it differs from this ([`CellValue::value_eq`]); a
/// reset writes this back. One notion of default — the value a new row carries,
/// the value a reset restores — so the indicator and the reset agree by
/// construction (the SSOT [`default_row`] maps over).
///
/// R961.1 honesty note: this is a **per-column** default (Unreal "reset to the
/// column's default value"), NOT a frozen per-row boot snapshot. It deliberately
/// differs from `hello-inspector` / `hello-property-grid`, which freeze the boot
/// SEED as the baseline (so those boot with zero modified cells). A per-column
/// default is the right fit for a **dynamic-length** grid — a runtime-added row
/// has no boot-snapshot entry, so a frozen per-row baseline would need threading
/// through every structural mutator (add / remove / move). The trade-off: a seed
/// whose values differ from the empty column defaults boots with those cells
/// marked modified (which is the same way Unreal shows a customized instance's
/// overridden properties), and a reset clears such a cell to the column default.
fn col_default(col: usize) -> CellValue {
    match COL_KINDS[col] {
        CellKind::Text => CellValue::Text(String::new()),
        CellKind::Int => CellValue::Int(0),
        CellKind::Float => CellValue::Float(0.0),
        CellKind::Bool => CellValue::Bool(false),
        CellKind::Choice => choice_cell(col, 0),
        // R943 — a fresh `Tint` cell starts at the first preset swatch.
        CellKind::Color => swatch_cell(0),
    }
}

/// R967.1 — the value-level "does this value differ from column `col`'s
/// [`col_default`]" predicate. The single source of truth the per-cell reset dot
/// ([`view_cell`], which has the value in hand) reads DIRECTLY, and the
/// [`cell_value_modified`] model-indexed wrapper reads through `model.get`. One
/// place encodes the value-vs-default comparison + its out-of-range guard, so the
/// paint gate and the wire reads cannot diverge (a session-review caught
/// `view_cell` duplicating this inline — divergence-is-a-bug, since the dot's
/// presence and the `modified.<…>` query MUST agree).
fn value_modified(value: &CellValue, col: usize) -> bool {
    col < NCOLS && !value.value_eq(&col_default(col))
}

/// R966 — the model-indexed "does cell `(row, col)` differ from its column
/// default" read, built on the [`value_modified`] atom. The SSOT the per-column /
/// per-row header reset dots, the `modified.<row>.<col>` / `col_modified.<col>` /
/// `row_modified.<row>` queries, and [`DataGridExternal::cell_modified`] all read.
/// Pure over the slice so the paint path (which already borrows the model) and
/// the coordinator both call it.
fn cell_value_modified(model: &[CellValue], row: usize, col: usize) -> bool {
    col < NCOLS
        && model
            .get(idx(row, col))
            .is_some_and(|v| value_modified(v, col))
}

/// R966 — whether ANY cell in column `col` differs from its column default
/// (drives the column-header reset dot + the `col_modified.<col>` query). Built
/// on the [`cell_value_modified`] atom over every data row.
fn col_modified(model: &[CellValue], col: usize) -> bool {
    let rows = model.len() / NCOLS;
    (0..rows).any(|row| cell_value_modified(model, row, col))
}

/// R966 — whether ANY cell in `row` differs from its column default (drives the
/// per-row reset dot + the `row_modified.<row>` query). Built on the
/// [`cell_value_modified`] atom across the row's columns.
fn row_modified(model: &[CellValue], row: usize) -> bool {
    (0..NCOLS).any(|col| cell_value_modified(model, row, col))
}

/// First-paint cell values (row-major). Each column's values match
/// [`COL_KINDS`]. R940 — the `Type` cells (col 1) are [`CellKind::Choice`]
/// values: `sprite` (option 0) / `mesh` (option 1), the same display labels
/// the prior `Text` cells showed (so the filter / group / sort assertions,
/// which read `display()`, stand unchanged).
fn default_cells() -> Vec<CellValue> {
    vec![
        CellValue::Text("Hero".to_owned()),
        choice_cell(TYPE_COL, 0),
        CellValue::Int(1),
        CellValue::Float(1.0),
        CellValue::Bool(true),
        swatch_cell(4),
        CellValue::Text("Tree".to_owned()),
        choice_cell(TYPE_COL, 1),
        CellValue::Int(24),
        CellValue::Float(2.5),
        CellValue::Bool(true),
        swatch_cell(3),
        CellValue::Text("Coin".to_owned()),
        choice_cell(TYPE_COL, 0),
        CellValue::Int(99),
        CellValue::Float(0.5),
        CellValue::Bool(false),
        swatch_cell(5),
        CellValue::Text("Boss".to_owned()),
        choice_cell(TYPE_COL, 1),
        CellValue::Int(1),
        CellValue::Float(4.0),
        CellValue::Bool(true),
        swatch_cell(2),
    ]
}

// ─── reactive holders (Owner::cache, shared view ↔ coordinator) ────

#[must_use]
fn use_data_model() -> Rc<Signal<Vec<CellValue>>> {
    let owner = Owner::current().expect("use_data_model requires an active Owner scope");
    owner.cache("data_grid.model", || {
        let seed = default_cells();
        // R930 — the grid is now dynamic-length ([`nrows`] derives the count
        // from this model), so `NROWS` survives only as the declared boot-seed
        // row count: assert the seed matches it (one-time, at model creation).
        assert_eq!(
            seed.len(),
            NROWS * NCOLS,
            "the boot seed is exactly NROWS rows"
        );
        Signal::new(seed)
    })
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

/// R1372 §5.38 — the cell-range selection anchor `Some((source_row, col))`, the
/// pinned corner the roving cursor extends a rectangle from (the spreadsheet /
/// Qt `SelectItems` model R952 gave the Table widget; here the bespoke grid gets
/// its own anchor Signal, the SOURCE-keyed peer of the [`use_focused_row`] /
/// [`use_focused_col`] cursor). `None` = no range (a plain arrow collapses to a
/// single cell). The selected RECTANGLE is derived at paint / copy time as the
/// bbox of the anchor's and cursor's CURRENT visible positions (the source-keyed
/// discipline every axis here follows — only paint / nav consult the visible
/// order), so the highlight is always one contiguous, paintable screen rectangle
/// even under an active sort / filter / group; an anchor hidden by a filter /
/// collapse collapses the selection to the cursor.
#[must_use]
fn use_cell_anchor() -> Rc<Signal<Option<(usize, usize)>>> {
    let owner = Owner::current().expect("use_cell_anchor requires an active Owner scope");
    owner.cache("data_grid.cell_anchor", || Signal::new(None))
}

/// Edit-mode latch — `Some((row, col))` while that cell is being edited (the
/// todomvc `editing_id`, keyed by a 2-D cell). `None` = navigating. R940 — a
/// text / int / float cell editing here renders the shared inline field; a
/// `Choice` cell editing here is the OPEN DROPDOWN (no inline field, since a
/// choice is not [`CellKind::is_text_editable`]) — the one latch serves both.
#[must_use]
fn use_editing_cell() -> Rc<Signal<Option<(usize, usize)>>> {
    let owner = Owner::current().expect("use_editing_cell requires an active Owner scope");
    owner.cache("data_grid.editing_cell", || Signal::new(None))
}

/// R940 — the open choice dropdown's roving active descendant (the keyboard
/// cursor within the option list). `Some(i)` while a popup is open; `None` when
/// closed. The property-grid popup-cursor pattern (data-grid's 2nd consumer).
#[must_use]
fn use_popup_cursor() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_popup_cursor requires an active Owner scope");
    owner.cache("data_grid.popup_cursor", || Signal::new(None))
}

/// R940 — the open dropdown's pointer-hovered option (the mouse highlight), or
/// `None`. Set by `PointerEnter` / `PointerLeave` on the option rows.
#[must_use]
fn use_popup_hover() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_popup_hover requires an active Owner scope");
    owner.cache("data_grid.popup_hover", || Signal::new(None))
}

/// R940 — tear down the open choice dropdown: clear the edit latch, the keyboard
/// cursor, and the pointer hover in one place (the property-grid `clear_popup`
/// SSOT). Takes the three Signals by ref so the coordinator's pointer / RPC path
/// (its `Rc` fields) and the keyboard free-fn path (the `use_*` hooks — the same
/// Owner-cached instances) share one teardown.
fn clear_popup(
    editing: &Signal<Option<(usize, usize)>>,
    cursor: &Signal<Option<usize>>,
    hover: &Signal<Option<usize>>,
) {
    editing.set(None);
    cursor.set(None);
    hover.set(None);
}

/// R937 — the live row-reorder drop gap (`0..=nrows`), `None` when no drag is in
/// flight. Shared by the coordinator (a `drag_to` writes it) and the view (reads
/// it to paint the insertion line — reading subscribes, so a drag move repaints).
#[must_use]
fn use_drag_preview() -> Rc<Signal<Option<usize>>> {
    let owner = Owner::current().expect("use_drag_preview requires an active Owner scope");
    owner.cache("data_grid.drag_preview", || Signal::new(None))
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

/// R932 — the shared edit history. The coordinator (which records cell / row
/// edits in its mutation methods), the free-fn `commit_edit` (the inline
/// editor's keyboard commit), the keyboard `Ctrl+Z` path, the status-line
/// `undo_label` read, and the [`UndoStackExternal`] all reach the same `Rc`
/// (the [`use_undo_stack`] sharing) — one undo source of truth.
#[must_use]
fn use_undo() -> Rc<UndoStack> {
    use_undo_stack(UNDO_KEY)
}

// ─── column grouping (R892 — the editable fold of the group axis) ──

/// R892 — the group-by label table: the distinct display values of column
/// `col` in SOURCE-order first appearance, so a group's id is STABLE across
/// sort / filter / edits (the collapse set keys on it). `labels[id]` is the
/// header's display name; [`group_of`] maps a source row to its id.
///
/// R1265 — dedup through a `BTreeSet` seen-guard, not the old `labels.contains`
/// linear scan: that made this O(rows · groups) per call, and `view` calls it
/// every frame (once via [`visible_rows`], once for the header labels). The
/// `seen` set gives O(rows · log groups) with a byte-identical first-appearance
/// `labels` (the R1261 precompute-an-index-once lesson, applied to the grid).
fn group_table(model: &[CellValue], col: usize) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for row in 0..nrows(model) {
        if let Some(key) = model.get(idx(row, col)).map(CellValue::display) {
            // `insert` returns `true` only on first appearance, so `labels`
            // keeps the exact source-order-first-appearance sequence the old
            // `!contains` guard produced.
            if seen.insert(key.clone()) {
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
///
/// R1265 — the id comes from the inverse `ids` index ([`group_index_of`]) in
/// O(log groups), not an O(groups) `table.iter().position` scan. [`group_rows`]
/// calls this once per source row, so the old scan was O(rows · groups) per
/// paint; the map lookup makes the grouped flatten O(rows · log groups). A key
/// absent from the index (only the empty display of a missing cell, which
/// [`group_table`] never records) falls back to group 0 — exactly as the old
/// `position(...).unwrap_or(0)` did.
fn group_of(model: &[CellValue], row: usize, col: usize, ids: &BTreeMap<String, usize>) -> usize {
    let key = model
        .get(idx(row, col))
        .map(CellValue::display)
        .unwrap_or_default();
    ids.get(&key).copied().unwrap_or(0)
}

/// R1265 — the inverse of [`group_table`]: display label → its group id, so a
/// source row's group is an O(log groups) lookup ([`group_of`]) instead of a
/// linear scan of the label table. Built once per grouped paint from the table
/// (the table stays the id → label SSOT; this is its label → id twin).
fn group_index_of(table: &[String]) -> BTreeMap<String, usize> {
    table
        .iter()
        .enumerate()
        .map(|(id, label)| (label.clone(), id))
        .collect()
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
        None => order
            .into_iter()
            .map(|source| GroupRow::Data { source })
            .collect(),
        Some(col) => {
            let table = group_table(model, col);
            // R1265 — the label -> id index, built ONCE, so the per-row
            // `group_of` closure is an O(log groups) map lookup instead of an
            // O(groups) linear scan of `table` (the old code was O(rows *
            // groups) per paint; grouping by a high-cardinality column made it
            // quadratic in the row count).
            let ids = group_index_of(&table);
            group_rows(
                &order,
                |row| group_of(model, row, col, &ids),
                // R893 — collapse is keyed on the group LABEL, so map the id
                // group_rows hands us back to its label before the lookup.
                |group| {
                    table
                        .get(group)
                        .is_some_and(|label| collapsed.contains(label))
                },
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

// ─── R1372 cell-range selection + copy (the copy/paste symmetry) ───

/// R1372 §5.38 — the selected cell rectangle in VISIBLE-position coordinates
/// `(pos0, col0, pos1, col1)` (inclusive, normalized), derived from the
/// SOURCE-keyed `anchor` + cursor (`cursor_row` is a SOURCE index) mapped through
/// the CURRENT `visible` data order. Because the two endpoints are re-projected
/// each read, the rectangle is ALWAYS one contiguous, paintable screen region
/// under any sort / filter / group — unlike the Table widget's data-indexed
/// rectangle (R952), which scatters under a sort and there suppresses its
/// overlay. `None` — the selection collapses to the single focused cell — when
/// there is no anchor, or the anchor OR the cursor row is not currently visible
/// (a filter excluded it, a collapse hid it): the same "endpoint off-view -> no
/// block" guard [`DataGridExternal::paste_block`] applies to its paste anchor,
/// so copy and paste agree on when the visible rectangle is well-defined.
fn cell_selection_bounds(
    visible: &[usize],
    anchor: Option<(usize, usize)>,
    cursor_row: usize,
    cursor_col: usize,
) -> Option<(usize, usize, usize, usize)> {
    let (anchor_row, anchor_col) = anchor?;
    let apos = visible.iter().position(|&s| s == anchor_row)?;
    let cpos = visible.iter().position(|&s| s == cursor_row)?;
    Some((
        apos.min(cpos),
        anchor_col.min(cursor_col),
        apos.max(cpos),
        anchor_col.max(cursor_col),
    ))
}

/// R1372.1 — stamp per-cell `aria-selected` onto the GROUPED treegrid's emitted
/// gridcells. The flat grid sets `GridCell.selected` at build time, but the
/// grouped path goes through `pinion_a11y::grouped_grid_access_nodes`, whose
/// selection axis is ROW-level only (`GroupedGridSelection`) — so, exactly like
/// [`emit_reset_affordances`] above it, the CONSUMER augments the substrate's
/// output rather than generalize the shared a11y for this one grid (no premature
/// abstraction; a 2nd grouped cell-select consumer would justify a substrate
/// variant). `visible_sources` is the grouped visible DATA order (the
/// [`cell_selection_bounds`] coordinate space); when a range is active every
/// gridcell gets `Some(in_range)`, else it is left `None` (omit) — the flat
/// grid's `GridCell.selected` semantics.
fn stamp_cell_selection(
    nodes: &mut [AccessNode],
    visible_sources: &[usize],
    focus: (usize, usize),
) {
    let Some((p0, c0, p1, c1)) =
        cell_selection_bounds(visible_sources, use_cell_anchor().get(), focus.0, focus.1)
    else {
        return;
    };
    for (pos, &source) in visible_sources.iter().enumerate() {
        for col in 0..NCOLS {
            let selected = pos >= p0 && pos <= p1 && col >= c0 && col <= c1;
            if let Some(node) = nodes.iter_mut().find(|n| n.tag == cell_tag(source, col)) {
                node.selected = Some(selected);
            }
        }
    }
}

// ─── undo commands (R932 §5.52) ───────────────────────────────────

/// R932.1 — the reactive holders an undo command needs to uphold the SAME
/// post-mutation discipline EVERY coordinator mutator follows (`set_cell` /
/// `toggle` / `add_row` / `remove_row` / `commit_edit`): after the model write
/// it must (1) cancel any in-flight edit latch — a structural splice shifts the
/// source indices, so a stale source-keyed `(row, col)` latch would commit into
/// the wrong row (the R930.1 stale-latch class) — and (2) re-anchor the cursor
/// into the visible set, so an undo whose restored row the *current* filter /
/// group / collapse hides never strands the cursor on a non-rendered row (the
/// R930.1 re-anchor invariant). Bundled so a command holds one `Rc`, and shared
/// (the SSOT both the External's commands and the free-fn `commit_edit` build
/// from the same reactive holders).
struct GridUndoCtx {
    model: Rc<Signal<Vec<CellValue>>>,
    focused_row: Rc<Signal<usize>>,
    editing_cell: Rc<Signal<Option<(usize, usize)>>>,
    sort: Rc<Signal<Option<(usize, bool)>>>,
    filter: Rc<Signal<Option<GridFilter>>>,
    group_col: Rc<Signal<Option<usize>>>,
    collapsed: Rc<Signal<BTreeSet<String>>>,
}

impl GridUndoCtx {
    /// Bundle the shared reactive holders (owner-scoped — every accessor is a
    /// cached hook returning the SAME `Rc` the coordinator + view hold).
    fn from_hooks() -> Self {
        Self {
            model: use_data_model(),
            focused_row: use_focused_row(),
            editing_cell: use_editing_cell(),
            sort: use_sort(),
            filter: use_filter(),
            group_col: use_group_col(),
            collapsed: use_collapsed(),
        }
    }

    /// Apply `mutate` to the model, restore the cursor to `cursor`, then run the
    /// shared post-mutation discipline: cancel the in-flight edit latch (the
    /// document changed under the inline editor, and a structural splice would
    /// leave a stale source-keyed latch) and re-anchor the cursor so it never
    /// lands on a row the current view hides. This is the undo/redo peer of
    /// `set_cell` / `remove_row`'s own re-anchor — the one funnel that keeps
    /// undo from being the lone mutator that breaks the R930.1 invariants.
    fn restore(&self, cursor: usize, mutate: impl FnOnce(&mut Vec<CellValue>)) {
        self.editing_cell.set(None);
        self.model.set_with(|prev| {
            let mut next = prev.clone();
            mutate(&mut next);
            next
        });
        self.focused_row.set(cursor);
        let visible = visible_data_order(
            &self.model.get(),
            self.sort.get(),
            self.filter.get().as_ref(),
            self.group_col.get(),
            &self.collapsed.get(),
        );
        // Keep the restored cursor when its row is visible; otherwise re-anchor
        // to the slot it would occupy (the view changed since the edit was
        // recorded, so the captured row may now be filtered out).
        let prior_vis = cursor_visual_pos(&visible, cursor);
        reanchor_cursor(&visible, &self.focused_row, prior_vis);
    }
}

/// R932 §5.52 — a reversible single-cell value edit (the `QUndoCommand` peer at
/// the data-grid's cell granularity). Captures the cell's before / after value
/// and the cursor's before / after source row; undo / redo restore the value and
/// cursor, then re-anchor through [`GridUndoCtx::restore`] (so unlike a node
/// editor's always-valid selection set, the single source-row cursor can never
/// be left on a now-hidden row).
struct SetCellEdit {
    ctx: Rc<GridUndoCtx>,
    index: usize,
    before: CellValue,
    after: CellValue,
    before_cursor: usize,
    after_cursor: usize,
    label: Cow<'static, str>,
}

impl SetCellEdit {
    /// Restore the cell to `value` and the cursor to `cursor` in one step,
    /// through the shared re-anchor / latch-cancel discipline.
    fn write(&self, value: &CellValue, cursor: usize) {
        let (index, value) = (self.index, value.clone());
        self.ctx.restore(cursor, move |next| {
            if let Some(cell) = next.get_mut(index) {
                *cell = value;
            }
        });
    }
}

impl UndoCommand for SetCellEdit {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.write(&self.after, self.after_cursor);
    }

    fn undo(&self) {
        self.write(&self.before, self.before_cursor);
    }
}

/// R932 §5.52 — a reversible row splice: `add_row` and `remove_row` as one
/// granular command. `at` is the affected row's flat model offset
/// ([`idx(row, 0)`](idx)); `redo` replaces the `removed` cells with `inserted`,
/// `undo` the inverse — so an append is (removed empty, inserted = one
/// [`default_row`]) and a delete is (removed = the row's `NCOLS` cells, inserted
/// empty). The cursor's before / after source row rides along through
/// [`GridUndoCtx::restore`] (which cancels the edit latch the structural splice
/// would strand, R930.1). A whole-row snapshot, never a whole-model one
/// (granular undo, not snapshot).
struct RowEdit {
    ctx: Rc<GridUndoCtx>,
    at: usize,
    removed: Vec<CellValue>,
    inserted: Vec<CellValue>,
    before_cursor: usize,
    after_cursor: usize,
    label: Cow<'static, str>,
}

impl RowEdit {
    /// Replace the `take.len()` cells at `at` with `put`, then re-anchor — the
    /// model splice + latch-cancel + cursor re-anchor in one funnel.
    fn splice(&self, take: &[CellValue], put: &[CellValue], cursor: usize) {
        let (at, end, put) = (self.at, self.at + take.len(), put.to_vec());
        self.ctx.restore(cursor, move |next| {
            next.splice(at..end, put);
        });
    }
}

impl UndoCommand for RowEdit {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.splice(&self.removed, &self.inserted, self.after_cursor);
    }

    fn undo(&self) {
        self.splice(&self.inserted, &self.removed, self.before_cursor);
    }
}

/// R937 — move the `NCOLS`-cell block at row `from` to rest at row `to` (both
/// PHYSICAL row indices in the post-removal `Vec`, so a redo and its undo are
/// perfectly symmetric — just swap `from` / `to`). A no-op when `from == to`.
/// The flat-model peer of `ReorderModel::apply_move`, block-wide because a grid
/// row is `NCOLS` contiguous cells (the array's single-element move would lose
/// the column grouping).
fn move_block(cells: &mut Vec<CellValue>, from: usize, to: usize) {
    if from == to {
        return;
    }
    let row: Vec<CellValue> = cells
        .splice(from * NCOLS..from * NCOLS + NCOLS, [])
        .collect();
    let at = to * NCOLS;
    cells.splice(at..at, row);
}

/// R937 §5.52 — a reversible row move: the dragged row's source index `from` and
/// its resting index `to` (both physical, where `to` is already removal-shifted),
/// so `redo` is [`move_block`]`(from -> to)` and `undo` the exact inverse
/// `move_block(to -> from)` — a granular two-index command, never a whole-model
/// snapshot ([[granular-undo-not-snapshot]]). The cursor's before / after source
/// row rides along through [`GridUndoCtx::restore`] (latch-cancel + re-anchor),
/// so an undo whose row the *current* view hides never strands the cursor — the
/// same R930.1 discipline every other mutator follows.
struct MoveRowEdit {
    ctx: Rc<GridUndoCtx>,
    from: usize,
    to: usize,
    before_cursor: usize,
    after_cursor: usize,
    label: Cow<'static, str>,
}

impl MoveRowEdit {
    /// Move the block from `src` to `dst` and restore the cursor — the model
    /// move + latch-cancel + cursor re-anchor in one funnel.
    fn shift(&self, src: usize, dst: usize, cursor: usize) {
        self.ctx
            .restore(cursor, move |next| move_block(next, src, dst));
    }
}

impl UndoCommand for MoveRowEdit {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.shift(self.from, self.to, self.after_cursor);
    }

    fn undo(&self) {
        self.shift(self.to, self.from, self.before_cursor);
    }
}

/// R932 — record one already-applied cell edit on `stack`: the [`SetCellEdit`]
/// constructor SSOT both the coordinator's [`DataGridExternal::edit_cell`] and
/// the free-fn [`commit_edit`] push through, so the two pre-existing cell-write
/// paths (the RPC / scrub funnel and the keyboard inline editor) journal
/// identically. A net no-op write (value and cursor both unchanged) records
/// nothing — a malformed numeric commit or a same-value set leaves no history.
#[allow(
    clippy::too_many_arguments,
    reason = "the SetCellEdit fields, captured around an already-applied write \
              by two distinct call sites; bundling them would just move the \
              argument list into a one-use struct"
)]
fn push_cell_edit(
    stack: &UndoStack,
    ctx: &Rc<GridUndoCtx>,
    index: usize,
    before: CellValue,
    after: CellValue,
    before_cursor: usize,
    after_cursor: usize,
    label: Cow<'static, str>,
) {
    if before == after && before_cursor == after_cursor {
        return;
    }
    stack.push_applied(SetCellEdit {
        ctx: Rc::clone(ctx),
        index,
        before,
        after,
        before_cursor,
        after_cursor,
        label,
    });
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
    /// R932 — the shared edit history ([`use_undo`]). The mutation methods
    /// record a granular [`SetCellEdit`] / [`RowEdit`] onto it, and the
    /// [`UndoStackExternal`] extra wraps the **same** `Rc` so RPC + keyboard
    /// undo drive one timeline.
    undo: Rc<UndoStack>,
    /// R932.1 — the reactive holders a recorded command replays through
    /// ([`GridUndoCtx`]), so an undo / redo re-anchors the cursor + cancels the
    /// edit latch exactly like every direct mutator (shared with `commit_edit`).
    undo_ctx: Rc<GridUndoCtx>,
    /// R932 — the cell + cursor snapshot captured at `arm_scrub` (before any
    /// dead-zone move mutates), so `end_scrub` can journal a whole drag as ONE
    /// [`SetCellEdit`] step (the node editor's "one move per gesture at release"
    /// rule): `(row, col, before value, before cursor row)`. The live scrub
    /// itself writes through the no-journal [`set_cell`](Self::set_cell) funnel.
    scrub_origin: RefCell<Option<(usize, usize, CellValue, usize)>>,
    /// R937 — the SOURCE row a handle `PointerDown` armed for a drag-to-reorder,
    /// `None` for any other press. [`External::begin_drag`] returns a reorder
    /// payload only when this is `Some` AND the view is plain (reorder-enabled);
    /// a numeric-cell press arms [`scrub_armed`](Self::scrub_armed) instead, and
    /// the two arms are mutually exclusive (each press clears the other) so only
    /// one of `begin_drag` / the scrub thinks itself active. The
    /// capture-lock-vs-drag-session conflict itself is resolved by the RUNTIME,
    /// not this arm: `wants_pointer_capture()` is unconditionally `true`, so a
    /// handle press arms the capture lock too, and the router's drag-session >
    /// capture-lock precedence (input.rs) is what makes the reorder win.
    reorder_arm: Cell<Option<usize>>,
    /// R937 — the live drop **gap** (`0..=nrows`, "insert before visual row g")
    /// the in-flight reorder drag is hovering, `None` when no drag is active. A
    /// reactive `Signal` (not a `Cell`) so the view repaints the insertion line
    /// AND a `scene/snapshot` after a `scene/drag` move observes it (the AI-first
    /// witness of the drag); `drag_release` consumes + clears it.
    drag_preview: Rc<Signal<Option<usize>>>,
    /// R940 — the open choice dropdown's roving active descendant + pointer
    /// hover ([`use_popup_cursor`] / [`use_popup_hover`]). `Rc` clones of the
    /// view-shared Signals (the property-grid popup shape), so the coordinator's
    /// pointer / RPC path and the keyboard free-fn path drive one popup.
    popup_cursor: Rc<Signal<Option<usize>>>,
    popup_hover: Rc<Signal<Option<usize>>>,
    /// R1372 — the cell-range selection anchor ([`use_cell_anchor`]). An `Rc`
    /// clone of the view-shared Signal (the cursor's SOURCE-keyed peer), so the
    /// coordinator's RPC path and the keyboard free-fn path pin / extend / clear
    /// one selection rectangle.
    cell_anchor: Rc<Signal<Option<(usize, usize)>>>,
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
        undo: Rc<UndoStack>,
        undo_ctx: Rc<GridUndoCtx>,
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
            undo,
            undo_ctx,
            scrub_origin: RefCell::new(None),
            reorder_arm: Cell::new(None),
            drag_preview: use_drag_preview(),
            popup_cursor: use_popup_cursor(),
            popup_hover: use_popup_hover(),
            cell_anchor: use_cell_anchor(),
        }
    }

    /// R937 — whether a manual row drag-to-reorder is meaningful right now: only
    /// in the **plain source view** (no sort / filter / group), where the visual
    /// order IS the source order, so moving a row visually moves it in the source
    /// `Vec` 1:1. A sort / filter / group derives the visual order from the data,
    /// so a manual position would be ambiguous (Qt / Excel disable reorder under a
    /// sort proxy) — the handle is then painted blank + `begin_drag` returns
    /// `None`. Reads the three view-transform signals (subscribes in the view).
    fn reorder_enabled(&self) -> bool {
        plain_view(
            self.sort.get(),
            self.filter.get().as_ref(),
            self.group_col.get(),
        )
    }

    /// R1372 §5.38 — start a cell range at SOURCE `(row, col)`: move the cursor
    /// there and pin the anchor to it, so the selection is that single cell (the
    /// spreadsheet click / plain-arrow model — a fresh selection a later
    /// [`extend_cell`](Self::extend_cell) grows into a rectangle). Out-of-range is
    /// a silent no-op returning `false` (the model-path guard the RPC surfaces).
    fn select_cell(&self, row: usize, col: usize) -> bool {
        if row >= self.nrows() || col >= NCOLS {
            return false;
        }
        self.focused_row.set(row);
        self.focused_col.set(col);
        self.cell_anchor.set(Some((row, col)));
        true
    }

    /// R1372 §5.38 — extend the cell range to SOURCE `(row, col)`: move the cursor
    /// but keep the pinned anchor (the `Shift`-arrow model), so the selection is
    /// the bounding rectangle of the anchor and the new cursor over the visible
    /// order. With no anchor yet, the current cursor becomes the anchor first, so
    /// the first extension is a single cell subsequent ones grow (the R952
    /// `Table::extend_cell` contract). Out-of-range is a silent no-op.
    fn extend_cell(&self, row: usize, col: usize) -> bool {
        if row >= self.nrows() || col >= NCOLS {
            return false;
        }
        if self.cell_anchor.get().is_none() {
            self.cell_anchor
                .set(Some((self.focused_row.get(), self.focused_col.get())));
        }
        self.focused_row.set(row);
        self.focused_col.set(col);
        true
    }

    /// R1372 §5.38 — drop the cell range (clear the anchor). The roving cursor is
    /// untouched — clearing a selection leaves the active cell navigable /
    /// editable (the R952 `Table::clear_cell_selection` contract).
    fn clear_cell_selection(&self) {
        self.cell_anchor.set(None);
    }

    /// R1372 §5.38 — the selected cell rectangle in VISIBLE-position coords over
    /// the current view ([`cell_selection_bounds`]), or `None` when no multi-cell
    /// range is well-defined. The AI-first read of "which cells are selected".
    fn cell_selection(&self) -> Option<(usize, usize, usize, usize)> {
        cell_selection_bounds(
            &self.cur_visible(),
            self.cell_anchor.get(),
            self.focused_row.get(),
            self.focused_col.get(),
        )
    }

    /// R1372 §5.38 — the number of cells in the selected rectangle (`0` when no
    /// range is active). The `(rows x cols)` area of [`cell_selection`](Self::cell_selection).
    fn cell_selection_count(&self) -> usize {
        self.cell_selection()
            .map_or(0, |(p0, c0, p1, c1)| (p1 - p0 + 1) * (c1 - c0 + 1))
    }

    /// R1372 §5.38 — the selected cell rectangle serialized as TSV, VISIBLE-order
    /// (top row first, following the active sort / filter / group exactly as the
    /// grid reads AND as [`paste_block`](Self::paste_block) writes — so a copy
    /// round-trips through a paste). Reads each cell's display in visible order,
    /// then the shared [`rows_to_tsv`] codec (the R1222 `Table::selected_tsv`
    /// SSOT, lifted at this 2nd consumer) sanitizes + joins. `None` when no
    /// multi-cell range is active; a bare `Ctrl`+`C` copies the single focused
    /// cell instead (see [`copy_tsv`](Self::copy_tsv)).
    fn selected_tsv(&self) -> Option<String> {
        let model = self.model.get();
        let visible = self.cur_visible();
        let (p0, c0, p1, c1) = cell_selection_bounds(
            &visible,
            self.cell_anchor.get(),
            self.focused_row.get(),
            self.focused_col.get(),
        )?;
        let rows: Vec<Vec<String>> = (p0..=p1)
            .map(|pos| {
                let source = visible[pos];
                (c0..=c1).map(|c| model[idx(source, c)].display()).collect()
            })
            .collect();
        Some(rows_to_tsv(&rows))
    }

    /// R1372 §5.38 — the payload a `Ctrl`+`C` copies: the selected rectangle as
    /// TSV, or — when no range is active — the single FOCUSED cell's display (a
    /// bare copy of one cell, the universal spreadsheet default). Always yields
    /// something copyable when the grid has focus, so the clipboard is never
    /// written an empty string on a lone cursor.
    fn copy_tsv(&self) -> String {
        self.selected_tsv().unwrap_or_else(|| {
            let model = self.model.get();
            let (r, c) = (self.focused_row.get(), self.focused_col.get());
            model
                .get(idx(r, c))
                .map(CellValue::display)
                .unwrap_or_default()
        })
    }

    /// R1372 §5.38 — the cell-range selection reads, the cross-grid wire the
    /// Table widget (R952) / `hello-cell-select` (R1222) speak, so an AI client
    /// drives copy identically on every grid: `cell_selection` = the rectangle
    /// "pos0,col0,pos1,col1" (Null when no range), `cell_selection_count` = its
    /// area, `cell_selection_tsv` = the spreadsheet TSV block. The coords are
    /// VISIBLE positions (like `source_at.<pos>`) — the rectangle is defined over
    /// the view, not the source order `value.<row>.<col>` addresses. `None` for
    /// any other path (the [`query`](ExternalIntrospect::query) falls through).
    fn query_cell_selection(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "cell_selection" => Some(
                self.cell_selection()
                    .map_or(IntrospectValue::Null, |(p0, c0, p1, c1)| {
                        IntrospectValue::Text(format!("{p0},{c0},{p1},{c1}"))
                    }),
            ),
            "cell_selection_count" => {
                Some(IntrospectValue::Int(int_of(self.cell_selection_count())))
            }
            "cell_selection_tsv" => Some(
                self.selected_tsv()
                    .map_or(IntrospectValue::Null, IntrospectValue::Text),
            ),
            _ => None,
        }
    }

    /// R891 — rows passing the active filter (`NROWS` when unfiltered), the
    /// derived data-row count the AI-first `set_filter` reports in one
    /// round-trip. Independent of grouping / collapse (the logical filtered
    /// count, not the rendered row count — that is [`visible_len`](Self::visible_len)).
    fn view_len(&self) -> usize {
        current_order(
            &self.model.get(),
            self.sort.get(),
            self.filter.get().as_ref(),
        )
        .len()
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
    /// into the resulting view. R997 — facets on an out-of-range column are
    /// dropped, and a conjunction left empty clamps to unfiltered (mirrors
    /// `GridSortState::set_filter`). Returns the resulting
    /// [`view_len`](Self::view_len). The one mutation path the wire's
    /// `intervene "filter"` and `invoke "set_filter"` share.
    fn set_filter(&self, filter: Option<GridFilter>) -> usize {
        let filter = filter.and_then(|f| f.clamped_to_col_count(NCOLS));
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
        self.group_col
            .get()
            .and_then(|col| group_table(&self.model.get(), col).get(group).cloned())
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
        let Some(CellValue::Bool(b)) = self.model.get().get(idx(row, col)).cloned() else {
            return false;
        };
        // R932 — a toggle is a discrete committed edit, so it journals through
        // the shared `edit_cell` funnel (one undo step), re-anchoring the cursor
        // exactly as before.
        self.edit_cell(row, col, CellValue::Bool(!b), Cow::Borrowed("Toggle cell"));
        true
    }

    /// R932 — the JOURNALED discrete cell write: snapshot the cell + cursor,
    /// apply through the clamped [`set_cell`](Self::set_cell) funnel (which
    /// re-anchors), then push one [`SetCellEdit`]. The RPC `value.<row>.<col>`
    /// intervene and the keyboard bool [`toggle`](Self::toggle) commit through
    /// here, so every discrete edit is undoable and the AI write and the GUI
    /// write reverse identically. The live numeric scrub uses the raw funnel
    /// and journals once at [`end_scrub`](Self::end_scrub) — one drag is one
    /// undo step.
    fn edit_cell(&self, row: usize, col: usize, value: CellValue, label: Cow<'static, str>) {
        if row >= self.nrows() || col >= NCOLS {
            return;
        }
        let index = idx(row, col);
        let before = self.model.get()[index].clone();
        let before_cursor = self.focused_row.get();
        self.set_cell(row, col, value);
        let after = self.model.get()[index].clone();
        push_cell_edit(
            &self.undo,
            &self.undo_ctx,
            index,
            before,
            after,
            before_cursor,
            self.focused_row.get(),
            label,
        );
    }

    /// R1237 — paste a TSV block anchored at the cursor (the spreadsheet block
    /// paste; the AI-first `invoke paste "<tsv>"`). Rows split on `\n`, cells on
    /// `\t`; row `i` / col `j` of the block lands at the cell `j` columns right of
    /// the cursor in the `i`-th VISIBLE data row from the cursor down — so the
    /// paste follows the active sort / filter / group exactly as the grid reads,
    /// never the raw source order. The visible target rows are SNAPSHOTTED once
    /// (before any write), so a paste into the sort-key column can re-sort the
    /// grid mid-write without the remaining rows chasing a moving target. Each
    /// cell parses through its COLUMN's [`CellKind`] + clamps (a value that does
    /// not parse for that type is skipped — the cell keeps its prior value, no
    /// data loss). R1244 — a block that overruns the last visible row GROWS the
    /// grid: each overrun line that LANDS at least one cell appends a fresh row
    /// (the model's row count is dynamic, [`add_row`](Self::add_row)) so the whole
    /// block lands. R1247 — an all-unparseable overrun line grows NOTHING (it
    /// mirrors the in-range no-op), so a paste never leaves a phantom empty row
    /// and the returned count never hides a growth. Columns past the grid's right
    /// edge still CLIP — the column schema ([`NCOLS`]) is fixed, unlike the rows.
    /// The whole paste (grown rows AND their cells) is ONE undo step (a
    /// `begin_macro` / `end_macro` transaction), so a single `Ctrl`+`Z` reverts
    /// every pasted cell and every row it grew. Returns the count of cells written.
    fn paste_block(&self, tsv: &str) -> usize {
        // R1239 — strip ONE trailing line terminator (`\n` or `\r\n`) before
        // splitting: real clipboards / spreadsheet copies emit a trailing
        // newline, and `"X\n".split('\n')` yields a phantom `""` row that would
        // overwrite the row below the block (an empty string parses as a valid
        // `Text` cell — silent data loss). Interior blank rows are still honored.
        let tsv = tsv.strip_suffix('\n').unwrap_or(tsv);
        let tsv = tsv.strip_suffix('\r').unwrap_or(tsv);
        if tsv.is_empty() {
            return 0;
        }
        // The current visible source-row order (through the `cur_visible` SSOT the
        // cursor re-anchor also reads), SNAPSHOTTED once so a write into the
        // sort-key column cannot make the remaining rows chase a moving target.
        let visible = self.cur_visible();
        // R1239 — anchor at the cursor's VISIBLE position; if the cursor's source
        // row is off-view (hidden by a filter / collapsed group), paste is a no-op
        // rather than silently dumping the block at visible row 0 (the
        // `cursor_visual_pos` `0`-fallback is for re-anchor callers, not a write
        // anchor).
        let Some(anchor_pos) = visible.iter().position(|&s| s == self.focused_row.get()) else {
            return 0;
        };
        let anchor_col = self.focused_col.get();
        self.undo.begin_macro("Paste");
        let mut written = 0;
        for (i, line) in tsv.split('\n').enumerate() {
            // R1247 — precompute the cells this line will actually LAND, BEFORE any
            // row grows: clip columns past the fixed schema (`take_while col <
            // NCOLS`, the rows-dynamic / columns-fixed split), then keep only cells
            // that parse for their column (an unparseable cell is skipped, keeping
            // the prior value — no data loss). Computing this first is what lets an
            // all-unparseable OVERRUN line grow NOTHING (mirroring the in-range
            // no-op); R1244 grew a phantom empty row per overrun line regardless of
            // whether any cell landed, and reported a `written` count that hid it.
            let landed: Vec<(usize, CellValue)> = line
                .split('\t')
                .enumerate()
                .map(|(j, text)| (anchor_col + j, text))
                .take_while(|&(col, _)| col < NCOLS)
                .filter_map(|(col, text)| COL_KINDS[col].parse(text).map(|v| (col, v)))
                .collect();
            // R1244 — an in-range visible row writes in place (the snapshot); an
            // overrun row GROWS the grid, but ONLY when it lands data. A grown row
            // writes by its known appended source index (never a visible-position
            // lookup, so a mid-paste re-sort cannot make it chase a moving target).
            let source_row = match visible.get(anchor_pos + i) {
                Some(&s) => s,
                None if landed.is_empty() => continue, // no data -> no phantom row
                None => self.append_default_row(false, Cow::Borrowed("Paste row")),
            };
            for (col, value) in landed {
                self.edit_cell(source_row, col, value, Cow::Borrowed("Paste cell"));
                written += 1;
            }
        }
        self.undo.end_macro();
        written
    }

    /// R930 — the live row count (the [`nrows`] free fn over this grid's model
    /// Signal). The dynamic-length bound every former `NROWS` read in the
    /// coordinator now consults, so add / remove track the same SSOT the paint
    /// and order derive from.
    fn nrows(&self) -> usize {
        nrows(&self.model.get())
    }

    /// R930 — append one default row (`NCOLS` typed empty cells) at the end and
    /// move the cursor onto it. Returns the new row's source index. The cell
    /// model IS the SSOT, so the very next paint re-derives the order / filter /
    /// group over the longer model — no separate row-count field to keep in
    /// sync.
    fn add_row(&self) -> usize {
        self.append_default_row(true, Cow::Borrowed("Add row"))
    }

    /// R1244 — the shared append core behind [`add_row`](Self::add_row) (the
    /// explicit "add a row" gesture) and [`paste_block`](Self::paste_block)'s
    /// auto-grow (append rows to fit an overrunning block): extend the model by
    /// one [`default_row`] and journal it as one reversible [`RowEdit`] (already
    /// applied — redo re-inserts the seed cells, undo drains them), returning the
    /// new source index. `move_cursor` gates the R930.1 cursor hop onto the fresh
    /// row: `add_row` wants the cursor there; a paste keeps it at the paste
    /// anchor (a per-row hop mid-paste would strand the cursor). An append never
    /// hides an existing row, so a moved cursor stays visible.
    fn append_default_row(&self, move_cursor: bool, label: Cow<'static, str>) -> usize {
        let new_row = self.nrows();
        let at = idx(new_row, 0);
        let before_cursor = self.focused_row.get();
        let cells = default_row();
        self.model.set_with({
            let cells = cells.clone();
            move |prev| {
                let mut next = prev.clone();
                next.extend(cells);
                next
            }
        });
        if move_cursor && self.cur_visible().contains(&new_row) {
            self.focused_row.set(new_row);
        }
        self.undo.push_applied(RowEdit {
            ctx: Rc::clone(&self.undo_ctx),
            at,
            removed: Vec::new(),
            inserted: cells,
            before_cursor,
            after_cursor: self.focused_row.get(),
            label,
        });
        new_row
    }

    /// R930 — drop source row `row` (its whole `NCOLS`-cell span) and clamp the
    /// cursor into the shrunk model. A no-op `false` for an out-of-range row or
    /// when removing the last row (a grid keeps at least one row so the cursor
    /// and the column header layout always have somewhere to land). Source
    /// indices above `row` shift down by one — the model is the order SSOT, so
    /// the next paint re-derives every view position.
    fn remove_row(&self, row: usize) -> bool {
        if row >= self.nrows() || self.nrows() <= 1 {
            return false;
        }
        // R930.1 — a structural row change invalidates the source-keyed
        // `(row, col)` edit latch (the removed row is gone; rows above it
        // shift down), so cancel any in-flight edit — otherwise a later
        // `commit_edit` would write a stale, now-out-of-range index.
        self.editing_cell.set(None);
        let at = idx(row, 0);
        let before_cursor = self.focused_row.get();
        // R932 — snapshot the dropped row's cells BEFORE the drain so undo can
        // re-insert them verbatim (granular row capture, not a model snapshot).
        let removed: Vec<CellValue> = self.model.get()[at..at + NCOLS].to_vec();
        // R930.1 — capture the cursor's visible slot BEFORE the drain, then
        // re-anchor after, exactly like every other mutator (set_cell /
        // set_filter / …). A bare clamp left the cursor on a filtered-out row
        // (dead arrow-nav); `reanchor` keeps it on the visible row now at its
        // prior screen slot.
        let prior_vis = self.cursor_prior_vis();
        self.model.set_with(move |prev| {
            let mut next = prev.clone();
            next.drain(at..at + NCOLS);
            next
        });
        self.reanchor(prior_vis);
        // R932 — journal the delete as one RowEdit (already applied): redo
        // drains the row again, undo re-inserts the captured cells.
        self.undo.push_applied(RowEdit {
            ctx: Rc::clone(&self.undo_ctx),
            at,
            removed,
            inserted: Vec::new(),
            before_cursor,
            after_cursor: self.focused_row.get(),
            label: Cow::Borrowed("Remove row"),
        });
        true
    }

    /// R937 — the removal-shift: a drop GAP `g` (`0..=nrows`, "insert before
    /// visual row g") becomes the moved row's resting index once `from` is
    /// removed — `g - 1` when the gap is past `from` (removal shifted everything
    /// after it down one), else `g`. The flat-model peer of the off-by-one in
    /// `ReorderModel::apply_move` (gap → resting index).
    fn gap_to_index(from: usize, gap: usize) -> usize {
        if gap > from { gap - 1 } else { gap }
    }

    /// R937 — apply + journal a row move from source row `from` to resting index
    /// `to` (both validated), recording ONE [`MoveRowEdit`]. A no-op `false` when
    /// `from == to` or either is out of range. The ONE funnel the drag release,
    /// the `move_row` RPC, and the keyboard Alt+Arrow all push through — a reorder
    /// is the same journaled mutation regardless of input (cf. [`push_cell_edit`]).
    /// The moved row follows the cursor to `to` (the grabbed row stays focused —
    /// Excel / Qt drag keeps the dragged row selected); since reorder is enabled
    /// only in the plain view, `to` is always visible, and undo restores the prior
    /// cursor through [`GridUndoCtx::restore`].
    fn move_row(&self, from: usize, to: usize) -> bool {
        let n = self.nrows();
        if from >= n || to >= n || from == to {
            return false;
        }
        // R930.1 — a structural move shifts source indices, so cancel any
        // in-flight edit latch (a stale `(row, col)` would commit into the
        // wrong row), exactly like `remove_row`.
        self.editing_cell.set(None);
        let before_cursor = self.focused_row.get();
        self.model.set_with(move |prev| {
            let mut next = prev.clone();
            move_block(&mut next, from, to);
            next
        });
        self.focused_row.set(to);
        self.undo.push_applied(MoveRowEdit {
            ctx: Rc::clone(&self.undo_ctx),
            from,
            to,
            before_cursor,
            after_cursor: to,
            label: Cow::Borrowed("Move row"),
        });
        true
    }

    /// R937 — move source row `from` to drop GAP `gap`, the drag-release funnel
    /// (the gesture reports a gap; [`move_row`](Self::move_row) takes a resting
    /// index, so [`gap_to_index`](Self::gap_to_index) removal-shifts between them).
    fn move_row_to_gap(&self, from: usize, gap: usize) -> bool {
        self.move_row(from, Self::gap_to_index(from, gap))
    }

    /// R937 — decode the drop GAP from a [`DropPoint`]: the target row (from the
    /// hovered handle `d<r>` or cell `<r>_<c>` sub-tag) plus its top / bottom half
    /// (`y_rel`) → insert before (`row`) or after (`row + 1`). `None` when the
    /// hovered tag is not a row (e.g. the header or off the rows) — the caller
    /// then holds the last preview (the `hello-dnd` no-snap-over-gaps behaviour).
    fn drop_gap(&self, over: Option<&DropPoint>) -> Option<usize> {
        let p = over?;
        let sub = split_subindex(&p.tag).1?;
        let row =
            parse_handle_sub(sub).or_else(|| GridSendKey::parse(sub).and_then(GridSendKey::row))?;
        (row < self.nrows()).then(|| row + usize::from(p.y_rel >= 0.5))
    }

    /// R937 — arm a row drag-to-reorder from a handle `PointerDown`: record the
    /// source row, clear any cell-scrub arm (the two are mutually exclusive), and
    /// focus the grabbed row so a subsequent move keeps it selected. The router
    /// then asks [`begin_drag`](Self::begin_drag), which honours this arm only in
    /// the plain (reorder-enabled) view.
    fn arm_reorder(&self, row: usize) {
        self.scrub_armed.set(None);
        self.reorder_arm.set(Some(row));
        self.focused_row
            .set(row.min(self.nrows().saturating_sub(1)));
    }

    /// R894 / R914 — write a typed value into cell `(row, col)`, clamped to the
    /// column's [`ColRange`] and re-anchoring the cursor. The one funnel the AI
    /// `value.<row>.<col>` intervene write and the live numeric scrub both
    /// commit through, so a drag cannot exceed a bound a programmatic set
    /// cannot (the R894 keyboard / RPC symmetry, now extended to the scrub). A
    /// no-op for an out-of-range cell.
    fn set_cell(&self, row: usize, col: usize, value: CellValue) {
        if row >= self.nrows() || col >= NCOLS {
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

    /// R960 — `true` when cell `(row, col)` differs from its column default
    /// ([`col_default`], via the [`CellValue::value_eq`] NaN-safe equality the
    /// inspector / property-grid modified indicators also use). The single
    /// predicate the per-cell reset dot, the `modified.<row>.<col>` query, and
    /// `reset_cell` all read, so the paint and the wire cannot disagree.
    fn cell_modified(&self, row: usize, col: usize) -> bool {
        cell_value_modified(&self.model.get(), row, col)
    }

    /// R960 — reset cell `(row, col)` to its column default through the
    /// [`set_cell`](Self::set_cell) funnel (so a reset that changes the sort key
    /// re-sorts + re-anchors exactly like an edit). Returns whether it was
    /// modified (the setter-returns-the-read-outcome contract; an already-default
    /// cell is a `false` no-op).
    fn reset_cell(&self, row: usize, col: usize) -> bool {
        if !self.cell_modified(row, col) {
            return false;
        }
        self.set_cell(row, col, col_default(col));
        true
    }

    /// R965 — the batched-reset SSOT. Reset every *modified* cell among
    /// `targets` to its column default, returning the count cleared.
    /// `reset_all` / `reset_row` / `reset_col` differ ONLY in which cells they
    /// scan, so the one-snapshot + in-place reset + single commit lives here
    /// (the [[three-site-internal-duplication-substrate-lift]] of the three
    /// bulk resets).
    ///
    /// R961.1 — **single batched pass**: snapshot the model once, reset in
    /// place, then do ONE `set` + ONE `reanchor`. The R960 first cut looped
    /// `reset_cell` → [`set_cell`](Self::set_cell) per cell, and each `set_cell`
    /// clones the whole model + runs two `cur_visible` sort passes + a repaint —
    /// so a bulk reset was O(cells²) allocation + a per-cell repaint storm (the
    /// R958.1 "hidden per-item walk" cost). The per-column [`col_default`] is
    /// hoisted once into a `[CellValue; NCOLS]` so a whole-grid reset does NOT
    /// recompute it per cell either (the R961.1 hoist, preserved through the
    /// lift rather than dropped back to a per-cell call).
    fn reset_cells(&self, targets: impl Iterator<Item = (usize, usize)>) -> usize {
        let defaults: [CellValue; NCOLS] = std::array::from_fn(col_default);
        let mut cells = self.model.get();
        let mut cleared = 0;
        for (row, col) in targets {
            let i = idx(row, col);
            if cells.get(i).is_some_and(|v| !v.value_eq(&defaults[col])) {
                cells[i] = defaults[col].clone();
                cleared += 1;
            }
        }
        if cleared > 0 {
            let prior_vis = self.cursor_prior_vis();
            self.model.set(cells);
            self.reanchor(prior_vis);
        }
        cleared
    }

    /// R960 / R961.1 — reset every modified cell to its column default, returning
    /// the count cleared. Delegates to the [`reset_cells`](Self::reset_cells)
    /// batched SSOT, scanning the whole grid.
    fn reset_all(&self) -> usize {
        let rows = self.nrows();
        self.reset_cells((0..NCOLS).flat_map(|col| (0..rows).map(move |row| (row, col))))
    }

    /// R965 — reset every modified cell in `row` to its column default (the
    /// bulk-reset behind the Qt / Excel "reset this row" — exposed here as the
    /// `reset_row` RPC verb; a header / context-menu control is a follow-up,
    /// R965.1 honesty), returning the count cleared. A no-op (0) for an
    /// out-of-range row. One batched pass via `reset_cells`.
    fn reset_row(&self, row: usize) -> usize {
        if row >= self.nrows() {
            return 0;
        }
        self.reset_cells((0..NCOLS).map(move |col| (row, col)))
    }

    /// R965 — reset every modified cell in `col` (all rows) to its column default
    /// (the "reset this column" bulk-reset, exposed as the `reset_col` RPC verb;
    /// a header control is a follow-up), returning the count cleared. A no-op (0)
    /// for an out-of-range column. One batched pass via `reset_cells`.
    fn reset_col(&self, col: usize) -> usize {
        if col >= NCOLS {
            return 0;
        }
        let rows = self.nrows();
        self.reset_cells((0..rows).map(move |row| (row, col)))
    }

    /// R960 — how many cells differ from their column default (the `reset_all`
    /// would-clear count, exposed as a query for the AI-first / demo path).
    ///
    /// R961.1 — one model snapshot then an in-place scan (the R960 first cut
    /// cloned the whole model per cell via `cell_modified`).
    fn modified_count(&self) -> usize {
        let cells = self.model.get();
        let rows = cells.len() / NCOLS;
        (0..NCOLS)
            .map(|col| {
                let def = col_default(col);
                (0..rows)
                    .filter(|&row| !cells[idx(row, col)].value_eq(&def))
                    .count()
            })
            .sum()
    }

    /// R960 / R966 — the dynamic `value.<row>.<col>` cell read + its modified-
    /// from-default predicate peers at cell / column / row granularity
    /// (`modified.<row>.<col>` / `col_modified.<col>` / `row_modified.<row>`).
    /// Split out of [`query`](ExternalIntrospect::query) so neither overflows
    /// the `too_many_lines` budget (the R959 `invoke_send` extraction precedent).
    /// `None` for an unknown path / malformed index; a well-formed out-of-range
    /// index reads through the underlying predicate (`false` for the aggregates,
    /// `None` for a missing cell).
    fn query_cell_path(&self, path: &str) -> Option<IntrospectValue> {
        if let Some(rest) = path.strip_prefix("value.") {
            let (row_str, col_str) = rest.split_once('.')?;
            let row: usize = row_str.parse().ok()?;
            let col: usize = col_str.parse().ok()?;
            let model = self.model.get();
            return model.get(idx(row, col)).map(CellValue::to_introspect);
        }
        // R960 — `modified.<row>.<col>` → does the cell differ from its column
        // default (the `value.<row>.<col>` predicate peer).
        if let Some(rest) = path.strip_prefix("modified.") {
            let (row_str, col_str) = rest.split_once('.')?;
            let row: usize = row_str.parse().ok()?;
            let col: usize = col_str.parse().ok()?;
            return Some(IntrospectValue::Bool(self.cell_modified(row, col)));
        }
        // R966 — `col_modified.<col>` / `row_modified.<row>` → does ANY cell in
        // that column / row differ from its column default (the header reset
        // dot's AI-first read peer, the 1-D aggregate of `modified.<row>.<col>`).
        if let Some(col_str) = path.strip_prefix("col_modified.") {
            let col: usize = col_str.parse().ok()?;
            return Some(IntrospectValue::Bool(col_modified(&self.model.get(), col)));
        }
        if let Some(row_str) = path.strip_prefix("row_modified.") {
            let row: usize = row_str.parse().ok()?;
            return Some(IntrospectValue::Bool(row_modified(&self.model.get(), row)));
        }
        None
    }

    /// R914 — arm a numeric cell scrub: a `PointerDown` over an Int / Float
    /// column records the cell so the first capture `pointer_move` calibrates.
    /// A press on a non-numeric (or out-of-range) cell leaves the arm clear (it
    /// never scrubs — text cells edit, bool cells toggle).
    fn arm_scrub(&self, row: usize, col: usize) {
        // R917 — a fresh press starts a fresh calibration: drop any prior anchor
        // so the scrub is self-contained and never inherits a stale base/source
        // from a drag whose release was missed (the R51.34 capture lock pairs
        // one release per press, so this is unreachable today — but arming a new
        // scrub should not depend on that contract holding).
        self.scrub_cal.end();
        // R937 — a cell press and a handle press arm mutually-exclusive gestures
        // (cell scrub vs row reorder); clear the reorder arm so a stale handle
        // press cannot turn this cell press into a drag.
        self.reorder_arm.set(None);
        let numeric = col < NCOLS && matches!(COL_KINDS[col], CellKind::Int | CellKind::Float);
        let armed = (numeric && row < self.nrows()).then_some((row, col));
        self.scrub_armed.set(armed);
        // R932 — snapshot the armed cell's value + cursor at press, BEFORE any
        // dead-zone move mutates, so `end_scrub` can journal the whole drag as
        // ONE undo step (the cell value at press IS the `before`, since the
        // dead-zone moves write nothing). Cleared when the press is not on a
        // scrubbable cell.
        *self.scrub_origin.borrow_mut() = armed.map(|(r, c)| {
            (
                r,
                c,
                self.model.get()[idx(r, c)].clone(),
                self.focused_row.get(),
            )
        });
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
                Some(CellValue::Int(i)) => Some(ScrubCell {
                    row,
                    col,
                    kind: CellKind::Int,
                    base: *i as f64,
                }),
                Some(CellValue::Float(f)) => Some(ScrubCell {
                    row,
                    col,
                    kind: CellKind::Float,
                    base: *f,
                }),
                // Nothing armed, or the armed cell is no longer numeric.
                _ => None,
            }
        }) else {
            return;
        };
        // R915 — a sub-threshold press is a click, not a scrub: stay in the dead
        // zone (no mutation) until the cursor strays past DRAG_CLICK_THRESHOLD_PX
        // (the framework click/drag SSOT), so a plain click focuses the cell
        // instead of nudging its value.
        if !self.is_scrubbing() {
            return;
        }
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

    /// R914 / R915 — tear the scrub down at release. Returns whether a real
    /// scrub ran (the cursor strayed past the click dead zone), so `PointerUp`
    /// can suppress the click action: a scrub must not also focus / toggle the
    /// cell. A sub-threshold press returns `false` — it was a click, so the
    /// release focuses the cell as usual.
    fn end_scrub(&self) -> bool {
        self.scrub_armed.set(None);
        let was_scrub = self.is_scrubbing();
        self.scrub_cal.end();
        // R932 — journal the whole drag as ONE SetCellEdit (the node editor's
        // "one move per gesture at release" rule): the live scrub wrote through
        // the no-journal funnel, so here we record the press→release delta.
        // `before` is the press snapshot, `after` the current value; a sub-
        // threshold press (a click, `was_scrub` false) journals nothing, and a
        // net-zero drag (dragged back to base) is filtered by `push_cell_edit`.
        if let Some((row, col, before, before_cursor)) = self.scrub_origin.borrow_mut().take() {
            if was_scrub {
                let index = idx(row, col);
                let after = self.model.get()[index].clone();
                push_cell_edit(
                    &self.undo,
                    &self.undo_ctx,
                    index,
                    before,
                    after,
                    before_cursor,
                    self.focused_row.get(),
                    Cow::Borrowed("Scrub cell"),
                );
            }
        }
        was_scrub
    }

    /// R914 / R915 — whether a *real* numeric cell scrub is live: the press has
    /// strayed past `DRAG_CLICK_THRESHOLD_PX` of travel across the grid basis
    /// (`GRID_VIEWPORT_W`). The one decision the scrub mutation gate, the
    /// click-suppression at release, and the AI-first `scrubbing` query share —
    /// a calibrated-but-still-within-the-dead-zone press is a click, not a scrub.
    fn is_scrubbing(&self) -> bool {
        self.scrub_cal
            .traveled_beyond(f64::from(GRID_VIEWPORT_W), DRAG_CLICK_THRESHOLD_PX)
    }

    /// R837 / R914 — route a composite cell `send` event to the cell at
    /// `(row, col)`. `PointerDown` arms a numeric scrub (the first capture
    /// `pointer_move` calibrates it); `PointerUp` ends the scrub and, if no
    /// drag ran, focuses the cell (and toggles a bool); `PointerLeave` /
    /// `PointerCancel` tear a strayed-off scrub down; `DoubleClick` edits an
    /// editable cell.
    /// Route a `send` composite payload (`"<sub>:<Event>[:<mods>]"`) to its
    /// target. R937 — a handle sub-key (`d<row>`) arms a row drag-to-reorder
    /// before the shared [`GridSendKey`] grammar (header / cell / group); the
    /// handle is data-grid-local, so it is decoded here, not in the shared SSOT
    /// (R773). Split out of [`invoke`](ExternalIntrospect::invoke) so that arm
    /// stays under the line budget.
    fn dispatch_send(&mut self, s: &str) -> Result<IntrospectValue, InvokeError> {
        // R880.1 — the `split_send_payload` `:` grammar SSOT strips a held-modifier
        // third segment (a hand-rolled split read "PointerUp:c" as the event name).
        let (key, event_name, _mods) =
            pinion_core::composite_tag::split_send_payload(s).ok_or(InvokeError::Rejected)?;
        // R940 — the choice dropdown's light-dismiss barrier + option targets,
        // routed back through this one `send` funnel (the property-grid popup
        // shape): `dismiss` closes the popup, `opt<i>` commits / hovers option i.
        if key == "dismiss" {
            if event_name == "PointerUp" {
                self.close_popup();
            }
            return Ok(IntrospectValue::Null);
        }
        if let Some(opt) = key.strip_prefix(CHOICE_OPT_PREFIX) {
            let i: usize = opt.parse().map_err(|_| InvokeError::Rejected)?;
            if event_name == "PointerUp" {
                self.commit_choice(i);
            } else {
                self.set_popup_hover(event_name, i);
            }
            return Ok(IntrospectValue::Null);
        }
        // R943 — a colour swatch chip (`sw<i>`) commits / hovers preset `i`, the
        // `opt<i>` peer for the swatch palette. `strip_prefix(COLOR_SW_PREFIX)`
        // is tried AFTER `opt` so the two prefixes never alias.
        if let Some(sw) = key.strip_prefix(COLOR_SW_PREFIX) {
            let i: usize = sw.parse().map_err(|_| InvokeError::Rejected)?;
            if event_name == "PointerUp" {
                self.commit_color_swatch(i);
            } else {
                self.set_popup_hover(event_name, i);
            }
            return Ok(IntrospectValue::Null);
        }
        if let Some(row) = parse_handle_sub(key) {
            if event_name == "PointerDown" && row < self.nrows() {
                self.arm_reorder(row);
            }
            return Ok(IntrospectValue::Null);
        }
        // R960 cell + R966 row / column — a click on a reset dot resets the
        // addressed cell / row / column to the column default(s). The
        // `reset`-prefixed key is data-grid-local (decoded before the shared
        // `GridSendKey` grammar, like the `d<row>` handle above) through the
        // shared [`ResetTarget`] grammar (the cell case reuses
        // `GridSendKey::Cell`, so the `<row>_<col>` address is not re-derived).
        if let Some(rest) = key.strip_prefix(RESET_PREFIX) {
            if event_name == "PointerUp" {
                match ResetTarget::parse(rest) {
                    Some(ResetTarget::Cell { row, col }) => {
                        self.reset_cell(row, col);
                    }
                    Some(ResetTarget::Row { row }) => {
                        self.reset_row(row);
                    }
                    Some(ResetTarget::Col { col }) => {
                        self.reset_col(col);
                    }
                    None => {}
                }
            }
            return Ok(IntrospectValue::Null);
        }
        match GridSendKey::parse(key).ok_or(InvokeError::Rejected)? {
            // R886 — a clicked column header cycles that column's sort through the
            // `cycle_col_sort` SSOT (unsorted → asc → desc → unsorted; a different
            // column jumps to it ascending), exactly the read-only grids' behaviour.
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
            // R892 — a clicked group header toggles that group's collapse (the
            // `GridSendKey::Group` wire, parallel to the column-header sort cycle).
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

    fn handle_cell_send(
        &self,
        row: usize,
        col: usize,
        event_name: &str,
    ) -> Result<IntrospectValue, InvokeError> {
        if row >= self.nrows() || col >= NCOLS {
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
                // R940 / R943 — a click on a choice / colour cell opens its
                // popup (the bool cell's single-click-toggle peer: both activate
                // the cell's primary action). `toggle` no-ops on a non-bool, so a
                // text / numeric cell click just focuses (the prior behaviour).
                match COL_KINDS[col] {
                    CellKind::Choice => {
                        self.open_choice(row, col);
                    }
                    CellKind::Color => {
                        self.open_color(row, col);
                    }
                    _ => {
                        self.toggle(row, col);
                    }
                }
                Ok(IntrospectValue::Null)
            }
            // R914 — the capture lock lets the cursor stray off the cell; a
            // release there arrives as PointerLeave / PointerCancel. Tear the
            // scrub down (the value is already committed).
            "PointerLeave" | "PointerCancel" => {
                self.end_scrub();
                Ok(IntrospectValue::Null)
            }
            // R940 / R943 — a choice / colour cell opens its popup (neither is
            // text-editable, so `begin_edit` would reject it); other kinds enter
            // inline edit.
            "DoubleClick" => Ok(IntrospectValue::Bool(match COL_KINDS[col] {
                CellKind::Choice => self.open_choice(row, col),
                CellKind::Color => self.open_color(row, col),
                _ => self.begin_edit(row, col),
            })),
            _ => Ok(IntrospectValue::Null),
        }
    }

    /// Enter edit mode on `(row, col)`: latch the cell, seed the shared
    /// editor with the formatted value (caret parked at the trailing edge),
    /// and request focus into the field. Returns `false` for a bool column
    /// (bools toggle) or an out-of-range cell.
    fn begin_edit(&self, row: usize, col: usize) -> bool {
        if row >= self.nrows() || col >= NCOLS || !COL_KINDS[col].is_text_editable() {
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

    /// R940 — open the choice dropdown on `(row, col)`: focus the cell, latch
    /// it (the [`use_editing_cell`] latch — a choice cell editing means the
    /// dropdown is open, no inline field), and seed the keyboard cursor at the
    /// committed option (so arrows start from the current value). Focus stays on
    /// the grid: the popup is the grid's roving active descendant, not a
    /// separate Tab stop (unlike the inline text field). Returns `false` for a
    /// non-choice column or an out-of-range / non-choice cell.
    fn open_choice(&self, row: usize, col: usize) -> bool {
        if row >= self.nrows() || col >= NCOLS || COL_KINDS[col] != CellKind::Choice {
            return false;
        }
        let model = self.model.get();
        let Some(CellValue::Choice { selected, .. }) = model.get(idx(row, col)) else {
            return false;
        };
        self.focused_row.set(row);
        self.focused_col.set(col);
        self.editing_cell.set(Some((row, col)));
        self.popup_cursor.set(Some(*selected));
        self.popup_hover.set(None);
        true
    }

    /// R940 — commit option `i` into the open choice cell, then close the popup.
    /// The pointer (option click), keyboard (`Enter` / `Space`), and RPC
    /// (`choose`) commit path. Writes through the JOURNALED `edit_cell` (the
    /// VALUE-level [`CellValue::with_intervene`] — one undo step, byte-identical
    /// to the RPC `intervene value` path), so a dropdown pick re-anchors + undoes
    /// like every other cell edit. `false` when no popup is open, the editing
    /// cell is not a choice, or `i` is out of the option range.
    fn commit_choice(&self, i: usize) -> bool {
        let Some((row, col)) = self.editing_cell.get() else {
            return false;
        };
        if col >= NCOLS || COL_KINDS[col] != CellKind::Choice {
            return false;
        }
        let Some(current) = self.model.get().get(idx(row, col)).cloned() else {
            return false;
        };
        let Ok(next) = current.with_intervene(IntrospectValue::Int(int_of(i))) else {
            // An out-of-range index commits nothing; close to avoid a stuck
            // popup (the keyboard cursor is always in range, so this is the
            // defensive RPC `choose <bad>` path).
            self.close_popup();
            return false;
        };
        self.edit_cell(row, col, next, Cow::Borrowed("Edit cell"));
        self.close_popup();
        true
    }

    /// R943 — open the colour swatch popup on `(row, col)`: focus + latch the
    /// cell (the shared [`use_editing_cell`] latch — a colour cell editing means
    /// the swatch palette is open, no inline field, since a colour is not
    /// [`CellKind::is_text_editable`]) and seed the keyboard cursor at the preset
    /// matching the current colour (or 0 for an off-palette hex). Focus stays on
    /// the grid: the swatch grid is its roving active descendant. Returns `false`
    /// for a non-colour column or an out-of-range / non-colour cell. The
    /// [`open_choice`](Self::open_choice) peer.
    fn open_color(&self, row: usize, col: usize) -> bool {
        if row >= self.nrows() || col >= NCOLS || COL_KINDS[col] != CellKind::Color {
            return false;
        }
        let model = self.model.get();
        let Some(CellValue::Color(c)) = model.get(idx(row, col)) else {
            return false;
        };
        let cursor = COLOR_SWATCHES
            .iter()
            .position(|(sw, _)| sw == c)
            .unwrap_or(0);
        self.focused_row.set(row);
        self.focused_col.set(col);
        self.editing_cell.set(Some((row, col)));
        self.popup_cursor.set(Some(cursor));
        self.popup_hover.set(None);
        true
    }

    /// R943 — commit preset swatch `i` into the open colour cell, then close the
    /// popup (the swatch click + RPC `pick_color` + keyboard path). Writes the
    /// preset colour through the JOURNALED `edit_cell` (one undo step, the same
    /// path the dropdown pick and every cell edit take), so a swatch pick
    /// re-anchors and undoes like any other edit. `false` when no popup is open,
    /// the editing cell is not a colour, or `i` is out of the palette range (the
    /// defensive RPC `pick_color <bad>` path closes to avoid a stuck popup). The
    /// [`commit_choice`](Self::commit_choice) peer.
    fn commit_color_swatch(&self, i: usize) -> bool {
        let Some((row, col)) = self.editing_cell.get() else {
            return false;
        };
        if col >= NCOLS || COL_KINDS[col] != CellKind::Color {
            return false;
        }
        let Some(&(color, _)) = COLOR_SWATCHES.get(i) else {
            self.close_popup();
            return false;
        };
        self.edit_cell(
            row,
            col,
            CellValue::Color(color),
            Cow::Borrowed("Edit cell"),
        );
        self.close_popup();
        true
    }

    /// R940 — close the choice dropdown without committing (the dismiss-barrier
    /// click, the keyboard `Escape`, and the RPC `close_popup` path). The one
    /// teardown the [`clear_popup`] SSOT performs over the coordinator's `Rc`
    /// holders.
    fn close_popup(&self) {
        clear_popup(&self.editing_cell, &self.popup_cursor, &self.popup_hover);
    }

    /// R940 — set / clear the dropdown's pointer hover (the `PointerEnter` /
    /// `PointerLeave` handler for the option rows).
    fn set_popup_hover(&self, event_name: &str, i: usize) {
        match event_name {
            "PointerEnter" => self.popup_hover.set(Some(i)),
            "PointerLeave" => {
                if self.popup_hover.get() == Some(i) {
                    self.popup_hover.set(None);
                }
            }
            _ => {}
        }
    }

    /// R940 / R941.1 — whether a choice dropdown is open **and visible**: the
    /// edit latch is on a [`CellKind::Choice`] cell whose row is present in the
    /// current flatten (not filtered / collapsed out). Routes through [`popup_pos`]
    /// — the SAME predicate the keyboard-keymap intercept, the popup paint, and
    /// the a11y gate use (those via [`popup_pos_live`], the hook-access peer over
    /// the identical [`rows`](Self::rows) flatten). So there is genuinely ONE gate
    /// and the `popup_open` query is consistent with the painted scene (no
    /// "open but no panel" divergence the session-review caught). The raw latch
    /// (still set when its row scrolls / collapses out of view) stays observable
    /// via the `editing_row` / `editing_col` queries.
    fn popup_open(&self) -> bool {
        popup_pos(self.editing_cell.get(), &self.rows()).is_some()
    }

    fn set_focused_row_clamped(&self, row: usize) {
        self.focused_row
            .set(row.min(self.nrows().saturating_sub(1)));
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
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
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

    /// R937 — arm a row drag-to-reorder. Returns a payload (so the router opens a
    /// drag session that drives `drag_to` / `drag_release`) ONLY when a handle
    /// press armed [`reorder_arm`](Self::reorder_arm) AND the view is plain
    /// (reorder-enabled); a cell press leaves the arm clear, so `begin_drag`
    /// returns `None` and the capture-lock scrub path runs unchanged. The router
    /// drives the drag session in preference to the capture lock (input.rs), so
    /// the two gestures never both fire for one press.
    fn begin_drag(&self) -> Option<DragPayload> {
        if !self.reorder_enabled() {
            return None;
        }
        let from = self.reorder_arm.get()?;
        Some(DragPayload {
            kind: Cow::Borrowed(REORDER_KIND),
            value: IntrospectValue::Int(i64::try_from(from).ok()?),
        })
    }

    /// R937 — refresh the live drop-gap preview as the drag rides over rows. Over
    /// a row, snap to its before / after gap; over no row (the header, off the
    /// rows), hold the last preview (no snapping over inter-row gaps — the
    /// `hello-dnd` / `hello-tree-reparent` hold-last behaviour).
    fn drag_to(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        if let Some(gap) = self.drop_gap(over.as_ref()) {
            self.drag_preview.set(Some(gap));
        }
    }

    /// R937 — commit the row move at release: the dragged source row (the armed
    /// row) moves to the resolved drop gap (or the last preview if the cursor left
    /// the rows), journaling ONE [`MoveRowEdit`]. Always clears the preview + the
    /// arm so a stray release cannot leave a ghost line or a stale arm.
    fn drag_release(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        if let Some(from) = self.reorder_arm.get() {
            if let Some(gap) = self
                .drop_gap(over.as_ref())
                .or_else(|| self.drag_preview.get())
            {
                self.move_row_to_gap(from, gap);
            }
        }
        self.drag_preview.set(None);
        self.reorder_arm.set(None);
    }

    /// R937.1 — drag ABORT (an OS gesture revoked the in-flight reorder): discard
    /// it — clear the live preview + the arm WITHOUT moving any row (unlike
    /// [`drag_release`](Self::drag_release), a cancel commits nothing, so the ghost
    /// insertion line the session-review found can no longer survive a revoke).
    fn drag_cancel(&mut self, _payload: &DragPayload) {
        self.drag_preview.set(None);
        self.reorder_arm.set(None);
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

/// R1353 — this binding's declared introspect surface, lifted out of
/// `schema()` so the fn stays under the workspace line cap now that each
/// parametric field declares its argument rather than hand-spelling a
/// template string. Same shape as `pinion_audio::RT_EXTERNAL_FIELDS`.
const GRID_SCHEMA_FIELDS: &[SchemaField] = &[
    SchemaField::new("row_count", "int"),
    SchemaField::new("col_count", "int"),
    SchemaField::new("focused_row", "int"),
    SchemaField::new("focused_col", "int"),
    SchemaField::new("editing_row", "int"),
    SchemaField::new("editing_col", "int"),
    SchemaField::parametric(
        "col_name.<col>",
        "string",
        const { &[SchemaArg::index("col", "col_count")] },
    ),
    SchemaField::parametric(
        "col_kind.<col>",
        "string",
        const { &[SchemaArg::index("col", "col_count")] },
    ),
    SchemaField::parametric(
        "col_range.<col>",
        "string",
        const { &[SchemaArg::index("col", "col_count")] },
    ),
    SchemaField::parametric(
        "value.<row>.<col>",
        "json",
        const { &[SchemaArg::open("row", "int"), SchemaArg::open("col", "int")] },
    ),
    SchemaField::new("sort", "string"),
    SchemaField::new("filter", "string"),
    SchemaField::new("view_len", "int"),
    SchemaField::new("group", "string"),
    SchemaField::new("group_count", "int"),
    SchemaField::new("visible_len", "int"),
    // R1372 §5.38 — the cell-range selection reads (the cross-grid wire).
    SchemaField::new("cell_selection", "string"),
    SchemaField::new("cell_selection_count", "int"),
    SchemaField::new("cell_selection_tsv", "string"),
    SchemaField::parametric(
        "source_at.<pos>",
        "int",
        const { &[SchemaArg::index("pos", "visible_len")] },
    ),
    SchemaField::parametric(
        "kind_at.<pos>",
        "string",
        const { &[SchemaArg::index("pos", "visible_len")] },
    ),
    SchemaField::parametric(
        "label_at.<pos>",
        "string",
        const { &[SchemaArg::index("pos", "visible_len")] },
    ),
    SchemaField::parametric(
        "collapsed.<group>",
        "bool",
        const { &[SchemaArg::open("group", "string")] },
    ),
    SchemaField::new("scrubbing", "bool"),
    SchemaField::new("send", "string"),
    SchemaField::new("toggle", "json"),
    SchemaField::new("begin", "json"),
    SchemaField::new("cycle_sort", "json"),
    SchemaField::new("set_filter", "string"),
    SchemaField::new("set_group", "string"),
    SchemaField::new("toggle_group", "int"),
    SchemaField::new("collapse_all", "json"),
    SchemaField::new("expand_all", "json"),
    SchemaField::new("add_row", "json"),
    SchemaField::new("remove_row", "int"),
    // R1237 — paste a TSV block at the cursor; returns the cells written.
    SchemaField::new("paste", "string"),
    // R937 — row drag-to-reorder: whether reorder is enabled now (the
    // plain view), the live drop gap a drag is hovering, and the move verb.
    SchemaField::new("reorder_enabled", "bool"),
    SchemaField::new("drag_preview", "int"),
    SchemaField::new("move_row", "string"),
    // R940 — choice dropdown: whether a popup is open + its keyboard
    // cursor (read side), and the open / commit / close verbs (the
    // AI-first peer of a pointer click on the cell + an option).
    SchemaField::new("popup_open", "bool"),
    SchemaField::new("popup_cursor", "int"),
    SchemaField::new("open_choice", "json"),
    SchemaField::new("choose", "int"),
    SchemaField::new("close_popup", "json"),
    // R943 — colour swatch popup: open it on the focused cell + commit a
    // preset swatch (the choice popup's colour peer; the AI-first path for
    // an arbitrary colour is `intervene value` with a `#RRGGBB` hex).
    SchemaField::new("open_color", "json"),
    SchemaField::new("pick_color", "int"),
    // R960 — per-cell modified-from-default + reset-to-default (the
    // editable grid's Unreal / Qt "reset property to default" affordance).
    SchemaField::parametric(
        "modified.<row>.<col>",
        "bool",
        const { &[SchemaArg::open("row", "int"), SchemaArg::open("col", "int")] },
    ),
    // R966 — does ANY cell in the column / row differ from default (the
    // header reset dot's AI-first read peer, the 1-D `modified.<…>`).
    SchemaField::parametric(
        "col_modified.<col>",
        "bool",
        const { &[SchemaArg::open("col", "int")] },
    ),
    SchemaField::parametric(
        "row_modified.<row>",
        "bool",
        const { &[SchemaArg::open("row", "int")] },
    ),
    SchemaField::new("modified_count", "int"),
    SchemaField::new("reset", "string"),
    SchemaField::new("reset_all", "json"),
    // R965 — reset a whole row / column to its column default(s).
    SchemaField::new("reset_row", "int"),
    SchemaField::new("reset_col", "int"),
];

impl ExternalIntrospect for DataGridExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(GRID_SCHEMA_FIELDS)
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // R1372 §5.38 — the cell-range selection reads, split out to keep this
        // dispatch under the line ceiling (the R952 `invoke_cell_select` shape).
        if let Some(v) = self.query_cell_selection(path) {
            return Some(v);
        }
        match path {
            "row_count" => Some(IntrospectValue::Int(int_of(self.nrows()))),
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
            // R940 — whether a choice dropdown is open (the edit latch is on a
            // choice cell), and its roving keyboard cursor (`Null` when closed).
            "popup_open" => Some(IntrospectValue::Bool(self.popup_open())),
            "popup_cursor" => Some(match self.popup_cursor.get() {
                Some(i) => IntrospectValue::Int(int_of(i)),
                None => IntrospectValue::Null,
            }),
            // R886 — the wire form is the cross-grid `grid_sort_str`
            // vocabulary ("<col>:asc" / "<col>:desc" / "" = unsorted),
            // byte-identical to the read-only sort proxies.
            "sort" => Some(IntrospectValue::Text(grid_sort_str(self.sort.get()))),
            // R891 — the cross-grid `grid_filter_str` vocabulary
            // ("none" / "<col>=<value>"), byte-identical to the read-only
            // `GridSortExternal` filter facet.
            "filter" => Some(IntrospectValue::Text(grid_filter_str(
                self.filter.get().as_ref(),
            ))),
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
            // R960 — how many cells differ from their column default.
            "modified_count" => Some(IntrospectValue::Int(int_of(self.modified_count()))),
            // R914 — whether a live numeric cell scrub is in flight (the
            // AI-first read peer of the capture-drag scrub gesture).
            "scrubbing" => Some(IntrospectValue::Bool(self.is_scrubbing())),
            // R937 — whether a manual row reorder is meaningful now (the plain
            // view); the grip + drag arm only when true.
            "reorder_enabled" => Some(IntrospectValue::Bool(self.reorder_enabled())),
            // R937 — the live drop gap (`0..=nrows`) an in-flight reorder drag is
            // hovering, Null when no drag is active (the AI-first witness of where
            // a release would land, the `dg_drop_line` paint peer).
            "drag_preview" => Some(
                self.drag_preview
                    .get()
                    .map_or(IntrospectValue::Null, |g| IntrospectValue::Int(int_of(g))),
            ),
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
                    return COL_NAMES
                        .get(col)
                        .map(|n| IntrospectValue::Text((*n).to_owned()));
                }
                if let Some(col_str) = path.strip_prefix("col_kind.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_KINDS
                        .get(col)
                        .map(|k| IntrospectValue::Text(k.name().to_owned()));
                }
                // R894 — the column's clamp range ("<min>..<max>" / "none"); an
                // out-of-range column is `None` (an unknown path), an unbounded
                // one is the text "none" (present-but-unconstrained).
                if let Some(col_str) = path.strip_prefix("col_range.") {
                    let col: usize = col_str.parse().ok()?;
                    return COL_RANGE.get(col).map(|range| {
                        IntrospectValue::Text(
                            range.map_or_else(|| "none".to_owned(), ColRange::wire),
                        )
                    });
                }
                // R960 / R966 — the per-cell value read + its modified-from-
                // default predicate peers (extracted to keep `query` under the
                // `too_many_lines` budget; SRP, the R959 `invoke_send` precedent).
                self.query_cell_path(path)
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "row_count" | "col_count" | "editing_row" | "editing_col" | "view_len"
            | "group_count" | "visible_len" | "reorder_enabled" | "drag_preview" | "popup_open"
            | "popup_cursor" => Err(InterveneError::ReadOnly),
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
                    self.set_group(if s == "none" {
                        None
                    } else {
                        s.parse::<usize>().ok()
                    });
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
                if [
                    "col_name.",
                    "col_kind.",
                    "col_range.",
                    "source_at.",
                    "kind_at.",
                    "label_at.",
                ]
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
                    let group: usize = g_str.parse().map_err(|_| InterveneError::UnknownPath)?;
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
                let (row_str, col_str) = rest.split_once('.').ok_or(InterveneError::UnknownPath)?;
                let row: usize = row_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                let col: usize = col_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                if row >= self.nrows() || col >= NCOLS {
                    return Err(InterveneError::UnknownPath);
                }
                // R894 / R914 — coerce the wire value and commit through the
                // shared clamped funnel (an AI write cannot exceed the bounds a
                // keyboard edit / a drag cannot, and the cursor re-anchors
                // identically). R932 — via the JOURNALED `edit_cell`, so an AI
                // `value.<row>.<col>` write is one undo step, byte-identical
                // reversal to the keyboard / scrub edit. R940 — through the
                // VALUE-level [`CellValue::with_intervene`] (the property-grid's
                // path), not the kind-level `coerce`, so a `Choice` cell takes an
                // option `Int` index (its options live on the value); scalar
                // kinds delegate to `coerce`, so their behaviour is unchanged.
                let next = self.model.get()[idx(row, col)].with_intervene(value)?;
                self.edit_cell(row, col, next, Cow::Borrowed("Edit cell"));
                Ok(())
            }
        }
    }

    // R943 — the RPC verb dispatch is a flat match; the colour verbs pushed it
    // past the line budget. A dispatcher match is the idiomatic `too_many_lines`
    // exception (the `view` fn carries the same allow).
    #[allow(clippy::too_many_lines)]
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Composite wire `"<row>_<col>:<EventName>"` (the shared
            // `GridSendKey` SSOT, the same grammar hello-table encodes /
            // decodes). PointerUp focuses the cell (and toggles a bool);
            // DoubleClick enters edit mode on an editable cell.
            "send" => match args {
                IntrospectValue::Text(ref s) => self.dispatch_send(s),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Toggle the focused bool cell (the `Space` keyboard path + RPC).
            "toggle" => {
                let toggled = self.toggle(self.focused_row.get(), self.focused_col.get());
                Ok(IntrospectValue::Bool(toggled))
            }
            // Enter edit mode on the focused cell (the `Enter` / `F2` path).
            // R940 / R943 — a choice / colour cell opens its popup instead of an
            // inline field (the keyboard `Enter` / `F2` peer of the click open).
            // Dispatches by kind exactly like the click path (`dispatch_send`), so
            // every activation route (click, double-click, keyboard) opens the
            // same popup — a colour cell is not a keyboard-inert second class.
            "begin" => {
                let (row, col) = (self.focused_row.get(), self.focused_col.get());
                let started = match COL_KINDS.get(col) {
                    Some(CellKind::Choice) => self.open_choice(row, col),
                    Some(CellKind::Color) => self.open_color(row, col),
                    _ => self.begin_edit(row, col),
                };
                Ok(IntrospectValue::Bool(started))
            }
            // R940 — open the choice dropdown on the focused cell (the AI-first
            // peer of a click; `false` when the focused cell is not a choice).
            "open_choice" => {
                let started = self.open_choice(self.focused_row.get(), self.focused_col.get());
                Ok(IntrospectValue::Bool(started))
            }
            // R940 — commit option `i` into the open dropdown, then close it (the
            // AI-first peer of an option click / keyboard `Enter`). `false` when
            // no popup is open or `i` is out of range.
            "choose" => match args {
                IntrospectValue::Int(i) => {
                    let idx = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.commit_choice(idx)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R943 — open the colour swatch popup on the focused cell (the AI-first
            // peer of a click; `false` when the focused cell is not a colour).
            "open_color" => {
                let started = self.open_color(self.focused_row.get(), self.focused_col.get());
                Ok(IntrospectValue::Bool(started))
            }
            // R943 — commit preset swatch `i` into the open colour popup, then
            // close it (the AI-first peer of a swatch click / keyboard `Enter`).
            // `false` when no colour popup is open or `i` is out of range. An
            // arbitrary (off-palette) colour is set through `intervene value`.
            "pick_color" => match args {
                IntrospectValue::Int(i) => {
                    let idx = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.commit_color_swatch(idx)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R940 — close the open dropdown without committing (the dismiss /
            // `Escape` peer).
            "close_popup" => {
                self.close_popup();
                Ok(IntrospectValue::Null)
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
                    IntrospectValue::Text(ref s) => self.set_group(if s == "none" {
                        None
                    } else {
                        s.parse::<usize>().ok()
                    }),
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
            // R930 — append a default row; returns its new source index. The
            // AI-first `add_row` verb (a GUI "+" button would funnel here when
            // one is added — there is no "+" affordance in the scene today).
            "add_row" => Ok(IntrospectValue::Int(int_of(self.add_row()))),
            // R930 — drop source row `i` (Int); `false` out-of-range or when it
            // is the last remaining row (a grid keeps >= 1 row).
            "remove_row" => match args {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Bool(self.remove_row(row)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R937 — reorder: move source row `from` to resting index `to`, passed
            // as a `"from,to"` payload (the move_elem peer; the AI-first reorder
            // primary + the drag's RPC twin). Returns whether it moved. Rejected
            // under a sort / filter / group — a manual position is meaningful only
            // in the plain view where the visual order IS the source order.
            "move_row" => match args {
                IntrospectValue::Text(ref s) => {
                    if !self.reorder_enabled() {
                        return Err(InvokeError::Rejected);
                    }
                    let (from, to) = s
                        .split_once(',')
                        .and_then(|(a, b)| {
                            Some((
                                a.trim().parse::<usize>().ok()?,
                                b.trim().parse::<usize>().ok()?,
                            ))
                        })
                        .ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.move_row(from, to)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R1237 — paste a TSV block at the cursor (rows `\n`, cells `\t`),
            // following the active sort / filter / group; one undo step. Returns
            // the count of cells written. The AI-first primary; a Ctrl+V that
            // reads the OS clipboard would funnel here (HW-gated, deferred).
            "paste" => match args {
                IntrospectValue::Text(ref s) => {
                    Ok(IntrospectValue::Int(int_of(self.paste_block(s))))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R1372 §5.38 — the cell-range selection actions, the cross-grid wire
            // the Table widget (R952) / `hello-cell-select` (R1222) speak:
            // `select-cell` / `extend-cell` take a `"row,col"` SOURCE-coord pair
            // (matching `focused_row`'s source semantics) and start / grow the
            // rectangle; `clear-cell-selection` drops it. `Bool(true)` on success,
            // `Bool(false)` on an out-of-range no-op; `Rejected` on a malformed
            // pair. The AI-first peers of the keyboard `Shift`+arrow / `Escape`.
            "select-cell" => match args {
                IntrospectValue::Text(ref s) => {
                    let (row, col) = parse_row_col(s).ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.select_cell(row, col)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "extend-cell" => match args {
                IntrospectValue::Text(ref s) => {
                    let (row, col) = parse_row_col(s).ok_or(InvokeError::Rejected)?;
                    Ok(IntrospectValue::Bool(self.extend_cell(row, col)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "clear-cell-selection" => {
                self.clear_cell_selection();
                Ok(IntrospectValue::Bool(true))
            }
            // R1372 §5.38 — copy the current selection (or, with no range, the
            // single focused cell) as a spreadsheet TSV block, RETURNING it. The
            // action peer of the range-only `query cell_selection_tsv`: `copy`
            // ALWAYS yields something copyable when the grid has focus (a lone
            // cursor copies its one cell), so the keyboard Ctrl+C and an AI client
            // share ONE serialization funnel. The inverse of `paste`.
            "copy" => Ok(IntrospectValue::Text(self.copy_tsv())),
            // R960 — reset cell `"<row>_<col>"` (the `GridSendKey::Cell` wire,
            // the same grammar the reset-dot click sends) to its column default;
            // returns whether it was modified. The AI-first peer of a click on
            // the cell's reset dot.
            "reset" => match args {
                IntrospectValue::Text(ref s) => {
                    let Some(GridSendKey::Cell { row, col }) = GridSendKey::parse(s) else {
                        return Err(InvokeError::Rejected);
                    };
                    Ok(IntrospectValue::Bool(self.reset_cell(row, col)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R960 — reset every modified cell to its column default; returns the
            // count cleared (the inspector `reset_all` shape).
            "reset_all" => Ok(IntrospectValue::Int(int_of(self.reset_all()))),
            // R965 — reset a whole row / column to its column default(s); returns
            // the count cleared (the Qt / Excel "reset row" / "reset column").
            "reset_row" => match args {
                IntrospectValue::Int(i) => {
                    let row = usize::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    Ok(IntrospectValue::Int(int_of(self.reset_row(row))))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "reset_col" => match args {
                IntrospectValue::Int(i) => {
                    let col = usize::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    Ok(IntrospectValue::Int(int_of(self.reset_col(col))))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
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
    let before_cursor = cursor.get();
    // R932 — capture the cell's before / after so the keyboard inline-editor
    // commit journals one [`SetCellEdit`] (the second pre-existing cell-write
    // path, alongside the External's `edit_cell`). Pushed AFTER the re-anchor,
    // so the recorded `after` cursor is the post-commit slot.
    let mut edit: Option<(usize, CellValue, CellValue)> = None;
    // R930.1 — guard the row too (not just col): a row removed while this edit
    // was in flight shrinks the model, so a stale latched `row` would index
    // past the end. `remove_row` cancels the latch, so this is belt-and-
    // suspenders — but it mirrors `set_cell`'s `row >= nrows` guard so the two
    // write paths never diverge.
    if col < NCOLS && row < nrows(&model.get()) {
        if let Some(parsed) = COL_KINDS[col].parse(&text) {
            // R894 — clamp the committed value to the column's range (the
            // bounded-spinbox contract; an out-of-range edit lands on the bound).
            let parsed = clamp_for_col(parsed, col);
            let index = idx(row, col);
            edit = Some((index, model.get()[index].clone(), parsed.clone()));
            model.set_with(move |prev| {
                let mut next = prev.clone();
                next[index] = parsed;
                next
            });
        }
    }
    end_edit_mode(restore_focus);
    // R891/R892 — if the committed value hid the row, re-anchor the now-hidden
    // cursor (no-op when the row stays visible).
    reanchor_cursor(&visible(), &cursor, prior_vis);
    // R932 — record the committed edit (a no-op / malformed commit left `edit`
    // None; an unchanged value is filtered inside `push_cell_edit`). The same
    // `GridUndoCtx` shape the External holds (built from the same cached hooks),
    // so the keyboard commit reverses through the identical re-anchor funnel.
    if let Some((index, before, after)) = edit {
        push_cell_edit(
            &use_undo(),
            &Rc::new(GridUndoCtx::from_hooks()),
            index,
            before,
            after,
            before_cursor,
            cursor.get(),
            Cow::Borrowed("Edit cell"),
        );
    }
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

/// R937 — resolve the mutable introspection of the External tagged `tag`, the one
/// place a keyboard / activation path reaches a coordinator to drive a verb.
/// `None` when the tag is absent or not introspectable. The within-file
/// rule-of-three lift: [`invoke_undo`], [`invoke_move_row`] and
/// [`activate_focused`] all folded the repeated `find_external_with_tag_mut` +
/// `introspect_mut` boilerplate here ([[three-site-internal-duplication-substrate-lift]]).
fn external_mut<'s>(scene: &'s mut Scene, tag: &str) -> Option<&'s mut dyn ExternalIntrospect> {
    scene
        .find_external_with_tag_mut(tag)?
        .handle
        .introspect_mut()
}

/// R980/R982/R983 §5.40 — augment a data-grid's a11y `nodes` (the FLAT grid
/// OR the grouped treegrid) with AT-reachable reset affordances over its VISIBLE
/// data rows — `sources` is the visible source indices in visual order: a reset
/// `button` child for each modified CELL (on its `gridcell`), each modified
/// COLUMN (on its `columnheader`), and each data ROW (on a `rowheader` cell
/// prepended as the row's first child). Every button is gated on the SAME
/// modified predicate the reset dot paints under (R886.1 one-gate), and its
/// Click routes through [`DataGridView::access_child_invoke`]'s reset-prefix
/// `send` branch (the pointer twin) — that routing is mode-independent, so only
/// the EMISSION (here) differs between the two grid modes (R983). The `rowheader`
/// is the WAI-ARIA-valid host a `button` needs (invalid as a bare `row` child,
/// R937.1); its tag is the painted gutter (`handle_tag`). `sources` is the
/// VISIBLE data rows by intent — a reset affordance should appear only on rows
/// the user can see (a collapsed group hides its members from `rows`; the flat
/// grid shows all). Orphan-freedom is NOT this iteration's job: `attach_child_button`
/// is orphan-free by construction (R984.1), so even a source whose cell / row
/// node were absent would emit nothing — the visible-source set is a UX choice,
/// not a dangling-node guard. Stays a single emission site across both modes, so
/// the `rowheader` scaffold needs no lift (R982's deferral holds).
fn emit_reset_affordances(nodes: &mut Vec<AccessNode>, model: &[CellValue], sources: &[usize]) {
    for &row in sources {
        for (col, col_name) in COL_NAMES.iter().enumerate() {
            if cell_value_modified(model, row, col) {
                attach_child_button(
                    nodes,
                    &cell_tag(row, col),
                    reset_cell_tag(row, col),
                    format!("Reset {col_name} to default"),
                );
            }
        }
    }
    for (col, col_name) in COL_NAMES.iter().enumerate() {
        if col_modified(model, col) {
            attach_child_button(
                nodes,
                &col_header_tag(col),
                reset_col_tag(col),
                format!("Reset {col_name} column to default"),
            );
        }
    }
    for (visual_pos, &row) in sources.iter().enumerate() {
        let rh_tag = handle_tag(row);
        if let Some(row_node) = nodes.iter_mut().find(|n| n.tag == data_row_tag(row)) {
            row_node.children.insert(0, rh_tag.clone());
        }
        nodes.push(
            AccessNode::new(rh_tag.clone(), AriaRole::RowHeader)
                .with_name(format!("Row {}", visual_pos + 1)),
        );
        if row_modified(model, row) {
            attach_child_button(
                nodes,
                &rh_tag,
                reset_row_tag(row),
                format!("Reset row {} to default", visual_pos + 1),
            );
        }
    }
}

/// R932 — drive `verb` (`undo` / `redo`) on the [`UndoStackExternal`] at
/// [`UNDO_TAG`] — the SAME SSOT the RPC path drives, so the keyboard adds no
/// hand-rolled undo logic to the coordinator (the hello-node-editor `invoke_undo`
/// shape). Returns `true` (the grid consumes Ctrl+Z even at a history boundary,
/// where the verb is a harmless no-op) once the undo external is found.
fn invoke_undo(scene: &mut Scene, verb: &str) -> bool {
    let Some(intro) = external_mut(scene, UNDO_TAG) else {
        return false;
    };
    let _ = intro.invoke(verb, IntrospectValue::Null);
    true
}

/// R937 — move source row `from` to resting index `to` through the coordinator's
/// `move_row` invoke funnel (so the keyboard Alt+Arrow reorder journals ONE
/// [`MoveRowEdit`] exactly like the drag / RPC — no hand-rolled keyboard mutation,
/// the `invoke_undo` shape). The funnel rejects under a sort / filter / group.
fn invoke_move_row(scene: &mut Scene, from: usize, to: usize) -> bool {
    let Some(intro) = external_mut(scene, GRID_TAG) else {
        return false;
    };
    let _ = intro.invoke("move_row", IntrospectValue::Text(format!("{from},{to}")));
    true
}

/// R940 — commit option `i` through the coordinator's `choose` invoke funnel (so
/// the keyboard `Enter` / `Space` journals ONE cell edit exactly like the option
/// click / RPC — no hand-rolled keyboard mutation, the [`invoke_undo`] shape).
fn invoke_choose(scene: &mut Scene, i: usize) -> bool {
    let Some(intro) = external_mut(scene, GRID_TAG) else {
        return false;
    };
    let _ = intro.invoke("choose", IntrospectValue::Int(int_of(i)));
    true
}

/// R940 — the open choice dropdown's keymap (the grid is focused, the popup is
/// its roving active descendant): arrows / Home / End rove the option cursor
/// (clamped — the list has ends), `Enter` / `Space` commit the cursor through the
/// journaled `choose` funnel, `Escape` dismisses. The property-grid
/// `apply_key_choice` shape (data-grid's 2nd consumer of this 1-D roving keymap;
/// at a 3rd consumer the pure `(len, cursor, key) -> action` core lifts — for now
/// the divergence is the commit funnel (journaled here, direct Signal there), so
/// a copy-adapt is correct, not a wrong-abstraction merge).
fn apply_key_choice(scene: &mut Scene, key: &str) -> bool {
    let model = use_data_model().get();
    let Some((row, col)) = use_editing_cell().get() else {
        return false;
    };
    let Some(CellValue::Choice { options, selected }) = model.get(idx(row, col)) else {
        return false;
    };
    let len = options.len();
    if len == 0 {
        return false;
    }
    let cursor = use_popup_cursor().get().unwrap_or(*selected).min(len - 1);
    let target = match key {
        "ArrowDown" => (cursor + 1).min(len - 1),
        "ArrowUp" => cursor.saturating_sub(1),
        "Home" => 0,
        "End" => len - 1,
        "Enter" | "Space" => return invoke_choose(scene, cursor),
        "Escape" => {
            clear_popup(&use_editing_cell(), &use_popup_cursor(), &use_popup_hover());
            return true;
        }
        _ => return false,
    };
    use_popup_cursor().set(Some(target));
    true
}

/// R943 — commit swatch `i` through the coordinator's `pick_color` invoke funnel,
/// so the keyboard `Enter` / `Space` journals ONE cell edit exactly like the
/// swatch click / RPC (the [`invoke_choose`] colour peer).
fn invoke_pick_color(scene: &mut Scene, i: usize) -> bool {
    let Some(intro) = external_mut(scene, GRID_TAG) else {
        return false;
    };
    let _ = intro.invoke("pick_color", IntrospectValue::Int(int_of(i)));
    true
}

/// R943 — the open colour popup's keymap (the grid is focused, the swatch grid is
/// its roving active descendant): Left / Right step the cursor, Up / Down jump a
/// palette row (`SWATCH_COLS`), Home / End go to the ends, `Enter` / `Space`
/// commit through the journaled `pick_color` funnel, `Escape` dismisses. The
/// 2-D [`apply_key_choice`] colour peer (the property-grid `apply_key_color`
/// shape; copy-adapt over the 1-D dropdown keymap, divergent nav geometry).
fn apply_key_color(scene: &mut Scene, key: &str) -> bool {
    let len = COLOR_SWATCHES.len();
    let cursor = use_popup_cursor().get().unwrap_or(0).min(len - 1);
    let target = match key {
        "ArrowRight" => (cursor + 1).min(len - 1),
        "ArrowLeft" => cursor.saturating_sub(1),
        "ArrowDown" => (cursor + SWATCH_COLS).min(len - 1),
        "ArrowUp" => cursor.saturating_sub(SWATCH_COLS),
        "Home" => 0,
        "End" => len - 1,
        "Enter" | "Space" => return invoke_pick_color(scene, cursor),
        "Escape" => {
            clear_popup(&use_editing_cell(), &use_popup_cursor(), &use_popup_hover());
            return true;
        }
        _ => return false,
    };
    use_popup_cursor().set(Some(target));
    true
}

/// Grid-focused keymap: undo / redo (Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z), then 2-D
/// roving navigation + activate.
/// R1372 §5.38 — the `Ctrl`/`Cmd`+C body: copy the current selection (or, with
/// no range, the lone focused cell) as a spreadsheet TSV block through the
/// coordinator's `copy` funnel (the same the AI-first `invoke "copy"` uses, so
/// there is ONE serialization path), then write it to the platform clipboard.
/// Split out of [`apply_key_grid`] for the line ceiling. `false` when the grid
/// external is absent (defensive), so the key can fall through.
fn copy_selection_to_clipboard(scene: &mut Scene) -> bool {
    let tsv = external_mut(scene, GRID_TAG).and_then(|intro| {
        match intro.invoke("copy", IntrospectValue::Null) {
            Ok(IntrospectValue::Text(t)) => Some(t),
            _ => None,
        }
    });
    if let Some(tsv) = tsv {
        use_app_clipboard(GRID_TAG).copy(tsv);
        return true;
    }
    false
}

/// R1372 §5.38 — the cell-range selection's response to a nav-mode keystroke,
/// BEFORE the plain 2-D nav moves the cursor (the keyboard twin of the
/// `select-cell` / `extend-cell` / `clear-cell-selection` wire): `Escape` clears
/// an active range (`Some(true)` = handled; `Some(false)` = nothing to clear,
/// fall through); a `Shift`+arrow / Home / End pins the anchor at the pre-move
/// `cursor` (if not already) so the following nav EXTENDS the rectangle; a plain
/// nav key drops the anchor so the move COLLAPSES to one cell. `None` = not a
/// selection-terminal key, let the caller's nav arm run.
fn maintain_cell_selection(
    key: &str,
    modifiers: Modifiers,
    cursor: (usize, usize),
) -> Option<bool> {
    let anchor = use_cell_anchor();
    if key == "Escape" {
        return Some(if anchor.get().is_some() {
            anchor.set(None);
            true
        } else {
            false
        });
    }
    if matches!(
        key,
        "ArrowDown" | "ArrowUp" | "ArrowLeft" | "ArrowRight" | "Home" | "End"
    ) {
        if modifiers.shift_key() {
            if anchor.get().is_none() {
                anchor.set(Some(cursor));
            }
        } else {
            anchor.set(None);
        }
    }
    None
}

fn apply_key_grid(scene: &mut Scene, key: &str, modifiers: Modifiers) -> bool {
    // R940 / R943 — an open + visible choice / colour popup owns the keymap (the
    // grid keeps focus; the popup is its roving active descendant). Intercepted
    // before undo + navigation. Gated on the popup being present in the flatten
    // ([`popup_pos_live`]), so a collapsed / filtered-out editing row never traps
    // the arrows (the property-grid `open_popup_kind` discipline). Dispatched by
    // the editing column's kind — the colour palette roves in 2-D.
    if let Some((_, col, _)) = popup_pos_live() {
        return if COL_KINDS[col] == CellKind::Color {
            apply_key_color(scene, key)
        } else {
            apply_key_choice(scene, key)
        };
    }
    // R932 — a held-Ctrl Z / Y drives the shared undo history before the plain
    // navigation keys (so Ctrl+Z is not read as a bare key).
    if let Some(verb) = undo_redo_verb(key, modifiers) {
        return invoke_undo(scene, verb);
    }
    // R1372 §5.38 — Ctrl/Cmd+C copies the selected rectangle (or lone cell) to
    // the platform clipboard, the copy half of the R1237 paste symmetry. Before
    // the plain nav so a held Ctrl+C is not read as a bare `c`; `!alt_key()`
    // mirrors the text_field chord decode (AltGr = Ctrl+Alt would else misfire).
    if modifiers.command_key() && !modifiers.alt_key() && key.eq_ignore_ascii_case("c") {
        return copy_selection_to_clipboard(scene);
    }
    let row_sig = use_focused_row();
    let col_sig = use_focused_col();
    let col = col_sig.get().min(NCOLS - 1);
    // R937 — Alt+Arrow moves the focused row one slot: the keyboard reorder path
    // (the AT-accessible twin of the drag handle), routed through the same
    // `move_row` funnel so it journals ONE step + rejects under a transform. Alt
    // distinguishes it from the plain cursor navigation below.
    if modifiers.alt_key() && matches!(key, "ArrowDown" | "ArrowUp") {
        let model = use_data_model().get();
        let n = nrows(&model);
        let from = row_sig.get().min(n.saturating_sub(1));
        let to = if key == "ArrowDown" {
            (from + 1).min(n.saturating_sub(1))
        } else {
            from.saturating_sub(1)
        };
        if from != to {
            return invoke_move_row(scene, from, to);
        }
        return true; // at an end — consumed, no move (no wrap)
    }
    // R1372 §5.38 — the cell-range selection's response to this keystroke BEFORE
    // the plain nav moves the cursor (Escape clears; Shift+arrow pins the anchor
    // to extend; a plain arrow drops it to collapse). `Some(b)` = fully handled.
    if let Some(handled) = maintain_cell_selection(key, modifiers, (row_sig.get(), col)) {
        return handled;
    }
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
            let model = use_data_model().get();
            let order = visible_data_order(
                &model,
                use_sort().get(),
                use_filter().get().as_ref(),
                use_group_col().get(),
                &use_collapsed().get(),
            );
            let row = row_sig.get().min(nrows(&model).saturating_sub(1));
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
    let Some(intro) = external_mut(scene, GRID_TAG) else {
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

/// Cell-sized M3 checkbox-box style. The bool cell renders the lifted
/// `view_checkbox_box` SSOT non-interactively (the grid coordinator owns the
/// toggle, so there is no per-cell `CheckboxExternal`) — one M3 checkbox
/// rendering across the catalog instead of a hand-rolled copy.
fn cell_checkbox_style() -> CheckboxStyle {
    CheckboxStyle {
        box_size: CHECKBOX_SIZE,
        glyph_size_px: 14,
        ..CheckboxStyle::m3_filled()
    }
}

/// One cell: tagged `data_grid#<row>_<col>` (the `GridSendKey` encoding) so a
/// click routes to the coordinator. Paints the shared inline field while
/// editing, else a checkbox (bool) or the value text.
/// R943 — a closed colour cell's inner: a filled swatch chip beside the
/// `#RRGGBB` hex (the property-grid `color_value_cell` skin). The swatch shows
/// the colour at a glance; the hex is the queryable / AT-readable value.
fn color_cell_inner(c: Color, theme: &Theme) -> Scene {
    let swatch = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(
                BoxStyle::filled(c)
                    .with_corner_radius(4)
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(CELL_PX + 5, CELL_PX + 5))),
    );
    let hex = Scene::Text(TextNode::styled(
        c.to_hex(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(CELL_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![swatch, hex]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(6),
        ),
    )
}

/// R1372 §5.38 — the fraction a selected (non-focused) cell's background washes
/// from `Surface` toward the theme's `Accent` (its "selection" role). Subtle
/// enough to read as a block, distinct from the greyer `OnSurface`-tinted focus
/// highlight the active cell keeps.
const SELECTION_TINT: f32 = 0.18;

/// R1372 §5.38 — a cell's background fill: the [`focus_fill`] highlight when it
/// is the active cell (the SSOT the property grid shares), else a subtle Accent
/// wash when it is inside the selected range, else transparent. Focus takes
/// precedence, so the active descendant reads distinctly within a washed block.
fn cell_fill(theme: &Theme, focused: bool, selected: bool) -> Color {
    if focused {
        focus_fill(theme, true)
    } else if selected {
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), SELECTION_TINT)
    } else {
        Color::TRANSPARENT
    }
}

fn view_cell(
    row: usize,
    col: usize,
    value: &CellValue,
    // R1372 — the precomputed background ([`cell_fill`]: focus highlight /
    // selection wash / transparent), so the cell need not know the focus /
    // selection semantics and the arg count stays under the ceiling.
    fill: Color,
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
    } else if let CellValue::Color(c) = value {
        // R943 — a closed colour cell: a filled swatch chip + the `#RRGGBB` hex
        // (the property-grid `color_value_cell` skin).
        color_cell_inner(*c, theme)
    } else {
        Scene::Text(TextNode::styled(
            value.display(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(CELL_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))
    };
    // R960 — a cell differs from its column default → paint the trailing-edge
    // reset dot (suppressed while the inline editor owns the cell). R967.1 — via
    // the [`value_modified`] atom (the SSOT the `modified.<…>` queries read), so
    // the dot's presence and the wire predicate cannot diverge.
    let modified = !edit_active && value_modified(value, col);
    let mut children = vec![inner];
    if modified {
        children.push(reset_dot(row, col, theme));
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(cell_tag(row, col))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                    .with_size(Size::px(COL_W[col], ROW_H)),
            ),
    )
}

/// R966 — the shared "modified, click to reset" accent dot: an absolutely-
/// positioned `RESET_DOT`-square tagged `tag` at `(abs_x, abs_y)` (out of the
/// flex flow, so the host's content layout is byte-identical without it). The
/// VISUAL is one SSOT across the per-cell (R960), per-column-header, and per-row
/// reset dots — the [[three-site-internal-duplication-substrate-lift]] of the
/// dot box. Only the click-target `tag` + the trailing-edge `(x, y)` diverge
/// (the cell / header-cell `COL_W` edge vs the handle gutter), so those stay the
/// caller's args — R960's "positioning diverges → share the atom, not the
/// layout" applied to the dot box itself. Inline (not a `pinion_widget_paint`
/// lift): the divergent positioning is per-binding, so per R735.1 only this
/// box + the `value_eq` modified atom are the shared SSOTs.
fn reset_dot_at(tag: String, abs_x: u32, abs_y: u32, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(tag)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::Accent))
                    .with_corner_radius(RESET_DOT / 2),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(RESET_DOT, RESET_DOT))
                    .with_absolute_position(abs_x, abs_y),
            ),
    )
}

/// R960 — the per-cell reset dot at the cell's trailing edge; a click routes to
/// [`reset_cell`](DataGridExternal::reset_cell) via the [`reset_cell_tag`]
/// target. Its presence doubles as the per-cell modified indicator. See
/// [`reset_dot_at`] for the shared visual.
fn reset_dot(row: usize, col: usize, theme: &Theme) -> Scene {
    reset_dot_at(
        reset_cell_tag(row, col),
        COL_W[col].saturating_sub(CELL_PAD + RESET_DOT),
        (ROW_H - RESET_DOT) / 2,
        theme,
    )
}

/// The column-header row.
/// R937 — the total content width (the leading handle column + every data
/// column); the handle width is always reserved so the data columns do not shift
/// when a sort / filter / group toggles reorder off.
fn content_w() -> u32 {
    HANDLE_W + COL_W.iter().sum::<u32>()
}

/// R937 — which edge of a data row the live drop line sits on (the insertion
/// point a release would land at), or `None` when this row is not the drop target.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DropEdge {
    /// Insert BEFORE this row (the drop gap is this row's visual position).
    Top,
    /// Insert AFTER this row (the drop gap is past the last row — drop at end).
    Bottom,
}

/// R937 — the drop edge for the row at visual position `pos` given the live drag
/// `gap` (`0..=nrows`): a top line when the gap inserts before this row, a bottom
/// line on the LAST row when the gap is past the end. Plain-view only (visual
/// position == source row), so the caller passes the source index as `pos`.
fn drop_edge_at(gap: Option<usize>, pos: usize, last: usize) -> Option<DropEdge> {
    match gap? {
        g if g == pos => Some(DropEdge::Top),
        g if g == last + 1 && pos == last => Some(DropEdge::Bottom),
        _ => None,
    }
}

/// R937 — the leading drag-handle cell of a data row. When reorder is enabled
/// (the plain view) it carries the grip glyph + the `data_grid#d<row>` press
/// target; otherwise it is an untagged blank spacer of the same width, so the
/// affordance is honest (a grip is shown exactly when a drag would work) while the
/// data columns never shift.
fn view_handle_cell(row: usize, enabled: bool, row_modified: bool, theme: &Theme) -> Scene {
    let mut children = if enabled {
        vec![Scene::Text(TextNode::styled(
            GRIP_GLYPH,
            Rect::default(),
            TextStyle::new()
                .with_size_px(CELL_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))]
    } else {
        Vec::new()
    };
    // R966 — a row holding any modified cell shows a reset dot in the handle
    // gutter (the per-cell dot at row granularity); a `resetrow<row>` click
    // routes to reset_row. Absolutely positioned (out of the grip's centered
    // flex flow) and carrying its own tag, so a dot click resets the row while a
    // press on the grip glyph still arms a drag-reorder (the R960 dot-vs-cell
    // coexistence, here dot-vs-grip). The gutter IS the row-header column the
    // header's leading blank cell aligns over — the natural row affordance home.
    if row_modified {
        children.push(reset_dot_at(
            reset_row_tag(row),
            HANDLE_W.saturating_sub(RESET_DOT + 2),
            (ROW_H - RESET_DOT) / 2,
            theme,
        ));
    }
    let mut node = ContainerNode::new(children).with_layout(
        LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_align_items(AlignItems::Center)
            .with_justify(JustifyContent::Center)
            .with_size(Size::px(HANDLE_W, ROW_H)),
    );
    if enabled {
        node = node.with_tag(handle_tag(row));
    }
    Scene::Container(node)
}

/// R937 — the drop insertion line: a thin accent bar absolutely positioned at the
/// target row's top (or the last row's bottom), tagged `dg_drop_line` so a
/// `scene/snapshot` witnesses where a release would land (the AI-first peer of the
/// `drag_preview` query).
fn view_drop_line(edge: DropEdge, theme: &Theme) -> Scene {
    let y = match edge {
        DropEdge::Top => 0,
        DropEdge::Bottom => ROW_H.saturating_sub(DROP_LINE_H),
    };
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag("dg_drop_line")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, y)
                    .with_size(Size::px(content_w(), DROP_LINE_H)),
            ),
    )
}

fn view_header(theme: &Theme, sort: Option<(usize, bool)>, model: &[CellValue]) -> Scene {
    // R937 — a leading blank cell aligning the header over the data rows' handle
    // column (the header has no grip — there is no header row to reorder).
    let mut cells: Vec<Scene> = vec![Scene::Container(
        ContainerNode::new(Vec::new())
            .with_layout(LayoutStyle::new().with_size(Size::px(HANDLE_W, ROW_H))),
    )];
    cells.extend(COL_NAMES.iter().enumerate().map(|(col, label)| {
        // R886 — the active sort column appends the direction glyph;
        // the header cell carries the composite `Header` send tag so a
        // click routes to the coordinator's sort cycle (the same
        // `h<col>` sub-key grammar the read-only grids use).
        let glyph = pinion_widget_paint::glyph::sort_glyph(col_sort_dir(sort, col))
            .map(|g| format!(" {g}"))
            .unwrap_or_default();
        let mut children = vec![Scene::Text(TextNode::styled(
            format!("{label}{glyph}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(HEADER_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))];
        // R966 — a column holding any modified cell shows a reset dot at the
        // header cell's trailing edge (the per-cell dot at column
        // granularity); a `resetcol<col>` click routes to reset_col. The dot
        // is a child on top of the header's sort target, so a dot click
        // resets while a click elsewhere on the header still sorts.
        if col_modified(model, col) {
            children.push(reset_dot_at(
                reset_col_tag(col),
                COL_W[col].saturating_sub(CELL_PAD + RESET_DOT),
                (ROW_H - RESET_DOT) / 2,
                theme,
            ));
        }
        Scene::Container(
            ContainerNode::new(children)
                .with_tag(col_header_tag(col))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_padding(Rect::new(CELL_PAD, 0, CELL_PAD, 0))
                        .with_size(Size::px(COL_W[col], ROW_H)),
                ),
        )
    }));
    Scene::Container(
        ContainerNode::new(cells)
            .with_tag("dg_header")
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHighest),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center),
            ),
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
        // R937 — span the full width including the (blank) handle column, so a
        // group header aligns with the data rows beneath it.
        content_w(),
        ROW_H,
    )
}

/// R886.1 — one data row: a flex row of [`view_cell`]s, tagged `dg_row<src>`
/// (the same tag its a11y `row` node uses, so AT bounds attach). The cursor /
/// edit latch are SOURCE-keyed, so this paints by source index regardless of
/// the visual order.
#[allow(clippy::too_many_arguments)] // one arg per orthogonal paint axis (R937 added reorder + drop edge)
fn view_data_row(
    row: usize,
    model: &[CellValue],
    focus: (usize, usize),
    editing: Option<(usize, usize)>,
    theme: &Theme,
    edit_field: (TextFieldState, u32),
    reorder_enabled: bool,
    drop_edge: Option<DropEdge>,
    // R1372 — the selected COLUMN range `(c0, c1)` when THIS row is inside the
    // cell-selection rectangle, else `None` (the caller resolves row membership
    // from the visible-position bounds; a cell washes when its col is in range).
    selection: Option<(usize, usize)>,
) -> Scene {
    let (focused_row, focused_col) = focus;
    // R937 — the leading drag-handle cell, then the data cells.
    let mut cells: Vec<Scene> = vec![view_handle_cell(
        row,
        reorder_enabled,
        row_modified(model, row),
        theme,
    )];
    cells.extend((0..NCOLS).map(|col| {
        let value = &model[idx(row, col)];
        let focused = row == focused_row && col == focused_col;
        let selected = selection.is_some_and(|(c0, c1)| (c0..=c1).contains(&col));
        let edit_active = editing == Some((row, col)) && COL_KINDS[col].is_text_editable();
        let fill = cell_fill(theme, focused, selected);
        view_cell(row, col, value, fill, edit_active, theme, edit_field)
    }));
    // R937 — the live drop line overlays the row's edge (absolutely positioned,
    // last in paint order) when this row is the in-flight drag's drop target.
    if let Some(edge) = drop_edge {
        cells.push(view_drop_line(edge, theme));
    }
    Scene::Container(
        ContainerNode::new(cells)
            .with_tag(data_row_tag(row))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// R932 — the scene-as-data undo readout: the labels of the steps `Ctrl+Z` /
/// `Ctrl+Y` would replay ("none" at a branch boundary). The reactive
/// `undo_label` reads subscribe the caller's paint, so it repaints on every
/// edit / undo / redo. The AI-first peer is the [`UNDO_TAG`] external's
/// `undo_label` / `redo_label` / `can_undo` query.
fn view_undo_status(theme: &Theme) -> Scene {
    let undo = use_undo();
    let undo_label = undo
        .undo_label()
        .map_or_else(|| "none".to_owned(), Cow::into_owned);
    let redo_label = undo
        .redo_label()
        .map_or_else(|| "none".to_owned(), Cow::into_owned);
    Scene::Text(
        TextNode::styled(
            format!("undo: {undo_label} \u{00B7} redo: {redo_label}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(HEADER_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("dg_undo"),
    )
}

// ─── choice dropdown paint + anchor (R940) ────────────────────────

/// R943 — whether column `col` is edited through a floating popup (a choice
/// dropdown or a colour swatch palette) rather than an inline field / toggle.
/// The SSOT the open dispatch + [`popup_pos`] gate share so "which columns
/// open a popup" lives in one place.
fn opens_popup(col: usize) -> bool {
    matches!(COL_KINDS.get(col), Some(CellKind::Choice | CellKind::Color))
}

/// R940 / R943 — the open popup's editing cell resolved to `(row, col, visual_pos)`,
/// or `None` when nothing is editing, the editing cell is not a popup column
/// (choice / colour), or its row is filtered / collapsed out of the current
/// flatten (so the row is hidden — no popup is painted, announced, or given the
/// keymap, the property-grid `popup_view_pos` discipline). `visual_pos` is the
/// row's index in the FULL flatten (group headers + data rows share the
/// [`ROW_H`] pitch), the basis the anchor's y math uses. The SSOT the popup
/// paint + a11y + keymap-intercept gate (both choice + colour).
fn popup_pos(
    editing: Option<(usize, usize)>,
    vis_rows: &[GroupRow],
) -> Option<(usize, usize, usize)> {
    let (row, col) = editing?;
    if col >= NCOLS || !opens_popup(col) {
        return None;
    }
    let pos = vis_rows
        .iter()
        .position(|r| matches!(r, GroupRow::Data { source } if *source == row))?;
    Some((row, col, pos))
}

/// R940 — [`popup_pos`] over the live reactive state (the keymap intercept +
/// a11y read it where the view's `vis_rows` is not in hand). Recomputes the
/// flatten from the hooks (cheap — small N); the view passes its `vis_rows` to
/// [`popup_pos`] directly to avoid the rebuild.
fn popup_pos_live() -> Option<(usize, usize, usize)> {
    let model = use_data_model().get();
    let vis = visible_rows(
        &model,
        use_sort().get(),
        use_filter().get().as_ref(),
        use_group_col().get(),
        &use_collapsed().get(),
    );
    popup_pos(use_editing_cell().get(), &vis)
}

/// R940 — the dropdown panel's height for `n` options ([`POPUP_OPT_H`] per row +
/// the panel's top/bottom padding). The property-grid panel-height formula.
fn choice_panel_h(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(0) * POPUP_OPT_H + 2 * POPUP_PAD
}

/// R940 — the open dropdown panel's GRID-LOCAL top-left, or `None` when the
/// editing row is scrolled out of the body viewport (so no panel is painted —
/// the property-grid hidden-row discipline, extended to the virtualized grid's
/// vertical scroll). Anchored under the value cell — dropping below the row, or
/// flipped above it when the panel would overflow the grid's bottom (the native
/// dropdown edge behaviour). GRID-LOCAL because the panel is a sibling of the
/// scroll viewport inside the grid container, so it subtracts both scroll
/// offsets by hand (it is outside the scrolls that translate the cells): `col`'s
/// x walks the column widths past the handle column; `visual_pos`'s y sits below
/// the one-[`ROW_H`] pinned header.
fn popup_anchor(
    visual_pos: usize,
    col: usize,
    panel_h: u32,
    v_off: i32,
    h_off: i32,
) -> Option<(u32, u32)> {
    let row_h = i32::try_from(ROW_H).ok()?;
    let pos = i32::try_from(visual_pos).ok()?;
    let body_h = i32::try_from(GRID_VIEWPORT_H.saturating_sub(ROW_H)).ok()?;
    let row_top_in_body = pos.checked_mul(row_h)?.checked_sub(v_off)?;
    // Off-screen (scrolled clear of the body viewport) → no panel painted.
    if row_top_in_body + row_h <= 0 || row_top_in_body >= body_h {
        return None;
    }
    let grid_row_top = row_h + row_top_in_body; // below the pinned header
    let ph = i32::try_from(panel_h).ok()?;
    let win_h = i32::try_from(GRID_VIEWPORT_H).ok()?;
    let below = grid_row_top + row_h;
    let y = if below + ph <= win_h {
        below
    } else {
        (grid_row_top - ph).max(0)
    };
    let col_left = i32::try_from(HANDLE_W + COL_W[..col].iter().sum::<u32>()).ok()?;
    let x = (col_left - h_off).max(0);
    Some((u32::try_from(x).ok()?, u32::try_from(y).ok()?))
}

/// R940 — the open dropdown panel: one [`view_option`] row per option (the R867
/// listbox skin), absolutely positioned in GRID-LOCAL coordinates. Each option
/// is tagged `{GRID_TAG}#opt<i>` so its click / hover routes to the coordinator.
fn view_choice_popup(
    x: u32,
    y: u32,
    options: &[String],
    selected: usize,
    cursor: usize,
    hover: Option<usize>,
    theme: &Theme,
) -> Scene {
    let rows: Vec<Scene> = options
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let state = if hover == Some(i) {
                ListboxItemState::Hover
            } else {
                ListboxItemState::Idle
            };
            view_option(
                &OptionRow {
                    tag: format!("{GRID_TAG}#{CHOICE_OPT_PREFIX}{i}"),
                    label,
                    state,
                    active: cursor == i,
                    selected: selected == i,
                },
                POPUP_W - 2 * POPUP_PAD,
                POPUP_OPT_H,
                theme,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(rows)
            .with_tag(CHOICE_POPUP_TAG)
            .with_style(popup_surface(theme))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x, y)
                    .with_size(Size::px(POPUP_W, choice_panel_h(options.len())))
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_gap(2)
                    .with_padding(Rect::new(POPUP_PAD, POPUP_PAD, POPUP_PAD, POPUP_PAD)),
            ),
    )
}

/// R943 — the colour popup palette's grid width / height (the `SWATCH_COLS`-wide
/// swatch grid + the panel padding). The palette wraps onto
/// `ceil(len / SWATCH_COLS)` rows. The [`choice_panel_h`] colour peer.
fn color_panel_w() -> u32 {
    let cols = u32::try_from(SWATCH_COLS).unwrap_or(1);
    cols * SWATCH_SIZE + cols.saturating_sub(1) * SWATCH_GAP + 2 * POPUP_PAD
}

fn color_panel_h() -> u32 {
    let n_rows = u32::try_from(COLOR_SWATCHES.len().div_ceil(SWATCH_COLS)).unwrap_or(1);
    n_rows * SWATCH_SIZE + n_rows.saturating_sub(1) * SWATCH_GAP + 2 * POPUP_PAD
}

/// R943 — one popup swatch chip, tagged `{GRID_TAG}#sw<i>` so its click / hover
/// routes to the coordinator. The cursor (active descendant), the committed
/// colour, and a hover each get a ring (the property-grid `view_swatch` skin).
fn view_swatch(
    i: usize,
    color: Color,
    is_selected: bool,
    is_active: bool,
    is_hover: bool,
    theme: &Theme,
) -> Scene {
    let (border_color, border_w) = if is_active || is_hover {
        (theme.resolve(ColorRole::Accent), 2)
    } else if is_selected {
        (theme.resolve(ColorRole::OnSurface), 2)
    } else {
        (theme.resolve(ColorRole::Outline), 1)
    };
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(format!("{GRID_TAG}#{COLOR_SW_PREFIX}{i}"))
            .with_style(
                BoxStyle::filled(color)
                    .with_corner_radius(4)
                    .with_border(Border::new(border_color, border_w)),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH_SIZE, SWATCH_SIZE))),
    )
}

/// R943 — the open colour popup: a `SWATCH_COLS`-wide grid of preset swatch
/// chips, absolutely positioned in GRID-LOCAL coordinates. Each swatch is tagged
/// `{GRID_TAG}#sw<i>` for its click / hover. (An in-popup GUI hex-entry field is
/// a documented follow-up; the presets + the RPC `intervene value` hex path
/// cover colour selection.) The [`view_choice_popup`] colour peer.
fn view_color_popup(
    x: u32,
    y: u32,
    current: Color,
    cursor: usize,
    hover: Option<usize>,
    theme: &Theme,
) -> Scene {
    let chip_rows: Vec<Scene> = (0..COLOR_SWATCHES.len())
        .step_by(SWATCH_COLS)
        .map(|start| {
            let end = (start + SWATCH_COLS).min(COLOR_SWATCHES.len());
            let chips: Vec<Scene> = (start..end)
                .map(|i| {
                    let (color, _) = COLOR_SWATCHES[i];
                    view_swatch(
                        i,
                        color,
                        color == current,
                        cursor == i,
                        hover == Some(i),
                        theme,
                    )
                })
                .collect();
            Scene::Container(
                ContainerNode::new(chips).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_gap(SWATCH_GAP),
                ),
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(chip_rows)
            .with_tag(COLOR_POPUP_TAG)
            .with_style(popup_surface(theme))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x, y)
                    .with_size(Size::px(color_panel_w(), color_panel_h()))
                    .flex(FlexDirection::Column)
                    .with_gap(SWATCH_GAP)
                    .with_padding(Rect::new(POPUP_PAD, POPUP_PAD, POPUP_PAD, POPUP_PAD)),
            ),
    )
}

/// R940 / R943 — the popup overlay (a grid-covering light-dismiss barrier + the
/// open choice dropdown or colour swatch panel) for the editing cell, both
/// GRID-LOCAL siblings of the scroll viewport inside the grid container. Empty
/// when nothing is editing, the editing cell is not a popup column, or its row is
/// filtered / collapsed / scrolled out (so no panel is shown). The barrier sorts
/// first so the panel hit-tests on top; a click outside the panel routes
/// `dismiss` to the coordinator.
fn view_choice_overlay(
    editing: Option<(usize, usize)>,
    vis_rows: &[GroupRow],
    model: &[CellValue],
    v_off: i32,
    h_off: i32,
    theme: &Theme,
) -> Vec<Scene> {
    let Some((row, col, pos)) = popup_pos(editing, vis_rows) else {
        return Vec::new();
    };
    let hover = use_popup_hover().get();
    let cursor_sig = use_popup_cursor().get();
    let panel = match model.get(idx(row, col)) {
        Some(CellValue::Choice { selected, options }) => {
            let Some((x, y)) = popup_anchor(pos, col, choice_panel_h(options.len()), v_off, h_off)
            else {
                return Vec::new();
            };
            view_choice_popup(
                x,
                y,
                options,
                *selected,
                cursor_sig.unwrap_or(*selected),
                hover,
                theme,
            )
        }
        Some(CellValue::Color(current)) => {
            let Some((x, y)) = popup_anchor(pos, col, color_panel_h(), v_off, h_off) else {
                return Vec::new();
            };
            view_color_popup(x, y, *current, cursor_sig.unwrap_or(0), hover, theme)
        }
        _ => return Vec::new(),
    };
    vec![
        dismiss_barrier(
            POPUP_DISMISS_TAG,
            (0, 0),
            (GRID_VIEWPORT_W, GRID_VIEWPORT_H),
        ),
        panel,
    ]
}

/// R940 / R943 — the open popup's `listbox` a11y nodes (one `option` per choice,
/// or one per colour swatch), or empty when no popup is open / visible. Gated on
/// [`popup_pos`] (the SSOT the paint uses) so the AT `listbox` is never announced
/// for a popup the screen does not show. The property-grid `popup_listbox_nodes`
/// shape; the choice + colour palettes are both `listbox` + `option`s, so they
/// share the builder and diverge only in the per-item tag / label / selection.
fn popup_listbox_nodes(
    model: &[CellValue],
    vis_rows: &[GroupRow],
    editing: Option<(usize, usize)>,
) -> Vec<AccessNode> {
    let Some((row, col, _)) = popup_pos(editing, vis_rows) else {
        return Vec::new();
    };
    let hover = use_popup_hover().get();
    match model.get(idx(row, col)) {
        Some(CellValue::Choice { selected, options }) => {
            let cursor = use_popup_cursor().get().unwrap_or(*selected);
            let tags: Vec<String> = (0..options.len())
                .map(|i| format!("{GRID_TAG}#{CHOICE_OPT_PREFIX}{i}"))
                .collect();
            let opts: Vec<ListOption<'_>> = options
                .iter()
                .enumerate()
                .map(|(i, label)| ListOption {
                    tag: &tags[i],
                    label: Some(label.as_str()),
                    state: if hover == Some(i) {
                        ListboxItemState::Hover
                    } else {
                        ListboxItemState::Idle
                    },
                    selected: *selected == i,
                    focused: cursor == i,
                })
                .collect();
            let name = format!("{} options", COL_NAMES[col]);
            listbox_option_nodes(CHOICE_POPUP_TAG, &name, false, &opts)
        }
        Some(CellValue::Color(current)) => {
            let cursor = use_popup_cursor().get().unwrap_or(0);
            let tags: Vec<String> = (0..COLOR_SWATCHES.len())
                .map(|i| format!("{GRID_TAG}#{COLOR_SW_PREFIX}{i}"))
                .collect();
            let opts: Vec<ListOption<'_>> = COLOR_SWATCHES
                .iter()
                .enumerate()
                .map(|(i, &(color, label))| ListOption {
                    tag: &tags[i],
                    label: Some(label),
                    state: if hover == Some(i) {
                        ListboxItemState::Hover
                    } else {
                        ListboxItemState::Idle
                    },
                    selected: color == *current,
                    focused: cursor == i,
                })
                .collect();
            let name = format!("{} swatches", COL_NAMES[col]);
            listbox_option_nodes(COLOR_POPUP_TAG, &name, false, &opts)
        }
        _ => Vec::new(),
    }
}

// R940 — the choice-dropdown overlay append pushed the paint fn past the
// line budget; a monolithic view fn is inherent to a paint surface (the
// hello-color-picker view carries the same allow pair).
#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_lines)]
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
    // R937 — reorder is enabled only in the plain source view (no transform), so
    // the grip paints + the drag arms only then; the live drop gap drives the
    // insertion line (reading both subscribes, so a drag move / a sort repaints).
    // The ONE `plain_view` gate the coordinator + a11y also use (R937.1 one-gate).
    let reorder_enabled = plain_view(sort, filter.as_ref(), group_col);
    let drag_gap = use_drag_preview().get();
    let last_row = nrows(&model).saturating_sub(1);
    let vis_rows = visible_rows(&model, sort, filter.as_ref(), group_col, &collapsed);
    let group_labels = group_col.map(|col| group_table(&model, col));
    // R1372 §5.38 — the cell-selection rectangle in VISIBLE-position coords, from
    // the SOURCE anchor + cursor over the current data order. Derived from the
    // SAME `vis_rows` snapshot the body windows over, so paint + selection agree.
    // The Data arm below washes the cells inside it; `None` = no range (the lone
    // cursor shows only its focus highlight).
    let sel_anchor = use_cell_anchor().get();
    let sel_visible: Vec<usize> = vis_rows.iter().filter_map(GroupRow::source).collect();
    let sel_bounds = cell_selection_bounds(&sel_visible, sel_anchor, focused_row, focused_col);
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
    let total_w: u32 = content_w();
    let (_, measured_h) = v_scroll.measured_viewport();
    let window = compute_visible_range(
        v_scroll.offset_y(),
        measured_h,
        vis_rows.len(),
        ROW_H,
        OVERSCAN,
    );
    let total_h = content_height(vis_rows.len(), ROW_H);
    let scrolled = view_virtual_grid_body(
        GridScroll {
            body: &v_scroll,
            horizontal: &h_scroll,
        },
        &window,
        total_w,
        total_h,
        ROW_H,
        view_header(&theme, sort, &model),
        |view_pos| match vis_rows[view_pos] {
            // R892 — a group header spanning the grid (label + member count +
            // collapse chevron; a click toggles collapse).
            GroupRow::Header {
                group,
                member_count,
                collapsed: is_collapsed,
            } => {
                let label = group_labels
                    .as_ref()
                    .and_then(|t| t.get(group))
                    .map_or("", String::as_str);
                view_group_header(group, label, member_count, is_collapsed, &theme)
            }
            GroupRow::Data { source } => {
                // R1372 — this row is in the selection when its source falls in
                // the visible-position band; pass the col range so its cells wash.
                let selection = sel_bounds
                    .filter(|&(p0, _, p1, _)| sel_visible[p0..=p1].contains(&source))
                    .map(|(_, c0, _, c1)| (c0, c1));
                view_data_row(
                    source,
                    &model,
                    (focused_row, focused_col),
                    editing,
                    &theme,
                    (edit_state, edit_caret),
                    reorder_enabled,
                    // Plain-view only, so the source index IS the visual position.
                    reorder_enabled
                        .then(|| drop_edge_at(drag_gap, source, last_row))
                        .flatten(),
                    selection,
                )
            }
        },
    );
    // R940 — the choice dropdown overlay (barrier + panel) rides as a GRID-LOCAL
    // sibling of the scroll viewport, so it floats over the rows (un-clipped by
    // the inner scroll) and tracks the cell as the body scrolls. Empty unless a
    // visible choice cell's popup is open.
    let mut grid_children = vec![scrolled];
    grid_children.extend(view_choice_overlay(
        editing,
        &vis_rows,
        &model,
        v_scroll.offset_y(),
        h_scroll.offset_x(),
        &theme,
    ));
    let grid = Scene::Container(
        ContainerNode::new(grid_children)
            .with_tag(GRID_TAG)
            .with_aria_label("Asset table")
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::Surface))
                    .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
            )
            // The fixed viewport bounds both scroll axes. The default
            // `AlignItems::Stretch` makes `scrolled` claim the full
            // GRID_VIEWPORT_W width, so the horizontal scroll has a viewport
            // narrower than the 570px columns to scroll against (R896.1 —
            // stating the cross-axis contract that was implicit before).
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(GRID_VIEWPORT_W, GRID_VIEWPORT_H))
                    .with_focusable(true),
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
    // R930 — the "of N" total is the live row count (boot N == NROWS == 4, so
    // the R891 ungrouped status assertions stand; add / remove move it).
    let total = nrows(&model);
    let status = Scene::Text(
        TextNode::styled(
            format!(
                "filter {} \u{00B7} showing {view_len} of {total}{group_suffix}",
                grid_filter_str(filter.as_ref()),
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(HEADER_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("dg_status"),
    );

    let undo_status = view_undo_status(&theme);

    Scene::Container(
        ContainerNode::new(vec![title, status, undo_status, grid])
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
            model,
            focused_row,
            focused_col,
            editing,
            editor,
            sort,
            filter,
            group_col,
            collapsed,
            use_undo(),
            Rc::new(GridUndoCtx::from_hooks()),
        ))
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            // R932 — the AI-first undo-history surface wraps the same shared
            // [`UndoStack`] the coordinator records onto (via [`use_undo`]), so
            // `query`/`invoke` at `/data_grid_undo/external/…` observe + drive
            // the identical history the cell / row mutators + keyboard use.
            // A coordinator-only extra: it paints nothing, not a focus stop.
            ExtraExternal::new(UNDO_TAG, Box::new(UndoStackExternal::new(use_undo()))),
            // R1250 — the shared commit-on-blur inline editor (lifted SSOT).
            blur_committing_field_extra(EDIT_TF_TAG),
        ]
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
            Some(GRID_TAG) => apply_key_grid(scene, key, modifiers),
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
        // R940 — while a choice dropdown is open + visible, the active descendant
        // is the popup OPTION (the option carries `focused`, and
        // `access_focus_target` rings it), NOT a grid cell; suppress the cell
        // focus so the AT has exactly ONE active descendant (the R873 paint + a11y
        // one-gate discipline — `usize::MAX` matches no valid row / col).
        let popup_active = popup_pos_live().is_some();
        let (focused_row, focused_col) = if popup_active {
            (usize::MAX, usize::MAX)
        } else {
            (use_focused_row().get(), use_focused_col().get())
        };
        let sort = use_sort().get();
        let filter = use_filter().get();
        let group_col = use_group_col().get();
        let collapsed = use_collapsed().get();
        // R940 — the open choice dropdown's `listbox` nodes are appended below
        // (gated on visibility in [`popup_listbox_nodes`]), so the AT announces
        // the dropdown options when one is open.
        let editing = use_editing_cell().get();
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
            // R1372 §5.38 — per-cell aria-selected from the cell-range rectangle
            // (the R952 `GridCell.selected` axis, now unblocked): `order` IS the
            // visible data sequence, so the visible-position selection bounds map
            // 1:1 to these rows. `Some(bool)` per cell while a range is active,
            // `None` (omit) otherwise — the R953 SelectItems a11y shape.
            let sel_bounds =
                cell_selection_bounds(&order, use_cell_anchor().get(), focused_row, focused_col);
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
                            selected: sel_bounds.map(|(p0, c0, p1, c1)| {
                                order[p0..=p1].contains(&row) && (c0..=c1).contains(&col)
                            }),
                        })
                        .collect(),
                })
                .collect();
            // R937.1 — the reorder is exposed to AT NOT as a row-child button (a
            // `button` is not a valid child of a grid `row`, and the painted grip
            // only arms a *drag* — an AT activation of it would never move a row),
            // but through [`access_child_invoke`](WidgetA11y::access_child_invoke):
            // an Increment / Decrement action on the focused cell moves its row
            // down / up (the hello-dnd reorder-a11y pattern), so the AT path is
            // valid + actionable. Keyboard Alt+Arrow is the keystroke twin.
            let mut nodes =
                grid_table_nodes(GRID_TAG, "Asset table", false, "dg_header", &columns, &rows);
            // R980/R982 §5.40 — augment the flat grid with AT-reachable reset
            // affordances (cell / column / row), the R967.1 carry. The flat
            // order IS the visible data sources (no group folds rows away).
            emit_reset_affordances(&mut nodes, &model, &order);
            // R940 — the flatten is the Data-only order ungrouped (the popup gate
            // indexes the SAME visual sequence the rows paint in).
            let vis: Vec<GroupRow> = order
                .iter()
                .map(|&source| GroupRow::Data { source })
                .collect();
            nodes.extend(popup_listbox_nodes(&model, &vis, editing));
            return nodes;
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
        let mut nodes = grouped_grid_access_nodes(
            &spec,
            &rows,
            VisibleWindow {
                first: 0,
                count: rows.len(),
            },
            |g| labels.get(g).cloned().unwrap_or_default(),
            cell_tag,
            |source, col| {
                format!(
                    "{}: {}",
                    COL_NAMES[col],
                    model
                        .get(idx(source, col))
                        .map(CellValue::display)
                        .unwrap_or_default()
                )
            },
            group_row_a11y_tag,
        );
        // R983 §5.40 — augment the grouped treegrid with the SAME AT-reachable
        // reset affordances (cell / column / row) the flat grid emits (R980/R982):
        // a painted reset dot is now AT-reachable + activatable in BOTH grid modes.
        // The VISIBLE data sources (group headers hold no cells; a collapsed
        // group's members drop out of `rows`) feed the shared emitter — a UX choice
        // so collapsed rows show no reset, NOT a dangling-node guard (that is
        // structural in `attach_child_button` since R984.1). `rows` IS the emitted
        // window here (the a11y projection passes `count: rows.len()`).
        let visible_sources: Vec<usize> = rows.iter().filter_map(GroupRow::source).collect();
        emit_reset_affordances(&mut nodes, &model, &visible_sources);
        // R1372.1 — the flat grid announces per-cell aria-selected; the grouped
        // treegrid must too. The substrate's grouped selection axis is row-level
        // only, so the consumer stamps the cell range onto the gridcells (the
        // emit_reset_affordances augmentation pattern).
        stamp_cell_selection(&mut nodes, &visible_sources, (focused_row, focused_col));
        // R940 — the open dropdown's `listbox` nodes (gated on the editing row
        // being present in this same grouped flatten — a collapsed group hides it).
        nodes.extend(popup_listbox_nodes(&model, &rows, editing));
        nodes
    }

    /// R940 — the `aria-activedescendant` target. When a visible choice dropdown
    /// is open while the grid holds focus, the active descendant is the cursor
    /// OPTION (the combobox a11y shape: the grid keeps focus, the popup is its
    /// roving descendant), gated on the same [`popup_pos_live`] visibility the
    /// paint and `access_node` use. Otherwise the default — ring `focused`
    /// atomically (the focused gridcell's own `focused` flag in `access_node`
    /// marks the cell active descendant, the pre-R940 behaviour).
    fn access_focus_target(_state: &RootState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(GRID_TAG)
            && let Some((_, col, _)) = popup_pos_live()
        {
            let cur = use_popup_cursor().get().unwrap_or(0);
            // R943 — the active descendant is the cursor's option (choice popup) or
            // swatch (colour popup); the composite suffix must match the open
            // popup's kind, or the AT points at a tag the popup never paints (a
            // dangling `aria-activedescendant`). The same per-kind tag prefix the
            // popup paint + `popup_listbox_nodes` stamp.
            let prefix = if COL_KINDS[col] == CellKind::Color {
                COLOR_SW_PREFIX
            } else {
                CHOICE_OPT_PREFIX
            };
            return Some(AccessFocus::composite(
                GRID_TAG,
                format!("{GRID_TAG}#{prefix}{cur}"),
            ));
        }
        focused.map(AccessFocus::atomic)
    }

    /// R937.1 — AT-driven row reorder: an `Increment` / `Decrement` action on a
    /// cell (the activedescendant rides the focused gridcell) moves that cell's
    /// ROW down / up through the SAME `move_row` funnel the keyboard / RPC / drag
    /// use — the hello-dnd reorder-a11y pattern. This is the VALID + ACTIONABLE
    /// AT reorder path (a `button` is not a valid child of a grid `row`, and the
    /// painted grip only arms a *drag* — an AT activation of it never moves a row;
    /// the R937 session-review caught that). `move_row` rejects under a sort /
    /// filter / group, so reorder is gated identically for AT, keyboard and RPC.
    /// Other actions return `false` → the shell's focus chain handles them.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        // R980 §5.40 — an AT Click / Default on a `reset…` child routes to the
        // reset `send` funnel (the pointer twin): the cell / column reset `button`
        // AccessNode is now emitted (the R967.1 carry cleared), so AT targets it
        // and this routes the activation through the same `send` wire a reset-dot
        // pointer click drains. (The row reset awaits a `rowheader` host.)
        if matches!(action, AccessAction::Click | AccessAction::Default)
            && sub_tag.starts_with(RESET_PREFIX)
        {
            let Some(intro) = external_mut(scene, GRID_TAG) else {
                return false;
            };
            return intro
                .invoke(
                    "send",
                    IntrospectValue::Text(format!("{sub_tag}:PointerUp")),
                )
                .is_ok();
        }
        // R937.1 — the row reorder rides the focused cell's node via Increment /
        // Decrement (a `button` is not a valid child of a grid `row`); Alt+Arrow
        // is the keystroke twin.
        let delta: isize = match action {
            AccessAction::Increment => 1,
            AccessAction::Decrement => -1,
            _ => return false,
        };
        let Some(row) = GridSendKey::parse(sub_tag).and_then(GridSendKey::row) else {
            return false;
        };
        let Some(to) = row.checked_add_signed(delta) else {
            return false;
        };
        let Some(intro) = external_mut(scene, GRID_TAG) else {
            return false;
        };
        matches!(
            intro.invoke("move_row", IntrospectValue::Text(format!("{row},{to}"))),
            Ok(IntrospectValue::Bool(true))
        )
    }
}

impl WidgetView for DataGridView {
    type Renderer = HelloDataGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<DataGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    // R940 — the popup-a11y test asserts the listbox role; AriaRole is test-only
    // here (the production a11y path names roles through the substrate builders).
    use pinion_a11y::AriaRole;
    use pinion_core::scene::ExternalNode;

    fn boot_scene() -> Scene {
        let mut children = vec![Scene::External(
            ExternalNode::new(DataGridView::create_external()).with_tag(GRID_TAG),
        )];
        for extra in DataGridView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
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
            assert_eq!(intro.query("col_count"), Some(IntrospectValue::Int(6)));
            assert_eq!(
                intro.query("col_name.0"),
                Some(IntrospectValue::Text("Asset".to_owned()))
            );
            assert_eq!(
                intro.query("col_name.5"),
                Some(IntrospectValue::Text("Tint".to_owned()))
            );
            assert_eq!(
                intro.query("col_kind.2"),
                Some(IntrospectValue::Text("int".to_owned()))
            );
            assert_eq!(
                intro.query("col_kind.4"),
                Some(IntrospectValue::Text("bool".to_owned()))
            );
            // R943 — the Tint column is a colour cell.
            assert_eq!(
                intro.query("col_kind.5"),
                Some(IntrospectValue::Text("color".to_owned()))
            );
            assert_eq!(
                intro.query("value.1.0"),
                Some(IntrospectValue::Text("Tree".to_owned()))
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(
                intro
                    .intervene("value.0.2", IntrospectValue::Int(7))
                    .is_ok()
            );
            assert_eq!(
                intro.intervene("value.0.2", IntrospectValue::Text("no".to_owned())),
                Err(InterveneError::TypeMismatch),
                "int column rejects text",
            );
            assert!(
                intro
                    .intervene("value.3.3", IntrospectValue::Float(9.5))
                    .is_ok()
            );
            assert_eq!(intro.query("value.0.2"), Some(IntrospectValue::Int(7)));
            assert_eq!(intro.query("value.3.3"), Some(IntrospectValue::Float(9.5)));
        });
    }

    #[test]
    fn r837_intervene_focus_clamps_both_axes() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(
                intro
                    .intervene("focused_row", IntrospectValue::Int(99))
                    .is_ok()
            );
            assert!(
                intro
                    .intervene("focused_col", IntrospectValue::Int(99))
                    .is_ok()
            );
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(3)));
            assert_eq!(intro.query("focused_col"), Some(IntrospectValue::Int(5)));
        });
    }

    // ─── R960 §5.38 §5.40 — per-cell modified-from-default + reset ───────────

    #[test]
    fn r960_cell_modified_and_reset_to_column_default() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // The Count column default is 0; the seed's row-0 Count is 1 -> modified.
            assert_eq!(intro.query("value.0.2"), Some(IntrospectValue::Int(1)));
            assert_eq!(
                intro.query("modified.0.2"),
                Some(IntrospectValue::Bool(true))
            );
            // Reset that cell to its column default (0); it was modified -> true.
            assert_eq!(
                intro
                    .invoke("reset", IntrospectValue::Text("0_2".to_owned()))
                    .unwrap(),
                IntrospectValue::Bool(true),
            );
            assert_eq!(
                intro.query("value.0.2"),
                Some(IntrospectValue::Int(0)),
                "reset to column default"
            );
            assert_eq!(
                intro.query("modified.0.2"),
                Some(IntrospectValue::Bool(false))
            );
            // Re-resetting an already-default cell is an idempotent false no-op.
            assert_eq!(
                intro
                    .invoke("reset", IntrospectValue::Text("0_2".to_owned()))
                    .unwrap(),
                IntrospectValue::Bool(false),
            );
        });
    }

    #[test]
    fn r960_edit_to_default_clears_modified() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Editing a cell TO its column default clears the modified flag — the
            // indicator tracks the value, not a separate "dirty" bit.
            assert!(
                intro
                    .intervene("value.0.2", IntrospectValue::Int(0))
                    .is_ok()
            );
            assert_eq!(
                intro.query("modified.0.2"),
                Some(IntrospectValue::Bool(false))
            );
            // Editing away from default sets it again.
            assert!(
                intro
                    .intervene("value.0.2", IntrospectValue::Int(7))
                    .is_ok()
            );
            assert_eq!(
                intro.query("modified.0.2"),
                Some(IntrospectValue::Bool(true))
            );
        });
    }

    #[test]
    fn r960_reset_all_clears_every_modified_cell() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let Some(IntrospectValue::Int(n)) = intro.query("modified_count") else {
                panic!("modified_count is an int");
            };
            assert!(n > 0, "the seed differs from the empty column defaults");
            // reset_all clears exactly that many cells.
            assert_eq!(
                intro.invoke("reset_all", IntrospectValue::Null).unwrap(),
                IntrospectValue::Int(n)
            );
            assert_eq!(
                intro.query("modified_count"),
                Some(IntrospectValue::Int(0)),
                "all at default"
            );
        });
    }

    /// R965 — `reset_row` clears only its own row, leaving the rest modified.
    #[test]
    fn r965_reset_row_clears_only_that_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Clean slate, then modify row 0 in TWO columns (Count col 2 + Asset
            // col 0) plus the Count cell in row 1. The two columns in row 0 catch
            // a buggy reset_row that only ever touched one column.
            intro.invoke("reset_all", IntrospectValue::Null).unwrap();
            intro
                .intervene("value.0.2", IntrospectValue::Int(50))
                .unwrap();
            intro
                .intervene("value.0.0", IntrospectValue::Text("renamed".to_owned()))
                .unwrap();
            intro
                .intervene("value.1.2", IntrospectValue::Int(60))
                .unwrap();
            assert_eq!(intro.query("modified_count"), Some(IntrospectValue::Int(3)));
            // reset_row(0) clears BOTH of row 0's cells (cols 0 and 2); row 1 stays.
            assert_eq!(
                intro.invoke("reset_row", IntrospectValue::Int(0)).unwrap(),
                IntrospectValue::Int(2)
            );
            assert_eq!(
                intro.query("modified.0.0"),
                Some(IntrospectValue::Bool(false)),
                "row 0 col 0 reset"
            );
            assert_eq!(
                intro.query("modified.0.2"),
                Some(IntrospectValue::Bool(false)),
                "row 0 col 2 reset"
            );
            assert_eq!(
                intro.query("modified.1.2"),
                Some(IntrospectValue::Bool(true)),
                "row 1 untouched"
            );
            assert_eq!(intro.query("modified_count"), Some(IntrospectValue::Int(1)));
            // An already-default row is a 0 no-op.
            assert_eq!(
                intro.invoke("reset_row", IntrospectValue::Int(0)).unwrap(),
                IntrospectValue::Int(0)
            );
        });
    }

    /// R965 — `reset_col` clears only its own column, across all rows.
    #[test]
    fn r965_reset_col_clears_only_that_column() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Clean slate, then modify two Count cells (col 2) + one Asset cell (col 0).
            intro.invoke("reset_all", IntrospectValue::Null).unwrap();
            intro
                .intervene("value.0.2", IntrospectValue::Int(50))
                .unwrap();
            intro
                .intervene("value.1.2", IntrospectValue::Int(60))
                .unwrap();
            intro
                .intervene("value.0.0", IntrospectValue::Text("renamed".to_owned()))
                .unwrap();
            assert_eq!(intro.query("modified_count"), Some(IntrospectValue::Int(3)));
            // reset_col(2) clears both Count cells; the Asset cell (col 0) is untouched.
            assert_eq!(
                intro.invoke("reset_col", IntrospectValue::Int(2)).unwrap(),
                IntrospectValue::Int(2)
            );
            assert_eq!(
                intro.query("modified.0.2"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                intro.query("modified.1.2"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                intro.query("modified.0.0"),
                Some(IntrospectValue::Bool(true)),
                "col 0 untouched"
            );
            assert_eq!(intro.query("modified_count"), Some(IntrospectValue::Int(1)));
        });
    }

    /// R965 — an out-of-range row / column is a 0 no-op; a non-Int arg is a type
    /// mismatch (never a panic).
    #[test]
    fn r965_reset_row_col_out_of_range_and_bad_arg() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("reset_row", IntrospectValue::Int(99)).unwrap(),
                IntrospectValue::Int(0)
            );
            assert_eq!(
                intro.invoke("reset_col", IntrospectValue::Int(99)).unwrap(),
                IntrospectValue::Int(0)
            );
            assert!(matches!(
                intro.invoke("reset_row", IntrospectValue::Text("x".to_owned())),
                Err(InvokeError::TypeMismatch),
            ));
            assert!(matches!(
                intro.invoke("reset_col", IntrospectValue::Null),
                Err(InvokeError::TypeMismatch),
            ));
        });
    }

    // ─── R966: row / column-header reset affordance + a11y ────────────

    #[test]
    fn r966_reset_target_decodes_cell_row_and_column() {
        // The shared decode grammar (the remainder AFTER `RESET_PREFIX`): a
        // `row` / `col` letter prefix is a 1-D bulk reset; a digit-leading
        // remainder reuses the `GridSendKey::Cell` `<row>_<col>` address.
        assert_eq!(
            ResetTarget::parse("row3"),
            Some(ResetTarget::Row { row: 3 })
        );
        assert_eq!(
            ResetTarget::parse("col2"),
            Some(ResetTarget::Col { col: 2 })
        );
        assert_eq!(
            ResetTarget::parse("0_2"),
            Some(ResetTarget::Cell { row: 0, col: 2 })
        );
        // The letter prefixes never alias the digit-leading cell form.
        assert_eq!(
            ResetTarget::parse("12_5"),
            Some(ResetTarget::Cell { row: 12, col: 5 })
        );
        // Malformed remainders decode to None (a no-op, never a panic).
        assert_eq!(ResetTarget::parse("row"), None, "no index");
        assert_eq!(ResetTarget::parse("colx"), None, "non-numeric index");
        assert_eq!(
            ResetTarget::parse("h2"),
            None,
            "a header key is not a reset target"
        );
    }

    #[test]
    fn r966_col_and_row_modified_predicates() {
        // A clean model (every cell at its column default) reports no modified
        // row or column; modifying one cell flips exactly its row + column.
        let mut model: Vec<CellValue> =
            (0..NROWS * NCOLS).map(|i| col_default(i % NCOLS)).collect();
        assert!(!col_modified(&model, 2), "clean column");
        assert!(!row_modified(&model, 1), "clean row");
        model[idx(1, 2)] = CellValue::Int(7); // col 2's default is 0
        assert!(
            col_modified(&model, 2),
            "column 2 now holds a modified cell"
        );
        assert!(row_modified(&model, 1), "row 1 now holds a modified cell");
        assert!(!col_modified(&model, 0), "an untouched column stays clean");
        assert!(!row_modified(&model, 0), "an untouched row stays clean");
        // Out-of-range axes are not modified (graceful, never a panic).
        assert!(!col_modified(&model, 99));
        assert!(!row_modified(&model, 99));
    }

    #[test]
    fn r966_resetcol_resetrow_pointer_send_resets_the_axis() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            intro.invoke("reset_all", IntrospectValue::Null).unwrap();
            // Modify the Count column (col 2) in rows 0 + 1, and the Asset cell
            // (col 0) in row 0 — so a column reset that wrongly cleared a row
            // (or vice versa) is caught.
            intro
                .intervene("value.0.2", IntrospectValue::Int(50))
                .unwrap();
            intro
                .intervene("value.1.2", IntrospectValue::Int(60))
                .unwrap();
            intro
                .intervene("value.0.0", IntrospectValue::Text("x".to_owned()))
                .unwrap();
            assert_eq!(intro.query("modified_count"), Some(IntrospectValue::Int(3)));
            // A `resetcol2` pointer send clears the whole Count column, leaving
            // the Asset cell modified.
            intro
                .invoke(
                    "send",
                    IntrospectValue::Text("resetcol2:PointerUp".to_owned()),
                )
                .unwrap();
            assert_eq!(
                intro.query("col_modified.2"),
                Some(IntrospectValue::Bool(false)),
                "column cleared"
            );
            assert_eq!(
                intro.query("modified.0.0"),
                Some(IntrospectValue::Bool(true)),
                "other column untouched"
            );
            // A `resetrow0` pointer send clears the remaining row-0 cell.
            intro
                .invoke(
                    "send",
                    IntrospectValue::Text("resetrow0:PointerUp".to_owned()),
                )
                .unwrap();
            assert_eq!(
                intro.query("row_modified.0"),
                Some(IntrospectValue::Bool(false)),
                "row cleared"
            );
            assert_eq!(intro.query("modified_count"), Some(IntrospectValue::Int(0)));
            // PointerDown is not an activation (only PointerUp resets).
            intro
                .intervene("value.0.2", IntrospectValue::Int(9))
                .unwrap();
            intro
                .invoke(
                    "send",
                    IntrospectValue::Text("resetcol2:PointerDown".to_owned()),
                )
                .unwrap();
            assert_eq!(
                intro.query("col_modified.2"),
                Some(IntrospectValue::Bool(true)),
                "PointerDown does not reset"
            );
        });
    }

    #[test]
    fn r966_col_row_modified_query_peers() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            intro.invoke("reset_all", IntrospectValue::Null).unwrap();
            assert_eq!(
                intro.query("col_modified.2"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                intro.query("row_modified.0"),
                Some(IntrospectValue::Bool(false))
            );
            intro
                .intervene("value.0.2", IntrospectValue::Int(5))
                .unwrap();
            assert_eq!(
                intro.query("col_modified.2"),
                Some(IntrospectValue::Bool(true)),
                "col 2 now modified"
            );
            assert_eq!(
                intro.query("row_modified.0"),
                Some(IntrospectValue::Bool(true)),
                "row 0 now modified"
            );
            assert_eq!(
                intro.query("col_modified.0"),
                Some(IntrospectValue::Bool(false)),
                "col 0 still clean"
            );
            // Out-of-range axes read false, not None (graceful AI-first read).
            assert_eq!(
                intro.query("col_modified.99"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                intro.query("row_modified.99"),
                Some(IntrospectValue::Bool(false))
            );
        });
    }

    #[test]
    fn r980_reset_is_at_reachable_via_access_child_invoke() {
        // R980 §5.40 — the cell / column reset is now AT-reachable (the R967.1
        // carry cleared): `access_node` emits a reset `button` child of the
        // modified gridcell / columnheader (verified over `scene/access`), and an
        // AT Click routes through the SAME `send` wire the reset-dot pointer click
        // drains — so AT reset and pointer reset are identical. The closing twin
        // of `r966_resetcol_resetrow_pointer_send_resets_the_axis`. (R982 added the
        // `rowheader` host for the row reset; R983 extends all three to grouped.)
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // An AT Click on the Asset (col 0) reset button clears the whole column.
            assert!(
                DataGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "resetcol0",
                    AccessAction::Click
                ),
                "an AT Click on a column reset is handled",
            );
            // An AT Click on a still-modified cell (0,1) reset clears just that cell.
            assert!(
                DataGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "reset0_1",
                    AccessAction::Click
                ),
                "an AT Click on a cell reset is handled",
            );
            // R982 — an AT Click on a ROW reset (rowheader-hosted button) clears
            // the whole row, routed through the same reset-prefix send branch.
            assert!(
                DataGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "resetrow2",
                    AccessAction::Click
                ),
                "an AT Click on a row reset is handled",
            );
            // A non-reset Click still falls through to the focus chain (false).
            assert!(
                !DataGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "0_0",
                    AccessAction::Click
                ),
                "a non-reset Click falls through (no reset prefix)",
            );
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("col_modified.0"),
                Some(IntrospectValue::Bool(false)),
                "the AT column reset cleared the Asset column",
            );
            assert_eq!(
                intro.query("modified.0.1"),
                Some(IntrospectValue::Bool(false)),
                "the AT cell reset cleared cell (0,1)",
            );
            assert_eq!(
                intro.query("row_modified.2"),
                Some(IntrospectValue::Bool(false)),
                "the AT row reset cleared row 2",
            );
        });
    }

    // R983 §5.40 — group the boot grid by Type (col 1): group 0 = sprite
    // (sources 0, 2), group 1 = mesh (sources 1, 3). The boot grid is fully
    // modified, so every visible cell / column / row is resettable.
    fn group_by_type(scene: &mut Scene) {
        let node = scene
            .find_external_with_tag_mut(GRID_TAG)
            .expect("grid present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
    }

    #[test]
    fn r983_grouped_treegrid_emits_reset_affordances() {
        // R983 §5.40 — the grouped treegrid emits the SAME AT-reachable reset
        // affordances (cell / column / row) as the flat grid (R980/R982), closing
        // the R982 grouped carry. The reset routing is mode-independent, so only
        // the emission was missing — this is the emission half.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            group_by_type(&mut scene);
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), None);
            let by_tag = |tag: &str| nodes.iter().find(|n| n.tag == tag);

            // (A) a treegrid with one named rowheader per VISIBLE data row (4 rows
            // across the two expanded groups), each leading its row.
            assert_eq!(
                by_tag(GRID_TAG).expect("grid root").role,
                AriaRole::TreeGrid,
                "grouped grid is a treegrid",
            );
            assert_eq!(
                nodes
                    .iter()
                    .filter(|n| n.role == AriaRole::RowHeader)
                    .count(),
                4,
                "a rowheader per visible data row",
            );
            // Data rows are level-2 `row`s; the 2 group headers are level-1 `row`s
            // and the column-header row carries no level.
            let data_rows: Vec<_> = nodes
                .iter()
                .filter(|n| n.role == AriaRole::Row && n.level == Some(2))
                .collect();
            assert_eq!(data_rows.len(), 4, "4 visible data rows (2 per group)");
            for r in &data_rows {
                let first = by_tag(&r.children[0]).expect("first child present");
                assert_eq!(
                    first.role,
                    AriaRole::RowHeader,
                    "each grouped data row leads with its rowheader (the row-reset host)",
                );
            }

            // (B) a modified cell, column, and row each host an AT-reachable reset
            // button. Targets are picked from the live model (not hardcoded), so the
            // test states the invariant, not a fixed boot layout. The sprite group's
            // sources (0, 2) are visible.
            let model = use_data_model().get();
            let (mr, mc) = (0..NROWS)
                .flat_map(|r| (0..NCOLS).map(move |c| (r, c)))
                .find(|&(r, c)| (r == 0 || r == 2) && cell_value_modified(&model, r, c))
                .expect("a visible sprite-group cell is modified at boot");
            let cell = by_tag(&cell_tag(mr, mc)).expect("the modified gridcell is present");
            assert!(
                cell.children.contains(&reset_cell_tag(mr, mc)),
                "a modified grouped gridcell hosts a reset button child",
            );
            assert_eq!(
                by_tag(&reset_cell_tag(mr, mc))
                    .expect("cell reset node present")
                    .role,
                AriaRole::Button,
                "the grouped cell reset is an AT-reachable button",
            );
            let modified_col = (0..NCOLS)
                .find(|&c| col_modified(&model, c))
                .expect("a column is modified");
            let colh = by_tag(&col_header_tag(modified_col)).expect("columnheader present");
            assert!(
                colh.children.contains(&reset_col_tag(modified_col)),
                "a modified column header hosts a reset button child",
            );
            let modified_row = (0..NROWS)
                .find(|&r| row_modified(&model, r))
                .expect("a row is modified");
            let rh = by_tag(&handle_tag(modified_row)).expect("rowheader present");
            assert_eq!(rh.role, AriaRole::RowHeader);
            assert!(
                rh.children.contains(&reset_row_tag(modified_row)),
                "a modified row's rowheader hosts a reset button child",
            );
            // Every visible data row is modified at boot -> a row reset per row.
            assert_eq!(
                nodes
                    .iter()
                    .filter(|n| n.role == AriaRole::Button && n.tag.contains("#resetrow"))
                    .count(),
                4,
                "one row reset per visible modified row",
            );
        });
    }

    #[test]
    fn r983_grouped_collapse_windows_out_reset_affordances() {
        // R983 §5.40 — a collapsed group's data rows window out of the flatten, so
        // their cell + row reset buttons leave the tree with NO orphan onto an
        // absent node (the dangling-node guard). A column reset persists — its
        // predicate is model-wide, not window-scoped.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            group_by_type(&mut scene);
            // Collapse the sprite group (0): sources 0 + 2 window out.
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("toggle_group", IntrospectValue::Int(0));
            }
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), None);
            assert_eq!(
                nodes
                    .iter()
                    .filter(|n| n.role == AriaRole::RowHeader)
                    .count(),
                2,
                "only the mesh group's 2 rows remain after collapse",
            );
            // No cell / row reset button addresses a collapsed source (0 or 2).
            for n in nodes.iter().filter(|n| n.role == AriaRole::Button) {
                assert!(
                    !n.tag.contains("#reset0_") && !n.tag.contains("#reset2_"),
                    "no cell reset orphans onto a collapsed source: {}",
                    n.tag,
                );
                assert!(
                    n.tag != reset_row_tag(0) && n.tag != reset_row_tag(2),
                    "no row reset orphans onto a collapsed source: {}",
                    n.tag,
                );
            }
            // The collapsed sources' gridcells / rowheaders are gone entirely.
            assert!(
                !nodes.iter().any(|n| n.tag == cell_tag(0, 1)),
                "a collapsed source's gridcell is windowed out of the tree",
            );
            assert!(
                !nodes.iter().any(|n| n.tag == handle_tag(0)),
                "a collapsed source's rowheader is windowed out of the tree",
            );
            // A column reset is unaffected by collapse: `col_modified` reads the
            // whole model, not the visible window, so Asset's header keeps it.
            assert!(
                nodes.iter().any(|n| n.tag == reset_col_tag(0)),
                "the column reset persists through collapse (model-wide predicate)",
            );
            // R984.1 — the POSITIVE orphan-freedom invariant (M3): EVERY reset
            // button node is referenced by exactly one present host's children.
            // The prior substring-only check would have passed a dangling button
            // whose tag did not contain a collapsed-source marker; this pins the
            // actual invariant that `attach_child_button` now guarantees.
            for btn in nodes.iter().filter(|n| n.role == AriaRole::Button) {
                let hosts = nodes
                    .iter()
                    .filter(|n| n.children.contains(&btn.tag))
                    .count();
                assert_eq!(
                    hosts, 1,
                    "reset button {} hangs off exactly one present host",
                    btn.tag
                );
            }
        });
    }

    #[test]
    fn r960_reset_rejects_bad_key_and_out_of_range_is_noop() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // A non-cell key (a header) is not a reset target -> Rejected.
            assert!(matches!(
                intro.invoke("reset", IntrospectValue::Text("h2".to_owned())),
                Err(InvokeError::Rejected),
            ));
            assert!(matches!(
                intro.invoke("reset", IntrospectValue::Int(3)),
                Err(InvokeError::TypeMismatch),
            ));
            // An out-of-range cell is not modified -> a false no-op, never a panic.
            assert_eq!(
                intro
                    .invoke("reset", IntrospectValue::Text("99_2".to_owned()))
                    .unwrap(),
                IntrospectValue::Bool(false),
            );
        });
    }

    #[test]
    fn r837_click_focuses_cell_and_toggles_bool() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Cell (2,4) is the Active bool = false.
            let _ = intro.invoke("send", IntrospectValue::Text("2_4:PointerUp".to_owned()));
            assert_eq!(intro.query("focused_row"), Some(IntrospectValue::Int(2)));
            assert_eq!(intro.query("focused_col"), Some(IntrospectValue::Int(4)));
            assert_eq!(
                intro.query("value.2.4"),
                Some(IntrospectValue::Bool(true)),
                "toggled"
            );
            // A click on a text cell focuses but does not toggle.
            let _ = intro.invoke("send", IntrospectValue::Text("0_0:PointerUp".to_owned()));
            assert_eq!(
                intro.query("value.0.0"),
                Some(IntrospectValue::Text("Hero".to_owned()))
            );
        });
    }

    #[test]
    fn r837_double_click_begins_edit_on_editable_cell() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("send", IntrospectValue::Text("1_2:DoubleClick".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(intro.query("editing_row"), Some(IntrospectValue::Int(1)));
            assert_eq!(intro.query("editing_col"), Some(IntrospectValue::Int(2)));
            assert_eq!(
                use_text_edit_state(EDIT_TF_TAG).text(),
                "24",
                "seeded with the int value"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(1));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(2));
            assert_eq!(
                intro.invoke("begin", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true))
            );
            use_text_edit_state(EDIT_TF_TAG).set_text("250".to_owned());
            commit_edit(true);
            assert_eq!(
                grid_intro(&scene).query("value.1.2"),
                Some(IntrospectValue::Int(250))
            );
            assert_eq!(
                grid_intro(&scene).query("editing_row"),
                Some(IntrospectValue::Null)
            );
        });
    }

    #[test]
    fn r837_commit_malformed_reverts() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(3));
            let _ = intro.invoke("begin", IntrospectValue::Null);
            use_text_edit_state(EDIT_TF_TAG).set_text("xyz".to_owned());
            commit_edit(true);
            assert_eq!(
                grid_intro(&scene).query("value.0.3"),
                Some(IntrospectValue::Float(1.0))
            );
        });
    }

    #[test]
    fn r837_keyboard_roves_both_axes_and_clamps() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowRight",
                m
            ));
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                m
            ));
            assert_eq!(use_focused_row().get(), 1);
            assert_eq!(use_focused_col().get(), 1);
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "End",
                m
            ));
            assert_eq!(use_focused_col().get(), NCOLS - 1);
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowRight",
                m
            ));
            assert_eq!(
                use_focused_col().get(),
                NCOLS - 1,
                "clamps at the last column"
            );
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Home",
                m
            ));
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
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Space",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("value.0.4"),
                Some(IntrospectValue::Bool(false))
            );
            // Focus the Count int of row 0 (col 2) and Enter -> edit mode.
            let _ = scene
                .find_external_with_tag_mut(GRID_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .map(|i| i.intervene("focused_col", IntrospectValue::Int(2)));
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Enter",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("editing_col"),
                Some(IntrospectValue::Int(2))
            );
        });
    }

    #[test]
    fn r837_edit_float_gate_allows_dot_drops_letter() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(3)); // Scale (float)
            let _ = intro.invoke("begin", IntrospectValue::Null);
            use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
            use_text_edit_state(EDIT_TF_TAG).set_caret(0);
            let m = Modifiers::empty();
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(EDIT_TF_TAG),
                "2",
                m
            ));
            assert!(
                DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), ".", m),
                "float accepts dot"
            );
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(EDIT_TF_TAG),
                "5",
                m
            ));
            assert!(
                !DataGridView::apply_key(&mut scene, Some(EDIT_TF_TAG), "z", m),
                "letter dropped"
            );
            assert_eq!(use_text_edit_state(EDIT_TF_TAG).text(), "2.5");
        });
    }

    #[test]
    fn r837_access_node_emits_grid_with_active_cell() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            use_focused_row().set(2);
            use_focused_col().set(2);
            let model = use_data_model().get();
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            // grid + header row + NCOLS columnheaders + NROWS rows + NROWS*NCOLS
            // cells (R837), plus R980 — one reset `button` per modified cell + per
            // modified column, plus R982 — a `rowheader` per data row + one reset
            // button per modified row (the boot seed differs from the column
            // defaults, so the grid boots fully modified). R937.1 — still NO
            // per-row reorder button (the AT reorder is an Increment/Decrement
            // action via `access_child_invoke`, adding no node).
            let modified_cells = (0..NROWS)
                .flat_map(|r| (0..NCOLS).map(move |c| (r, c)))
                .filter(|&(r, c)| cell_value_modified(&model, r, c))
                .count();
            let modified_cols = (0..NCOLS).filter(|&c| col_modified(&model, c)).count();
            let modified_rows = (0..NROWS).filter(|&r| row_modified(&model, r)).count();
            let skeleton = 1 + 1 + NCOLS + NROWS + NROWS * NCOLS;
            assert_eq!(
                nodes.len(),
                skeleton + NROWS + modified_cells + modified_cols + modified_rows,
            );
            assert_eq!(nodes[0].role, pinion_a11y::AriaRole::Grid);
            let active = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#2_2"))
                .expect("focused cell present");
            assert!(
                active.state.focused,
                "the focused cell is the active descendant"
            );
            assert_eq!(active.name.as_deref(), Some("Count: 99"));
            // R937.1 / R980 — a `button` is a valid child of a `gridcell` /
            // `columnheader` but NOT of a bare grid `row`. R980's reset buttons are
            // emitted (the grid boots modified) and each hangs off a cell-level
            // host, never a row (the invalid-nesting bug the session-review caught).
            let button_tags: Vec<&str> = nodes
                .iter()
                .filter(|n| n.role == pinion_a11y::AriaRole::Button)
                .map(|n| n.tag.as_str())
                .collect();
            assert!(
                !button_tags.is_empty(),
                "R980 reset buttons are emitted on the modified grid"
            );
            for row in nodes
                .iter()
                .filter(|n| n.role == pinion_a11y::AriaRole::Row)
            {
                assert!(
                    row.children
                        .iter()
                        .all(|c| !button_tags.contains(&c.as_str())),
                    "no button is a direct child of a grid row",
                );
            }
            // R982 — each DATA row leads with a `rowheader` cell (the WAI-ARIA
            // host for the row reset button); the header row keeps its
            // columnheaders. A modified row's reset button hangs off that
            // rowheader, never the row.
            for row in nodes
                .iter()
                .filter(|n| n.role == pinion_a11y::AriaRole::Row && n.tag != "dg_header")
            {
                let first = row.children.first().map(String::as_str);
                let rh = nodes.iter().find(|n| Some(n.tag.as_str()) == first);
                assert_eq!(
                    rh.map(|n| n.role),
                    Some(pinion_a11y::AriaRole::RowHeader),
                    "each data row leads with a rowheader cell",
                );
            }
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
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#0_0")),
                "cell (0,0) painted"
            );
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}#3_4")),
                "cell (3,4) painted"
            );
            assert!(
                !scene.contains_tag(EDIT_TF_TAG),
                "no inline field when not editing"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.query("sort"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
            assert_eq!(
                intro.query("source_at.1"),
                Some(IntrospectValue::Int(1)),
                "identity"
            );

            let _ = intro.invoke("send", IntrospectValue::Text("h2:PointerUp".to_owned()));
            assert_eq!(
                intro.query("sort"),
                Some(IntrospectValue::Text("2:ascending".to_owned()))
            );
            for (pos, src) in [(0, 0), (1, 3), (2, 1), (3, 2)] {
                assert_eq!(
                    intro.query(&format!("source_at.{pos}")),
                    Some(IntrospectValue::Int(src)),
                    "stable ascending order",
                );
            }

            let _ = intro.invoke("send", IntrospectValue::Text("h2:PointerUp".to_owned()));
            assert_eq!(
                intro.query("sort"),
                Some(IntrospectValue::Text("2:descending".to_owned()))
            );
            assert_eq!(intro.query("source_at.0"), Some(IntrospectValue::Int(2)));

            let _ = intro.invoke("send", IntrospectValue::Text("h2:PointerUp".to_owned()));
            assert_eq!(
                intro.query("sort"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("cycle_sort", IntrospectValue::Int(2));
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(2));
            assert_eq!(
                intro.invoke("begin", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true))
            );
            use_text_edit_state(EDIT_TF_TAG).set_text("500".to_owned());
            commit_edit(true);
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("value.0.2"),
                Some(IntrospectValue::Int(500)),
                "source write"
            );
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
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(3)),
                "ArrowDown steps the VISUAL sequence",
            );
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowUp",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(0))
            );
        });
    }

    #[test]
    fn r886_sort_intervene_round_trips_and_clamps() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // decode = inverse of encode (the cross-grid wire vocabulary).
            assert_eq!(
                intro.intervene("sort", IntrospectValue::Text("3:descending".to_owned())),
                Ok(())
            );
            assert_eq!(
                intro.query("sort"),
                Some(IntrospectValue::Text("3:descending".to_owned()))
            );
            // Out-of-range column clamps to unsorted (GridSortState mirror).
            assert_eq!(
                intro.intervene("sort", IntrospectValue::Text("9:ascending".to_owned())),
                Ok(())
            );
            assert_eq!(
                intro.query("sort"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
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
            assert_eq!(
                header.sort,
                Some(SortDirection::Ascending),
                "aria-sort on the key col"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.query("view_len"),
                Some(IntrospectValue::Int(4)),
                "unfiltered = NROWS"
            );
            assert_eq!(
                intro.query("filter"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
            // set_filter returns the new view_len in one round-trip.
            assert_eq!(
                intro.invoke("set_filter", IntrospectValue::Text("1=mesh".to_owned())),
                Ok(IntrospectValue::Int(2)),
                "two rows carry Type=mesh",
            );
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(2)));
            assert_eq!(
                intro.query("filter"),
                Some(IntrospectValue::Text("1=mesh".to_owned()))
            );
            // The view holds only the matching source rows, in source order.
            assert_eq!(
                intro.query("source_at.0"),
                Some(IntrospectValue::Int(1)),
                "Tree"
            );
            assert_eq!(
                intro.query("source_at.1"),
                Some(IntrospectValue::Int(3)),
                "Boss"
            );
            assert_eq!(
                intro.query("source_at.2"),
                Some(IntrospectValue::Null),
                "view shrank"
            );
            // Clearing restores the full grid.
            assert_eq!(
                intro.invoke("set_filter", IntrospectValue::Null),
                Ok(IntrospectValue::Int(4)),
            );
            assert_eq!(
                intro.query("filter"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
        });
    }

    #[test]
    fn r891_filter_wire_round_trips_read_write() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // intervene decode = inverse of query encode (the cross-grid vocab).
            assert_eq!(
                intro.intervene("filter", IntrospectValue::Text("1=sprite".to_owned())),
                Ok(())
            );
            assert_eq!(
                intro.query("filter"),
                Some(IntrospectValue::Text("1=sprite".to_owned()))
            );
            assert_eq!(
                intro.query("view_len"),
                Some(IntrospectValue::Int(2)),
                "Hero + Coin"
            );
            // Null clears (the header-less filter axis).
            assert_eq!(intro.intervene("filter", IntrospectValue::Null), Ok(()));
            assert_eq!(
                intro.query("filter"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
            // view_len is read-only; a non-text/non-null filter is a mismatch.
            assert_eq!(
                intro.intervene("view_len", IntrospectValue::Int(1)),
                Err(InterveneError::ReadOnly)
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // An out-of-range column clamps to unfiltered (GridSortState mirror).
            assert_eq!(
                intro.invoke("set_filter", IntrospectValue::Text("9=x".to_owned())),
                Ok(IntrospectValue::Int(4)),
            );
            assert_eq!(
                intro.query("filter"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
        });
    }

    #[test]
    fn r891_filter_composes_with_sort() {
        // filter Type=mesh keeps Tree (Count 24) + Boss (Count 1); sorting
        // Count ascending orders the survivors [3 (Boss, 1), 1 (Tree, 24)].
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_filter", IntrospectValue::Text("1=mesh".to_owned()));
            let _ = intro.invoke("cycle_sort", IntrospectValue::Int(2)); // Count asc
            assert_eq!(
                intro.query("view_len"),
                Some(IntrospectValue::Int(2)),
                "filter survives sort"
            );
            assert_eq!(
                intro.query("source_at.0"),
                Some(IntrospectValue::Int(3)),
                "Boss (1) first"
            );
            assert_eq!(
                intro.query("source_at.1"),
                Some(IntrospectValue::Int(1)),
                "Tree (24) second"
            );
        });
    }

    #[test]
    fn r891_filter_change_reanchors_filtered_out_cursor() {
        // Cursor on row 0 (Hero, Type=sprite); applying Type=mesh excludes it,
        // so the cursor re-anchors to the visible row at its prior visual slot
        // (Tree, source row 1) — never the silent teleport the sort fold noted.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
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
        // R940 — Type is now a Choice column, written by option index (sprite =
        // 0) through the VALUE intervene (the AI-first typed write), not an inline
        // text edit; the live filter / re-anchor behaviour is unchanged.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("set_filter", IntrospectValue::Text("1=mesh".to_owned()));
                let _ = intro.intervene("focused_row", IntrospectValue::Int(1)); // Tree
                assert_eq!(
                    intro.intervene("value.1.1", IntrospectValue::Int(0)),
                    Ok(())
                ); // -> sprite
            }
            let intro = grid_intro(&scene);
            assert_eq!(
                cell_choice(&scene, "value.1.1"),
                0,
                "source write landed (Tree -> sprite)"
            );
            assert_eq!(
                intro.query("view_len"),
                Some(IntrospectValue::Int(1)),
                "Tree dropped from view"
            );
            assert_eq!(
                intro.query("source_at.0"),
                Some(IntrospectValue::Int(3)),
                "only Boss remains"
            );
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
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(2)),
                "Coin"
            );
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(2)),
                "clamps at the last visible row",
            );
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowUp",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(0)),
                "back to Hero"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.query("group"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
            assert_eq!(
                intro.query("group_count"),
                Some(IntrospectValue::Int(0)),
                "ungrouped"
            );
            // set_group returns the new group count in one round-trip.
            assert_eq!(
                intro.invoke("set_group", IntrospectValue::Text("1".to_owned())),
                Ok(IntrospectValue::Int(2)),
                "Type has two distinct values",
            );
            assert_eq!(
                intro.query("group"),
                Some(IntrospectValue::Text("1".to_owned()))
            );
            assert_eq!(
                intro.query("visible_len"),
                Some(IntrospectValue::Int(6)),
                "2 headers + 4 data"
            );
            // kind_at disambiguates header vs data positions.
            assert_eq!(
                intro.query("kind_at.0"),
                Some(IntrospectValue::Text("header".to_owned()))
            );
            assert_eq!(
                intro.query("kind_at.1"),
                Some(IntrospectValue::Text("data".to_owned()))
            );
            // source_at: headers report Null, data rows their source.
            assert_eq!(
                intro.query("source_at.0"),
                Some(IntrospectValue::Null),
                "header"
            );
            assert_eq!(
                intro.query("source_at.1"),
                Some(IntrospectValue::Int(0)),
                "sprite: Hero"
            );
            assert_eq!(
                intro.query("source_at.2"),
                Some(IntrospectValue::Int(2)),
                "sprite: Coin"
            );
            assert_eq!(
                intro.query("source_at.4"),
                Some(IntrospectValue::Int(1)),
                "mesh: Tree"
            );
            // label_at gives a header's group label.
            assert_eq!(
                intro.query("label_at.0"),
                Some(IntrospectValue::Text("sprite".to_owned()))
            );
            assert_eq!(
                intro.query("label_at.3"),
                Some(IntrospectValue::Text("mesh".to_owned()))
            );
            assert_eq!(
                intro.query("label_at.1"),
                Some(IntrospectValue::Null),
                "data row has no label"
            );
        });
    }

    // ── R1265 — grouped flatten is O(rows · log groups), not O(rows · groups) ──

    /// R1265 — a model whose col-0 `Asset` values are all distinct (`n` rows),
    /// so grouping by col 0 is the high-cardinality case the old O(rows·groups)
    /// scan made quadratic. Every col-0 value is unique => one member per group.
    fn distinct_asset_model(n: usize) -> Vec<CellValue> {
        (0..n)
            .flat_map(|i| {
                vec![
                    CellValue::Text(format!("Asset{i}")),
                    choice_cell(TYPE_COL, i % 2),
                    CellValue::Int(i64::try_from(i).expect("row index fits i64")),
                    CellValue::Float(1.0),
                    CellValue::Bool(true),
                    swatch_cell(0),
                ]
            })
            .collect()
    }

    /// R1265 — the grouped flatten scales without dropping or duplicating a row:
    /// `n` distinct-key rows => exactly `n` headers (first-appearance order,
    /// one member each) interleaved with `n` data rows in source order. The
    /// output-preserving proof at a scale (n = 300, so groups = rows) where the
    /// old linear `group_of` scan was O(rows²) per paint — the grid's real
    /// 10k-row target.
    #[test]
    fn r1265_grouped_flatten_scales_without_drop_or_dup() {
        let n = 300;
        let model = distinct_asset_model(n);
        assert_eq!(nrows(&model), n, "the scale model has n rows");
        let table = group_table(&model, 0);
        assert_eq!(table.len(), n, "n distinct Asset values => n groups");
        assert_eq!(
            table,
            (0..n).map(|i| format!("Asset{i}")).collect::<Vec<_>>(),
            "labels stay in source-order first appearance",
        );
        let rows = visible_rows(&model, None, None, Some(0), &BTreeSet::new());
        assert_eq!(
            rows.len(),
            2 * n,
            "n headers + n data, nothing dropped/duped"
        );
        for i in 0..n {
            assert_eq!(
                rows[2 * i],
                GroupRow::Header {
                    group: i,
                    member_count: 1,
                    collapsed: false,
                },
                "header {i} sits at its interleaved slot with one member",
            );
            assert_eq!(
                rows[2 * i + 1],
                GroupRow::Data { source: i },
                "data row {i} carries its source identity in order",
            );
        }
    }

    /// R1265 — the `group_index_of` map lookup is byte-identical to the old
    /// `table.iter().position(...)` linear scan for EVERY row, including
    /// repeats: same first-appearance id, same group-0 fallback. This pins the
    /// asymptotic refactor as output-preserving, not just faster.
    #[test]
    fn r1265_group_index_matches_the_linear_scan() {
        // Out-of-order repeats: A, B, A, C, B, A => 3 groups {A:0, B:1, C:2}.
        let assets = ["A", "B", "A", "C", "B", "A"];
        let model: Vec<CellValue> = assets
            .iter()
            .flat_map(|a| {
                vec![
                    CellValue::Text((*a).to_owned()),
                    choice_cell(TYPE_COL, 0),
                    CellValue::Int(0),
                    CellValue::Float(0.0),
                    CellValue::Bool(true),
                    swatch_cell(0),
                ]
            })
            .collect();
        let table = group_table(&model, 0);
        assert_eq!(
            table,
            vec!["A", "B", "C"],
            "first-appearance order preserved with out-of-order repeats",
        );
        let ids = group_index_of(&table);
        for row in 0..nrows(&model) {
            let via_map = group_of(&model, row, 0, &ids);
            let key = model
                .get(idx(row, 0))
                .map(CellValue::display)
                .unwrap_or_default();
            let via_scan = table.iter().position(|l| *l == key).unwrap_or(0);
            assert_eq!(via_map, via_scan, "map lookup == linear scan for row {row}");
        }
    }

    #[test]
    fn r892_collapse_hides_members_and_reanchors_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0)); // Hero, sprite group
            // Collapse the sprite group (group 0); its members (0, 2) vanish.
            assert_eq!(
                intro.invoke("toggle_group", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(
                intro.query("collapsed.0"),
                Some(IntrospectValue::Bool(true))
            );
            assert_eq!(
                intro.query("visible_len"),
                Some(IntrospectValue::Int(4)),
                "2 headers + 2 mesh"
            );
            assert_eq!(
                intro.query("source_at.2"),
                Some(IntrospectValue::Int(1)),
                "first mesh row"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // intervene decode = inverse of query encode.
            assert_eq!(
                intro.intervene("group", IntrospectValue::Text("1".to_owned())),
                Ok(())
            );
            assert_eq!(
                intro.query("group"),
                Some(IntrospectValue::Text("1".to_owned()))
            );
            // collapse_all / expand_all bound the rendered rows.
            let _ = intro.invoke("collapse_all", IntrospectValue::Null);
            assert_eq!(
                intro.query("visible_len"),
                Some(IntrospectValue::Int(2)),
                "two headers only"
            );
            let _ = intro.invoke("expand_all", IntrospectValue::Null);
            assert_eq!(
                intro.query("visible_len"),
                Some(IntrospectValue::Int(6)),
                "all members back"
            );
            // collapsed.<g> is a writable bool axis.
            assert_eq!(
                intro.intervene("collapsed.1", IntrospectValue::Bool(true)),
                Ok(())
            );
            assert_eq!(
                intro.query("collapsed.1"),
                Some(IntrospectValue::Bool(true))
            );
            // Null clears the group (decode), reported group_count drops to 0.
            assert_eq!(intro.intervene("group", IntrospectValue::Null), Ok(()));
            assert_eq!(
                intro.query("group"),
                Some(IntrospectValue::Text("none".to_owned()))
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            // Before: sprite group [0, 2] leads, mesh [1, 3] follows.
            assert_eq!(intro.query("source_at.1"), Some(IntrospectValue::Int(0)));
            assert_eq!(intro.query("source_at.2"), Some(IntrospectValue::Int(2)));
            // Edit Hero's Type sprite -> mesh (R940 — Choice option 1): it joins
            // the mesh group live.
            assert_eq!(
                intro.intervene("value.0.1", IntrospectValue::Int(1)),
                Ok(())
            );
            assert_eq!(
                intro.query("group_count"),
                Some(IntrospectValue::Int(2)),
                "still two values"
            );
            // Now mesh [0, 1, 3] leads (first appearance), sprite [2] follows:
            // visible = [H(mesh), D0, D1, D3, H(sprite), D2].
            assert_eq!(
                intro.query("label_at.0"),
                Some(IntrospectValue::Text("mesh".to_owned()))
            );
            assert_eq!(
                intro.query("source_at.1"),
                Some(IntrospectValue::Int(0)),
                "Hero now in mesh"
            );
            assert_eq!(
                intro.query("source_at.3"),
                Some(IntrospectValue::Int(3)),
                "mesh has 3 members"
            );
            assert_eq!(
                intro.query("label_at.4"),
                Some(IntrospectValue::Text("sprite".to_owned()))
            );
            assert_eq!(
                intro.query("source_at.5"),
                Some(IntrospectValue::Int(2)),
                "sprite: only Coin"
            );
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
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(2)),
                "sprite: Coin"
            );
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                m
            ));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "into mesh: Tree"
            );
        });
    }

    #[test]
    fn r892_header_click_toggles_collapse() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            // A click on the group-0 header (the GridSendKey::Group wire) toggles.
            let _ = intro.invoke("send", IntrospectValue::Text("g0:PointerUp".to_owned()));
            assert_eq!(
                intro.query("collapsed.0"),
                Some(IntrospectValue::Bool(true))
            );
            let _ = intro.invoke("send", IntrospectValue::Text("g0:PointerUp".to_owned()));
            assert_eq!(
                intro.query("collapsed.0"),
                Some(IntrospectValue::Bool(false))
            );
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
            assert_eq!(
                nodes[0].role,
                pinion_a11y::AriaRole::TreeGrid,
                "grouped grid is a treegrid"
            );
            let header = nodes
                .iter()
                .find(|n| n.tag == group_header_tag(0))
                .expect("sprite group header present (painted-tag parity)");
            assert_eq!(header.role, pinion_a11y::AriaRole::Row);
            assert_eq!(header.level, Some(1), "group header is aria-level 1");
            assert_eq!(header.expanded, Some(true));
            // Cell-focus: the focused gridcell is the activedescendant, not the row.
            let cell = nodes
                .iter()
                .find(|n| n.tag == cell_tag(2, 2))
                .expect("focused cell");
            assert!(
                cell.state.focused,
                "the focused cell carries activedescendant"
            );
            let row = nodes
                .iter()
                .find(|n| n.tag == data_row_tag(2))
                .expect("data row 2");
            assert!(
                !row.state.focused,
                "the data row does not (cell focus, not row focus)"
            );
        });
    }

    #[test]
    fn r892_ungrouped_a11y_stays_a_flat_grid() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            assert_eq!(
                nodes[0].role,
                pinion_a11y::AriaRole::Grid,
                "ungrouped stays a flat grid"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Active (col 4) = [true, true, false, true]; filter keeps 0, 1, 3.
            let _ = intro.invoke("set_filter", IntrospectValue::Text("4=true".to_owned()));
            let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
            let _ = intro.intervene("focused_col", IntrospectValue::Int(4));
            assert_eq!(intro.query("view_len"), Some(IntrospectValue::Int(3)));
            // Toggle Hero's Active true -> false: it leaves the filter.
            let _ = intro.invoke("toggle", IntrospectValue::Null);
            assert_eq!(
                intro.query("value.0.4"),
                Some(IntrospectValue::Bool(false)),
                "toggled"
            );
            assert_eq!(
                intro.query("view_len"),
                Some(IntrospectValue::Int(2)),
                "Hero dropped"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned()));
            assert_eq!(
                intro.invoke("toggle_group", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(
                intro.query("visible_len"),
                Some(IntrospectValue::Int(4)),
                "sprite collapsed"
            );
            // Both sprite rows (0, 2) -> mesh (R940 — Choice option 1); "sprite"
            // no longer exists.
            let _ = intro.intervene("value.0.1", IntrospectValue::Int(1));
            let _ = intro.intervene("value.2.1", IntrospectValue::Int(1));
            assert_eq!(
                intro.query("group_count"),
                Some(IntrospectValue::Int(1)),
                "only mesh remains"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            let _ = intro.invoke("set_group", IntrospectValue::Text("1".to_owned())); // 2 groups
            assert_eq!(
                intro.query("collapsed.9"),
                Some(IntrospectValue::Null),
                "OOR group -> Null"
            );
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
            assert_eq!(
                intro.query("collapsed.0"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                intro.query("visible_len"),
                Some(IntrospectValue::Int(6)),
                "all expanded"
            );
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
            assert_eq!(
                grid_intro(&scene).query("value.0.2"),
                Some(IntrospectValue::Int(1000)),
                "clamp to max"
            );
            commit(&mut scene, 3, "-5");
            assert_eq!(
                grid_intro(&scene).query("value.0.3"),
                Some(IntrospectValue::Float(-5.0)),
                "unbounded — stores as-is"
            );
            commit(&mut scene, 2, "42");
            assert_eq!(
                grid_intro(&scene).query("value.0.2"),
                Some(IntrospectValue::Int(42)),
                "in-range unchanged"
            );
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
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(
                intro
                    .intervene("value.0.2", IntrospectValue::Int(5000))
                    .is_ok()
            );
            assert_eq!(
                intro.query("value.0.2"),
                Some(IntrospectValue::Int(1000)),
                "clamp to max"
            );
            assert!(
                intro
                    .intervene("value.0.3", IntrospectValue::Float(-5.0))
                    .is_ok()
            );
            assert_eq!(
                intro.query("value.0.3"),
                Some(IntrospectValue::Float(-5.0)),
                "unbounded — stores as-is"
            );
            assert!(
                intro
                    .intervene("value.1.3", IntrospectValue::Float(5.0))
                    .is_ok()
            );
            assert_eq!(
                intro.query("value.1.3"),
                Some(IntrospectValue::Float(5.0)),
                "positive float kept"
            );
        });
    }

    #[test]
    fn r894_col_range_wire_query() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("col_range.2"),
                Some(IntrospectValue::Text("0..1000".to_owned()))
            );
            assert_eq!(
                intro.query("col_range.3"),
                Some(IntrospectValue::Text("none".to_owned())),
                "Scale unbounded"
            );
            assert_eq!(
                intro.query("col_range.0"),
                Some(IntrospectValue::Text("none".to_owned())),
                "Asset unbounded"
            );
            assert_eq!(
                intro.query("col_range.9"),
                None,
                "out-of-range column -> None"
            );
        });
    }

    #[test]
    fn r894_unbounded_column_is_unclamped() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Asset (col 0, Text, no range): a long value is stored verbatim.
            assert!(
                intro
                    .intervene(
                        "value.0.0",
                        IntrospectValue::Text("VeryLongAssetName".to_owned())
                    )
                    .is_ok()
            );
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
        let node = scene
            .find_external_with_tag_mut(GRID_TAG)
            .expect("grid present");
        node.handle.pointer_move(x, 0.0);
    }

    /// R914 — send a composite cell event (`<row>_<col>:<Event>`) to the grid:
    /// `PointerDown` arms, `PointerUp` releases / clicks.
    fn grid_send(scene: &mut Scene, payload: &str) {
        let node = scene
            .find_external_with_tag_mut(GRID_TAG)
            .expect("grid present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        intro
            .invoke("send", IntrospectValue::Text(payload.to_owned()))
            .expect("send accepted");
    }

    fn cell_int(scene: &Scene, path: &str) -> i64 {
        match grid_intro(scene).query(path) {
            Some(IntrospectValue::Int(i)) => i,
            other => panic!("expected int at {path}, got {other:?}"),
        }
    }

    /// R940 — the selected option index of a `Choice` cell (its `to_introspect`
    /// JSON `{selected, label, options}`); panics on a non-choice cell.
    fn cell_choice(scene: &Scene, path: &str) -> usize {
        match grid_intro(scene).query(path) {
            Some(IntrospectValue::Json(v)) => usize::try_from(
                v.get("selected")
                    .and_then(serde_json::Value::as_u64)
                    .expect("choice has selected"),
            )
            .expect("selected fits usize"),
            other => panic!("expected a choice cell at {path}, got {other:?}"),
        }
    }

    fn cell_float(scene: &Scene, path: &str) -> f64 {
        match grid_intro(scene).query(path) {
            Some(IntrospectValue::Float(f)) => f,
            other => panic!("expected float at {path}, got {other:?}"),
        }
    }

    fn scrubbing(scene: &Scene) -> bool {
        matches!(
            grid_intro(scene).query("scrubbing"),
            Some(IntrospectValue::Bool(true))
        )
    }

    #[test]
    fn r914_float_cell_scrub_tracks_cursor() {
        // Scale (col 3, Float, unbounded) boots at 1.0 in row 0. A press arms
        // the cell; the calibration frame + the dead zone do not scrub (R915);
        // once the cursor strays past the click threshold each move scrubs
        // `base + travel_px · 0.01`, travel_px = delta·GRID_VIEWPORT_W.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            grid_send(&mut scene, "0_3:PointerDown");
            assert!(
                !scrubbing(&scene),
                "armed but not yet calibrated => not scrubbing"
            );
            grid_pointer_move(&mut scene, 0.5); // calibrate (the R51.35 press forward)
            assert!(
                !scrubbing(&scene),
                "R915: the calibration frame is a click so far, not a scrub"
            );
            assert!(
                (cell_float(&scene, "value.0.3") - 1.0).abs() < f64::EPSILON,
                "the calibration frame does not mutate",
            );
            grid_pointer_move(&mut scene, 0.75); // +0.25 fraction (well past the dead zone)
            assert!(
                scrubbing(&scene),
                "a real drag past the threshold is a scrub"
            );
            let expected = 1.0 + 0.25 * f64::from(GRID_VIEWPORT_W) * SCRUB_FLOAT_PER_PX;
            let got = cell_float(&scene, "value.0.3");
            assert!(
                (got - expected).abs() < 1e-6,
                "Scale scrubbed to ~{expected}, got {got}"
            );
            assert!(got > 1.0, "a rightward drag increases the value");
            // A leftward drag is signed — back below the press value.
            grid_pointer_move(&mut scene, 0.25); // -0.25 fraction from the press
            assert!(
                cell_float(&scene, "value.0.3") < 1.0,
                "a leftward drag decreases the value"
            );
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
            assert_eq!(
                cell_int(&scene, "value.0.2"),
                1 + steps,
                "Count steps +{steps} in whole units"
            );
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
            assert_eq!(
                cell_int(&scene, "value.0.2"),
                1000,
                "rightward scrub clamps to the max"
            );
            grid_send(&mut scene, "0_2:PointerUp");
            // A huge leftward drag clamps at the column minimum.
            grid_send(&mut scene, "0_2:PointerDown");
            grid_pointer_move(&mut scene, 0.5); // recalibrate (base now 1000)
            grid_pointer_move(&mut scene, -50.0); // far left => < 0 before clamp
            assert_eq!(
                cell_int(&scene, "value.0.2"),
                0,
                "leftward scrub clamps to the min"
            );
            grid_send(&mut scene, "0_2:PointerUp");
        });
    }

    /// R917 — arming a fresh press starts a fresh calibration: re-arming a cell
    /// (even without an intervening release — a lost PointerUp) drops the stale
    /// anchor, so a scrub never inherits the prior drag's base/cell.
    #[test]
    fn r917_re_arming_starts_a_fresh_calibration() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Scrub Count of row 0 past the threshold.
            grid_send(&mut scene, "0_2:PointerDown");
            grid_pointer_move(&mut scene, 0.5); // calibrate
            grid_pointer_move(&mut scene, 0.8); // a real drag
            assert!(scrubbing(&scene), "row 0 scrub is live");
            // Re-arm a DIFFERENT cell WITHOUT a release: the stale calibration is
            // dropped, so the scrub is self-contained (no inherited base/cell).
            grid_send(&mut scene, "1_2:PointerDown");
            assert!(
                !scrubbing(&scene),
                "re-arming cleared the stale calibration"
            );
            grid_send(&mut scene, "1_2:PointerUp");
        });
    }

    #[test]
    fn r915_numeric_click_focuses_within_dead_zone_and_drag_scrubs() {
        // R915 — a press whose cursor stays within DRAG_CLICK_THRESHOLD_PX is a
        // CLICK, not a scrub: it focuses the numeric cell (and never nudges the
        // value), exactly like a non-numeric cell. Only a press that strays past
        // the threshold becomes a scrub (which suppresses the focus-click).
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // (a) numeric cell click: press + the R51.35 press-time forward, no
            //     real travel => a click; the release focuses the cell.
            grid_send(&mut scene, "0_2:PointerDown");
            grid_pointer_move(&mut scene, 0.5); // the press-time forward (delta 0)
            assert!(
                !scrubbing(&scene),
                "the calibration frame alone is a click, not a scrub"
            );
            grid_send(&mut scene, "0_2:PointerUp");
            assert!(!scrubbing(&scene), "the release tears the calibration down");
            assert_eq!(
                cell_int(&scene, "value.0.2"),
                1,
                "Count unchanged by the click"
            );
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(0))
            );
            assert_eq!(
                grid_intro(&scene).query("focused_col"),
                Some(IntrospectValue::Int(2)),
                "R915: the click focuses the numeric cell (no longer absorbed)"
            );

            // (b) dead-zone drag: a press that strays only ~1px (< 4px threshold,
            //     1/370 ≈ 0.0027 fraction) is still a click — value unchanged.
            grid_send(&mut scene, "0_3:PointerDown");
            grid_pointer_move(&mut scene, 0.5);
            // +~1px: 1/370 ≈ 0.0027 fraction, well within the 4px click dead zone.
            grid_pointer_move(&mut scene, 0.5027);
            assert!(
                !scrubbing(&scene),
                "a 1px stray stays within the click dead zone"
            );
            grid_send(&mut scene, "0_3:PointerUp");
            assert!(
                (cell_float(&scene, "value.0.3") - 1.0).abs() < f64::EPSILON,
                "the dead-zone press did not scrub Scale"
            );

            // (c) the Active bool (col 4, row 2 = false) never arms a scrub; even a
            //     real cursor march stays a click → the release toggles the bool.
            assert_eq!(
                grid_intro(&scene).query("value.2.4"),
                Some(IntrospectValue::Bool(false))
            );
            grid_send(&mut scene, "2_4:PointerDown");
            grid_pointer_move(&mut scene, 0.5);
            grid_pointer_move(&mut scene, 0.8); // a real cursor march, but col 4 is not numeric
            assert!(!scrubbing(&scene), "a non-numeric press never scrubs");
            grid_send(&mut scene, "2_4:PointerUp");
            assert_eq!(
                grid_intro(&scene).query("value.2.4"),
                Some(IntrospectValue::Bool(true)),
                "the bool toggles on release (the press did not scrub)",
            );
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(2)),
                "the non-numeric click focuses its cell"
            );
        });
    }

    // R930 — dynamic rows: add / remove track the model SSOT.

    #[test]
    fn r930_add_row_appends_a_default_row_and_moves_the_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.query("row_count"),
                Some(IntrospectValue::Int(4)),
                "boot is 4 rows"
            );
            // add_row returns the new source index and grows the count.
            assert_eq!(
                intro.invoke("add_row", IntrospectValue::Null),
                Ok(IntrospectValue::Int(4))
            );
            assert_eq!(
                intro.query("row_count"),
                Some(IntrospectValue::Int(5)),
                "one row added"
            );
            // the new row carries typed defaults, one per column kind.
            assert_eq!(
                intro.query("value.4.0"),
                Some(IntrospectValue::Text(String::new()))
            );
            assert_eq!(intro.query("value.4.2"), Some(IntrospectValue::Int(0)));
            assert_eq!(intro.query("value.4.3"), Some(IntrospectValue::Float(0.0)));
            assert_eq!(intro.query("value.4.4"), Some(IntrospectValue::Bool(false)));
            assert_eq!(
                intro.query("focused_row"),
                Some(IntrospectValue::Int(4)),
                "cursor on the new row"
            );
            // the appended row edits exactly like a seeded one (no parallel machinery).
            assert!(
                intro
                    .intervene("value.4.0", IntrospectValue::Text("New".to_owned()))
                    .is_ok()
            );
            assert_eq!(
                intro.query("value.4.0"),
                Some(IntrospectValue::Text("New".to_owned()))
            );
        });
    }

    #[test]
    fn r930_remove_row_drops_and_shifts_indices_down() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Boot source rows: 0 Hero, 1 Tree, 2 Coin, 3 Boss.
            assert_eq!(
                intro.query("value.1.0"),
                Some(IntrospectValue::Text("Tree".to_owned()))
            );
            assert_eq!(
                intro.invoke("remove_row", IntrospectValue::Int(1)),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                intro.query("row_count"),
                Some(IntrospectValue::Int(3)),
                "one row removed"
            );
            // rows above the removed shift down: old row 2 (Coin) is now index 1.
            assert_eq!(
                intro.query("value.1.0"),
                Some(IntrospectValue::Text("Coin".to_owned()))
            );
            assert_eq!(
                intro.query("value.2.0"),
                Some(IntrospectValue::Text("Boss".to_owned()))
            );
            assert_eq!(intro.query("value.3.0"), None, "the source space shrank");
        });
    }

    /// R937 — `move_block` moves a whole row block and its inverse restores the
    /// model exactly (the symmetric redo / undo property).
    #[test]
    fn r937_move_block_is_symmetric() {
        let mut cells: Vec<CellValue> = (0..4 * NCOLS)
            .map(|i| CellValue::Int(i64::try_from(i).unwrap()))
            .collect();
        let orig = cells.clone();
        move_block(&mut cells, 0, 2); // row 0 -> resting index 2
        assert_eq!(
            cells[idx(2, 0)],
            CellValue::Int(0),
            "moved row landed at index 2"
        );
        assert_eq!(
            cells[idx(0, 0)],
            CellValue::Int(i64::try_from(NCOLS).unwrap()),
            "old row 1 shifted up"
        );
        move_block(&mut cells, 2, 0); // the exact inverse
        assert_eq!(cells, orig, "move then inverse restores the model");
        // A degenerate move is a no-op.
        move_block(&mut cells, 1, 1);
        assert_eq!(cells, orig);
    }

    /// R937 — the gap → resting-index removal shift (the [`ReorderModel::apply_move`]
    /// off-by-one peer).
    #[test]
    fn r937_gap_to_index_removal_shift() {
        assert_eq!(
            DataGridExternal::gap_to_index(1, 4),
            3,
            "a gap past `from` shifts down one"
        );
        assert_eq!(
            DataGridExternal::gap_to_index(3, 1),
            1,
            "a gap before `from` is the index itself"
        );
        assert_eq!(
            DataGridExternal::gap_to_index(2, 2),
            2,
            "a gap at `from` is a no-op move"
        );
    }

    /// R937 — `move_row` reorders the source model and the moved row follows the
    /// cursor; `from == to` / out-of-range are no-ops.
    #[test]
    fn r937_move_row_via_rpc_reorders_and_follows_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Boot: 0 Hero, 1 Tree, 2 Coin, 3 Boss. Move Hero (0) to index 2.
            assert_eq!(
                intro.invoke("move_row", IntrospectValue::Text("0,2".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                intro.query("value.0.0"),
                Some(IntrospectValue::Text("Tree".to_owned()))
            );
            assert_eq!(
                intro.query("value.1.0"),
                Some(IntrospectValue::Text("Coin".to_owned()))
            );
            assert_eq!(
                intro.query("value.2.0"),
                Some(IntrospectValue::Text("Hero".to_owned())),
                "Hero moved to index 2"
            );
            assert_eq!(
                intro.query("value.3.0"),
                Some(IntrospectValue::Text("Boss".to_owned()))
            );
            assert_eq!(
                intro.query("focused_row"),
                Some(IntrospectValue::Int(2)),
                "the moved row follows the cursor"
            );
            assert_eq!(
                intro.invoke("move_row", IntrospectValue::Text("1,1".to_owned())),
                Ok(IntrospectValue::Bool(false)),
                "from == to is a no-op",
            );
            assert_eq!(
                intro.invoke("move_row", IntrospectValue::Text("0,9".to_owned())),
                Ok(IntrospectValue::Bool(false)),
                "out-of-range is a no-op",
            );
        });
    }

    /// R937 — a row move is one undo step: undo restores the original order +
    /// cursor, redo re-applies (the symmetric `MoveRowEdit`).
    #[test]
    fn r937_move_row_undo_redo_symmetric() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("move_row", IntrospectValue::Text("0,2".to_owned()));
                assert_eq!(
                    intro.query("value.2.0"),
                    Some(IntrospectValue::Text("Hero".to_owned()))
                );
            }
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                grid_intro(&scene).query("value.0.0"),
                Some(IntrospectValue::Text("Hero".to_owned())),
                "undo restored Hero to index 0"
            );
            assert_eq!(
                grid_intro(&scene).query("value.2.0"),
                Some(IntrospectValue::Text("Coin".to_owned()))
            );
            assert!(undo_invoke(&mut scene, "redo"));
            assert_eq!(
                grid_intro(&scene).query("value.2.0"),
                Some(IntrospectValue::Text("Hero".to_owned())),
                "redo re-moved Hero to index 2"
            );
        });
    }

    /// R937 — a sort makes manual order meaningless, so the grip / drag / move_row
    /// are disabled (`reorder_enabled` false, `move_row` rejected).
    #[test]
    fn r937_reorder_disabled_under_sort() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert_eq!(
                grid_intro(&scene).query("reorder_enabled"),
                Some(IntrospectValue::Bool(true)),
                "plain view: enabled"
            );
            use_sort().set(Some((0, true))); // a column sort derives the visual order
            assert_eq!(
                grid_intro(&scene).query("reorder_enabled"),
                Some(IntrospectValue::Bool(false)),
                "sorted: disabled"
            );
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert!(
                intro
                    .invoke("move_row", IntrospectValue::Text("0,2".to_owned()))
                    .is_err(),
                "move_row rejected under sort"
            );
        });
    }

    /// R937 — Alt+Arrow moves the focused row one slot (the keyboard reorder path),
    /// journaled like the drag / RPC; the cursor follows.
    #[test]
    fn r937_keyboard_alt_arrow_moves_focused_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            use_focused_row().set(0); // Hero
            let alt = Modifiers {
                alt: true,
                ..Modifiers::empty()
            };
            assert!(
                DataGridView::apply_key(&mut scene, Some(GRID_TAG), "ArrowDown", alt),
                "Alt+Down consumed"
            );
            // [Hero, Tree, Coin, Boss] -> Hero down one -> [Tree, Hero, Coin, Boss].
            assert_eq!(
                grid_intro(&scene).query("value.0.0"),
                Some(IntrospectValue::Text("Tree".to_owned()))
            );
            assert_eq!(
                grid_intro(&scene).query("value.1.0"),
                Some(IntrospectValue::Text("Hero".to_owned())),
                "Alt+Down moved Hero down one"
            );
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "the cursor follows"
            );
        });
    }

    /// R937.1 (session-review) — the drop-line classifier: a gap puts a Top line on
    /// its row, a past-the-end gap a Bottom line on the last row, nothing else.
    #[test]
    fn r937_1_drop_edge_classification() {
        assert_eq!(
            drop_edge_at(Some(2), 2, 3),
            Some(DropEdge::Top),
            "gap inserts before its row"
        );
        assert_eq!(
            drop_edge_at(Some(4), 3, 3),
            Some(DropEdge::Bottom),
            "gap past the end = last row bottom"
        );
        assert_eq!(
            drop_edge_at(Some(2), 1, 3),
            None,
            "a non-target row gets no line"
        );
        assert_eq!(drop_edge_at(None, 2, 3), None, "no drag = no line");
    }

    /// R937.1 (session-review) — the drag HOOKS directly (`scene/drag` is atomic,
    /// so this is the only path that witnesses the mid-drag `drag_preview` the demo
    /// cannot): a handle press arms a reorder, `drag_to` sets the live drop gap
    /// (AI-observable), and `drag_release` commits the move + clears the preview.
    #[test]
    fn r937_1_drag_hooks_set_preview_then_commit() {
        Owner::new().run(|| {
            let mut ext = DataGridView::create_external();
            // Arm row 0's handle (the press the router dispatches before begin_drag).
            let _ = ext
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("d0:PointerDown".to_owned()));
            let payload = ext
                .begin_drag()
                .expect("a handle press in the plain view arms a reorder drag");
            // Drag over row 2's bottom half → drop gap 3; the preview is observable.
            let drop = DropPoint {
                tag: cell_tag(2, 0),
                x_rel: 0.5,
                y_rel: 0.8,
            };
            ext.drag_to(&payload, Some(drop.clone()));
            assert_eq!(
                use_drag_preview().get(),
                Some(3),
                "drag_to publishes the live drop gap"
            );
            ext.drag_release(&payload, Some(drop));
            assert_eq!(use_drag_preview().get(), None, "release clears the preview");
            assert_eq!(
                use_data_model().get()[idx(2, 0)],
                CellValue::Text("Hero".to_owned()),
                "Hero moved to index 2"
            );
        });
    }

    /// R937.1 (session-review) — `drag_cancel` (an OS gesture revoke) DISCARDS the
    /// in-flight reorder: the preview clears and NO row moves (the ghost-line +
    /// stale-arm leak the review found).
    #[test]
    fn r937_1_drag_cancel_discards_without_moving() {
        Owner::new().run(|| {
            let mut ext = DataGridView::create_external();
            let _ = ext
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("d0:PointerDown".to_owned()));
            let payload = ext.begin_drag().expect("armed reorder");
            ext.drag_to(
                &payload,
                Some(DropPoint {
                    tag: cell_tag(2, 0),
                    x_rel: 0.5,
                    y_rel: 0.8,
                }),
            );
            assert_eq!(use_drag_preview().get(), Some(3), "preview set mid-drag");
            ext.drag_cancel(&payload);
            assert_eq!(use_drag_preview().get(), None, "cancel clears the preview");
            assert_eq!(
                use_data_model().get()[idx(0, 0)],
                CellValue::Text("Hero".to_owned()),
                "cancel moved nothing"
            );
        });
    }

    /// R937.1 (session-review) — the a11y reorder path: an `Increment` / `Decrement`
    /// action on a cell moves its ROW down / up (the hello-dnd pattern, replacing
    /// the invalid + inert row-child button). Clamped at the ends; non-reorder
    /// actions fall through (`false`).
    #[test]
    fn r937_1_access_child_invoke_reorders_the_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Increment on row 0's cell: [Hero,Tree,Coin,Boss] -> [Tree,Hero,Coin,Boss].
            assert!(DataGridView::access_child_invoke(
                &mut scene,
                GRID_TAG,
                "0_0",
                AccessAction::Increment
            ));
            assert_eq!(
                grid_intro(&scene).query("value.1.0"),
                Some(IntrospectValue::Text("Hero".to_owned())),
                "Increment moved Hero down"
            );
            // Decrement on row 0 cannot go up — no wrap, not handled.
            assert!(
                !DataGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "0_0",
                    AccessAction::Decrement
                ),
                "no wrap above the first row"
            );
            // A non-reorder action falls through to the shell focus chain.
            assert!(
                !DataGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "0_0",
                    AccessAction::Click
                ),
                "Click is not a reorder action"
            );
            // Under a sort, reorder is disabled — the action is rejected.
            use_sort().set(Some((0, true)));
            assert!(
                !DataGridView::access_child_invoke(
                    &mut scene,
                    GRID_TAG,
                    "1_0",
                    AccessAction::Increment
                ),
                "no AT reorder under a sort"
            );
        });
    }

    #[test]
    fn r930_remove_row_rejects_out_of_range_and_keeps_at_least_one() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            assert_eq!(
                intro.invoke("remove_row", IntrospectValue::Int(99)),
                Ok(IntrospectValue::Bool(false)),
                "out-of-range row rejected",
            );
            assert_eq!(intro.query("row_count"), Some(IntrospectValue::Int(4)));
            // Drain to a single row, then refuse to remove the last one.
            for _ in 0..3 {
                assert_eq!(
                    intro.invoke("remove_row", IntrospectValue::Int(0)),
                    Ok(IntrospectValue::Bool(true)),
                );
            }
            assert_eq!(
                intro.query("row_count"),
                Some(IntrospectValue::Int(1)),
                "down to one row"
            );
            assert_eq!(
                intro.invoke("remove_row", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(false)),
                "a grid keeps >= 1 row",
            );
            assert_eq!(
                intro.query("row_count"),
                Some(IntrospectValue::Int(1)),
                "still one row"
            );
        });
    }

    #[test]
    fn r930_added_rows_participate_in_sort() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Add a row and set its Count (col 2) below every seed value (1, 24, 99, 1).
            assert_eq!(
                intro.invoke("add_row", IntrospectValue::Null),
                Ok(IntrospectValue::Int(4))
            );
            assert!(
                intro
                    .intervene("value.4.2", IntrospectValue::Int(-5))
                    .is_ok()
            );
            // Sort ascending by Count: the new row (-5) must lead the visible order.
            assert!(intro.invoke("cycle_sort", IntrospectValue::Int(2)).is_ok());
            assert_eq!(
                intro.query("visible_len"),
                Some(IntrospectValue::Int(5)),
                "all 5 rows visible"
            );
            assert_eq!(
                intro.query("source_at.0"),
                Some(IntrospectValue::Int(4)),
                "the added row sorts to the front (smallest Count)"
            );
        });
    }

    // R930.1 — session-review fixes: a structural row change must not leave a
    // stale edit latch (panic on commit) or strand the cursor on a hidden row.

    #[test]
    fn r930_1_remove_row_cancels_an_in_flight_edit() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene
                .find_external_with_tag_mut(GRID_TAG)
                .expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            intro
                .intervene("focused_row", IntrospectValue::Int(3))
                .expect("focus the last row");
            assert!(
                intro.invoke("begin", IntrospectValue::Null).is_ok(),
                "begin editing the focused cell"
            );
            assert_eq!(
                intro.query("editing_row"),
                Some(IntrospectValue::Int(3)),
                "row 3 is editing"
            );
            // Removing a row invalidates the source-keyed (3, col) latch; it must
            // be canceled, else a later commit writes next[idx(3, col)] past the
            // shrunk model (the R930.1 reachable panic).
            assert_eq!(
                intro.invoke("remove_row", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                intro.query("editing_row"),
                Some(IntrospectValue::Null),
                "remove canceled the in-flight edit (no stale latch to panic on)",
            );
        });
    }

    #[test]
    fn r930_1_remove_row_reanchors_the_cursor_to_a_visible_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid present");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // Filter Count(col 2)==99 → only Coin (source row 2) is visible.
            assert_eq!(
                intro.invoke("set_filter", IntrospectValue::Text("2=99".to_owned())),
                Ok(IntrospectValue::Int(1)),
                "one row passes the Count==99 filter",
            );
            intro.intervene("focused_row", IntrospectValue::Int(2)).expect("cursor on the visible Coin");
            // Remove the HIDDEN source row 0 (Hero): Coin shifts to source 1, still
            // Count 99 (visible). A bare clamp would strand the cursor on source 2
            // (Boss, Count 1, filtered out); the re-anchor keeps it on Coin.
            assert_eq!(
                intro.invoke("remove_row", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                intro.query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "cursor re-anchored onto the still-visible Coin, not stranded on a filtered-out row",
            );
        });
    }

    // ─── R932 undo / redo (adopts the shared UndoStack substrate) ─────

    /// R932 — invoke a verb (`undo` / `redo`) on the [`UNDO_TAG`] external (the
    /// AI-first surface the keyboard + RPC both drive). Returns whether a step
    /// actually happened.
    fn undo_invoke(scene: &mut Scene, verb: &str) -> bool {
        let node = scene
            .find_external_with_tag_mut(UNDO_TAG)
            .expect("undo external present");
        let intro = node.handle.introspect_mut().expect("introspectable");
        matches!(
            intro.invoke(verb, IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true))
        )
    }

    /// R932 — query a slot on the [`UNDO_TAG`] external.
    fn undo_query(scene: &Scene, slot: &str) -> Option<IntrospectValue> {
        scene
            .find_external_with_tag(UNDO_TAG)
            .and_then(|n| n.handle.introspect())
            .and_then(|i| i.query(slot))
    }

    #[test]
    fn r932_undo_external_is_wired_to_the_shared_stack() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Boot history is empty.
            assert_eq!(
                undo_query(&scene, "can_undo"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(undo_query(&scene, "count"), Some(IntrospectValue::Int(0)));
            assert_eq!(
                undo_query(&scene, "undo_label"),
                Some(IntrospectValue::Null)
            );
            // Prove it is the SAME stack the coordinator records onto (not a
            // separate empty one): a mutation through the GRID external must be
            // observable through the UNDO external.
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                assert!(
                    intro
                        .intervene("value.0.2", IntrospectValue::Int(5))
                        .is_ok()
                );
            }
            assert_eq!(
                undo_query(&scene, "count"),
                Some(IntrospectValue::Int(1)),
                "the GRID edit is visible on the UNDO external — one shared stack"
            );
            assert_eq!(
                undo_query(&scene, "can_undo"),
                Some(IntrospectValue::Bool(true))
            );
        });
    }

    #[test]
    fn r932_cell_edit_undo_restores_value_and_redo_reapplies() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // An AI `value.1.2` write (Tree's Count: 24 -> 7) is one undo step.
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                assert!(
                    intro
                        .intervene("value.1.2", IntrospectValue::Int(7))
                        .is_ok()
                );
            }
            assert_eq!(
                grid_intro(&scene).query("value.1.2"),
                Some(IntrospectValue::Int(7))
            );
            assert_eq!(
                undo_query(&scene, "can_undo"),
                Some(IntrospectValue::Bool(true))
            );
            assert_eq!(undo_query(&scene, "count"), Some(IntrospectValue::Int(1)));
            assert_eq!(
                undo_query(&scene, "undo_label"),
                Some(IntrospectValue::Text("Edit cell".to_owned()))
            );
            // Undo restores the original value; redo re-applies it (verbatim
            // before / after snapshots).
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                grid_intro(&scene).query("value.1.2"),
                Some(IntrospectValue::Int(24)),
                "undo restores 24"
            );
            assert_eq!(
                undo_query(&scene, "can_undo"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                undo_query(&scene, "can_redo"),
                Some(IntrospectValue::Bool(true))
            );
            assert!(undo_invoke(&mut scene, "redo"));
            assert_eq!(
                grid_intro(&scene).query("value.1.2"),
                Some(IntrospectValue::Int(7)),
                "redo re-applies 7"
            );
        });
    }

    #[test]
    fn r932_keyboard_inline_commit_is_undoable() {
        // The second pre-existing cell-write path: the inline editor's keyboard
        // commit (`commit_edit`), distinct from the External's `edit_cell`. It
        // must journal identically.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                intro
                    .intervene("focused_row", IntrospectValue::Int(0))
                    .expect("focus row 0");
                intro
                    .intervene("focused_col", IntrospectValue::Int(0))
                    .expect("focus col 0");
                assert!(
                    intro.invoke("begin", IntrospectValue::Null).is_ok(),
                    "edit Hero's name"
                );
            }
            // Type a new value into the shared inline field, then commit.
            use_text_edit_state(EDIT_TF_TAG).set_text("Renamed".to_owned());
            commit_edit(true);
            assert_eq!(
                grid_intro(&scene).query("value.0.0"),
                Some(IntrospectValue::Text("Renamed".to_owned()))
            );
            assert_eq!(
                undo_query(&scene, "count"),
                Some(IntrospectValue::Int(1)),
                "one history step"
            );
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                grid_intro(&scene).query("value.0.0"),
                Some(IntrospectValue::Text("Hero".to_owned())),
                "undo restores the original name",
            );
        });
    }

    #[test]
    fn r932_malformed_commit_records_nothing() {
        // A malformed numeric commit keeps the prior value (no data loss) AND
        // journals nothing — `push_cell_edit`'s no-op guard.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                intro
                    .intervene("focused_row", IntrospectValue::Int(0))
                    .expect("focus row 0");
                intro
                    .intervene("focused_col", IntrospectValue::Int(2))
                    .expect("focus Count (int)");
                assert!(intro.invoke("begin", IntrospectValue::Null).is_ok());
            }
            use_text_edit_state(EDIT_TF_TAG).set_text("not-a-number".to_owned());
            commit_edit(true);
            assert_eq!(
                grid_intro(&scene).query("value.0.2"),
                Some(IntrospectValue::Int(1)),
                "value unchanged"
            );
            assert_eq!(
                undo_query(&scene, "count"),
                Some(IntrospectValue::Int(0)),
                "no history for a no-op"
            );
        });
    }

    #[test]
    fn r932_toggle_is_one_undo_step() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Solid (col 4, bool) of row 0 boots true; toggle it via RPC.
            assert_eq!(
                grid_intro(&scene).query("value.0.4"),
                Some(IntrospectValue::Bool(true))
            );
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                intro
                    .intervene("focused_row", IntrospectValue::Int(0))
                    .expect("focus row 0");
                intro
                    .intervene("focused_col", IntrospectValue::Int(4))
                    .expect("focus the bool col");
                assert_eq!(
                    intro.invoke("toggle", IntrospectValue::Null),
                    Ok(IntrospectValue::Bool(true))
                );
            }
            assert_eq!(
                grid_intro(&scene).query("value.0.4"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                undo_query(&scene, "undo_label"),
                Some(IntrospectValue::Text("Toggle cell".to_owned()))
            );
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                grid_intro(&scene).query("value.0.4"),
                Some(IntrospectValue::Bool(true)),
                "toggle reversed"
            );
        });
    }

    #[test]
    fn r932_add_row_undo_removes_then_redo_readds() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                assert_eq!(
                    intro.invoke("add_row", IntrospectValue::Null),
                    Ok(IntrospectValue::Int(4))
                );
            }
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(5))
            );
            assert_eq!(
                undo_query(&scene, "undo_label"),
                Some(IntrospectValue::Text("Add row".to_owned()))
            );
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(4)),
                "undo drops the row"
            );
            assert!(undo_invoke(&mut scene, "redo"));
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(5)),
                "redo re-adds it"
            );
        });
    }

    #[test]
    fn r932_remove_row_undo_restores_cells_and_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                intro
                    .intervene("focused_row", IntrospectValue::Int(1))
                    .expect("cursor on Tree");
                // Remove source row 1 (Tree, Count 24).
                assert_eq!(
                    intro.invoke("remove_row", IntrospectValue::Int(1)),
                    Ok(IntrospectValue::Bool(true))
                );
            }
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(3))
            );
            assert_eq!(
                grid_intro(&scene).query("value.1.0"),
                Some(IntrospectValue::Text("Coin".to_owned())),
                "Coin shifted into the freed slot"
            );
            // Undo re-inserts the whole row verbatim at its index, cursor back.
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(4)),
                "row restored"
            );
            assert_eq!(
                grid_intro(&scene).query("value.1.0"),
                Some(IntrospectValue::Text("Tree".to_owned())),
                "Tree's name re-inserted"
            );
            assert_eq!(
                grid_intro(&scene).query("value.1.2"),
                Some(IntrospectValue::Int(24)),
                "Tree's Count re-inserted (whole-row capture, not just the name)"
            );
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "cursor restored to the re-inserted row"
            );
        });
    }

    #[test]
    fn r932_scrub_is_one_undo_step() {
        // A continuous numeric scrub commits live during the drag but journals
        // exactly ONE step at release (the node editor's "one move per gesture"
        // rule) — undo restores the press value, not each intermediate frame.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let base = cell_float(&scene, "value.0.3"); // Scale boots at 1.0
            grid_send(&mut scene, "0_3:PointerDown");
            grid_pointer_move(&mut scene, 0.5); // calibrate (dead zone, no mutation)
            grid_pointer_move(&mut scene, 0.8); // drag past the threshold
            assert!(
                cell_float(&scene, "value.0.3") > base,
                "the live scrub moved the value"
            );
            grid_send(&mut scene, "0_3:PointerUp");
            assert_eq!(
                undo_query(&scene, "count"),
                Some(IntrospectValue::Int(1)),
                "one step for the whole drag"
            );
            assert_eq!(
                undo_query(&scene, "undo_label"),
                Some(IntrospectValue::Text("Scrub cell".to_owned()))
            );
            assert!(undo_invoke(&mut scene, "undo"));
            assert!(
                (cell_float(&scene, "value.0.3") - base).abs() < f64::EPSILON,
                "undo restores the press value"
            );
        });
    }

    #[test]
    fn r932_view_state_changes_are_not_journaled() {
        // Honest scope: undo journals DATA edits (cells + row structure), not
        // VIEW state (sort / filter / group / collapse) — the Qt / Unreal
        // convention. Each of the four view ops adds no undo step.
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                assert!(intro.invoke("cycle_sort", IntrospectValue::Int(2)).is_ok());
                assert!(
                    intro
                        .invoke("set_filter", IntrospectValue::Text("4=true".to_owned()))
                        .is_ok()
                );
                assert!(
                    intro
                        .invoke("set_group", IntrospectValue::Text("1".to_owned()))
                        .is_ok()
                );
                assert!(intro.invoke("collapse_all", IntrospectValue::Null).is_ok());
            }
            assert_eq!(
                undo_query(&scene, "count"),
                Some(IntrospectValue::Int(0)),
                "sort / filter / group / collapse are view state, not undoable edits"
            );
        });
    }

    #[test]
    fn r932_keyboard_ctrl_z_drives_undo() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                assert!(
                    intro
                        .intervene("value.0.2", IntrospectValue::Int(50))
                        .is_ok()
                );
            }
            let ctrl = Modifiers {
                shift: false,
                ctrl: true,
                alt: false,
                meta: false,
            };
            let ctrl_shift = Modifiers {
                shift: true,
                ctrl: true,
                alt: false,
                meta: false,
            };
            // Ctrl+Z (grid-focused) drives undo through the same UNDO external.
            assert!(apply_key_grid(&mut scene, "z", ctrl), "Ctrl+Z is consumed");
            assert_eq!(
                grid_intro(&scene).query("value.0.2"),
                Some(IntrospectValue::Int(1)),
                "undone to the boot value"
            );
            // Ctrl+Shift+Z redoes; Ctrl+Y is the same verb.
            assert!(
                apply_key_grid(&mut scene, "z", ctrl_shift),
                "Ctrl+Shift+Z is consumed"
            );
            assert_eq!(
                grid_intro(&scene).query("value.0.2"),
                Some(IntrospectValue::Int(50)),
                "redone"
            );
            assert_eq!(undo_redo_verb("y", ctrl), Some("redo"));
            assert_eq!(undo_redo_verb("z", ctrl), Some("undo"));
            assert_eq!(
                undo_redo_verb("z", Modifiers::empty()),
                None,
                "a bare z is navigation, not undo"
            );
        });
    }

    // R932.1 — session-review fixes: the undo path must uphold the SAME
    // R930.1 invariants every direct mutator does — re-anchor the cursor and
    // cancel the in-flight edit latch — not restore raw state verbatim.

    #[test]
    fn r932_1_undo_under_filter_does_not_strand_the_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                // Edit Hero's Count (row 0) 1 -> 24 (matching Tree's 24), cursor on Hero.
                intro
                    .intervene("focused_row", IntrospectValue::Int(0))
                    .expect("cursor on Hero");
                assert!(
                    intro
                        .intervene("value.0.2", IntrospectValue::Int(24))
                        .is_ok()
                );
                // Filter Count==24: Hero(0) + Tree(1) visible; cursor stays on Hero.
                assert_eq!(
                    intro.invoke("set_filter", IntrospectValue::Text("2=24".to_owned())),
                    Ok(IntrospectValue::Int(2))
                );
            }
            // Undo the edit: Hero's Count reverts to 1, so Hero leaves the
            // Count==24 view. The cursor was recorded as Hero (row 0); restoring
            // it verbatim would strand it on a now-filtered-out row (dead
            // arrow-nav, the R930.1 bug). The undo must re-anchor onto the
            // still-visible Tree (row 1).
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                grid_intro(&scene).query("value.0.2"),
                Some(IntrospectValue::Int(1)),
                "undo restored Hero's Count"
            );
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "cursor re-anchored onto the still-visible Tree, not stranded on hidden Hero"
            );
        });
    }

    #[test]
    fn r932_1_undo_of_a_structural_change_cancels_the_edit_latch() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            {
                let node = scene
                    .find_external_with_tag_mut(GRID_TAG)
                    .expect("grid present");
                let intro = node.handle.introspect_mut().expect("introspectable");
                // Remove source row 0 (Hero): Tree/Coin/Boss shift to 0/1/2.
                assert_eq!(
                    intro.invoke("remove_row", IntrospectValue::Int(0)),
                    Ok(IntrospectValue::Bool(true))
                );
                // Begin editing the source-keyed cell (2, 0) — now Boss.
                intro
                    .intervene("focused_row", IntrospectValue::Int(2))
                    .expect("focus a row");
                intro
                    .intervene("focused_col", IntrospectValue::Int(0))
                    .expect("focus col 0");
                assert!(
                    intro.invoke("begin", IntrospectValue::Null).is_ok(),
                    "begin editing"
                );
                assert_eq!(
                    intro.query("editing_row"),
                    Some(IntrospectValue::Int(2)),
                    "row 2 editing"
                );
            }
            // Undo the remove: Hero is re-inserted at 0, shifting every source
            // index up. The latch (2, 0) would now point at a DIFFERENT row, so a
            // later commit would write the wrong cell (the R930.1 stale-latch
            // class). The undo must cancel the latch, exactly as `remove_row` does.
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(4)),
                "row restored"
            );
            assert_eq!(
                grid_intro(&scene).query("editing_row"),
                Some(IntrospectValue::Null),
                "the structural undo cancelled the in-flight edit latch"
            );
        });
    }

    // ─── R940 choice column / dropdown ────────────────────────────

    fn grid_invoke(
        scene: &mut Scene,
        verb: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let node = scene
            .find_external_with_tag_mut(GRID_TAG)
            .expect("grid present");
        node.handle
            .introspect_mut()
            .expect("introspectable")
            .invoke(verb, args)
    }

    fn grid_set(scene: &mut Scene, path: &str, v: IntrospectValue) -> Result<(), InterveneError> {
        let node = scene
            .find_external_with_tag_mut(GRID_TAG)
            .expect("grid present");
        node.handle
            .introspect_mut()
            .expect("introspectable")
            .intervene(path, v)
    }

    fn grid_key(scene: &mut Scene, k: &str) -> bool {
        DataGridView::apply_key(scene, Some(GRID_TAG), k, Modifiers::empty())
    }

    /// Focus `(row, col)` and open its choice dropdown via the RPC verb.
    fn open_choice_at(scene: &mut Scene, row: usize, col: usize) -> bool {
        let _ = grid_set(scene, "focused_row", IntrospectValue::Int(int_of(row)));
        let _ = grid_set(scene, "focused_col", IntrospectValue::Int(int_of(col)));
        grid_invoke(scene, "open_choice", IntrospectValue::Null) == Ok(IntrospectValue::Bool(true))
    }

    fn open_color_at(scene: &mut Scene, row: usize, col: usize) -> bool {
        let _ = grid_set(scene, "focused_row", IntrospectValue::Int(int_of(row)));
        let _ = grid_set(scene, "focused_col", IntrospectValue::Int(int_of(col)));
        grid_invoke(scene, "open_color", IntrospectValue::Null) == Ok(IntrospectValue::Bool(true))
    }

    fn cell_hex(scene: &Scene, path: &str) -> String {
        match grid_intro(scene).query(path) {
            Some(IntrospectValue::Json(v)) => v
                .get("hex")
                .and_then(serde_json::Value::as_str)
                .expect("colour cell has a hex field")
                .to_owned(),
            other => panic!("expected a colour cell at {path}, got {other:?}"),
        }
    }

    #[test]
    fn r940_seed_type_column_is_choice() {
        Owner::new().run(|| {
            let scene = boot_scene();
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("col_kind.1"),
                Some(IntrospectValue::Text("choice".to_owned()))
            );
            assert_eq!(
                cell_choice(&scene, "value.0.1"),
                0,
                "Hero = sprite (option 0)"
            );
            assert_eq!(
                cell_choice(&scene, "value.1.1"),
                1,
                "Tree = mesh (option 1)"
            );
            assert_eq!(cell_choice(&scene, "value.2.1"), 0, "Coin = sprite");
            assert_eq!(cell_choice(&scene, "value.3.1"), 1, "Boss = mesh");
            // The other columns keep their kinds; R943 appended the Tint colour
            // column, so the grid is now NCOLS=6.
            assert_eq!(
                intro.query("col_kind.0"),
                Some(IntrospectValue::Text("text".to_owned()))
            );
            assert_eq!(
                intro.query("col_kind.2"),
                Some(IntrospectValue::Text("int".to_owned()))
            );
            assert_eq!(intro.query("col_count"), Some(IntrospectValue::Int(6)));
        });
    }

    #[test]
    fn r940_added_row_choice_carries_full_options() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert_eq!(
                grid_invoke(&mut scene, "add_row", IntrospectValue::Null),
                Ok(IntrospectValue::Int(4))
            );
            // The new row's Type cell is option 0 with the FULL option list
            // (not an empty Vec — `default_row` is column-aware), so it edits
            // exactly like a seeded choice cell.
            assert_eq!(
                cell_choice(&scene, "value.4.1"),
                0,
                "added Type defaults to option 0"
            );
            assert!(
                grid_set(&mut scene, "value.4.1", IntrospectValue::Int(2)).is_ok(),
                "an added row's choice takes option 2"
            );
            assert_eq!(cell_choice(&scene, "value.4.1"), 2);
        });
    }

    #[test]
    fn r940_click_and_doubleclick_open_dropdown() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // A single click on a choice cell opens its dropdown (the bool
            // single-click-toggle peer), seeding the cursor at the committed option.
            grid_send(&mut scene, "1_1:PointerUp");
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("popup_open"), Some(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("editing_row"), Some(IntrospectValue::Int(1)));
            assert_eq!(intro.query("editing_col"), Some(IntrospectValue::Int(1)));
            assert_eq!(
                intro.query("popup_cursor"),
                Some(IntrospectValue::Int(1)),
                "cursor seeded at mesh"
            );
            // A double-click on another choice cell re-opens there.
            grid_send(&mut scene, "0_1:DoubleClick");
            assert_eq!(
                grid_intro(&scene).query("editing_row"),
                Some(IntrospectValue::Int(0))
            );
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(0))
            );
        });
    }

    #[test]
    fn r940_open_choice_rejects_non_choice_cell() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // The focused cell is the Asset (text) column → open_choice is a no-op.
            assert!(
                !open_choice_at(&mut scene, 0, 0),
                "text column has no dropdown"
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            // A bool column likewise (it toggles, no popup).
            assert!(
                !open_choice_at(&mut scene, 0, 4),
                "bool column has no dropdown"
            );
        });
    }

    #[test]
    fn r940_keyboard_roves_then_commits_one_step() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_choice_at(&mut scene, 0, 1), "Hero Type popup opens"); // cursor 0 (sprite)
            assert!(
                grid_key(&mut scene, "ArrowDown"),
                "consumed by the open popup"
            );
            assert!(grid_key(&mut scene, "ArrowDown"));
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(2)),
                "roved to material"
            );
            assert!(grid_key(&mut scene, "End"));
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(4)),
                "End clamps to last"
            );
            assert!(grid_key(&mut scene, "Home"));
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(0))
            );
            // Enter commits the cursor (option 0 = sprite, the seed) and closes.
            assert!(grid_key(&mut scene, "ArrowDown")); // -> 1 (mesh)
            assert!(grid_key(&mut scene, "Enter"));
            assert_eq!(cell_choice(&scene, "value.0.1"), 1, "committed mesh");
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false)),
                "closed on commit"
            );
        });
    }

    #[test]
    fn r940_escape_closes_without_committing() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_choice_at(&mut scene, 1, 1)); // Tree = mesh (1)
            assert!(grid_key(&mut scene, "ArrowUp")); // cursor 1 -> 0, not committed
            assert!(grid_key(&mut scene, "Escape"));
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                cell_choice(&scene, "value.1.1"),
                1,
                "value unchanged on Escape"
            );
        });
    }

    #[test]
    fn r940_option_click_commits_dismiss_does_not() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            grid_send(&mut scene, "1_1:PointerUp"); // open Tree Type
            grid_send(&mut scene, "opt3:PointerUp"); // click "audio"
            assert_eq!(
                cell_choice(&scene, "value.1.1"),
                3,
                "option click committed audio"
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            // Re-open + click the dismiss barrier: closes, no commit.
            grid_send(&mut scene, "1_1:PointerUp");
            grid_send(&mut scene, "dismiss:PointerUp");
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                cell_choice(&scene, "value.1.1"),
                3,
                "dismiss kept the prior value"
            );
        });
    }

    #[test]
    fn r940_choose_rpc_commits_out_of_range_rejected() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_choice_at(&mut scene, 0, 1));
            assert_eq!(
                grid_invoke(&mut scene, "choose", IntrospectValue::Int(2)),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(
                cell_choice(&scene, "value.0.1"),
                2,
                "choose committed material"
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            // An out-of-range choose commits nothing and closes any popup.
            assert!(open_choice_at(&mut scene, 0, 1));
            assert_eq!(
                grid_invoke(&mut scene, "choose", IntrospectValue::Int(99)),
                Ok(IntrospectValue::Bool(false))
            );
            assert_eq!(
                cell_choice(&scene, "value.0.1"),
                2,
                "unchanged on out-of-range choose"
            );
        });
    }

    #[test]
    fn r940_choice_commit_is_one_undo_step() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_choice_at(&mut scene, 0, 1)); // sprite (0)
            assert_eq!(
                grid_invoke(&mut scene, "choose", IntrospectValue::Int(3)),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(cell_choice(&scene, "value.0.1"), 3, "committed audio");
            // The dropdown pick journals exactly like every other cell edit.
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                cell_choice(&scene, "value.0.1"),
                0,
                "undo reverts to sprite"
            );
            assert!(undo_invoke(&mut scene, "redo"));
            assert_eq!(cell_choice(&scene, "value.0.1"), 3, "redo re-applies audio");
        });
    }

    #[test]
    fn r940_value_intervene_choice_by_int_strict() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // The AI-first typed write: a choice cell takes an option Int index.
            assert_eq!(
                grid_set(&mut scene, "value.0.1", IntrospectValue::Int(4)),
                Ok(())
            );
            assert_eq!(cell_choice(&scene, "value.0.1"), 4, "set to script");
            // Out-of-range index → OutOfRange; wrong payload type → TypeMismatch.
            assert_eq!(
                grid_set(&mut scene, "value.0.1", IntrospectValue::Int(9)),
                Err(InterveneError::OutOfRange)
            );
            assert_eq!(
                grid_set(
                    &mut scene,
                    "value.0.1",
                    IntrospectValue::Text("mesh".to_owned())
                ),
                Err(InterveneError::TypeMismatch)
            );
            assert_eq!(
                cell_choice(&scene, "value.0.1"),
                4,
                "rejected writes left the value"
            );
        });
    }

    #[test]
    fn r940_scalar_intervene_unchanged_by_value_path() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // The value-level `with_intervene` delegates scalar kinds to `coerce`,
            // so a numeric / bool write behaves exactly as before the choice change.
            assert_eq!(
                grid_set(&mut scene, "value.0.2", IntrospectValue::Int(42)),
                Ok(())
            );
            assert_eq!(cell_int(&scene, "value.0.2"), 42);
            assert_eq!(
                grid_set(
                    &mut scene,
                    "value.0.2",
                    IntrospectValue::Text("x".to_owned())
                ),
                Err(InterveneError::TypeMismatch)
            );
        });
    }

    #[test]
    fn r940_popup_anchor_geometry() {
        // Below the cell at rest; flipped above near the bottom; gone when the
        // editing row scrolls out of the body viewport; x tracks the h-scroll.
        let ph = choice_panel_h(5); // 5 * 30 + 12 = 162
        assert_eq!(ph, 162);
        // Row at the top, no scroll: anchored under the cell, x past the handle
        // column (HANDLE_W + COL_W[0] = 22 + 160 = 182), y below the row.
        assert_eq!(popup_anchor(0, TYPE_COL, ph, 0, 0), Some((182, 72)));
        // A row deep in the flatten flips the panel above (it would overflow the
        // grid's bottom dropping down).
        assert_eq!(popup_anchor(5, TYPE_COL, ph, 0, 0), Some((182, 54)));
        // Scrolled clear of the body viewport → no panel.
        assert_eq!(popup_anchor(0, TYPE_COL, ph, 100, 0), None);
        // The horizontal scroll slides x left (the panel is outside the scroll).
        assert_eq!(popup_anchor(0, TYPE_COL, ph, 0, 50), Some((132, 72)));
    }

    #[test]
    fn r940_open_popup_a11y_is_a_listbox_with_one_active_descendant() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_choice_at(&mut scene, 0, 1)); // cursor 0 (sprite)
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            // The open dropdown is announced as a listbox with one option per kind.
            let listbox = nodes
                .iter()
                .find(|n| n.tag == CHOICE_POPUP_TAG)
                .expect("listbox node");
            assert_eq!(listbox.role, AriaRole::Listbox);
            let opt0 = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#{CHOICE_OPT_PREFIX}0"))
                .expect("option 0");
            assert!(
                opt0.state.focused,
                "the cursor option is the active descendant"
            );
            // The grid CELL focus is suppressed while the popup owns the descendant
            // (exactly one active descendant — the R873 one-gate).
            let cell = nodes
                .iter()
                .find(|n| n.tag == cell_tag(0, 1))
                .expect("the Type cell");
            assert!(
                !cell.state.focused,
                "no double active-descendant while the popup is open"
            );
            // The focus target rings the option within the grid (combobox shape).
            let focus =
                DataGridView::access_focus_target(&(TextFieldState::Idle, 0), Some(GRID_TAG))
                    .expect("focus");
            assert_eq!(focus.focus_tag, GRID_TAG);
            assert_eq!(focus.active_descendant.as_deref(), Some("data_grid#opt0"));
        });
    }

    #[test]
    fn r940_closed_popup_emits_no_listbox() {
        Owner::new().run(|| {
            let _scene = boot_scene();
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            assert!(
                nodes.iter().all(|n| n.tag != CHOICE_POPUP_TAG),
                "no listbox when no popup is open"
            );
            // The focused cell is the active descendant again (not suppressed).
            let cell = nodes
                .iter()
                .find(|n| n.tag == cell_tag(0, 0))
                .expect("the focused cell");
            assert!(cell.state.focused);
        });
    }

    #[test]
    fn r940_keyboard_intercept_only_while_popup_open() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            // No popup → ArrowDown moves the grid cursor down a row.
            assert!(grid_key(&mut scene, "ArrowDown"));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(1))
            );
            // Open a dropdown → ArrowDown now roves the OPTION cursor, the grid
            // row cursor is frozen (the popup owns the keymap).
            assert!(open_choice_at(&mut scene, 1, 1));
            assert!(grid_key(&mut scene, "ArrowDown"));
            assert_eq!(
                grid_intro(&scene).query("focused_row"),
                Some(IntrospectValue::Int(1)),
                "grid cursor frozen"
            );
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(2)),
                "option cursor moved"
            );
        });
    }

    #[test]
    fn r940_choice_composes_with_filter_and_group() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // The choice column degrades to its selected label, so the existing
            // filter / group proxies index it for free — set a row to a new value
            // and watch the filter pick it up.
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "set_filter",
                    IntrospectValue::Text("1=material".to_owned())
                ),
                Ok(IntrospectValue::Int(0))
            );
            assert!(grid_set(&mut scene, "value.0.1", IntrospectValue::Int(2)).is_ok()); // Hero -> material
            // Re-apply the filter (the edit landed under it); now Hero passes.
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "set_filter",
                    IntrospectValue::Text("1=material".to_owned())
                ),
                Ok(IntrospectValue::Int(1))
            );
            assert_eq!(
                grid_intro(&scene).query("source_at.0"),
                Some(IntrospectValue::Int(0)),
                "Hero (material) is the only match"
            );
        });
    }

    #[test]
    fn r941_1_popup_open_gates_on_visibility_not_latch() {
        // R941.1 session-review fix: popup_open reports the VISIBLE state (ONE
        // gate, popup_pos — the same predicate the keyboard / a11y / paint use),
        // not just the edit latch. A filter that hides the editing row makes
        // popup_open=false (matching the un-painted panel), while the raw latch
        // stays observable via editing_row — a view change (filter / group /
        // collapse) does NOT cancel the latch (only a structural splice / undo does).
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_choice_at(&mut scene, 0, 1)); // Hero, Type=sprite — visible
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(true))
            );
            // Filter Type=mesh: Hero (sprite) is hidden, but the latch survives.
            let _ = grid_invoke(
                &mut scene,
                "set_filter",
                IntrospectValue::Text("1=mesh".to_owned()),
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false)),
                "a filtered-out editing row reports closed (matches the un-painted panel)",
            );
            assert_eq!(
                grid_intro(&scene).query("editing_row"),
                Some(IntrospectValue::Int(0)),
                "the raw latch is still observable (the view change did not cancel it)",
            );
            // Clearing the filter re-shows the row → popup_open is true again.
            let _ = grid_invoke(&mut scene, "set_filter", IntrospectValue::Null);
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(true)),
                "re-showing the row re-opens the still-latched popup",
            );
        });
    }

    // ─── R943 colour swatch column ────────────────────────────────

    #[test]
    fn r943_seed_tint_column_is_colour() {
        Owner::new().run(|| {
            let scene = boot_scene();
            assert_eq!(
                grid_intro(&scene).query("col_kind.5"),
                Some(IntrospectValue::Text("color".to_owned())),
            );
            // The seeded Tint cells are the preset swatches (4 / 3 / 5 / 2).
            assert_eq!(
                cell_hex(&scene, "value.0.5"),
                COLOR_SWATCHES[4].0.to_hex(),
                "row 0 = Blue"
            );
            assert_eq!(
                cell_hex(&scene, "value.1.5"),
                COLOR_SWATCHES[3].0.to_hex(),
                "row 1 = Green"
            );
            assert_eq!(
                cell_hex(&scene, "value.3.5"),
                COLOR_SWATCHES[2].0.to_hex(),
                "row 3 = Red"
            );
        });
    }

    #[test]
    fn r943_click_and_doubleclick_open_swatch_popup() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // A single click on a colour cell opens the swatch popup, seeding the
            // cursor at the preset matching the cell's colour (row 0 = swatch 4).
            grid_send(&mut scene, "0_5:PointerUp");
            let intro = grid_intro(&scene);
            assert_eq!(intro.query("popup_open"), Some(IntrospectValue::Bool(true)));
            assert_eq!(intro.query("editing_col"), Some(IntrospectValue::Int(5)));
            assert_eq!(
                intro.query("popup_cursor"),
                Some(IntrospectValue::Int(4)),
                "cursor at current preset"
            );
            // A double-click on another colour cell re-opens there (row 1 = swatch 3).
            grid_send(&mut scene, "1_5:DoubleClick");
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(3))
            );
        });
    }

    #[test]
    fn r943_open_color_rejects_non_colour_cell() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(
                !open_color_at(&mut scene, 0, 0),
                "text column has no swatch popup"
            );
            assert!(
                !open_color_at(&mut scene, 0, 1),
                "choice column has no swatch popup"
            );
            assert!(
                !open_color_at(&mut scene, 0, 4),
                "bool column has no swatch popup"
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
        });
    }

    #[test]
    fn r943_swatch_click_commits_dismiss_does_not() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            grid_send(&mut scene, "0_5:PointerUp"); // open (row 0 = Blue / swatch 4)
            grid_send(&mut scene, "sw2:PointerUp"); // click Red (swatch 2)
            assert_eq!(
                cell_hex(&scene, "value.0.5"),
                COLOR_SWATCHES[2].0.to_hex(),
                "swatch click commits Red"
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            // Re-open + dismiss-barrier click: closes, no commit.
            grid_send(&mut scene, "0_5:PointerUp");
            grid_send(&mut scene, "dismiss:PointerUp");
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            assert_eq!(
                cell_hex(&scene, "value.0.5"),
                COLOR_SWATCHES[2].0.to_hex(),
                "dismiss kept Red"
            );
        });
    }

    #[test]
    fn r943_keyboard_2d_roves_then_commits_one_step() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_color_at(&mut scene, 0, 5)); // current Blue = swatch 4
            assert!(grid_key(&mut scene, "Home"));
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(0))
            );
            // 2-D nav: ArrowDown jumps a palette row (SWATCH_COLS = 4).
            assert!(grid_key(&mut scene, "ArrowDown"));
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(4)),
                "down jumps a row"
            );
            assert!(grid_key(&mut scene, "ArrowRight")); // 4 -> 5 (Yellow)
            assert!(grid_key(&mut scene, "Enter")); // commit + close
            assert_eq!(
                cell_hex(&scene, "value.0.5"),
                COLOR_SWATCHES[5].0.to_hex(),
                "committed Yellow"
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            // The swatch pick journals exactly like every other cell edit (one step).
            assert!(undo_invoke(&mut scene, "undo"));
            assert_eq!(
                cell_hex(&scene, "value.0.5"),
                COLOR_SWATCHES[4].0.to_hex(),
                "undo reverts to Blue"
            );
        });
    }

    #[test]
    fn r943_pick_color_rpc_and_arbitrary_hex_intervene() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_color_at(&mut scene, 2, 5));
            // The AI-first preset path: pick_color commits swatch i + closes.
            assert_eq!(
                grid_invoke(&mut scene, "pick_color", IntrospectValue::Int(6)),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                cell_hex(&scene, "value.2.5"),
                COLOR_SWATCHES[6].0.to_hex(),
                "pick_color committed Cyan"
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(false))
            );
            // An out-of-range pick_color commits nothing and closes any popup.
            assert!(open_color_at(&mut scene, 2, 5));
            assert_eq!(
                grid_invoke(&mut scene, "pick_color", IntrospectValue::Int(99)),
                Ok(IntrospectValue::Bool(false)),
            );
            assert_eq!(
                cell_hex(&scene, "value.2.5"),
                COLOR_SWATCHES[6].0.to_hex(),
                "unchanged on bad index"
            );
            // The arbitrary (off-palette) colour path: intervene value with a hex.
            assert_eq!(
                grid_set(
                    &mut scene,
                    "value.2.5",
                    IntrospectValue::Text("#123456".to_owned())
                ),
                Ok(()),
            );
            assert_eq!(
                cell_hex(&scene, "value.2.5"),
                "#123456",
                "off-palette hex set via intervene"
            );
        });
    }

    #[test]
    fn r943_open_popup_a11y_is_a_listbox_of_swatches() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_color_at(&mut scene, 0, 5)); // Blue = swatch 4
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            let listbox = nodes
                .iter()
                .find(|n| n.tag == COLOR_POPUP_TAG)
                .expect("swatch listbox node");
            assert_eq!(listbox.role, AriaRole::Listbox);
            let sw_count = nodes
                .iter()
                .filter(|n| n.tag.starts_with(&format!("{GRID_TAG}#{COLOR_SW_PREFIX}")))
                .count();
            assert_eq!(
                sw_count,
                COLOR_SWATCHES.len(),
                "one option per preset swatch"
            );
            let cursor = nodes
                .iter()
                .find(|n| n.tag == format!("{GRID_TAG}#{COLOR_SW_PREFIX}4"))
                .expect("swatch 4");
            assert!(
                cursor.state.focused,
                "the cursor swatch is the active descendant"
            );
            // Exactly one active descendant — the grid CELL focus is suppressed.
            let cell = nodes
                .iter()
                .find(|n| n.tag == cell_tag(0, 5))
                .expect("the Tint cell");
            assert!(
                !cell.state.focused,
                "no double active-descendant while the popup is open"
            );
        });
    }

    #[test]
    fn r943_closed_cell_paints_and_open_paints_the_swatch_popup() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // R897 — seed the measured viewport so the virtualized body builds
            // the seeded rows (the read-only grid tests' convention).
            use_scroll_state(V_SCROLL_KEY).set_measured_viewport(GRID_VIEWPORT_W, GRID_VIEWPORT_H);
            let painted = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(
                painted.contains_tag(&cell_tag(0, 5)),
                "the closed Tint cell paints"
            );
            assert!(
                !painted.contains_tag(COLOR_POPUP_TAG),
                "no swatch popup at rest"
            );
            // Opening the popup paints the palette + the cursor swatch chip.
            assert!(open_color_at(&mut scene, 0, 5));
            let open = view((TextFieldState::Idle, 0), &Frame::new());
            assert!(
                open.contains_tag(COLOR_POPUP_TAG),
                "the swatch palette paints when open"
            );
            assert!(
                open.contains_tag(&format!("{GRID_TAG}#{COLOR_SW_PREFIX}4")),
                "the swatch chips paint (one per preset)",
            );
        });
    }

    // ─── R943.1 session-review clearance ──────────────────────────

    #[test]
    fn r943_1_keyboard_begin_opens_the_colour_popup() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // The keyboard `Enter` / `F2` activate path is the `begin` verb. On a
            // colour cell it must open the swatch popup (like a choice cell's
            // dropdown), not fall through to `begin_edit` — a colour is not
            // text-editable, so the old `Choice`-only `begin` left colour cells
            // keyboard-inert (openable by click only). Now every activation route
            // opens the popup.
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(5));
            assert_eq!(
                grid_invoke(&mut scene, "begin", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true)),
                "begin opens the swatch popup on a colour cell",
            );
            assert_eq!(
                grid_intro(&scene).query("popup_open"),
                Some(IntrospectValue::Bool(true))
            );
            assert_eq!(
                grid_intro(&scene).query("editing_col"),
                Some(IntrospectValue::Int(5))
            );
            assert_eq!(
                grid_intro(&scene).query("popup_cursor"),
                Some(IntrospectValue::Int(4)),
                "cursor seeded at the current preset (Blue)",
            );
        });
    }

    #[test]
    fn r943_1_colour_focus_target_rings_a_swatch_not_a_phantom_option() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(open_color_at(&mut scene, 0, 5)); // Blue = swatch 4
            let focus =
                DataGridView::access_focus_target(&(TextFieldState::Idle, 0), Some(GRID_TAG))
                    .expect("focus target while the popup owns the descendant");
            assert_eq!(focus.focus_tag, GRID_TAG);
            // The active descendant must be the cursor SWATCH (sw4), not a choice
            // OPTION (opt4) the colour popup never paints — a dangling
            // `aria-activedescendant` would point the AT at a non-existent node.
            assert_eq!(focus.active_descendant.as_deref(), Some("data_grid#sw4"));
            // And the painted + a11y swatch tag matches it (the R873 byte-match).
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            assert!(
                nodes.iter().any(|n| n.tag == "data_grid#sw4"),
                "the active-descendant swatch is a real a11y node",
            );
        });
    }

    #[test]
    fn r1237_paste_writes_a_tsv_block_at_the_cursor_one_undo() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Anchor the cursor at (row 0, col 2 = Int); col 3 is Float.
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(2));
            // A 2x2 block into the Int + Float columns.
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("42\t1.5\n7\t9.5".to_owned()),
                ),
                Ok(IntrospectValue::Int(4)),
                "four cells written",
            );
            {
                let intro = grid_intro(&scene);
                assert_eq!(intro.query("value.0.2"), Some(IntrospectValue::Int(42)));
                assert_eq!(intro.query("value.0.3"), Some(IntrospectValue::Float(1.5)));
                assert_eq!(intro.query("value.1.2"), Some(IntrospectValue::Int(7)));
                assert_eq!(intro.query("value.1.3"), Some(IntrospectValue::Float(9.5)));
            }
            // The whole block is ONE undo step.
            assert_eq!(
                undo_query(&scene, "undo_label"),
                Some(IntrospectValue::Text("Paste".to_owned())),
                "the block folds into one labelled undo step",
            );
            assert!(use_undo().undo(), "one undo reverts the whole block");
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("value.0.2"),
                Some(IntrospectValue::Int(1)),
                "row0 Int restored"
            );
            assert_eq!(
                intro.query("value.1.2"),
                Some(IntrospectValue::Int(24)),
                "row1 Int restored"
            );
            assert_eq!(
                intro.query("value.1.3"),
                Some(IntrospectValue::Float(2.5)),
                "row1 Float restored"
            );
        });
    }

    #[test]
    fn r1237_paste_skips_unparseable_and_empty_is_noop() {
        Owner::new().run(|| {
            let mut scene = boot_scene(); // 4 rows
            // A value that does not parse for the column's type is skipped (the
            // cell keeps its prior value — no data loss).
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(2)); // the Int column
            assert_eq!(
                grid_invoke(&mut scene, "paste", IntrospectValue::Text("abc".to_owned())),
                Ok(IntrospectValue::Int(0)),
                "'abc' does not parse into the Int column",
            );
            assert_eq!(
                grid_intro(&scene).query("value.0.2"),
                Some(IntrospectValue::Int(1)),
                "the cell keeps its prior value",
            );
            // An empty paste is a no-op — and neither an unparseable nor an empty
            // paste grows the grid (R1247: growth is per LANDED row, not per
            // overrun line, so a line that lands no cell grows nothing).
            assert_eq!(
                grid_invoke(&mut scene, "paste", IntrospectValue::Text(String::new())),
                Ok(IntrospectValue::Int(0)),
                "an empty paste writes nothing",
            );
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(4)),
                "no unparseable / empty paste grows the grid",
            );
        });
    }

    #[test]
    fn r1247_all_unparseable_overrun_line_grows_no_phantom_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene(); // 4 rows
            // A 2-row block at the LAST row where the OVERRUN line's only cell
            // fails its column type (a text label over the Int column — the real
            // spreadsheet case). Pre-R1247 the overrun grew a PHANTOM empty row
            // and `written` hid it; now it grows nothing (mirrors the in-range
            // no-op for an unparseable cell).
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(3));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(2)); // Int col
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("55\nTotal".to_owned())
                ),
                Ok(IntrospectValue::Int(1)),
                "only the anchor cell (55) lands; the unparseable overrun lands nothing",
            );
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(4)),
                "the all-unparseable overrun line grew NO phantom row",
            );
            assert_eq!(
                grid_intro(&scene).query("value.3.2"),
                Some(IntrospectValue::Int(55)),
                "the anchor row still got 55",
            );
        });
    }

    #[test]
    fn r1247_partial_overrun_line_grows_one_row_lands_only_parseable() {
        Owner::new().run(|| {
            let mut scene = boot_scene(); // 4 rows
            // An overrun line with a parseable cell (Int) AND an unparseable one
            // (a text over the Float column) grows exactly one row and lands only
            // the parseable cell; the row's other cells keep their typed default.
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(3));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(2)); // Int, then Float
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("10\t1.0\n20\txyz".to_owned())
                ),
                Ok(IntrospectValue::Int(3)),
                "anchor row (2 cells) + grown row's one parseable Int = 3 landed",
            );
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(5)),
                "the partial overrun line grew exactly one row (it landed data)",
            );
            assert_eq!(
                grid_intro(&scene).query("value.4.2"),
                Some(IntrospectValue::Int(20)),
                "the grown row's Int cell landed",
            );
            // The grown row's Float cell got no data — it keeps the column default
            // (col 3 default is Float(0.0)), NOT the unparseable "NaNish".
            assert_eq!(
                grid_intro(&scene).query("value.4.3"),
                Some(IntrospectValue::Float(0.0)),
                "the grown row's Float cell kept its default (unparseable skipped)",
            );
        });
    }

    #[test]
    fn r1244_paste_grows_rows_to_fit_overrun() {
        Owner::new().run(|| {
            let mut scene = boot_scene(); // 4 rows (sources 0..3)
            // A 2-row block anchored at the LAST row: the 2nd row overruns the
            // grid. Pre-R1244 it clipped; now the grid GROWS one row so the whole
            // block lands (the spreadsheet-paste convention — the row model is
            // dynamic, the column schema is not).
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(3));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(2)); // Int col
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("55\n66".to_owned())
                ),
                Ok(IntrospectValue::Int(2)),
                "both rows land — the overrun grew a row instead of clipping",
            );
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(5)),
                "the grid grew by one row to fit the block",
            );
            assert_eq!(
                grid_intro(&scene).query("value.3.2"),
                Some(IntrospectValue::Int(55)),
                "the anchor row got 55",
            );
            assert_eq!(
                grid_intro(&scene).query("value.4.2"),
                Some(IntrospectValue::Int(66)),
                "the grown row (source 4) got 66",
            );
        });
    }

    #[test]
    fn r1244_paste_grow_is_one_undo_step() {
        Owner::new().run(|| {
            let mut scene = boot_scene(); // 4 rows
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(3));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(2));
            let before_anchor = grid_intro(&scene).query("value.3.2");
            // A 3-row block at the last row grows TWO rows (sources 4, 5).
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("10\n20\n30".to_owned())
                ),
                Ok(IntrospectValue::Int(3)),
                "all three rows land",
            );
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(6)),
                "grew from 4 to 6 rows",
            );
            assert_eq!(
                grid_intro(&scene).query("value.5.2"),
                Some(IntrospectValue::Int(30)),
                "the last grown row got 30",
            );
            // ONE undo reverts the WHOLE paste: the grown rows AND their cells.
            assert!(
                undo_invoke(&mut scene, "undo"),
                "one undo reverts the paste"
            );
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(4)),
                "back to 4 rows — the grown rows are gone",
            );
            assert_eq!(
                grid_intro(&scene).query("value.3.2"),
                before_anchor,
                "the anchor row's cell reverted too (one atomic paste)",
            );
            // Redo re-applies the whole grown paste.
            assert!(undo_invoke(&mut scene, "redo"), "redo re-splices the paste");
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(6)),
                "redo re-grows the rows",
            );
            assert_eq!(
                grid_intro(&scene).query("value.5.2"),
                Some(IntrospectValue::Int(30)),
                "and re-writes the grown cell",
            );
        });
    }

    #[test]
    fn r1244_paste_still_clips_columns() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Rows grow, but the COLUMN schema ([`NCOLS`]) is FIXED: a cell past
            // the right edge clips (no new column, no row growth from it). Anchor
            // at the last column (Tint / Color); a 2-cell line writes the colour
            // and clips the overflow cell.
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(5)); // last col (Color)
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("#a1b2c3\tCLIP".to_owned())
                ),
                Ok(IntrospectValue::Int(1)),
                "only the in-range colour cell lands; the overflow column clips",
            );
            assert_eq!(
                grid_intro(&scene).query("row_count"),
                Some(IntrospectValue::Int(4)),
                "a column overrun never grows rows (columns are a fixed schema)",
            );
        });
    }

    #[test]
    fn r1237_paste_follows_active_sort_not_source_order() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Sort by the Asset column so the VISUAL order differs from source.
            use_sort().set(Some((0, true)));
            let model = use_data_model().get();
            let visible = visible_data_order(
                &model,
                use_sort().get(),
                None,
                None,
                &std::collections::BTreeSet::new(),
            );
            assert_ne!(visible, vec![0, 1, 2, 3], "the sort reorders the rows");
            // Cursor at visual row 0; paste a 2-row Text block down col 0.
            use_focused_row().set(visible[0]);
            use_focused_col().set(0);
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("Alpha\nBeta".to_owned()),
                ),
                Ok(IntrospectValue::Int(2)),
            );
            // The block landed on the VISUAL rows (source visible[0], visible[1]),
            // never source rows 0 and 1.
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query(&format!("value.{}.0", visible[0])),
                Some(IntrospectValue::Text("Alpha".to_owned())),
                "visual row 0 got Alpha",
            );
            assert_eq!(
                intro.query(&format!("value.{}.0", visible[1])),
                Some(IntrospectValue::Text("Beta".to_owned())),
                "visual row 1 got Beta",
            );
        });
    }

    #[test]
    fn r1239_paste_strips_one_trailing_newline_no_phantom_write() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // A single-cell paste with a trailing newline (what OS clipboards
            // emit) into the Text column must NOT write a phantom "" into row 1.
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(0));
            assert_eq!(
                grid_invoke(&mut scene, "paste", IntrospectValue::Text("X\n".to_owned())),
                Ok(IntrospectValue::Int(1)),
                "the trailing newline does not create a 2nd written cell",
            );
            assert_eq!(
                grid_intro(&scene).query("value.0.0"),
                Some(IntrospectValue::Text("X".to_owned())),
                "row 0 Asset got X",
            );
            assert_eq!(
                grid_intro(&scene).query("value.1.0"),
                Some(IntrospectValue::Text("Tree".to_owned())),
                "row 1 Asset is UNTOUCHED (no phantom empty-string write)",
            );
            // A CRLF terminator is stripped too.
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(2));
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("Y\r\n".to_owned())
                ),
                Ok(IntrospectValue::Int(1)),
                "CRLF terminator also stripped",
            );
            assert_eq!(
                grid_intro(&scene).query("value.3.0"),
                Some(IntrospectValue::Text("Boss".to_owned())),
                "row 3 untouched by the CRLF paste",
            );
            // An INTERIOR blank row is still honored (2 rows, middle empty).
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("A\n\nC".to_owned())
                ),
                Ok(IntrospectValue::Int(3)),
                "interior blank row writes an empty Text cell (not stripped)",
            );
            assert_eq!(
                grid_intro(&scene).query("value.1.0"),
                Some(IntrospectValue::Text(String::new()))
            );
            assert_eq!(
                grid_intro(&scene).query("value.2.0"),
                Some(IntrospectValue::Text("C".to_owned()))
            );
        });
    }

    #[test]
    fn r1239_paste_off_view_cursor_is_a_no_op() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Filter to show only "Tree" (source row 1); row 0 is now hidden.
            grid_invoke(
                &mut scene,
                "set_filter",
                IntrospectValue::Text("0=Tree".to_owned()),
            )
            .unwrap();
            // Point the cursor at the HIDDEN row 0 (the intervene path clamps to
            // [0,nrows) but does not check visibility).
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(0));
            // Paste is a no-op — NOT silently dumped onto the top visible row (1).
            assert_eq!(
                grid_invoke(&mut scene, "paste", IntrospectValue::Text("Z".to_owned())),
                Ok(IntrospectValue::Int(0)),
                "an off-view cursor makes paste a no-op",
            );
            assert_eq!(
                grid_intro(&scene).query("value.1.0"),
                Some(IntrospectValue::Text("Tree".to_owned())),
                "the top visible row was NOT clobbered",
            );
            assert_eq!(
                grid_intro(&scene).query("value.0.0"),
                Some(IntrospectValue::Text("Hero".to_owned())),
                "the hidden cursor row is untouched too",
            );
        });
    }

    #[test]
    fn r1239_paste_skips_non_text_editable_columns() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // Col 1 is Choice, col 4 is Bool — neither is text-parseable, so a
            // block spanning them skips those cells (keeps prior value), while the
            // adjacent Text/Int cells land.
            let _ = grid_set(&mut scene, "focused_row", IntrospectValue::Int(0));
            let _ = grid_set(&mut scene, "focused_col", IntrospectValue::Int(0));
            // cols 0(Text) 1(Choice) 2(Int): "Zed\tanything\t77"
            assert_eq!(
                grid_invoke(
                    &mut scene,
                    "paste",
                    IntrospectValue::Text("Zed\tanything\t77".to_owned()),
                ),
                Ok(IntrospectValue::Int(2)),
                "only the Text + Int cells land; the Choice cell is skipped",
            );
            let intro = grid_intro(&scene);
            assert_eq!(
                intro.query("value.0.0"),
                Some(IntrospectValue::Text("Zed".to_owned()))
            );
            assert_eq!(intro.query("value.0.2"), Some(IntrospectValue::Int(77)));
            // The Choice cell (col 1) kept its seed value (Hero's type = index 0).
            assert_eq!(
                intro.query("value.0.1"),
                grid_intro(&boot_scene()).query("value.0.1"),
                "the Choice cell is unchanged (not text-pasteable)",
            );
        });
    }

    // ─── R1372 cell-range selection + copy (the copy/paste symmetry) ───

    #[test]
    fn r1372_cell_selection_bounds_projects_source_endpoints_to_visible_rect() {
        // Pure: the SOURCE anchor + cursor project through the visible order to a
        // normalized visible-POSITION rectangle; an off-view endpoint => None.
        let visible = [3usize, 0, 1, 2]; // a sort permutation: source -> position
        // anchor source 3 (pos 0), cursor source 1 (pos 2), cols 0..=2.
        assert_eq!(
            cell_selection_bounds(&visible, Some((3, 0)), 1, 2),
            Some((0, 0, 2, 2)),
            "endpoints project to the visible-position bbox",
        );
        // Normalizes regardless of endpoint order (anchor after the cursor).
        assert_eq!(
            cell_selection_bounds(&visible, Some((1, 2)), 3, 0),
            Some((0, 0, 2, 2)),
        );
        assert_eq!(
            cell_selection_bounds(&visible, None, 1, 0),
            None,
            "no anchor"
        );
        assert_eq!(
            cell_selection_bounds(&visible, Some((9, 0)), 1, 0),
            None,
            "anchor source not visible (filtered) collapses the range",
        );
        assert_eq!(
            cell_selection_bounds(&visible, Some((3, 0)), 9, 0),
            None,
            "cursor source not visible collapses the range",
        );
    }

    #[test]
    fn r1372_parse_row_col_rejects_malformed() {
        assert_eq!(parse_row_col("2,1"), Some((2, 1)));
        assert_eq!(parse_row_col(" 2 , 1 "), Some((2, 1)), "trims whitespace");
        assert_eq!(parse_row_col("2"), None, "no comma");
        assert_eq!(parse_row_col("x,1"), None, "non-numeric row");
        assert_eq!(parse_row_col("2,y"), None, "non-numeric col");
    }

    #[test]
    fn r1372_cell_fill_precedence_focus_over_selection() {
        let theme = Theme::light();
        // Focus wins over selection (the active descendant reads distinctly).
        assert_eq!(cell_fill(&theme, true, true), focus_fill(&theme, true));
        assert_eq!(cell_fill(&theme, true, false), focus_fill(&theme, true));
        // A selected (non-focused) cell washes — distinct from focus + transparent.
        let wash = cell_fill(&theme, false, true);
        assert_ne!(wash, Color::TRANSPARENT, "a selected cell washes");
        assert_ne!(
            wash,
            focus_fill(&theme, true),
            "the selection tone != focus"
        );
        // Neither => transparent (the surface shows through).
        assert_eq!(cell_fill(&theme, false, false), Color::TRANSPARENT);
    }

    #[test]
    fn r1372_rpc_select_extend_clear_and_reads() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // No range at boot.
            assert_eq!(intro.query("cell_selection"), Some(IntrospectValue::Null));
            assert_eq!(
                intro.query("cell_selection_count"),
                Some(IntrospectValue::Int(0))
            );
            assert_eq!(
                intro.query("cell_selection_tsv"),
                Some(IntrospectValue::Null)
            );
            // Select (0,0) then extend to (2,1): a 3x2 rectangle over the plain
            // (unsorted) view, so visible positions == source rows.
            assert_eq!(
                intro.invoke("select-cell", IntrospectValue::Text("0,0".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                intro.invoke("extend-cell", IntrospectValue::Text("2,1".to_owned())),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(
                intro.query("cell_selection"),
                Some(IntrospectValue::Text("0,0,2,1".to_owned())),
            );
            assert_eq!(
                intro.query("cell_selection_count"),
                Some(IntrospectValue::Int(6)),
                "3 rows x 2 cols",
            );
            // The Asset column (col 0) is Hero / Tree / Coin down rows 0..=2.
            let IntrospectValue::Text(tsv) = intro.query("cell_selection_tsv").unwrap() else {
                panic!("cell_selection_tsv is Text when a range is active");
            };
            assert_eq!(tsv.matches('\n').count(), 2, "3 rows -> 2 newlines");
            for line in tsv.split('\n') {
                assert_eq!(line.matches('\t').count(), 1, "2 cols -> 1 tab per row");
            }
            assert!(tsv.starts_with("Hero\t"), "row 0 asset is Hero");
            assert!(tsv.contains("\nTree\t"), "row 1 asset is Tree");
            assert!(tsv.contains("\nCoin\t"), "row 2 asset is Coin");
            // Out-of-range select is a no-op (false); a malformed pair is Rejected.
            assert_eq!(
                intro.invoke("select-cell", IntrospectValue::Text("99,0".to_owned())),
                Ok(IntrospectValue::Bool(false)),
            );
            assert!(
                intro
                    .invoke("extend-cell", IntrospectValue::Text("bad".to_owned()))
                    .is_err(),
            );
            // Clear drops the range (the cursor stays put).
            assert_eq!(
                intro.invoke("clear-cell-selection", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true)),
            );
            assert_eq!(intro.query("cell_selection"), Some(IntrospectValue::Null));
        });
    }

    #[test]
    fn r1372_copy_yields_range_then_the_lone_focused_cell() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
            let intro = node.handle.introspect_mut().expect("introspectable");
            // With no range, `copy` yields the lone FOCUSED cell (boot cursor
            // (0,0) = Hero) — a bare Ctrl+C copies one cell.
            assert_eq!(
                intro.invoke("copy", IntrospectValue::Null),
                Ok(IntrospectValue::Text("Hero".to_owned())),
            );
            // With a range, `copy` == `cell_selection_tsv` (the one funnel).
            let _ = intro.invoke("select-cell", IntrospectValue::Text("0,0".to_owned()));
            let _ = intro.invoke("extend-cell", IntrospectValue::Text("1,0".to_owned()));
            let copied = intro.invoke("copy", IntrospectValue::Null).unwrap();
            assert_eq!(copied, intro.query("cell_selection_tsv").unwrap());
            assert_eq!(copied, IntrospectValue::Text("Hero\nTree".to_owned()));
        });
    }

    #[test]
    fn r1372_keyboard_shift_arrow_extends_plain_collapses_escape_clears() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let shift = Modifiers {
                shift: true,
                ..Modifiers::empty()
            };
            let plain = Modifiers::empty();
            // From (0,0): Shift+ArrowDown then Shift+ArrowRight grow a 2x2 rect
            // from the pinned (0,0) anchor.
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                shift
            ));
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowRight",
                shift
            ));
            assert_eq!(
                grid_intro(&scene).query("cell_selection"),
                Some(IntrospectValue::Text("0,0,1,1".to_owned())),
                "Shift+arrows grow the rectangle from the pinned anchor",
            );
            assert_eq!(
                grid_intro(&scene).query("cell_selection_count"),
                Some(IntrospectValue::Int(4)),
            );
            // A plain arrow collapses the range (anchor dropped) and moves.
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowDown",
                plain
            ));
            assert_eq!(
                grid_intro(&scene).query("cell_selection"),
                Some(IntrospectValue::Null),
                "a plain arrow collapses the selection",
            );
            // Escape after a fresh Shift-extend clears the range + is consumed.
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "ArrowUp",
                shift
            ));
            assert_ne!(
                grid_intro(&scene).query("cell_selection"),
                Some(IntrospectValue::Null),
                "Shift+ArrowUp re-armed a range",
            );
            assert!(DataGridView::apply_key(
                &mut scene,
                Some(GRID_TAG),
                "Escape",
                plain
            ));
            assert_eq!(
                grid_intro(&scene).query("cell_selection"),
                Some(IntrospectValue::Null),
                "Escape cleared the range",
            );
        });
    }

    #[test]
    fn r1372_a11y_cell_aria_selected_tracks_the_range() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // No range: cells omit aria-selected (None).
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            assert_eq!(
                nodes
                    .iter()
                    .find(|n| n.tag == cell_tag(0, 0))
                    .unwrap()
                    .selected,
                None,
                "no range -> aria-selected omitted",
            );
            // Select the 2x2 rectangle (0,0)-(1,1).
            {
                let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("select-cell", IntrospectValue::Text("0,0".to_owned()));
                let _ = intro.invoke("extend-cell", IntrospectValue::Text("1,1".to_owned()));
            }
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            let sel = |t: String| nodes.iter().find(|n| n.tag == t).unwrap().selected;
            assert_eq!(
                sel(cell_tag(0, 0)),
                Some(true),
                "in-range cell aria-selected"
            );
            assert_eq!(sel(cell_tag(1, 1)), Some(true));
            assert_eq!(sel(cell_tag(0, 2)), Some(false), "col outside the range");
            assert_eq!(sel(cell_tag(2, 0)), Some(false), "row outside the range");
        });
    }

    /// R1372 — the selection wash is guarded at the scene-data level (§2 #7),
    /// not a GPU screenshot: a flat cell background is fully determined by the
    /// `view` threading selection -> [`cell_fill`], so assert the painted cell
    /// container's fill directly (deterministic + CI-safe, no pixels).
    fn fill_of_cell(scene: &Scene, tag: &str) -> Option<Color> {
        if scene.tag() == Some(tag) {
            if let Scene::Container(c) = scene {
                return Some(c.style.fill);
            }
        }
        match scene {
            Scene::Container(n) => n.children.iter().find_map(|c| fill_of_cell(c, tag)),
            Scene::Scroll(n) => fill_of_cell(&n.content, tag),
            _ => None,
        }
    }

    #[test]
    fn r1372_selected_cells_wash_distinctly_in_the_paint() {
        Owner::new().run(|| {
            let mut scene0 = boot_scene();
            {
                let node = scene0.find_external_with_tag_mut(GRID_TAG).expect("grid");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("select-cell", IntrospectValue::Text("0,0".to_owned()));
                let _ = intro.invoke("extend-cell", IntrospectValue::Text("1,1".to_owned()));
            }
            // R897 — seed a viewport so the body windows the seeded rows (the
            // read-only grids' unit-test convention; no shell layout pass).
            use_scroll_state(V_SCROLL_KEY).set_measured_viewport(GRID_VIEWPORT_W, GRID_VIEWPORT_H);
            let scene = view((TextFieldState::Idle, 0), &Frame::new());
            // Cursor ended at (1,1), so (0,0) is selected-but-not-focused (a pure
            // selection wash); (3,3) is neither (transparent).
            let sel = fill_of_cell(&scene, &cell_tag(0, 0)).expect("selected cell painted");
            let unsel = fill_of_cell(&scene, &cell_tag(3, 3)).expect("unselected cell painted");
            assert_eq!(
                unsel,
                Color::TRANSPARENT,
                "an unselected, unfocused cell is transparent (the surface shows through)",
            );
            assert_ne!(sel, Color::TRANSPARENT, "the selected cell is washed");
            assert_ne!(
                sel, unsel,
                "a selected cell paints distinctly from an unselected one"
            );
        });
    }

    #[test]
    fn r1372_1_grouped_treegrid_cells_carry_aria_selected() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            group_by_type(&mut scene);
            // Group by Type: group0 = {Hero(0), Coin(2)}, group1 = {Tree(1),
            // Boss(3)}; the visible data order is [0, 2, 1, 3]. Select col 0 of
            // the first two VISIBLE rows (sources 0 and 2) — a range that spans a
            // group boundary is still one contiguous visible band.
            {
                let node = scene.find_external_with_tag_mut(GRID_TAG).expect("grid");
                let intro = node.handle.introspect_mut().expect("introspectable");
                let _ = intro.invoke("select-cell", IntrospectValue::Text("0,0".to_owned()));
                let _ = intro.invoke("extend-cell", IntrospectValue::Text("2,0".to_owned()));
            }
            let nodes = DataGridView::access_node(&(TextFieldState::Idle, 0), Some(GRID_TAG));
            let sel = |t: String| nodes.iter().find(|n| n.tag == t).unwrap().selected;
            assert_eq!(
                sel(cell_tag(0, 0)),
                Some(true),
                "Hero col0 in the grouped range"
            );
            assert_eq!(
                sel(cell_tag(2, 0)),
                Some(true),
                "Coin col0 in the grouped range"
            );
            assert_eq!(sel(cell_tag(0, 1)), Some(false), "col 1 outside the range");
            assert_eq!(
                sel(cell_tag(1, 0)),
                Some(false),
                "Tree (visible pos 2) outside"
            );
        });
    }
}
