//! `hello-table` — R707 §5.38 interactive **data table**: the GUI
//! consumer of the R707
//! [`Table`](pinion_core::widgets::table::Table) single-row-selection
//! coordinator + the §5.50 [`pinion_widget_paint::table`] data-grid
//! paint.
//!
//! A data table presents tabular data as a grid (the WAI-ARIA `grid`
//! role with `row` / `columnheader` / `gridcell` children — the
//! **second consumer** of the R704 grid-role family, and the first to
//! exercise the [`AriaRole::Row`] role, which the date picker's flat
//! calendar grid does not). Exactly one row may be selected; activating
//! any cell in a row selects that row (single-row exclusion —
//! framework-owned in the coordinator, like a `RadioGroup`'s
//! sibling-deselect).
//!
//! ## Composition — one composite External
//!
//! The state scene holds **one** [`TableExternal`] at the [`PRIMARY_TAG`]
//! composite paint root (mirroring `hello-datepicker` over
//! [`DatePickerExternal`](pinion_core::widgets::datepicker::DatePickerExternal)).
//! Each cell is tagged `"table#<row>_<col>"` (the R51.41 composite paint
//! convention), so the
//! [`InputRouter`](pinion_runtime::input::InputRouter) R51.42 `'#'`-split
//! routes a click on cell `(r, c)` to
//! `invoke("send", Text("<r>_<c>:<EventName>"))` against the single
//! coordinator, which enforces single-row exclusion on the activate edge
//! and emits the §5.20 `"selected"` intent (the new row index).
//!
//! ## Keyboard model (WAI-ARIA APG data grid)
//!
//! The grid is a **single Tab stop** (only the grid root is marked
//! `.with_focusable(true)` — the scene-derived §5.39 Tab stop), and the
//! focused cell is an internal 2-D roving *active
//! descendant* — the coordinator's `focused_row` / `focused_col` slots —
//! not a shell-focus tag. This is the WAI-ARIA grid pattern (single tab
//! stop + `aria-activedescendant`), the same model `hello-datepicker`
//! uses, generalized to two axes. While the grid root owns shell focus:
//! - `ArrowLeft` / `ArrowRight` move the active descendant column `∓1` /
//!   `±1`, clamped; `ArrowUp` / `ArrowDown` move the row `∓1` / `±1`,
//!   clamped; the first arrow into the grid lands on cell `(0, 0)`;
//! - `Home` / `End` jump to the first / last column of the current row;
//! - `PageUp` / `PageDown` jump to the first / last row (same column);
//! - `Enter` / `Space` activate (select) the active-descendant cell's
//!   row.
//!
//! The active descendant moves through the coordinator's `focused_row` /
//! `focused_col` intervene slots (the data-grid mirror of the date
//! picker's `focused_day`) so the visible focus ring, the AT
//! `aria-activedescendant`, and the RPC `focused_row` / `focused_col`
//! queries stay in lockstep. A mouse click on a cell routes through the
//! input router's `'#'`-split to the coordinator's `"<r>_<c>:<EventName>"`
//! send, whose activate edge syncs the active descendant to the clicked
//! cell.
//!
//! **Deferred axes** (our-code, honestly deferred): row virtualization +
//! scrolling for large datasets (this slice renders a fixed in-view
//! dataset); multi-row selection; sortable columns (`aria-sort`); column
//! resize / per-column widths; editable / dynamic-data (insert / delete /
//! Model-View binding to a signal). The grid-role family is the
//! foundation for those follow-ups; this first slice validates it as the
//! second consumer.

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, GridCell, GridColumn, GridRow, SortDirection,
    WidgetA11y, grid_table_nodes,
};
// R816 §5.40 — `AriaRole` is now only referenced by the test asserts (the
// lifted `grid_table_nodes` builder owns role + state tagging in prod).
#[cfg(test)]
use pinion_a11y::AriaRole;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::grid_sort::col_sort_dir;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::table::{
    TableExternal, read_cols, read_focused_col, read_focused_row, read_rows,
};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{TableData, TableSelection, TableStyle, view_table};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTableRenderer, HelloTableRendererError);

