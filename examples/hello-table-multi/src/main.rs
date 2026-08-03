//! `hello-table-multi` — R735 §5.38 **multi-select** data table: the GUI
//! consumer of [`TableExternal::with_multiselect`](pinion_core::widgets::table::TableExternal::with_multiselect)
//! + the §5.50 [`pinion_widget_paint::table`] data-grid paint.
//!
//! The WAI-ARIA `aria-multiselectable="true"` data grid — the 2-D grid
//! analog of `hello-listbox-multi`, and the **second consumer** of the
//! R51.98 `aria-multiselectable` attribute (after the list box). Where
//! `hello-table` (R707) enforces single-row exclusion (one `Radio` leaf
//! per row, sibling-deselect), this binding constructs the same `Table`
//! coordinator in multi-select mode: activating a row **toggles only that
//! row's** selection bit, so any subset of rows can report
//! `aria-selected="true"` at once (the use case behind a DCC asset
//! browser's multi-pick, an IDE's batch-select file list, a layer panel's
//! multi-layer selection).
//!
//! ## Composition — one composite External
//!
//! The state scene holds **one** [`TableExternal`] at the [`PRIMARY_TAG`]
//! composite paint root (identical wiring to `hello-table`). Each cell is
//! tagged `"table#<row>_<col>"`; the
//! `InputRouter` R51.42 `'#'`-split
//! routes a click on cell `(r, c)` to
//! `invoke("send", Text("<r>_<c>:<EventName>"))` against the coordinator,
//! whose activate edge **toggles** row `r` (multi-mode) and emits a
//! §5.20 `"selected"` intent carrying the flipped row index.
//!
//! ## Keyboard model (WAI-ARIA APG data grid)
//!
//! The grid is a **single Tab stop** (only the grid root is marked
//! `.with_focusable(true)` — the scene-derived §5.39 Tab stop); the
//! focused cell is an internal 2-D roving *active
//! descendant* (the coordinator's `focused_row` / `focused_col` slots).
//! While the grid root owns shell focus:
//! - `ArrowLeft` / `ArrowRight` move the active descendant column `∓1` /
//!   `±1`, clamped; `ArrowUp` / `ArrowDown` move the row, clamped; the
//!   first arrow into the grid lands on cell `(0, 0)`;
//! - `Home` / `End` jump to the first / last column of the current row;
//! - `PageUp` / `PageDown` jump to the first / last row (same column);
//! - `Enter` / `Space` **toggle** the active-descendant cell's row in the
//!   selection set (vs `hello-table`'s exclusive *select*).
//!
//! There is no sort axis here — sortable columns are demonstrated by
//! `hello-table` (R730) and are orthogonal to selection cardinality;
//! folding both into one binding would only duplicate that machinery
//! (the data is rendered in data order, `row_ids` identity).
//!
//! **Deferred axes** (our-code, honestly deferred, shared with R707):
//! row virtualization + scrolling for large datasets; column resize /
//! per-column widths; editable / dynamic-data (insert / delete /
//! Model-View binding); range / shift-click selection (this slice is
//! single-row toggle, the `aria-multiselectable` substrate; range
//! selection is an app-logic refinement over it).

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, GridCell, GridColumn, GridRow, WidgetA11y,
    grid_table_nodes,
};
// R816 §5.40 — `AriaRole` is now only referenced by the test asserts (the
// lifted `grid_table_nodes` builder owns role + state tagging in prod).
#[cfg(test)]
use pinion_a11y::AriaRole;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::table::{
    TableExternal, read_cols, read_focused_col, read_focused_row, read_rows,
};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{TableData, TableSelection, TableStyle, view_table};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTableMultiRenderer, HelloTableMultiRendererError);

const WIN_W: u32 = 540;
const WIN_H: u32 = 360;
/// [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key — the `"app"` convention shared across
/// the example gallery.
const THEME_TAG: &str = "app";

