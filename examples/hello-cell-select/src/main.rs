//! `hello-cell-select` — R953 §5.38 **cell range selection** grid: the
//! dedicated GUI consumer of the R952
//! [`Table`](pinion_core::widgets::table::Table) `SelectItems` API (the
//! `select-cell` / `extend-cell` / `clear-cell-selection` wire), painted by
//! the §5.50 [`pinion_widget_paint::table`] data grid.
//!
//! A spreadsheet selects a **rectangle of cells** by anchor + extent (Excel,
//! Qt `QTableView::SelectItems`): a click / plain arrow starts a single-cell
//! selection at the cursor, and a `Shift`-drag / `Shift`-arrow grows the
//! rectangle from the pinned anchor. This is a *different selection model*
//! from `hello-table`'s `SelectRows` (one whole row washed). R952 added the
//! cell-range model to the `Table` coordinator while `hello-table` still
//! drove `SelectRows`; running **both** models in one grid is a no-mode smell
//! (a real editor picks one), so R953 splits the cell-range model into this
//! dedicated grid and reverts `hello-table` to pure row selection.
//!
//! ## Composition — one composite External
//!
//! The state scene holds **one** [`TableExternal`] at the [`PRIMARY_TAG`]
//! composite paint root (the same single-coordinator shape as `hello-table`),
//! constructed with the R954 [`SelectItems`](pinion_core::widgets::table::SelectionBehavior::SelectItems)
//! behavior. Each cell is tagged `"grid#<row>_<col>"` (the R51.41 composite
//! paint convention); the [`InputRouter`](pinion_runtime::input::InputRouter)
//! R51.42 `'#'`-split routes a click on cell `(r, c)` to the coordinator,
//! whose `SelectItems` behavior selects that **cell** (a plain click
//! collapses, a `Shift`+click extends the rectangle) rather than washing the
//! row — so the mouse drives the same cell-range model the keyboard does.
//!
//! ## Selection model — cell range (anchor + extent)
//!
//! The grid is a **single Tab stop** ([`focusable_tags`] returns only the
//! grid root) with an internal 2-D roving *active descendant* (the
//! coordinator's `focused_row` / `focused_col` slots), reported as the
//! `aria-activedescendant` — the WAI-ARIA grid pattern. While the grid owns
//! shell focus the keyboard drives the **cell range** selection (the
//! spreadsheet arrow model):
//! - `ArrowLeft` / `ArrowRight` / `ArrowUp` / `ArrowDown` move the cursor
//!   `∓1` / `±1`, clamped; the first arrow into the grid lands on `(0, 0)`;
//! - `Home` / `End` jump to the first / last column of the current row;
//! - `PageUp` / `PageDown` jump to the first / last row (same column);
//! - a **plain** move `select-cell`s the new cursor cell — a fresh
//!   single-cell selection that collapses any prior rectangle;
//! - a **`Shift`** move `extend-cell`s it — the rectangle grows from the
//!   pinned anchor (`bbox(anchor, cursor)`, normalized);
//! - `Escape` clears the selection (the cursor stays put).
//!
//! The selected rectangle is the bounding box of the anchor and the roving
//! cursor in **data** coordinates; there is no sort (a sorted view would
//! scatter the rectangle across the screen — the R952 overlay-omit case —
//! and a `SelectItems` grid has no use for a row sort), so data coords are
//! screen coords and the bordered accent overlay always lines up with the
//! selected cells.
//!
//! An AI agent / AT drives + reads the whole thing over the §5.12 RPC plane:
//! `select-cell "r,c"` / `extend-cell "r,c"` / `clear-cell-selection`, and
//! reads back `cell_selection` (the `"r0,c0,r1,c1"` rectangle) +
//! `cell_selection_count`.
//!
//! Pointer: a click selects the clicked cell (collapse), a `Shift`+click
//! extends the rectangle from the anchor (R954 `SelectItems` — the held
//! modifier rides the composite `send` wire's third segment, R880.1), the
//! same cell-range model the keyboard drives.
//!
//! **Deferred axes** (our-code, honestly deferred): **live marquee drag**
//! (press-drag-release to sweep a rectangle in one gesture) — that needs the
//! coordinator's capture-path pointer wiring with a pixel→cell hit-test
//! (the click / `Shift`+click range here is the discrete form). Also
//! deferred: a `Ctrl`-click disjoint multi-rectangle selection, fill-handle /
//! range copy, and editable cells.