const WIN_W: u32 = 540;
const WIN_H: u32 = 360;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across
/// the example gallery.
const THEME_TAG: &str = "app";

/// Composite paint root tag carrying the single [`TableExternal`].
/// Cells are tagged `"table#<row>_<col>"`; the R51.42 `'#'`-split routes
/// a click on cell `(r, c)` to the coordinator's `"<r>_<c>:<EventName>"`
/// send.
const PRIMARY_TAG: &str = "table";

/// Column headers for the fixed dataset (the pinion widget catalog
/// itself — a self-referential table).
const HEADERS: [&str; 4] = ["Widget", "Round", "Status", "Role"];

/// Immutable demo dataset: one row per recently-landed catalog widget.
/// The data is a `const` (the single source of truth) so [`TableState`]
/// stays `Copy` without a heap `Vec`; the coordinator is constructed from
/// the same const, and the view fn reads it directly. Dynamic /
/// editable data is a deferred axis (see the module docs).
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

/// Cached projection of the table: the selected row, the 2-D roving
/// active descendant `(focused_row, focused_col)`, and one [`RadioState`]
/// per row. Read from the single [`TableExternal`]'s introspect slots.
/// `Copy` (fixed-size array) so the shell hands the snapshot into the
/// `paint_producer` closure without lifetime gymnastics.
///
/// The per-row array is sized exactly to [`NROWS`]: the dataset is
/// immutable (data lives in the [`ROWS`] const), so unlike the date
/// picker's variable-length month (which needs a `MAX_DAYS` upper
/// bound), the row count is a compile-time constant with no headroom or
/// silent-truncation slack. Dynamic / editable rows are a deferred axis
/// (they would move the data to a signal-backed model anyway).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TableState {
    /// The selected row (0-based), or `None`.
    selected_row: Option<usize>,
    /// R707 §5.40 — the roving active-descendant row (0-based), or
    /// `None` before any navigation.
    focused_row: Option<usize>,
    /// R707 §5.40 — the roving active-descendant column (0-based).
    focused_col: usize,
    /// Per-row interaction state, indexed by **data** row (exactly
    /// [`NROWS`]). Selection / interaction stay data-indexed so a sort
    /// needs no remap (R730).
    row_states: [RadioState; NROWS],
    /// R730 §5.40 — the active sort key `(col, ascending)`, or `None`.
    sort: Option<(usize, bool)>,
    /// R730 §5.40 — the visual→data row permutation
    /// ([`TableExternal::order`]): `order[visual]` is the data row painted
    /// at visual position `visual`. Identity when unsorted.
    order: [usize; NROWS],
}