/// Composite paint root tag carrying the single [`TableExternal`].
/// Cells are tagged `"table#<row>_<col>"`; the R51.42 `'#'`-split routes
/// a click on cell `(r, c)` to the coordinator's `"<r>_<c>:<EventName>"`
/// send (a toggle in multi-select mode).
const PRIMARY_TAG: &str = "table";

/// Column headers for the fixed dataset (the pinion widget catalog
/// itself — a self-referential table).
const HEADERS: [&str; 4] = ["Widget", "Round", "Status", "Role"];

/// Immutable demo dataset: one row per recently-landed catalog widget.
/// `const` (single source of truth) so [`TableMultiState`] stays `Copy`
/// without a heap `Vec`; the coordinator is constructed from the same
/// const and the view fn reads it directly. Dynamic / editable data is a
/// deferred axis (see the module docs).
const ROWS: [[&str; 4]; 6] = [
    ["Tabs", "R690", "Done", "tablist"],
    ["Menu", "R691", "Done", "menu"],
    ["Toolbar", "R692", "Done", "toolbar"],
    ["Dialog", "R693", "Done", "dialog"],
    ["Tooltip", "R695", "Done", "tooltip"],
    ["Table", "R707", "Active", "grid"],
];

const NROWS: usize = ROWS.len();
const NCOLS: usize = HEADERS.len();

/// R735 — rows pre-selected in [`WidgetCore::create_external`] so the
/// boot frame shows the defining feature (multiple `aria-selected` rows)
/// as a live pixel, the way `hello-segmented-button` seeds its default
/// segment. A multi-select grid restored from saved preferences with two
/// rows already picked is a legitimate default state (not a contrived
/// setup) — these are data rows 0 ("Tabs") and 2 ("Toolbar").
const BOOT_SELECTED: [usize; 2] = [0, 2];

/// Cached projection of the table: the per-row selection bitmap, the 2-D
/// roving active descendant `(focused_row, focused_col)`, and one
/// [`RadioState`] per row. Read from the single [`TableExternal`]'s
/// introspect slots. `Copy` (fixed-size arrays) so the shell hands the
/// snapshot into the `paint_producer` closure without lifetime gymnastics.
///
/// The per-row arrays are sized exactly to [`NROWS`]: the dataset is
/// immutable (data lives in the [`ROWS`] const), so the row count is a
/// compile-time constant with no headroom or silent-truncation slack.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TableMultiState {
    /// R735 §5.38 — per-row selection bitmap, indexed by data row. Unlike
    /// `hello-table`'s single `Option<usize>`, any subset may be `true`.
    row_selected: [bool; NROWS],
    /// R707 §5.40 — the roving active-descendant row (0-based), or `None`
    /// before any navigation.
    focused_row: Option<usize>,
    /// R707 §5.40 — the roving active-descendant column (0-based).
    focused_col: usize,
    /// Per-row interaction state, indexed by data row (exactly [`NROWS`]).
    row_states: [RadioState; NROWS],
}

impl TableMultiState {
    fn idle() -> Self {
        Self {
            row_selected: [false; NROWS],
            focused_row: None,
            focused_col: 0,
            row_states: [RadioState::Idle; NROWS],
        }
    }

    fn row_state(&self, row: usize) -> RadioState {
        self.row_states
            .get(row)
            .copied()
            .unwrap_or(RadioState::Idle)
    }
}

/// The cell reported as the grid's `aria-activedescendant` when the grid
/// owns focus: the roving `(focused_row, focused_col)` if set, else
/// `(0, 0)`. Clamped into the dataset so a stale value never points past
/// the last row / column.
fn active_cell(state: &TableMultiState) -> (usize, usize) {
    let row = state.focused_row.unwrap_or(0).min(NROWS - 1);
    let col = state.focused_col.min(NCOLS - 1);
    (row, col)
}

/// Establish the active-descendant row if none is set yet (a horizontal
/// arrow / Home / End entering the grid lands the cursor on row 0).
fn ensure_row(intro: &mut dyn pinion_core::external::ExternalIntrospect) {
    if read_focused_row(intro).is_none() {
        let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
    }
}