use pinion_a11y::{
    grid_table_nodes, AccessAction, AccessFocus, AccessNode, GridCell, GridColumn, GridRow,
    WidgetA11y,
};
#[cfg(test)]
use pinion_a11y::AriaRole;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::table::{
    read_cols, read_focused_col, read_focused_row, read_rows, TableExternal,
};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::{view_table, TableData, TableSelection, TableStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCellSelectRenderer, HelloCellSelectRendererError);

const WIN_W: u32 = 540;
const WIN_H: u32 = 360;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across
/// the example gallery.
const THEME_TAG: &str = "app";

/// Composite paint root tag carrying the single [`TableExternal`].
/// Cells are tagged `"grid#<row>_<col>"`; the R51.42 `'#'`-split routes a
/// click on cell `(r, c)` to the coordinator.
const PRIMARY_TAG: &str = "grid";

/// Column headers for the fixed spreadsheet dataset.
const HEADERS: [&str; 4] = ["Q1", "Q2", "Q3", "Q4"];

/// Immutable demo dataset: a small numeric spreadsheet (quarterly figures),
/// the natural motivation for selecting a **rectangle** of cells and reading
/// the group. A `const` (the single source of truth) so [`CellSelectState`]
/// stays `Copy` without a heap `Vec`; the coordinator is built from the same
/// const and the view fn reads it directly.
const ROWS: [[&str; 4]; 6] = [
    ["120", "135", "128", "150"],
    ["88", "92", "99", "105"],
    ["210", "198", "205", "220"],
    ["45", "50", "48", "52"],
    ["310", "325", "330", "340"],
    ["17", "19", "22", "25"],
];

const NROWS: usize = ROWS.len();
const NCOLS: usize = HEADERS.len();

/// Cached projection of the grid: the 2-D roving active descendant
/// `(focused_row, focused_col)` and the selected cell rectangle. Read from
/// the single [`TableExternal`]'s introspect slots. `Copy` (no heap) so the
/// shell hands the snapshot into the `paint_producer` closure without
/// lifetime gymnastics.
///
/// Unlike `hello-table` there is **no** per-row selection / interaction
/// array, no sort key, and no visual→data permutation: this grid's only
/// selection model is the cell rectangle, and it never sorts (see the module
/// docs).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct CellSelectState {
    /// R707 §5.40 — the roving active-descendant row (0-based), or `None`
    /// before any navigation.
    focused_row: Option<usize>,
    /// R707 §5.40 — the roving active-descendant column (0-based).
    focused_col: usize,
    /// R952 §5.38 — the selected cell rectangle `(row0, col0, row1, col1)`
    /// (data coords, inclusive) from the coordinator's `cell_selection`
    /// slot, or `None` when no cells are selected.
    cell_selection: Option<(usize, usize, usize, usize)>,
}

impl CellSelectState {
    fn idle() -> Self {
        Self { focused_row: None, focused_col: 0, cell_selection: None }
    }
}

/// The cell reported as the grid's `aria-activedescendant` when the grid
/// owns focus: the roving `(focused_row, focused_col)` if set, else `(0, 0)`.
/// Clamped into the dataset so a stale value never points past the last
/// row / column.
fn active_cell(state: &CellSelectState) -> (usize, usize) {
    let row = state.focused_row.unwrap_or(0).min(NROWS - 1);
    let col = state.focused_col.min(NCOLS - 1);
    (row, col)
}

/// R953 §5.38 — decode the coordinator's `cell_selection` wire value into
/// the rectangle `(row0, col0, row1, col1)`. The **inverse** of
/// [`TableExternal`]'s Text encode (the `"r0,c0,r1,c1"` join the
/// `cell_selection` query emits); `Null` / absent / malformed → `None`.
///
/// This grid owns the bounds decode (R953 — the only consumer of the
/// 4-tuple wire form); the encode is a trivial `format!` join in the
/// coordinator's query and the decode is the non-trivial parse, so only the
/// decode is a named function (the R803 wire-form convention). The two
/// halves are pinned symmetric by the `wire_round_trip_*` test below.
fn parse_cell_bounds(
    intro: &dyn pinion_core::external::ExternalIntrospect,
) -> Option<(usize, usize, usize, usize)> {
    let IntrospectValue::Text(s) = intro.query("cell_selection")? else {
        return None;
    };
    let mut it = s.split(',').map(|p| p.trim().parse::<usize>().ok());
    Some((it.next()??, it.next()??, it.next()??, it.next()??))
}

