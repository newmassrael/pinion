//! `hello-column-select` — R1563 §5.40 **a selection with two axes**.
//!
//! R1561 made a selection a set of *runs* over rows. R1562 made the vertical
//! header band press select the row through it — and named the mirror it could
//! not build: a **column** header cannot select its column, because there was
//! no column axis to select on. The windowed coordinator's selection was a set
//! of rows, so "column 3" was not a statement it could hold at any price.
//!
//! This binding is the consumer that axis was missing. Same 10 000 rows as
//! `hello-grid-multi-select`, eight columns instead of three, and one
//! difference that changes what a press means:
//!
//! ```text
//! VirtualSelectExternal::new_multi(N)
//!     .with_columns(NCOLS)                                // opens the column axis
//!     .with_behavior(SelectionBehavior::SelectItems)      // Qt setSelectionBehavior
//!     .with_section_press(SectionPress::Select)           // Qt sectionPressed
//! ```
//!
//! * **plain click on a cell** — select that one cell (Qt `SelectItems`,
//!   where `hello-grid-multi-select` is `SelectRows`).
//! * **`Ctrl`-click a cell** — flip that cell, leaving the rest alone.
//! * **`Shift`-click a cell** — the **rectangle** from the extension origin to
//!   it (Qt `QItemSelectionRange(anchor, current)`).
//! * **click a COLUMN header** — select the whole column (Qt
//!   `QHeaderView::sectionPressed` → `QTableView::selectColumn`), with the same
//!   three chords. The address is `vtbl#h<col>`, and it reaches
//!   `select_column` / `toggle_column` / `extend_to_column` because
//!   `GridSendKey::col()` answers with its column — the derivation R1562 built
//!   on the other axis, mirrored.
//! * **click a ROW header** — the whole record (R1562), whatever the
//!   behaviour: a row header addresses a record, which is what the band is for.
//! * **click the CORNER** — the tri-state select-all (R1562).
//!
//! ## What is new against the eager `hello-cell-select` (R953)
//!
//! That binding gave the **eager** `Table` coordinator Qt's `SelectItems`: an
//! anchor plus one rectangle. This is the windowed Model/View coordinator, and
//! three things follow from that:
//!
//! 1. the selection is **not a rectangle** — `Ctrl`-clicking scattered cells,
//!    or a column crossing a row, is a shape no `(row0, col0, row1, col1)` can
//!    hold;
//! 2. it is held as canonical **bands**, so selecting every cell of a
//!    10 000 × 8 model is *one* band and eleven bytes on the wire, where an
//!    eager rectangle is bounded by a model small enough to materialise;
//! 3. the **column** is addressable at all — neither coordinator had that, and
//!    Qt's third `SelectionBehavior` arm (`SelectColumns`) had no peer here
//!    until this round.
//!
//! ## What the model holds
//!
//! A set of cells has **no unique minimal decomposition** into rectangles — a
//! cross is two rectangles two different ways, both minimal — and this
//! framework's selection is canonical, because that is what lets it report
//! whether an interaction changed anything. So [`CellSelection`] holds the
//! function *row → column set* grouped by its value: one band per distinct
//! [`ColumnSpan`].
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/click` on `vtbl#h3` reports `cells = [{"rows": [[0, 9999]],
//! "columns": [[3, 3]]}]` and `column_selection = [[3, 3]]` — Qt's
//! `selectedColumns()`, which there costs one `QModelIndex` per row to
//! compute. `Shift`-clicking `vtbl#h5` widens the span to `[[3, 5]]` in the
//! same eleven bytes. `invoke("select_cell", [4, 2])` puts one cell in, and the
//! row band shows *partial* — a tri-state Qt's `highlightSections` bool cannot
//! express. See `tools/r1563_selection_has_two_axes.py`.
//!
//! ## a11y
//!
//! [`windowed_grid_nodes_cells`]: `aria-selected` on the rendered
//! `gridcell`s, on the `row`s selected as whole records, and on the
//! `columnheader` of a column selected in every row. Qt has the cell accessor
//! (`QAccessibleTableCell::isSelected`) and **no header one at all**, so a
//! fully selected Qt column announces exactly as an unselected one.

