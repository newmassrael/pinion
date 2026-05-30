//! `hello-table` — R707 §5.51 interactive **data table**: the GUI
//! consumer of the R707
//! [`Table`](pinion_core::widgets::table::Table) single-row-selection
//! coordinator + the §5.51 [`pinion_widget_paint::table`] data-grid
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
//! The grid is a **single Tab stop** ([`focusable_tags`] returns only the
//! grid root), and the focused cell is an internal 2-D roving *active
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
    AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, WidgetA11y,
};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::table::TableExternal;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::{view_table, TableData, TableStyle};

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
    /// R707 §5.51 — the roving active-descendant row (0-based), or
    /// `None` before any navigation.
    focused_row: Option<usize>,
    /// R707 §5.51 — the roving active-descendant column (0-based).
    focused_col: usize,
    /// Per-row interaction state, indexed by row (exactly [`NROWS`]).
    row_states: [RadioState; NROWS],
}

impl TableState {
    fn idle() -> Self {
        Self {
            selected_row: None,
            focused_row: None,
            focused_col: 0,
            row_states: [RadioState::Idle; NROWS],
        }
    }

    fn row_state(&self, row: usize) -> RadioState {
        self.row_states.get(row).copied().unwrap_or(RadioState::Idle)
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

/// Read the live `focused_row` active descendant (`-1` / absent → `None`).
fn read_focused_row(intro: &dyn pinion_core::external::ExternalIntrospect) -> Option<usize> {
    match intro.query("focused_row") {
        Some(IntrospectValue::Int(r)) if r >= 0 => usize::try_from(r).ok(),
        _ => None,
    }
}

/// Read the live `focused_col` active descendant (defaults to 0).
fn read_focused_col(intro: &dyn pinion_core::external::ExternalIntrospect) -> usize {
    match intro.query("focused_col") {
        Some(IntrospectValue::Int(c)) if c >= 0 => usize::try_from(c).unwrap_or(0),
        _ => 0,
    }
}

/// Read the row count from the introspect surface (0 if unavailable).
fn read_rows(intro: &dyn pinion_core::external::ExternalIntrospect) -> usize {
    match intro.query("rows") {
        Some(IntrospectValue::Int(r)) => usize::try_from(r).unwrap_or(0),
        _ => 0,
    }
}

/// Read the column count from the introspect surface (0 if unavailable).
fn read_cols(intro: &dyn pinion_core::external::ExternalIntrospect) -> usize {
    match intro.query("cols") {
        Some(IntrospectValue::Int(c)) => usize::try_from(c).unwrap_or(0),
        _ => 0,
    }
}

/// Establish the active-descendant row if none is set yet (a horizontal
/// arrow / Home / End entering the grid lands the cursor on row 0).
fn ensure_row(intro: &mut dyn pinion_core::external::ExternalIntrospect) {
    if read_focused_row(intro).is_none() {
        let _ = intro.intervene("focused_row", IntrospectValue::Int(0));
    }
}

/// Move the active-descendant row by `delta`, clamped within the dataset.
/// From `None`, the first vertical arrow lands on row 0.
fn move_row(intro: &mut dyn pinion_core::external::ExternalIntrospect, delta: i64) {
    let rows = read_rows(intro);
    if rows == 0 {
        return;
    }
    let max = i64::try_from(rows - 1).unwrap_or(0);
    let next = match read_focused_row(intro) {
        None => 0,
        Some(current) => (i64::try_from(current).unwrap_or(0) + delta).clamp(0, max),
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
    let _ = intro.intervene("focused_col", IntrospectValue::Int(i64::try_from(clamped).unwrap_or(0)));
}

/// Set the active-descendant row to a specific value (`PageUp` /
/// `PageDown`), clamped within the dataset.
fn set_row(intro: &mut dyn pinion_core::external::ExternalIntrospect, row: usize) {
    let rows = read_rows(intro);
    if rows == 0 {
        return;
    }
    let clamped = row.min(rows - 1);
    let _ = intro.intervene("focused_row", IntrospectValue::Int(i64::try_from(clamped).unwrap_or(0)));
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
    let style = TableStyle::m3();
    let rows: Vec<&[&str]> = ROWS.iter().map(|r| &r[..]).collect();
    let table = view_table(
        PRIMARY_TAG,
        TableData { headers: &HEADERS, rows: &rows },
        state.selected_row,
        &state.row_states,
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
        "pinion hello-table (R707 §5.51 interactive data grid)"
    }

    /// WAI-ARIA APG data-grid focus model: the grid is a **single Tab
    /// stop** ([`PRIMARY_TAG`]), with the focused cell tracked as an
    /// internal 2-D roving *active descendant* (the coordinator's
    /// `focused_row` / `focused_col` slots) — the same single-tab-stop +
    /// active-descendant model `hello-datepicker` uses. The one tag is
    /// static, so the boot-seeded `focusable_tags` enumeration is stable.
    fn focusable_tags() -> Vec<&'static str> {
        vec![PRIMARY_TAG]
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
                set_row(intro, 0);
                true
            }
            "PageDown" => {
                let last = read_rows(intro).saturating_sub(1);
                set_row(intro, last);
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
    /// R707 §5.51 — composite AccessKit tree contribution. Emits the
    /// `grid` root, a header `row` of `columnheader` nodes, one data
    /// `row` per dataset row (carrying `aria-selected` + position/size in
    /// set), and one `gridcell` per cell. The single-row-selection
    /// invariant lives in the coordinator; at most one data row reports
    /// `aria-selected="true"`. The active descendant (when the grid owns
    /// focus) is the cell `aria-activedescendant` points to.
    fn access_node(state: &TableState, focused: Option<&str>) -> Vec<AccessNode> {
        let grid_focused = focused == Some(PRIMARY_TAG);
        let (active_row, active_col) = active_cell(state);

        let mut nodes: Vec<AccessNode> =
            Vec::with_capacity(NROWS * (NCOLS + 1) + NCOLS + 2);

        // Grid root: children are the header row + each data row.
        let mut grid = AccessNode::new(PRIMARY_TAG, AriaRole::Grid)
            .with_name("pinion widget catalog");
        grid = grid.with_child(format!("{PRIMARY_TAG}_hrow"));
        for row in 0..NROWS {
            grid = grid.with_child(format!("{PRIMARY_TAG}_row{row}"));
        }
        nodes.push(grid);

        // Header row + its columnheader cells.
        let mut header_row = AccessNode::new(format!("{PRIMARY_TAG}_hrow"), AriaRole::Row);
        for col in 0..NCOLS {
            header_row = header_row.with_child(format!("{PRIMARY_TAG}_ch{col}"));
        }
        nodes.push(header_row);
        for (col, label) in HEADERS.iter().enumerate() {
            nodes.push(
                AccessNode::new(format!("{PRIMARY_TAG}_ch{col}"), AriaRole::ColumnHeader)
                    .with_name(*label),
            );
        }

        // Data rows + their gridcells.
        for (row, row_data) in ROWS.iter().enumerate() {
            let is_selected = state.selected_row == Some(row);
            let interaction = state.row_state(row);
            let mut row_node = AccessNode::new(format!("{PRIMARY_TAG}_row{row}"), AriaRole::Row)
                .with_selected(is_selected)
                .with_position_in_set(u32::try_from(row + 1).unwrap_or(1))
                .with_size_of_set(u32::try_from(NROWS).unwrap_or(1));
            for col in 0..NCOLS {
                row_node = row_node.with_child(format!("{PRIMARY_TAG}#{row}_{col}"));
            }
            nodes.push(row_node);

            for (col, header) in HEADERS.iter().enumerate() {
                let cell_tag = format!("{PRIMARY_TAG}#{row}_{col}");
                let cell_focused = grid_focused && active_row == row && active_col == col;
                let name = format!("{header}: {}", row_data[col]);
                nodes.push(
                    AccessNode::new(&cell_tag, AriaRole::GridCell)
                        .with_name(name)
                        .with_state(AccessState {
                            focused: cell_focused,
                            disabled: matches!(interaction, RadioState::Disabled),
                            hovered: matches!(interaction, RadioState::Hover),
                            pressed: matches!(interaction, RadioState::Pressed),
                            checked: None,
                        }),
                );
            }
        }
        nodes
    }

    /// R707 §5.51 — composite focus model (mirror of `hello-datepicker`).
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

    /// R707 §5.51 — composite child action dispatch. A cell carries a
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
                    let _ = intro
                        .invoke("send", IntrospectValue::Text(format!("{row}_{col}:{ev}")));
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
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
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
        Scene::External(
            ExternalNode::new(TableView::create_external()).with_tag(PRIMARY_TAG),
        )
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
        assert!(TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "ArrowDown", pinion_core::Modifiers::default()));
        assert_eq!(focused(&scene), (0, 0), "first ArrowDown enters at row 0");
        let _ = TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "ArrowDown", pinion_core::Modifiers::default());
        assert_eq!(focused(&scene), (1, 0), "ArrowDown -> row 1");
        let _ = TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "ArrowRight", pinion_core::Modifiers::default());
        assert_eq!(focused(&scene), (1, 1), "ArrowRight -> col 1");
        // PageDown jumps to the last row; ArrowDown then clamps.
        let _ = TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "PageDown", pinion_core::Modifiers::default());
        assert_eq!(focused(&scene).0, i64::try_from(NROWS - 1).unwrap(), "PageDown -> last row");
        let _ = TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "ArrowDown", pinion_core::Modifiers::default());
        assert_eq!(focused(&scene).0, i64::try_from(NROWS - 1).unwrap(), "ArrowDown clamps at last row");
        // End jumps to the last column.
        let _ = TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "End", pinion_core::Modifiers::default());
        assert_eq!(focused(&scene).1, i64::try_from(NCOLS - 1).unwrap(), "End -> last col");
    }

    #[test]
    fn enter_activates_active_descendant_row() {
        let mut scene = scene_fixture();
        // Three ArrowDowns: enter at row 0, then advance to row 2.
        let _ = TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "ArrowDown", pinion_core::Modifiers::default());
        let _ = TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "ArrowDown", pinion_core::Modifiers::default());
        let _ = TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "ArrowDown", pinion_core::Modifiers::default());
        assert_eq!(focused(&scene), (2, 0));
        assert!(TableView::apply_key(&mut scene, Some(PRIMARY_TAG), "Enter", pinion_core::Modifiers::default()));
        assert_eq!(selected_row(&scene), Some(2), "Enter selects active row 2");
    }

    #[test]
    fn keys_ignored_when_grid_unfocused() {
        let mut scene = scene_fixture();
        assert!(!TableView::apply_key(&mut scene, None, "ArrowDown", pinion_core::Modifiers::default()));
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
        assert_eq!(row3.selected, Some(true), "selected row reports aria-selected");
        let row0 = nodes
            .iter()
            .find(|n| n.tag == format!("{PRIMARY_TAG}_row0"))
            .expect("data row 0");
        assert_eq!(row0.selected, Some(false), "unselected row is aria-selected=false");
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
        assert_eq!(selected_row(&scene), Some(1), "AT click on cell 1_2 selects row 1");
    }

    #[test]
    fn read_state_round_trips_selection() {
        let mut scene = scene_fixture();
        TableView::access_child_invoke(&mut scene, PRIMARY_TAG, "4_0", AccessAction::Click);
        let state = TableView::read_state(&scene);
        assert_eq!(state.selected_row, Some(4));
    }
}