/// R953 §5.38 — the target cell of a navigation `key` from the current
/// cursor, clamped into the dataset; `None` for a non-navigation key. The
/// first navigation into the grid (no cursor yet) lands on `(0, 0)` — the
/// WAI-ARIA "first arrow into the grid" entry. The coordinator's
/// `select-cell` / `extend-cell` move the cursor there, so this only
/// computes *where*, not the selection action (plain vs `Shift`).
fn nav_target(
    intro: &dyn pinion_core::external::ExternalIntrospect,
    key: &str,
) -> Option<(usize, usize)> {
    let rows = read_rows(intro);
    let cols = read_cols(intro);
    if rows == 0 || cols == 0 {
        return None;
    }
    let r_last = rows - 1;
    let c_last = cols - 1;
    let Some(cur_row) = read_focused_row(intro) else {
        // Grid entry: the first navigation key lands the cursor on (0, 0),
        // regardless of which arrow / Home / End / Page key it is.
        return matches!(
            key,
            "ArrowLeft"
                | "ArrowRight"
                | "ArrowUp"
                | "ArrowDown"
                | "Home"
                | "End"
                | "PageUp"
                | "PageDown"
        )
        .then_some((0, 0));
    };
    let cur_row = cur_row.min(r_last);
    let cur_col = read_focused_col(intro).min(c_last);
    let target = match key {
        "ArrowLeft" => (cur_row, cur_col.saturating_sub(1)),
        "ArrowRight" => (cur_row, (cur_col + 1).min(c_last)),
        "ArrowUp" => (cur_row.saturating_sub(1), cur_col),
        "ArrowDown" => ((cur_row + 1).min(r_last), cur_col),
        "Home" => (cur_row, 0),
        "End" => (cur_row, c_last),
        "PageUp" => (0, cur_col),
        "PageDown" => (r_last, cur_col),
        _ => return None,
    };
    Some(target)
}

/// view-fn (§6.3): pure sync mapping [`CellSelectState`] -> [`Scene`]. Wraps
/// [`view_table`] in a centred surface container. The keyboard-focus ring is
/// the shell's job (R694 `paint_focus_ring` over the active-descendant cell,
/// resolved through `access_focus_target`), so the view fn paints no focus
/// state itself.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &CellSelectState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // R1020 §5.39 — the grid is a single Tab stop; opt the table into the
    // scene-derived focus enumeration so its PRIMARY_TAG is collected.
    let style = TableStyle::m3().with_focusable(true);
    let rows: Vec<&[&str]> = ROWS.iter().map(|r| &r[..]).collect();
    // SelectItems grid: no row is washed (the cell rectangle is the only
    // selection), so the per-row bitmap is all-false and the cells carry the
    // selected rectangle. Unsorted, so the overlay maps to one contiguous
    // screen rectangle (R952 overlay-omit applies only to a sorted view).
    let no_rows_selected = [false; NROWS];
    let row_states = [RadioState::Idle; NROWS];
    let table = view_table(
        PRIMARY_TAG,
        TableData { headers: &HEADERS, rows: &rows, row_ids: &[] },
        TableSelection { rows: &no_rows_selected, cells: state.cell_selection },
        &row_states,
        None,
        &theme,
        &style,
    );
    Scene::Container(
        ContainerNode::new(vec![table])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

struct CellSelectView;

impl WidgetCore for CellSelectView {
    type State = CellSelectState;
    // Every state change flows through `apply_key` (keyboard) or the
    // §5.12 RPC `invoke` plane (`select-cell` / `extend-cell`), never the
    // shell's enum-typed `keybinding` channel — so `()` satisfies the
    // trait's `Copy` bound without an unused event variant.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let headers = HEADERS.iter().map(|s| (*s).to_string()).collect();
        let rows = ROWS
            .iter()
            .map(|r| r.iter().map(|c| (*c).to_string()).collect())
            .collect();
        // R954 §5.38 — the `SelectItems` behavior: a pointer click selects the
        // clicked cell (collapse) and `Shift`+click extends the rectangle, so
        // the mouse and the keyboard drive the *same* cell-range model (no
        // invisible row washing — the R953 SelectRows-on-click smell).
        Box::new(TableExternal::with_select_items(headers, rows))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> CellSelectState {
        let mut out = CellSelectState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // The introspect channel is the single source of truth: an AI client
        // running `scene/query /grid/external/cell_selection` sees exactly
        // the rectangle the view fn renders.
        out.focused_row = read_focused_row(intro);
        out.focused_col = read_focused_col(intro);
        out.cell_selection = parse_cell_bounds(intro);
        out
    }

    fn view(state: CellSelectState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // No typed events reach the keybinding channel; the single source of
        // truth for the keymap lives in `apply_key`.
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-cell-select (R953 §5.38 cell range selection)"
    }

    /// R953 §5.38 — the cell-range keyboard model. While the grid owns shell
    /// focus a navigation key moves the 2-D cursor and either collapses the
    /// selection to it (plain — `select-cell`) or extends the rectangle from
    /// the pinned anchor (`Shift` — `extend-cell`); `Escape` clears the
    /// selection. `select-cell` / `extend-cell` move the cursor themselves,
    /// so this computes the clamped target and drives the wire — there is no
    /// separate cursor `intervene`.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(PRIMARY_TAG) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        if let Some((row, col)) = nav_target(intro, key) {
            let action = if modifiers.shift { "extend-cell" } else { "select-cell" };
            let _ = intro.invoke(action, IntrospectValue::Text(format!("{row},{col}")));
            return true;
        }
        if key == "Escape" {
            let _ = intro.invoke("clear-cell-selection", IntrospectValue::Null);
            return true;
        }
        false
    }

    fn fmt_state_log(state: &CellSelectState) -> String {
        let (fr, fc) = active_cell(state);
        let sel = state.cell_selection.map_or_else(
            || "none".to_string(),
            |(r0, c0, r1, c1)| format!("{r0},{c0},{r1},{c1}"),
        );
        format!("cursor=({fr},{fc}) cells={sel}")
    }
}