/// Move the active-descendant row by `delta` (data order — no sort axis
/// here), clamped within the dataset. From `None`, the first vertical
/// arrow lands on row 0.
fn move_row(intro: &mut dyn pinion_core::external::ExternalIntrospect, delta: i64) {
    let rows = read_rows(intro);
    if rows == 0 {
        return;
    }
    let max = i64::try_from(rows - 1).unwrap_or(0);
    let next = match read_focused_row(intro) {
        None => 0,
        Some(r) => (i64::try_from(r).unwrap_or(0) + delta).clamp(0, max),
    };
    let _ = intro.intervene("focused_row", IntrospectValue::Int(next));
}

/// Move the active-descendant column by `delta`, clamped within the
/// dataset, establishing the row first (grid entry via horizontal arrow).
fn move_col(intro: &mut dyn pinion_core::external::ExternalIntrospect, delta: i64) {
    ensure_row(intro);
    let cols = read_cols(intro);
    if cols == 0 {
        return;
    }
    let max = i64::try_from(cols - 1).unwrap_or(0);
    let next = (i64::try_from(read_focused_col(intro)).unwrap_or(0) + delta).clamp(0, max);
    let _ = intro.intervene("focused_col", IntrospectValue::Int(next));
}

/// Set the active-descendant column to a specific value (Home / End),
/// clamped within the dataset, establishing the row first.
fn set_col(intro: &mut dyn pinion_core::external::ExternalIntrospect, col: usize) {
    ensure_row(intro);
    let cols = read_cols(intro);
    if cols == 0 {
        return;
    }
    let clamped = col.min(cols - 1);
    let _ = intro.intervene(
        "focused_col",
        IntrospectValue::Int(i64::try_from(clamped).unwrap_or(0)),
    );
}

/// Set the active-descendant row to a specific value (`PageUp` /
/// `PageDown`), clamped within the dataset.
fn set_row(intro: &mut dyn pinion_core::external::ExternalIntrospect, row: usize) {
    let rows = read_rows(intro);
    if rows == 0 {
        return;
    }
    let clamped = row.min(rows - 1);
    let _ = intro.intervene(
        "focused_row",
        IntrospectValue::Int(i64::try_from(clamped).unwrap_or(0)),
    );
}