use pinion_a11y::{
    AccessNode, WidgetA11y, attach_corner_button, attach_row_headers, windowed_grid_nodes_cells,
};
use pinion_core::composite_tag::GridTag;
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::cell_selection::{
    CellSelection, ColumnSpan, GridSelection, SelectionBehavior,
};
use pinion_core::widgets::index_runs::IndexRuns;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{
    RowMetrics, SectionPress, SelectionExtent, VirtualSelectExternal, nav_select_key, read_cells,
    read_selection_at,
};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{
    CellIndex, CornerExtent, GridModel, GridScroll, HeaderAxis, RowHeaderAxis, TableStyle,
    VirtualTableData, header_from_slice, no_decoration, no_edit, view_virtual_table,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloColumnSelectRenderer, HelloColumnSelectRendererError);

/// Initial window size — freely resizable; the grid body re-windows on every
/// `Resized` event (R774 `AutoSizer`). Eight columns do not fit, which is the
/// point: the horizontal scroll engages and the vertical band stays pinned.
const WIN_W: u32 = 560;
const WIN_H: u32 = 480;
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`). Eight, so a row's selected columns
/// fit one `u8` in the paint projection and a `Shift`-extend across the column
/// axis is a real span rather than two neighbours.
const NCOLS: usize = 8;
/// Uniform column width; `NCOLS × COL_W` exceeds `WIN_W`, so the grid windows
/// its column axis too (R1523).
const COL_W: u32 = 96;
/// Data-row height (the windowing pitch + the scroll-into-view pitch).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Status-bar height above the grid.
const STATUS_H: u32 = 40;
/// Column header labels.
const HEADERS: [&str; NCOLS] = [
    "Index", "Name", "Status", "Owner", "Size", "Kind", "Tag", "Note",
];
/// Paint-root + a11y `grid` tag, and the [`VirtualSelectExternal`] anchor: cell
/// clicks (`vtbl#<row>_<col>`), column-header clicks (`vtbl#h<col>`), row-header
/// clicks (`vtbl#r<row>`) and the corner (`vtbl#c`) all route here through the
/// R51.42 composite protocol.
const TABLE_TAG: &str = "vtbl";
const SCROLL_KEY: &str = "vtbl_scroll";
const H_SCROLL_KEY: &str = "vtbl_hscroll";
const STATUS_TAG: &str = "vtbl_status";

/// Every column of an eight-column model.
const ALL_COLUMNS: u8 = 0xFF;

/// Copy paint-snapshot of the coordinator's two-axis selection.
///
/// One **column bitmask per row** rather than a bool: with `NCOLS = 8` a row's
/// whole answer is a `u8`, so this projection is the same size as the per-row
/// bitmap `hello-grid-multi-select` builds for a one-axis selection. The
/// coordinator itself holds bands ([`CellSelection`] — no per-row state at
/// all); this exists only so the shell can hand a `Copy` snapshot into the view
/// fn, [`WidgetCore::State`] being `Copy` while a band set owns a `Vec`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct CellSnapshot {
    /// Per-row selected-column bitmask, indexed by absolute data row.
    rows: [u8; N],
    /// Per-column "selected in every row" — Qt `selectedColumns()`, read off
    /// the coordinator's own O(bands) answer rather than recomputed here by
    /// scanning [`Self::rows`] ten thousand times per column.
    columns: [bool; NCOLS],
    /// How many **cells** are selected (the status readout).
    cells: usize,
    /// How many **bands** hold them: the size of the statement. Selecting the
    /// whole model is `1`.
    bands: usize,
    /// How many rows are selected as whole **records** — the corner control's
    /// numerator. Off [`CellSelection::rows_all_columns`], which is the `All`
    /// band read in O(1), rather than by scanning [`Self::rows`] for a
    /// saturated mask.
    records: usize,
}

impl CellSnapshot {
    fn empty() -> Self {
        Self {
            rows: [0; N],
            columns: [false; NCOLS],
            cells: 0,
            bands: 0,
            records: 0,
        }
    }

    /// Project the bands into the bitmask: one `fill` per run per band, so a
    /// select-all is a memset rather than ten thousand writes, and a whole
    /// column is one too.
    fn from_bands(cells: &CellSelection, columns: &IndexRuns) -> Self {
        let mut s = Self::empty();
        for band in cells.bands() {
            let mask = column_mask(&band.columns);
            for run in band.rows.clamped_below(N).runs() {
                s.rows[run.first..=run.last].fill(mask);
            }
        }
        for col in columns.clamped_below(NCOLS).iter() {
            s.columns[col] = true;
        }
        s.cells = cells.cell_count(NCOLS);
        s.bands = cells.band_count();
        s.records = cells.rows_all_columns().clamped_below(N).len();
        s
    }