impl TableState {
    fn idle() -> Self {
        Self {
            selected_row: None,
            focused_row: None,
            focused_col: 0,
            row_states: [RadioState::Idle; NROWS],
            sort: None,
            order: core::array::from_fn(|i| i),
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
/// `(selected_row, 0)`, else `(0, 0)` — mirrors `hello-datepicker`'s
/// `active_day`. Clamped into the dataset so a stale value never points
/// past the last row / column.
fn active_cell(state: &TableState) -> (usize, usize) {
    let row = state
        .focused_row
        .or(state.selected_row)
        .unwrap_or(0)
        .min(NROWS - 1);
    let col = state.focused_col.min(NCOLS - 1);
    (row, col)
}

/// R730 §5.40 — read the coordinator's visual→data permutation
/// ([`TableExternal::order`]) via the `order.<visual>` introspect slots.
/// `read_order(...)[visual]` is the data row painted at `visual`; identity
/// when unsorted.
fn read_order(intro: &dyn pinion_core::external::ExternalIntrospect, rows: usize) -> Vec<usize> {
    (0..rows)
        .map(|v| match intro.query(&format!("order.{v}")) {
            Some(IntrospectValue::Int(d)) if d >= 0 => usize::try_from(d).unwrap_or(v),
            _ => v,
        })
        .collect()
}

/// Set the data-row active descendant to the data row at visual position
/// `visual` (clamped). The R730 visual↔data bridge: keyboard navigation
/// reasons in **visual** (displayed) rows, but `focused_row` is stored as
/// the **data** row so a sort needs no remap.
fn set_visual_row(intro: &mut dyn pinion_core::external::ExternalIntrospect, visual: usize) {
    let rows = read_rows(intro);
    if rows == 0 {
        return;
    }
    let order = read_order(intro, rows);
    let data = order.get(visual.min(rows - 1)).copied().unwrap_or(0);
    let _ = intro.intervene(
        "focused_row",
        IntrospectValue::Int(i64::try_from(data).unwrap_or(0)),
    );
}

/// Establish the active-descendant row if none is set yet (a horizontal
/// arrow / Home / End entering the grid lands the cursor on the first
/// **visual** row).
fn ensure_row(intro: &mut dyn pinion_core::external::ExternalIntrospect) {
    if read_focused_row(intro).is_none() {
        set_visual_row(intro, 0);
    }
}

/// Move the active-descendant row by `delta` in **visual** order, clamped
/// within the dataset. From `None`, the first vertical arrow lands on the
/// first visual row. The current data row is located in the sort
/// permutation, stepped visually, then mapped back to a data row.
fn move_row(intro: &mut dyn pinion_core::external::ExternalIntrospect, delta: i64) {
    let rows = read_rows(intro);
    if rows == 0 {
        return;
    }
    let order = read_order(intro, rows);
    let max = i64::try_from(rows - 1).unwrap_or(0);
    let next_visual = match read_focused_row(intro).and_then(|d| order.iter().position(|&x| x == d))
    {
        None => 0,
        Some(v) => (i64::try_from(v).unwrap_or(0) + delta).clamp(0, max),
    };
    let next_visual = usize::try_from(next_visual).unwrap_or(0);
    let next_data = order.get(next_visual).copied().unwrap_or(0);
    let _ = intro.intervene(
        "focused_row",
        IntrospectValue::Int(i64::try_from(next_data).unwrap_or(0)),
    );
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

/// view-fn (§6.3): pure sync mapping [`TableState`] -> [`Scene`]. Wraps
/// [`view_table`] in a centred surface container, mirroring
/// `hello-datepicker`'s outer container + theme. The keyboard-focus ring
/// is the shell's job (R694 `paint_focus_ring` over the active-descendant
/// cell, resolved through `access_focus_target`), so the view fn paints
/// no focus state itself.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &TableState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // R1020 §5.39 — the grid is a single Tab stop; opt the table into the
    // scene-derived focus enumeration so its PRIMARY_TAG is collected.
    let style = TableStyle::m3().with_focusable(true);
    // R730 §5.40 — paint rows in the coordinator's sorted (visual) order;
    // `row_ids` carries each visual row's data index so cells tag by data
    // id and the data-indexed selection / state stay correct.
    let rows: Vec<&[&str]> = state.order.iter().map(|&d| &ROWS[d][..]).collect();
    // R735 §5.38 — `view_table` takes a data-indexed selection bitmap
    // (uniform with `row_states`); single-select projects its one
    // `selected_row` into the bitmap.
    let row_selected: [bool; NROWS] = core::array::from_fn(|i| state.selected_row == Some(i));
    let table = view_table(
        PRIMARY_TAG,
        TableData {
            headers: &HEADERS,
            rows: &rows,
            row_ids: &state.order,
        },
        // Single-row selection only; the spreadsheet cell range selection is
        // the dedicated `hello-cell-select` grid's model (R953 — one selection
        // model per example, Excel / Qt `SelectRows` vs `SelectItems`).
        TableSelection {
            rows: &row_selected,
            cells: None,
        },
        &state.row_states,
        state.sort,
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

struct TableView;

impl WidgetCore for TableView {
    type State = TableState;
    // Every state change flows through `apply_key` (keyboard) or the
    // InputRouter's composite `"<r>_<c>:<EventName>"` dispatch (pointer),
    // never the shell's enum-typed `keybinding` channel — so `()`
    // satisfies the trait's `Copy` bound without an unused event variant
    // (mirror of `hello-datepicker`).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let headers = HEADERS.iter().map(|s| (*s).to_string()).collect();
        let rows = ROWS
            .iter()
            .map(|r| r.iter().map(|c| (*c).to_string()).collect())
            .collect();
        Box::new(TableExternal::new(headers, rows))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> TableState {
        let mut out = TableState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // The introspect channel is the single source of truth: an AI
        // client running `scene/query /table/external/selected_row` sees
        // exactly the value the view fn renders.
        out.selected_row = match intro.query("selected_row") {
            Some(IntrospectValue::Int(r)) if r >= 0 => usize::try_from(r).ok(),
            _ => None,
        };
        out.focused_row = read_focused_row(intro);
        out.focused_col = read_focused_col(intro);
        let rows = read_rows(intro);
        for r in 0..rows {
            let st = match intro.query(&format!("state.{r}")) {
                Some(IntrospectValue::Text(name)) => RadioState::from_name_or_default(&name),
                _ => RadioState::Idle,
            };
            if let Some(slot) = out.row_states.get_mut(r) {
                *slot = st;
            }
        }
        // R730 §5.40 — sort key + visual→data permutation, read from the
        // coordinator (the single source of truth for both paint + a11y).
        let sort_col = match intro.query("sort_col") {
            Some(IntrospectValue::Int(c)) if c >= 0 => usize::try_from(c).ok(),
            _ => None,
        };
        if let Some(col) = sort_col {
            let ascending = matches!(intro.query("sort_dir"), Some(IntrospectValue::Text(d)) if d == "ascending");
            out.sort = Some((col, ascending));
        }
        out.order = core::array::from_fn(|v| match intro.query(&format!("order.{v}")) {
            Some(IntrospectValue::Int(d)) if d >= 0 => usize::try_from(d).unwrap_or(v),
            _ => v,
        });
        out
    }

    fn view(state: TableState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // No typed events reach the keybinding channel; the single source
        // of truth for the keymap lives in `apply_key`.
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-table (R707 §5.38 interactive data grid)"
    }

    /// WAI-ARIA APG data-grid keyboard model. All keys route only when
    /// the grid owns shell focus: arrows move the 2-D active descendant
    /// (clamped), `Home` / `End` jump to the first / last column of the
    /// current row, `PageUp` / `PageDown` jump to the first / last row,
    /// and `Enter` / `Space` activate (select) the active-descendant row.
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
                set_visual_row(intro, 0);
                true
            }
            "PageDown" => {
                let last = read_rows(intro).saturating_sub(1);
                set_visual_row(intro, last);
                true
            }
            "Enter" | "Space" => {
                // Activate the active-descendant cell's row. Run the full
                // pointer cycle through the coordinator's wire format so
                // single-row exclusion + the §5.20 intent fire exactly as
                // a click would. With no active descendant yet, Enter
                // selects cell (0, 0) — the WAI-ARIA "activate the focused
                // cell" default.
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

    fn fmt_state_log(state: &TableState) -> String {
        let sel = state
            .selected_row
            .map_or_else(|| "none".to_string(), |r| r.to_string());
        let (fr, fc) = active_cell(state);
        format!("sel={sel} active=({fr},{fc})")
    }
}

impl WidgetA11y for TableView {
    /// R707 §5.40 — composite AccessKit tree contribution. Emits the
    /// `grid` root, a header `row` of `columnheader` nodes, one data
    /// `row` per dataset row (carrying `aria-selected` + position/size in
    /// set), and one `gridcell` per cell. The single-row-selection
    /// invariant lives in the coordinator; at most one data row reports
    /// `aria-selected="true"`. The active descendant (when the grid owns
    /// focus) is the cell `aria-activedescendant` points to.
    fn access_node(state: &TableState, focused: Option<&str>) -> Vec<AccessNode> {
        // R816 §5.40 — lifted `grid_table_nodes` builder (one of two
        // consumers). Rows are passed in **visual** (sorted) order so the
        // builder's slice-derived `aria-posinset` is the displayed row
        // number; tags stay **data-indexed** so selection survives a sort.
        // The active sort column carries `aria-sort`.
        let grid_focused = focused == Some(PRIMARY_TAG);
        let (active_row, active_col) = active_cell(state);
        let columns: Vec<GridColumn> = HEADERS
            .iter()
            .enumerate()
            .map(|(col, label)| GridColumn {
                tag: format!("{PRIMARY_TAG}_ch{col}"),
                label: (*label).to_owned(),
                sort: col_sort_dir(state.sort, col).map(SortDirection::from_ascending),
            })
            .collect();
        let rows: Vec<GridRow> = state
            .order
            .iter()
            .map(|&data| GridRow {
                tag: format!("{PRIMARY_TAG}_row{data}"),
                selected: state.selected_row == Some(data),
                state: state.row_state(data),
                cells: HEADERS
                    .iter()
                    .enumerate()
                    .map(|(col, header)| GridCell {
                        tag: format!("{PRIMARY_TAG}#{data}_{col}"),
                        name: format!("{header}: {}", ROWS[data][col]),
                        focused: grid_focused && active_row == data && active_col == col,
                        // Row-selection grid: cells carry no `aria-selected`
                        // (the row does). Per-cell range selection is the
                        // dedicated `hello-cell-select` grid's model (R953).
                        selected: None,
                    })
                    .collect(),
            })
            .collect();
        grid_table_nodes(
            PRIMARY_TAG,
            "pinion widget catalog",
            false,
            &format!("{PRIMARY_TAG}_hrow"),
            &columns,
            &rows,
        )
    }

    /// R707 §5.40 — composite focus model (mirror of `hello-datepicker`).
    /// When the grid owns shell focus, focus stays on the grid root
    /// ([`PRIMARY_TAG`]) and the active-descendant cell is reported as the
    /// `aria-activedescendant`. Any other focused tag passes through
    /// atomically.
    fn access_focus_target(state: &TableState, focused: Option<&str>) -> Option<AccessFocus> {
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

    /// R707 §5.40 — composite child action dispatch. A cell carries a
    /// composite tag (`"table#<r>_<c>"`), so an AT `Click` / `Default` on
    /// a cell splits at `'#'` and arrives here with the `"<r>_<c>"`
    /// sub-tag. Activation runs the pointer cycle against the
    /// coordinator's wire format (the same path a real click takes, so
    /// single-row exclusion holds). `Focus` is accepted without mutation;
    /// other actions decline so the shell keeps its fallback chain.
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

impl WidgetView for TableView {
    type Renderer = HelloTableRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TableView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    /// Build a fresh table scene mirroring `create_external` so
    /// `apply_key` / `access_child_invoke` walk the live shell's exact
    /// topology (one composite `Scene::External` at `PRIMARY_TAG`).
    fn scene_fixture() -> Scene {
        Scene::External(ExternalNode::new(TableView::create_external()).with_tag(PRIMARY_TAG))
    }

    fn selected_row(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        match node.handle.introspect()?.query("selected_row") {
            Some(IntrospectValue::Int(r)) if r >= 0 => Some(r),
            _ => None,
        }
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
    fn initial_state_has_no_selection_or_focus() {
        let scene = scene_fixture();
        assert_eq!(selected_row(&scene), None);
        assert_eq!(focused(&scene), (-1, 0));
    }

    #[test]
    fn arrows_move_2d_active_descendant_clamped() {
        let mut scene = scene_fixture();
        // First vertical arrow enters the grid at row 0.
        assert!(TableView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowDown",
            pinion_core::Modifiers::default()
        ));
        assert_eq!(focused(&scene), (0, 0), "first ArrowDown enters at row 0");
        let _ = TableView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowDown",
            pinion_core::Modifiers::default(),
        );
        assert_eq!(focused(&scene), (1, 0), "ArrowDown -> row 1");
        let _ = TableView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowRight",
            pinion_core::Modifiers::default(),
        );
        assert_eq!(focused(&scene), (1, 1), "ArrowRight -> col 1");
        // PageDown jumps to the last row; ArrowDown then clamps.
        let _ = TableView::apply_key(
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
        let _ = TableView::apply_key(
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
        // End jumps to the last column.
        let _ = TableView::apply_key(
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
    fn enter_activates_active_descendant_row() {
        let mut scene = scene_fixture();
        // Three ArrowDowns: enter at row 0, then advance to row 2.
        let _ = TableView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowDown",
            pinion_core::Modifiers::default(),
        );
        let _ = TableView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowDown",
            pinion_core::Modifiers::default(),
        );
        let _ = TableView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "ArrowDown",
            pinion_core::Modifiers::default(),
        );
        assert_eq!(focused(&scene), (2, 0));
        assert!(TableView::apply_key(
            &mut scene,
            Some(PRIMARY_TAG),
            "Enter",
            pinion_core::Modifiers::default()
        ));
        assert_eq!(selected_row(&scene), Some(2), "Enter selects active row 2");
    }

    #[test]
    fn keys_ignored_when_grid_unfocused() {
        let mut scene = scene_fixture();
        assert!(!TableView::apply_key(
            &mut scene,
            None,
            "ArrowDown",
            pinion_core::Modifiers::default()
        ));
        assert_eq!(focused(&scene), (-1, 0));
    }

    #[test]
    fn access_node_emits_grid_row_columnheader_gridcell() {
        let nodes = TableView::access_node(&TableState::idle(), None);
        assert_eq!(nodes[0].role, AriaRole::Grid);
        // Header row + columnheaders.
        let hrow = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}_hrow"))
            .expect("header row node");
        assert_eq!(hrow.role, AriaRole::Row);
        for col in 0..NCOLS {
            let ch = nodes
                .iter()
                .find(|n| n.tag == format!("{PRIMARY_TAG}_ch{col}"))
                .expect("columnheader node");
            assert_eq!(ch.role, AriaRole::ColumnHeader);
        }
        // Data rows are the first consumer of the `row` role.
        let row0 = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}_row0"))
            .expect("data row node");
        assert_eq!(row0.role, AriaRole::Row);
        assert_eq!(row0.size_of_set, Some(u32::try_from(NROWS).unwrap()));
        // Cells are gridcells.
        let cell = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}#0_0"))
            .expect("gridcell node");
        assert_eq!(cell.role, AriaRole::GridCell);
    }

    #[test]
    fn selected_row_marks_aria_selected() {
        let mut state = TableState::idle();
        state.selected_row = Some(3);
        let nodes = TableView::access_node(&state, None);
        let row3 = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}_row3"))
            .expect("data row 3");
        assert_eq!(
            row3.selected,
            Some(true),
            "selected row reports aria-selected"
        );
        let row0 = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}_row0"))
            .expect("data row 0");
        assert_eq!(
            row0.selected,
            Some(false),
            "unselected row is aria-selected=false"
        );
    }

    #[test]
    fn access_focus_target_reports_active_descendant_cell() {
        let mut state = TableState::idle();
        state.focused_row = Some(2);
        state.focused_col = 3;
        let target = TableView::access_focus_target(&state, Some(PRIMARY_TAG))
            .expect("composite focus target");
        assert_eq!(target.focus_tag, PRIMARY_TAG);
        assert_eq!(target.active_descendant.as_deref(), Some("table#2_3"));
    }

    #[test]
    fn access_child_invoke_click_selects_row() {
        let mut scene = scene_fixture();
        assert!(TableView::access_child_invoke(
            &mut scene,
            PRIMARY_TAG,
            "1_2",
            AccessAction::Click,
        ));
        assert_eq!(
            selected_row(&scene),
            Some(1),
            "AT click on cell 1_2 selects row 1"
        );
    }

    #[test]
    fn read_state_round_trips_selection() {
        let mut scene = scene_fixture();
        TableView::access_child_invoke(&mut scene, PRIMARY_TAG, "4_0", AccessAction::Click);
        let state = TableView::read_state(&scene);
        assert_eq!(state.selected_row, Some(4));
    }
}