impl WidgetA11y for CellSelectView {
    /// R953 §5.40 — composite AccessKit tree contribution. Emits the `grid`
    /// root, a header `row` of `columnheader` nodes, one data `row` per
    /// dataset row, and one `gridcell` per cell. Each cell carries
    /// `aria-selected` from the cell-range membership (the WAI-ARIA
    /// `SelectItems` grid; rows themselves are never selected here). The
    /// active descendant (when the grid owns focus) is the cell
    /// `aria-activedescendant` points to.
    fn access_node(state: &CellSelectState, focused: Option<&str>) -> Vec<AccessNode> {
        let grid_focused = focused == Some(PRIMARY_TAG);
        let (active_row, active_col) = active_cell(state);
        let columns: Vec<GridColumn> = HEADERS
            .iter()
            .enumerate()
            .map(|(col, label)| GridColumn {
                tag: format!("{PRIMARY_TAG}_ch{col}"),
                label: (*label).to_owned(),
                sort: None,
            })
            .collect();
        let rows: Vec<GridRow> = (0..NROWS)
            .map(|data| GridRow {
                tag: format!("{PRIMARY_TAG}_row{data}"),
                // Cell-range grid: the row itself is never aria-selected; the
                // selection lives on the cells.
                selected: false,
                state: RadioState::Idle,
                cells: HEADERS
                    .iter()
                    .enumerate()
                    .map(|(col, header)| GridCell {
                        tag: format!("{PRIMARY_TAG}#{data}_{col}"),
                        name: format!("{header}: {}", ROWS[data][col]),
                        focused: grid_focused && active_row == data && active_col == col,
                        // R953 §5.38 — per-cell aria-selected when a cell
                        // range is active (`Some`); `None` (omit) otherwise.
                        // Inclusive-rectangle membership against the bounds
                        // (the a11y is the only consumer of per-cell
                        // membership — the paint overlay draws one geometric
                        // rect from the same bounds, so there is no shared
                        // lift).
                        selected: state.cell_selection.map(|(r0, c0, r1, c1)| {
                            data >= r0 && data <= r1 && col >= c0 && col <= c1
                        }),
                    })
                    .collect(),
            })
            .collect();
        grid_table_nodes(
            PRIMARY_TAG,
            "quarterly figures",
            false,
            &format!("{PRIMARY_TAG}_hrow"),
            &columns,
            &rows,
        )
    }