    /// R1562 — the extent the corner control shows: how much of the model is
    /// selected **as records**, which is what the corner's press toggles.
    fn extent(&self) -> CornerExtent {
        CornerExtent::of(self.records, N)
    }
}

/// A span as the bitmask this projection stores.
///
/// [`ColumnSpan::All`] resolves to `ALL_COLUMNS` **here** while staying
/// count-independent in the model: a paint snapshot is one moment and is
/// allowed to resolve it, which is exactly the distinction
/// [`ColumnSpan::resolved`] names.
fn column_mask(span: &ColumnSpan) -> u8 {
    match span {
        ColumnSpan::All => ALL_COLUMNS,
        ColumnSpan::Runs(runs) => runs
            .clamped_below(NCOLS)
            .iter()
            .fold(0u8, |mask, col| mask | (1u8 << col)),
    }
}

/// R1563 — the snapshot answers the paint's and the AT tree's selection
/// question directly, so neither holds a second copy of it (the R1536 rule,
/// widened to two axes).
impl GridSelection for CellSnapshot {
    fn cell(&self, row: usize, col: usize) -> bool {
        row < N && col < NCOLS && self.rows[row] & (1u8 << col) != 0
    }

    fn row(&self, row: usize) -> SelectionExtent {
        if row >= N {
            return SelectionExtent::Empty;
        }
        SelectionExtent::of(self.rows[row].count_ones() as usize, NCOLS)
    }

    fn column(&self, col: usize) -> SelectionExtent {
        if col >= NCOLS {
            return SelectionExtent::Empty;
        }
        if self.columns[col] {
            return SelectionExtent::All;
        }
        // Not covered everywhere — but "is it covered anywhere" is what decides
        // whether the section shows as involved, and only a look can answer it.
        // Asked once per PAINTED section, so the cost is the column window's
        // width times the model's height, not the model's area.
        if self.rows.iter().any(|m| m & (1u8 << col) != 0) {
            SelectionExtent::Partial
        } else {
            SelectionExtent::Empty
        }
    }
}

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        // R1020 §5.39 — the grid is a single Tab stop.
        focusable: true,
        ..TableStyle::m3()
    }
}

/// Synthetic cell texts (the `hello-grid-nav` dataset, widened to eight
/// columns).
fn cell_text(c: CellIndex) -> String {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    const STATUS: [&str; 3] = ["Idle", "Active", "Done"];
    const OWNERS: [&str; 4] = ["ada", "linus", "grace", "ken"];
    const KINDS: [&str; 3] = ["mesh", "texture", "clip"];
    let id = c.row;
    match c.col {
        0 => format!("{id:05}"),
        1 => CATEGORIES[id % CATEGORIES.len()].to_string(),
        2 => STATUS[id % STATUS.len()].to_string(),
        3 => OWNERS[id % OWNERS.len()].to_string(),
        4 => format!("{} kB", id % 997),
        5 => KINDS[id % KINDS.len()].to_string(),
        6 => format!("t{}", id % 31),
        _ => format!("n{}", id % 17),
    }
}