/// view-fn (§6.3): pure sync mapping [`TableMultiState`] -> [`Scene`].
/// Wraps [`view_table`] in a centred surface container. The data renders
/// in data order (no sort), so `row_ids` is identity (`&[]` fallback).
/// The keyboard-focus ring is the shell's job (R694), so the view fn
/// paints no focus state itself.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &TableMultiState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // R1020 §5.39 — the grid is a single Tab stop; opt the table into the
    // scene-derived focus enumeration so its PRIMARY_TAG is collected.
    let style = TableStyle::m3();
    let rows: Vec<&[&str]> = ROWS.iter().map(|r| &r[..]).collect();
    let table = view_table(
        PRIMARY_TAG,
        TableData {
            headers: &HEADERS,
            rows: &rows,
            row_ids: &[],
            decoration: None,
            header_decoration: None,
        },
        // R952 — row-multi-select grid: per-row bitmap, no cell range selection.
        TableSelection {
            rows: &state.row_selected,
            cells: None,
        },
        &state.row_states,
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

struct TableMultiView;

impl WidgetCore for TableMultiView {
    type State = TableMultiState;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's composite `"<r>_<c>:<EventName>"` dispatch (pointer),
    // never the shell's enum-typed `keybinding` channel — so `()`
    // satisfies the trait's `Copy` bound (mirror of `hello-table`).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let headers = HEADERS.iter().map(|s| (*s).to_string()).collect();
        let rows = ROWS
            .iter()
            .map(|r| r.iter().map(|c| (*c).to_string()).collect())
            .collect();
        let mut ext = TableExternal::with_multiselect(headers, rows);
        // R735 — seed the multi-select boot state so the boot frame's live
        // pixel shows two accent-washed rows (the defining feature). This
        // is a persisted-restore / boot-default, i.e. the **admin**
        // slot-assignment path (`set_selected_rows`), not the interaction
        // path: it must not move the active descendant, so the grid still
        // boots with no roving cursor (focused_row == None), the same
        // clean keyboard-entry state hello-table boots with.
        ext.set_selected_rows(&BOOT_SELECTED);
        Box::new(ext)
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> TableMultiState {
        let mut out = TableMultiState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // The introspect channel is the single source of truth: an AI
        // client running `scene/query /table/external/selected.0` sees
        // exactly the bit the view fn renders.
        out.focused_row = read_focused_row(intro);
        out.focused_col = read_focused_col(intro);
        let rows = read_rows(intro);
        for r in 0..rows {
            if let Some(slot) = out.row_selected.get_mut(r) {
                *slot = matches!(
                    intro.query(&format!("selected.{r}")),
                    Some(IntrospectValue::Bool(true))
                );
            }
            let st = match intro.query(&format!("state.{r}")) {
                Some(IntrospectValue::Text(name)) => RadioState::from_name_or_default(&name),
                _ => RadioState::Idle,
            };
            if let Some(slot) = out.row_states.get_mut(r) {
                *slot = st;
            }
        }
        out
    }

    fn view(state: TableMultiState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-table-multi (R735 §5.38 multi-select data grid)"
    }

    /// WAI-ARIA APG data-grid keyboard model. All keys route only when
    /// the grid owns shell focus: arrows move the 2-D active descendant
    /// (clamped), `Home` / `End` jump to the first / last column of the
    /// current row, `PageUp` / `PageDown` jump to the first / last row,
    /// and `Enter` / `Space` **toggle** the active-descendant row's
    /// selection bit (vs `hello-table`'s exclusive select).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
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
        match key {
            "ArrowLeft" => {
                move_col(intro, -1);
                true
            }
            "ArrowRight" => {
                move_col(intro, 1);
                true
            }
            "ArrowUp" => {
                move_row(intro, -1);
                true
            }
            "ArrowDown" => {
                move_row(intro, 1);
                true
            }
            "Home" => {
                set_col(intro, 0);
                true
            }
            "End" => {
                let last = read_cols(intro).saturating_sub(1);
                set_col(intro, last);
                true
            }
            "PageUp" => {
                set_row(intro, 0);
                true
            }
            "PageDown" => {
                let last = read_rows(intro).saturating_sub(1);
                set_row(intro, last);
                true
            }
            "Enter" | "Space" => {
                // Toggle the active-descendant cell's row. Run the full
                // pointer cycle through the coordinator's wire format so
                // the multi-select toggle + §5.20 intent fire exactly as a
                // click would. With no active descendant yet, Enter toggles
                // cell (0, 0).
                let row = read_focused_row(intro).unwrap_or(0);
                let col = read_focused_col(intro);
                let mut handled = false;
                for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                    handled |= intro
                        .invoke("send", IntrospectValue::Text(format!("{row}_{col}:{ev}")))
                        .is_ok();
                }
                handled
            }
            _ => false,
        }
    }

    fn fmt_state_log(state: &TableMultiState) -> String {
        let sel: Vec<usize> = (0..NROWS).filter(|&r| state.row_selected[r]).collect();
        let (fr, fc) = active_cell(state);
        format!("selected={sel:?} active=({fr},{fc})")
    }
}