    /// R953 §5.40 — composite focus model. When the grid owns shell focus,
    /// focus stays on the grid root ([`PRIMARY_TAG`]) and the
    /// active-descendant cell is reported as the `aria-activedescendant`.
    /// Any other focused tag passes through atomically.
    fn access_focus_target(state: &CellSelectState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(PRIMARY_TAG) {
            let (row, col) = active_cell(state);
            Some(AccessFocus::composite(
                PRIMARY_TAG,
                format!("{PRIMARY_TAG}#{row}_{col}"),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// R953 §5.38 — composite child action dispatch. A cell carries a
    /// composite tag (`"grid#<r>_<c>"`), so an AT `Click` / `Default` on a
    /// cell splits at `'#'` and arrives here with the `"<r>_<c>"` sub-tag;
    /// it `select-cell`s that single cell (collapse) — the cell-range
    /// model's AT affordance, the pointer equivalent of a plain arrow.
    /// `Focus` is accepted without mutation; other actions decline so the
    /// shell keeps its fallback chain.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let Some((row_str, col_str)) = sub_tag.split_once('_') else {
            return false;
        };
        let (Ok(row), Ok(col)) = (row_str.parse::<usize>(), col_str.parse::<usize>()) else {
            return false;
        };
        if row >= read_rows(intro) || col >= read_cols(intro) {
            return false;
        }
        match action {
            AccessAction::Click | AccessAction::Default => {
                let _ = intro.invoke("select-cell", IntrospectValue::Text(format!("{row},{col}")));
                true
            }
            AccessAction::Focus => true,
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for CellSelectView {
    type Renderer = HelloCellSelectRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<CellSelectView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    /// Build a fresh grid scene mirroring `create_external` so `apply_key` /
    /// `access_child_invoke` walk the live shell's exact topology (one
    /// composite `Scene::External` at `PRIMARY_TAG`).
    fn scene_fixture() -> Scene {
        Scene::External(ExternalNode::new(CellSelectView::create_external()).with_tag(PRIMARY_TAG))
    }

    fn key(scene: &mut Scene, name: &str, shift: bool) -> bool {
        let modifiers = pinion_core::Modifiers { shift, ..Default::default() };
        CellSelectView::apply_key(scene, Some(PRIMARY_TAG), name, modifiers)
    }

    fn bounds(scene: &Scene) -> Option<(usize, usize, usize, usize)> {
        let Scene::External(node) = scene else {
            return None;
        };
        parse_cell_bounds(node.handle.introspect().expect("introspect"))
    }

    fn cursor(scene: &Scene) -> (i64, i64) {
        let Scene::External(node) = scene else {
            return (-1, -1);
        };
        let intro = node.handle.introspect().expect("introspect");
        let r = match intro.query("focused_row") {
            Some(IntrospectValue::Int(r)) => r,
            _ => -1,
        };
        let c = match intro.query("focused_col") {
            Some(IntrospectValue::Int(c)) => c,
            _ => -1,
        };
        (r, c)
    }

    #[test]
    fn initial_state_has_no_cursor_or_selection() {
        let scene = scene_fixture();
        assert_eq!(cursor(&scene), (-1, 0));
        assert_eq!(bounds(&scene), None);
    }

    #[test]
    fn first_arrow_enters_at_origin_and_selects_one_cell() {
        let mut scene = scene_fixture();
        assert!(key(&mut scene, "ArrowDown", false), "first ArrowDown handled");
        assert_eq!(cursor(&scene), (0, 0), "first arrow enters at (0,0)");
        assert_eq!(bounds(&scene), Some((0, 0, 0, 0)), "a plain arrow selects one cell");
    }

    #[test]
    fn plain_arrow_collapses_selection_to_cursor() {
        let mut scene = scene_fixture();
        key(&mut scene, "ArrowDown", false); // (0,0)
        key(&mut scene, "ArrowDown", true); // extend to (1,0)
        key(&mut scene, "ArrowRight", true); // extend to (1,1) -> 2x2
        assert_eq!(bounds(&scene), Some((0, 0, 1, 1)), "Shift built a 2x2 rectangle");
        // A plain arrow collapses the rectangle to the single new cell.
        key(&mut scene, "ArrowDown", false);
        assert_eq!(cursor(&scene), (2, 1), "cursor advanced");
        assert_eq!(bounds(&scene), Some((2, 1, 2, 1)), "plain arrow collapsed to one cell");
    }

    #[test]
    fn shift_arrow_extends_rectangle_from_anchor() {
        let mut scene = scene_fixture();
        key(&mut scene, "ArrowDown", false); // select (0,0) -> anchor (0,0)
        key(&mut scene, "ArrowDown", true); // extend (1,0)
        key(&mut scene, "ArrowDown", true); // extend (2,0)
        key(&mut scene, "ArrowRight", true); // extend (2,1)
        assert_eq!(bounds(&scene), Some((0, 0, 2, 1)), "rectangle (0,0)->(2,1)");
        // Extending back toward the anchor shrinks it (anchor stays pinned).
        key(&mut scene, "ArrowUp", true); // (1,1)
        assert_eq!(bounds(&scene), Some((0, 0, 1, 1)), "anchor (0,0) pinned, extent shrank");
    }

    #[test]
    fn shift_arrow_normalizes_past_the_anchor() {
        let mut scene = scene_fixture();
        // Land the cursor at (2,2), collapse-select it (anchor (2,2)).
        key(&mut scene, "ArrowDown", false);
        key(&mut scene, "ArrowDown", false);
        key(&mut scene, "ArrowDown", false); // (2,0)
        key(&mut scene, "ArrowRight", false);
        key(&mut scene, "ArrowRight", false); // (2,2), anchor (2,2)
        assert_eq!(bounds(&scene), Some((2, 2, 2, 2)));
        // Extend up-left past the anchor: bounds normalize (row0<=row1).
        key(&mut scene, "ArrowUp", true); // (1,2)
        key(&mut scene, "ArrowLeft", true); // (1,1)
        assert_eq!(bounds(&scene), Some((1, 1, 2, 2)), "bounds normalize regardless of order");
    }

    #[test]
    fn escape_clears_selection_keeps_cursor() {
        let mut scene = scene_fixture();
        key(&mut scene, "ArrowDown", false);
        key(&mut scene, "ArrowDown", true);
        assert_eq!(bounds(&scene), Some((0, 0, 1, 0)));
        assert!(key(&mut scene, "Escape", false), "Escape handled");
        assert_eq!(bounds(&scene), None, "selection cleared");
        assert_eq!(cursor(&scene), (1, 0), "cursor stays after clear");
    }

    #[test]
    fn home_end_page_jump_within_dataset() {
        let mut scene = scene_fixture();
        key(&mut scene, "ArrowDown", false); // (0,0)
        key(&mut scene, "End", false);
        assert_eq!(cursor(&scene).1, i64::try_from(NCOLS - 1).unwrap(), "End -> last col");
        key(&mut scene, "Home", false);
        assert_eq!(cursor(&scene).1, 0, "Home -> first col");
        key(&mut scene, "PageDown", false);
        assert_eq!(cursor(&scene).0, i64::try_from(NROWS - 1).unwrap(), "PageDown -> last row");
        key(&mut scene, "PageUp", false);
        assert_eq!(cursor(&scene).0, 0, "PageUp -> first row");
    }

    #[test]
    fn keys_ignored_when_grid_unfocused() {
        let mut scene = scene_fixture();
        assert!(!CellSelectView::apply_key(
            &mut scene,
            None,
            "ArrowDown",
            pinion_core::Modifiers::default()
        ));
        assert_eq!(cursor(&scene), (-1, 0));
    }

    #[test]
    fn access_node_emits_grid_with_cell_aria_selected() {
        let mut state = CellSelectState::idle();
        state.cell_selection = Some((1, 1, 2, 2));
        let nodes = CellSelectView::access_node(&state, Some(PRIMARY_TAG));
        assert_eq!(nodes[0].role, AriaRole::Grid);
        // A cell inside the rectangle is aria-selected; one outside is not.
        let inside = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}#2_1"))
            .expect("gridcell 2_1");
        assert_eq!(inside.role, AriaRole::GridCell);
        assert_eq!(inside.selected, Some(true), "cell in the rectangle is aria-selected");
        let outside = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}#0_0"))
            .expect("gridcell 0_0");
        assert_eq!(outside.selected, Some(false), "cell outside is aria-selected=false");
        // Rows themselves are never selected in a SelectItems grid.
        let row0 = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}_row0"))
            .expect("data row 0");
        assert_eq!(row0.selected, Some(false), "rows are not selected");
    }

    #[test]
    fn access_node_omits_cell_selected_with_no_selection() {
        let nodes = CellSelectView::access_node(&CellSelectState::idle(), None);
        let cell = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}#0_0"))
            .expect("gridcell 0_0");
        assert_eq!(cell.selected, None, "no aria-selected attribute without a selection");
    }

    #[test]
    fn access_focus_target_reports_active_descendant_cell() {
        let mut state = CellSelectState::idle();
        state.focused_row = Some(2);
        state.focused_col = 3;
        let target = CellSelectView::access_focus_target(&state, Some(PRIMARY_TAG))
            .expect("composite focus target");
        assert_eq!(target.focus_tag, PRIMARY_TAG);
        assert_eq!(target.active_descendant.as_deref(), Some("grid#2_3"));
    }

    #[test]
    fn at_click_selects_single_cell() {
        let mut scene = scene_fixture();
        assert!(CellSelectView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "3_2",
            AccessAction::Click,
        ));
        assert_eq!(bounds(&scene), Some((3, 2, 3, 2)), "AT click selects that one cell");
        assert_eq!(cursor(&scene), (3, 2), "the cursor followed the AT click");
    }