/// Status bar: a literal scene-as-data readout of both axes — how many cells,
/// and how many bands say so. On a one-axis selection those two questions
/// collapse into one; here selecting the whole model is 80 000 cells in **one**
/// band, and the second number is the one an agent budgeting a read wants.
fn status_bar(theme: &Theme, snapshot: &CellSnapshot) -> Scene {
    let (cells, bands) = (snapshot.cells, snapshot.bands);
    let cols = snapshot.columns.iter().filter(|&&c| c).count();
    let cell_noun = if cells == 1 { "cell" } else { "cells" };
    let band_noun = if bands == 1 { "band" } else { "bands" };
    let text = Scene::Text(
        TextNode::styled(
            format!(
                "selected {cells} {cell_noun} in {bands} {band_noun} \u{00B7} \
                 {cols} full column(s)"
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG),
    );
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::auto().with_height(SizeValue::Px(STATUS_H)))
                    .with_flex_grow(0.0)
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping `snapshot -> Scene`. The dataset is
/// virtual on **both** axes — `view_virtual_table` asks `cell_text` only for
/// the windowed rows × windowed columns — and the selection question is asked
/// only about what is painted.
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(snapshot: &CellSnapshot, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let grid = view_virtual_table(
        TABLE_TAG,
        GridScroll {
            body: &scroll,
            horizontal: &h_scroll,
        },
        VirtualTableData {
            column_count: NCOLS,
            item_count: N,
            overscan: OVERSCAN,
            sort: None,
            sort_tag: None,
            order: None,
            col_widths: None,
            resizable: false,
            frozen_cols: 0,
            row_style: None,
            delegate: None,
            editing: None,
        },
        &theme,
        &style,
        // R1563 — the two-axis question, where every row-select grid passes a
        // `Fn(usize) -> bool`. Both satisfy `GridSelection`; the difference is
        // that this one can answer about a cell and about a column.
        snapshot,
        GridModel {
            cell: cell_text,
            columns: HeaderAxis::labelled(header_from_slice(&HEADERS)),
            rows: Some(RowHeaderAxis::select_all(
                HeaderAxis::row_numbers(),
                snapshot.extent(),
            )),
            decoration: no_decoration,
            edit: no_edit,
        },
    );

    Scene::Container(
        ContainerNode::new(vec![status_bar(&theme, snapshot), grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct ColumnSelectView;

impl WidgetCore for ColumnSelectView {
    type State = CellSnapshot;
    type Event = ();

    /// The coordinator, with its column axis declared and its press behaviour
    /// stated. Without [`VirtualSelectExternal::with_columns`] every cell- and
    /// column-addressed path refuses — a grid that never said how wide it is
    /// cannot be asked to select its third column, and inferring a width from
    /// whatever is on screen would make the answer depend on the scroll.
    fn create_external() -> Box<dyn External> {
        Box::new(
            VirtualSelectExternal::new_multi(N)
                .with_columns(NCOLS)
                .with_behavior(SelectionBehavior::SelectItems)
                // Qt reaches this through two independent connections to one
                // `QHeaderView` (`sectionPressed` → select, `sectionClicked` →
                // sort) with nothing declaring which a given header has. This
                // grid does not sort, so its sections select — and it says so.
                .with_section_press(SectionPress::Select),
        )
    }

    fn tag() -> &'static str {
        TABLE_TAG
    }

    /// Project both axes off the coordinator: the bands from `cells`, and the
    /// fully-selected columns from `column_selection` — which the coordinator
    /// answers in O(bands), where recomputing it here would be one scan of the
    /// model per column.
    fn read_state(scene: &Scene) -> CellSnapshot {
        let Some(intro) = scene
            .find_external_with_tag(TABLE_TAG)
            .and_then(|node| node.handle.introspect())
        else {
            return CellSnapshot::empty();
        };
        CellSnapshot::from_bands(
            &read_cells(intro),
            &read_selection_at(intro, "column_selection"),
        )
    }

    fn view(state: CellSnapshot, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Keyboard selection stays on the **row** axis, delegated to the shared
    /// `nav_select_key` controller: arrows move the active record, `Ctrl+A`
    /// takes the model. A two-axis keyboard vocabulary (Qt's `Ctrl+Space` on a
    /// cell, `Ctrl+Shift+Arrow` growing a rectangle) is a keyboard round, not
    /// this one — stated here so the pointer and the keyboard are not silently
    /// assumed to cover the same ground.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        nav_select_key(
            scene,
            &use_scroll_state(SCROLL_KEY),
            TABLE_TAG,
            focused,
            key,
            modifiers,
            RowMetrics {
                item_count: N,
                row_pitch: ROW_H,
            },
        )
    }

    fn title() -> &'static str {
        "pinion hello-column-select (R1563 §5.40 a selection with two axes)"
    }

    fn fmt_state_log(state: &CellSnapshot) -> String {
        format!("selected={} cells in {} bands", state.cells, state.bands)
    }
}

impl WidgetA11y for ColumnSelectView {
    /// Two-axis WAI-ARIA virtualized `grid`. The window is the same
    /// `compute_visible_range` over the measured viewport the view fn uses, so
    /// the AT tree and the painted tree never disagree about which rows exist.
    fn access_node(snapshot: &CellSnapshot, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        let mut nodes = windowed_grid_nodes_cells(
            TABLE_TAG,
            "Cell-selectable data grid",
            NCOLS,
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
            snapshot,
        );
        attach_row_headers(&mut nodes, TABLE_TAG, &window, |view_pos| view_pos);
        attach_corner_button(
            &mut nodes,
            TABLE_TAG,
            &GridTag::header_row(TABLE_TAG),
            snapshot.extent(),
        );
        nodes
    }
}

impl WidgetView for ColumnSelectView {
    type Renderer = HelloColumnSelectRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ColumnSelectView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    fn snapshot_of(cells: &CellSelection) -> CellSnapshot {
        let columns = cells.columns_covering_all_rows(N, NCOLS);
        CellSnapshot::from_bands(cells, &columns)
    }

    fn one_cell(row: usize, col: usize) -> CellSelection {
        let mut cells = CellSelection::new();
        cells.add(&IndexRuns::run(row, row), &ColumnSpan::column(col));
        cells
    }

    fn whole_column(col: usize) -> CellSelection {
        let mut cells = CellSelection::new();
        cells.add(&IndexRuns::run(0, N - 1), &ColumnSpan::column(col));
        cells
    }

    fn run_view(cells: &CellSelection) -> Scene {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_H).unwrap());
            scroll.set_measured_viewport(WIN_W, 384);
            scroll.scroll_to(0, 0);
            let h = use_scroll_state(H_SCROLL_KEY);
            h.set_measured_viewport(WIN_W, 384);
            view(&snapshot_of(cells), &Frame::default())
        })
    }

    fn selection_wash() -> pinion_core::style::Color {
        let theme = pinion_core::theme::Theme::light();
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16)
    }

    fn fill_of(scene: &Scene, want: &str) -> Option<pinion_core::style::Color> {
        fn walk(scene: &Scene, want: &str) -> Option<pinion_core::style::Color> {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref() == Some(want) {
                        return Some(c.style.fill);
                    }
                    c.children.iter().find_map(|ch| walk(ch, want))
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), want),
                _ => None,
            }
        }
        walk(scene, want)
    }

    fn has_fill(scene: &Scene, want: pinion_core::style::Color) -> bool {
        match scene {
            Scene::Container(c) => {
                c.style.fill == want || c.children.iter().any(|ch| has_fill(ch, want))
            }
            Scene::Scroll(s) => has_fill(s.content.as_ref(), want),
            _ => false,
        }
    }

    /// The selection ink is on the CELL, not on the row: a row with one
    /// selected cell must not wash whole, or a two-axis selection would be
    /// unreadable the moment it stops being rectangular.
    #[test]
    fn r1563_a_selected_cell_inks_without_washing_its_row() {
        let wash = selection_wash();
        let scene = run_view(&one_cell(2, 1));
        assert_ne!(
            fill_of(&scene, &format!("{TABLE_TAG}_row2")),
            Some(wash),
            "the row strip must not wash for one selected cell"
        );
        assert!(
            has_fill(&scene, wash),
            "the selected cell carries the ink instead"
        );
    }

    /// A press on a column header selects the column, and the band through it
    /// shows so — the fact R1562 could not build.
    #[test]
    fn r1563_a_selected_column_washes_its_header_section() {
        let wash = selection_wash();
        let scene = run_view(&whole_column(1));
        assert_eq!(
            fill_of(&scene, &GridTag::col_header(TABLE_TAG, 1)),
            Some(wash),
            "the fully selected column's section washes accent"
        );
        assert_ne!(
            fill_of(&scene, &GridTag::col_header(TABLE_TAG, 2)),
            Some(wash),
            "its neighbour does not"
        );
    }

    /// A partly selected row's band is neither of the two states a bool has —
    /// the tri-state Qt's `highlightSections` cannot express.
    #[test]
    fn r1563_a_partly_selected_row_band_is_neither_state() {
        let theme = pinion_core::theme::Theme::light();
        let idle = theme.resolve(ColorRole::SurfaceContainerHigh);
        let scene = run_view(&one_cell(2, 1));
        let band = fill_of(&scene, &GridTag::row_header(TABLE_TAG, 2));
        assert!(band.is_some(), "row 2's band is painted");
        assert_ne!(
            band,
            Some(selection_wash()),
            "row 2 is not selected as a record"
        );
        assert_ne!(band, Some(idle), "and it is not untouched either");
    }

    #[test]
    fn r1563_a11y_marks_the_cell_the_row_and_the_column() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 384);
            ColumnSelectView::access_node(&snapshot_of(&whole_column(1)), None)
        });
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert!(nodes[0].multiselectable);
        let header_selected = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader && n.selected == Some(true))
            .count();
        assert_eq!(header_selected, 1, "exactly the selected column's header");
        let cells_selected = nodes
            .iter()
            .filter(|n| n.role == AriaRole::GridCell && n.selected == Some(true))
            .count();
        let rows_selected = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.selected == Some(true))
            .count();
        assert!(cells_selected > 0, "the column's windowed cells are marked");
        assert_eq!(
            rows_selected, 0,
            "no row is selected as a record — one of eight columns is not a row"
        );
    }
}