impl WidgetA11y for TableMultiView {
    /// R735 §5.40 — composite AccessKit tree contribution. Emits the
    /// `grid` root carrying `aria-multiselectable="true"` (the 2nd
    /// consumer of R51.98 `aria-multiselectable` after `hello-listbox-multi`),
    /// a header `row` of `columnheader` nodes, one data `row` per dataset
    /// row (carrying `aria-selected` for its bit + position/size in set),
    /// and one `gridcell` per cell. **Any subset** of data rows may report
    /// `aria-selected="true"` — the multi-select invariant.
    fn access_node(state: &TableMultiState, focused: Option<&str>) -> Vec<AccessNode> {
        // R816 §5.40 — lifted `grid_table_nodes` builder. No sort axis; rows
        // in data order (= visual order). The grid is `multiselectable` and
        // each row reports its own `aria-selected` bit (multiple may be
        // true). The builder derives `aria-posinset` / `aria-setsize` from
        // the slice.
        let grid_focused = focused == Some(PRIMARY_TAG);
        let (active_row, active_col) = active_cell(state);
        let columns: Vec<GridColumn> = (0..HEADERS.len())
            .map(|col| GridColumn {
                tag: format!("{PRIMARY_TAG}_ch{col}"),
                sort: None,
            })
            .collect();
        let rows: Vec<GridRow> = ROWS
            .iter()
            .enumerate()
            .map(|(data, row_data)| GridRow {
                tag: format!("{PRIMARY_TAG}_row{data}"),
                selected: state.row_selected[data],
                state: state.row_state(data),
                cells: HEADERS
                    .iter()
                    .enumerate()
                    .map(|(col, header)| GridCell {
                        tag: format!("{PRIMARY_TAG}#{data}_{col}"),
                        name: format!("{header}: {}", row_data[col]),
                        focused: grid_focused && active_row == data && active_col == col,
                        selected: None, // R952 — row-multi-select grid: no cell selection
                    })
                    .collect(),
            })
            .collect();
        grid_table_nodes(
            PRIMARY_TAG,
            "pinion widget catalog (multi-select)",
            true,
            &format!("{PRIMARY_TAG}_hrow"),
            &columns,
            &rows,
        )
    }