    /// R954 §5.38 — a real pointer click routes through the composite `send`
    /// wire (the R51.42 `'#'`-split) and, because the grid is `SelectItems`,
    /// selects the clicked *cell* (collapse) instead of washing the row; a
    /// `Shift`+click extends the rectangle.
    #[test]
    fn pointer_click_and_shift_click_drive_cell_range() {
        use pinion_core::composite_tag::compose_send_payload;
        let mut scene = scene_fixture();
        let click = |scene: &mut Scene, cell: &str, shift: bool| {
            let Scene::External(node) = scene else {
                panic!("external");
            };
            let intro = node.handle.introspect_mut().expect("introspect");
            let mods = pinion_core::Modifiers { shift, ..Default::default() };
            // A real click is the full pointer cycle; only PointerUp activates.
            for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                let wire = compose_send_payload(Some(cell), ev, mods);
                let _ = intro.invoke("send", IntrospectValue::Text(wire));
            }
        };
        click(&mut scene, "2_1", false);
        assert_eq!(bounds(&scene), Some((2, 1, 2, 1)), "plain click selects the single cell");
        assert_eq!(cursor(&scene), (2, 1), "the cursor followed the click");
        click(&mut scene, "0_0", true);
        assert_eq!(bounds(&scene), Some((0, 0, 2, 1)), "Shift+click extends from the anchor");
    }

    #[test]
    fn read_state_round_trips_selection() {
        let mut scene = scene_fixture();
        key(&mut scene, "ArrowDown", false); // (0,0)
        key(&mut scene, "ArrowRight", true); // extend (0,1)
        let state = CellSelectView::read_state(&scene);
        assert_eq!(state.cell_selection, Some((0, 0, 0, 1)));
        assert_eq!(state.focused_row, Some(0));
        assert_eq!(state.focused_col, 1);
    }

    /// R953 §5.38 (R8 pin) — the bounds wire is symmetric: the coordinator's
    /// `cell_selection` Text encode round-trips through this grid's
    /// [`parse_cell_bounds`] decode back to the structured rectangle the
    /// coordinator computed. Exercises both halves across the RPC boundary so
    /// the encode (substrate `format!`) and decode (this grid) cannot drift.
    #[test]
    fn wire_round_trip_pins_encode_decode_symmetry() {
        let mut scene = scene_fixture();
        // Build a non-degenerate rectangle: anchor (1,0) extent (3,2).
        key(&mut scene, "ArrowDown", false);
        key(&mut scene, "ArrowDown", false); // cursor (1,0), anchor (1,0)
        key(&mut scene, "ArrowDown", true);
        key(&mut scene, "ArrowDown", true); // extent (3,0)
        key(&mut scene, "ArrowRight", true);
        key(&mut scene, "ArrowRight", true); // extent (3,2)
        let Scene::External(node) = &scene else {
            panic!("external");
        };
        let intro = node.handle.introspect().expect("introspect");
        // Decode of the wire encode == the structured truth.
        assert_eq!(parse_cell_bounds(intro), Some((1, 0, 3, 2)));
        // And the raw wire text is exactly the documented "r0,c0,r1,c1" form.
        assert_eq!(
            intro.query("cell_selection"),
            Some(IntrospectValue::Text("1,0,3,2".to_string())),
        );
    }
}