    /// R735 §5.40 — composite focus model (mirror of `hello-table`). When
    /// the grid owns shell focus, focus stays on the grid root and the
    /// active-descendant cell is reported as `aria-activedescendant`.
    fn access_focus_target(state: &TableMultiState, focused: Option<&str>) -> Option<AccessFocus> {
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

    /// R735 §5.40 — composite child action dispatch. A cell carries a
    /// composite tag (`"table#<r>_<c>"`); an AT `Click` / `Default` on a
    /// cell splits at `'#'` and arrives here with the `"<r>_<c>"` sub-tag,
    /// toggling that row through the coordinator's wire format (the same
    /// path a real click takes, so the multi-select toggle holds).
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
                for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                    let _ =
                        intro.invoke("send", IntrospectValue::Text(format!("{row}_{col}:{ev}")));
                }
                true
            }
            AccessAction::Focus => true,
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for TableMultiView {
    type Renderer = HelloTableMultiRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TableMultiView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    /// Build a fresh multi-select table scene mirroring `create_external`
    /// (one composite `Scene::External` at `PRIMARY_TAG`, two rows
    /// boot-seeded selected).
    fn scene_fixture() -> Scene {
        Scene::External(ExternalNode::new(TableMultiView::create_external()).with_tag(PRIMARY_TAG))
    }

    fn selected_bits(scene: &Scene) -> Vec<usize> {
        let Scene::External(node) = scene else {
            return Vec::new();
        };
        let intro = node.handle.introspect().expect("introspect");
        (0..NROWS)
            .filter(|&r| {
                matches!(
                    intro.query(&format!("selected.{r}")),
                    Some(IntrospectValue::Bool(true))
                )
            })
            .collect()
    }

    fn focused(scene: &Scene) -> (i64, i64) {
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
    fn boot_seeds_two_selected_rows() {
        let scene = scene_fixture();
        assert_eq!(
            selected_bits(&scene),
            BOOT_SELECTED.to_vec(),
            "boot frame shows the two seeded selections (live-pixel target)",
        );
        // Multi-mode: `selected_row` is the `-1` sentinel (no single row).
        let Scene::External(node) = &scene else {
            panic!("external")
        };
        assert_eq!(
            node.handle.introspect().unwrap().query("selected_row"),
            Some(IntrospectValue::Int(-1)),
            "multi-mode selected_row is the -1 sentinel",
        );
        assert_eq!(
            node.handle.introspect().unwrap().query("multiselect"),
            Some(IntrospectValue::Bool(true)),
        );
    }

    #[test]
    fn enter_toggles_without_deselecting_siblings() {
        let mut scene = scene_fixture();
        // Navigate to row 4 (unselected) and toggle it on: rows 0, 2 stay.
        for _ in 0..5 {
            let _ = TableMultiView::apply_key(
                &mut scene,
                Some(PRIMARY_TAG),
                "ArrowDown",
                pinion_core::Modifiers::default(),
            );
        }
        assert_eq!(focused(&scene).0, 4);
        assert!(TableMultiView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "Enter",
            pinion_core::Modifiers::default()
        ));
        assert_eq!(
            selected_bits(&scene),
            vec![0, 2, 4],
            "Enter adds row 4, siblings 0/2 untouched (multi-select)",
        );
        // Toggle row 4 back off.
        assert!(TableMultiView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "Space",
            pinion_core::Modifiers::default()
        ));
        assert_eq!(
            selected_bits(&scene),
            vec![0, 2],
            "Space toggles row 4 off again"
        );
    }

    #[test]
    fn click_toggles_a_row_off() {
        let mut scene = scene_fixture();
        // AT click on a cell of the already-selected row 0 toggles it off.
        assert!(TableMultiView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "0_1",
            AccessAction::Click,
        ));
        assert_eq!(
            selected_bits(&scene),
            vec![2],
            "clicking selected row 0 toggles it off"
        );
    }

    #[test]
    fn arrows_move_2d_active_descendant_clamped() {
        let mut scene = scene_fixture();
        assert!(TableMultiView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowDown",
            pinion_core::Modifiers::default()
        ));
        assert_eq!(focused(&scene), (0, 0), "first ArrowDown enters at row 0");
        let _ = TableMultiView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowRight",
            pinion_core::Modifiers::default(),
        );
        assert_eq!(focused(&scene), (0, 1), "ArrowRight -> col 1");
        let _ = TableMultiView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "PageDown",
            pinion_core::Modifiers::default(),
        );
        assert_eq!(
            focused(&scene).0,
            i64::try_from(NROWS - 1).unwrap(),
            "PageDown -> last row"
        );
        let _ = TableMultiView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowDown",
            pinion_core::Modifiers::default(),
        );
        assert_eq!(
            focused(&scene).0,
            i64::try_from(NROWS - 1).unwrap(),
            "ArrowDown clamps at last row"
        );
        let _ = TableMultiView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "End",
            pinion_core::Modifiers::default(),
        );
        assert_eq!(
            focused(&scene).1,
            i64::try_from(NCOLS - 1).unwrap(),
            "End -> last col"
        );
    }

    #[test]
    fn keys_ignored_when_grid_unfocused() {
        let mut scene = scene_fixture();
        assert!(!TableMultiView::apply_key(
            &mut scene,
            None,
            "ArrowDown",
            pinion_core::Modifiers::default()
        ));
    }

    #[test]
    fn access_node_grid_is_multiselectable() {
        let nodes = TableMultiView::access_node(&TableMultiState::idle(), None);
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert!(
            nodes[0].multiselectable,
            "grid root carries aria-multiselectable"
        );
    }

    #[test]
    fn access_node_reports_multiple_aria_selected() {
        let mut state = TableMultiState::idle();
        state.row_selected[1] = true;
        state.row_selected[3] = true;
        let nodes = TableMultiView::access_node(&state, None);
        for (data, want) in [(0, false), (1, true), (3, true), (5, false)] {
            let row = nodes
                .iter()
                .find(|n| n.tag == format!("{PRIMARY_TAG}_row{data}"))
                .expect("data row node");
            assert_eq!(
                row.selected,
                Some(want),
                "row {data} aria-selected reflects its bit (multiple may be true)",
            );
        }
    }

    #[test]
    fn read_state_round_trips_selection_bitmap() {
        let scene = scene_fixture();
        let state = TableMultiView::read_state(&scene);
        assert!(state.row_selected[0] && state.row_selected[2]);
        assert!(!state.row_selected[1] && !state.row_selected[3]);
    }
}
