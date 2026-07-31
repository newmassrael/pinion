//! `hello-virtual-columns` — R1523 §5.27 §5.45 **column-axis
//! virtualization**.
//!
//! A **200-column × 10,000-row** Material-3 data-grid — the wide process
//! table / packet dissector / DCC attribute sheet shape — that windows
//! **both** axes: the scene tree holds only the rows the measured viewport
//! height exposes (R744/R775) *and* only the columns its measured width
//! exposes (this round).
//!
//! ## The asymmetry this example exists to close
//!
//! R744 windowed the row axis because "the layout pass walks every node,
//! the cache holds every node, the introspection snapshot serializes every
//! node". R784 then gave the grid a horizontal viewport — but only a
//! *viewport*: every column was still built, for every windowed row, and
//! merely positioned off-screen. `hello-grid-hscroll`'s own view fn said so
//! ("no column virtualization at 8 columns"), and at 8 columns nothing was
//! at stake. At 200 columns with ~5 on screen the un-windowed axis costs
//! 40× the cells, in the layout pass, the paint encode, the a11y tree and
//! the `scene/snapshot` — the exact list R744 gives as its own reason.
//!
//! The columns here are **deliberately unequal** ([`col_width`] cycles five
//! widths), so a window computed from a uniform pitch cannot land on the
//! right set: the arithmetic has to be the prefix-sum + binary search the
//! row axis already uses for variable row heights
//! ([`pinion_core::widgets::column_widths::visible_columns`]).
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` reports ~`viewport_w / col_width` cells per row out of
//! [`NCOLS`], and the header band reports the **same** column set (one
//! scroll, so header and body can never disagree). `scene/set_scroll_offset`
//! on the horizontal scroll slides the window: far columns appear, near ones
//! leave the tree entirely — not merely off-screen. The full extent stays
//! observable throughout: `max_x` is the whole 24,000px content width minus
//! the viewport, so the scrollbar sizes against 200 columns while ~5 exist. No
//! pixels required (see `tools/demos/r1523_virtual_columns.py`).
//!
//! ## a11y (WAI-ARIA two-axis virtualized grid)
//!
//! The row axis conveys "which of how many" through
//! `aria-setsize`/`aria-posinset`; the column axis conveys it through
//! `aria-colcount` on the grid and `aria-colindex` on every windowed cell,
//! so an AT reads "column 137 of 200" for a cell whose 136 predecessors are
//! not in the tree. Windowing an axis without its extent pair would make the
//! grid *less* readable than before it scaled.

use pinion_a11y::{AccessNode, WidgetA11y, windowed_grid_nodes_wide};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{BoxStyle, FlexDirection, LayoutStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::column_widths::use_column_widths;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{GridScroll, TableStyle, VirtualTableData, view_virtual_table};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(
    HelloVirtualColumnsRenderer,
    HelloVirtualColumnsRendererError
);

/// Initial window size. Deliberately far narrower than the 24,000px content
/// width, so the overwhelming majority of columns are off-window at any
/// offset.
const WIN_W: u32 = 560;
const WIN_H: u32 = 420;
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key.
const THEME_TAG: &str = "app";
/// Total data-row count — the row axis this example inherits already
/// windowed.
const N: usize = 10_000;
/// Column count. The axis under test: two orders of magnitude more columns
/// than any pre-R1523 grid in the tree (the widest was 10).
const NCOLS: usize = 200;
/// Data-row height (must match the windowing pitch used in `access_node`).
const ROW_H: u32 = 36;
/// Rows *and* columns built beyond the strict visible window on each side.
const OVERSCAN: usize = 2;
/// Paint-root + a11y `grid` tag, and the [`StubExternal`] anchor tag.
const TABLE_TAG: &str = "vcol";
/// Cache key (and input-router tag) for the vertical body `ScrollState`.
const SCROLL_KEY: &str = "vcol_scroll";
/// Cache key (and input-router tag) for the outer horizontal `ScrollState`
/// — `scene/set_scroll_offset` on this tag slides the **column** window.
const H_SCROLL_KEY: &str = "vcol_hscroll";
/// Cache key for the shared [`ColumnWidths`](pinion_core::widgets::column_widths::ColumnWidths)
/// model, whose prefix-sum resolves which columns the viewport exposes.
const COLS_KEY: &str = "vcol_cols";

/// Width of column `col`, in logical pixels.
///
/// Five distinct widths on a cycle, so no uniform pitch describes this axis:
/// a window computed as `offset_x / pitch` lands on the wrong column for
/// every offset past the first cycle. Column 0 is the widest (the identity
/// column a wide table pins the eye on).
#[must_use]
fn col_width(col: usize) -> u32 {
    const CYCLE: [u32; 5] = [150, 90, 120, 105, 135];
    CYCLE[col % CYCLE.len()]
}

/// Every column's width — the seed for the shared `ColumnWidths` model.
#[must_use]
fn col_widths() -> Vec<u32> {
    (0..NCOLS).map(col_width).collect()
}

/// Column header labels. Generated rather than a literal array: 200 hand
/// written labels would be noise, and the zero-padded index makes every
/// `scene/snapshot` header unambiguous about *which* column survived the
/// window.
#[must_use]
fn headers() -> Vec<String> {
    (0..NCOLS).map(|c| format!("C{c:03}")).collect()
}

fn table_style() -> TableStyle {
    TableStyle {
        row_height: ROW_H,
        ..TableStyle::m3()
    }
}

/// Synthetic cell texts for a data row across all [`NCOLS`] columns.
///
/// `r<row>c<col>` names both coordinates, so a snapshot cell proves which
/// (row, column) pair the tree actually holds — the assertion the demo
/// needs on both axes at once.
fn row_cells(id: usize) -> Vec<String> {
    (0..NCOLS).map(|c| format!("r{id}c{c}")).collect()
}

/// view-fn (§6.3): pure sync `() -> Scene`. Both axes are virtual —
/// `view_virtual_table` builds cells only for the windowed rows *and* the
/// windowed columns, both windows derived from the runtime-measured
/// viewport.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let widths = use_column_widths(COLS_KEY, col_widths);
    let width_snapshot = widths.widths();
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();
    let labels = headers();
    let header_refs: Vec<&str> = labels.iter().map(String::as_str).collect();

    let grid = view_virtual_table(
        TABLE_TAG,
        GridScroll {
            body: &scroll,
            horizontal: &h_scroll,
        },
        VirtualTableData {
            headers: &header_refs,
            item_count: N,
            overscan: OVERSCAN,
            sort: None,
            sort_tag: None,
            order: None,
            col_widths: Some(&width_snapshot),
            resizable: false,
            frozen_cols: 0,
            row_style: None,
        },
        &theme,
        &style,
        |_| false, // display-only grid: no selection
        row_cells,
    );

    Scene::Container(
        ContainerNode::new(vec![grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct VirtualColumnsView;

impl WidgetCore for VirtualColumnsView {
    type State = ();
    type Event = ();

    /// Display-only: the addressable anchor is the no-op [`StubExternal`] at
    /// [`TABLE_TAG`] (input router + a11y `grid` bounds). Both scroll axes
    /// route through their own `ScrollState` tags, so neither needs an
    /// External of its own.
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn tag() -> &'static str {
        TABLE_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-virtual-columns (R1523 §5.27 column-axis virtualization)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for VirtualColumnsView {
    /// WAI-ARIA grid windowed on **both** axes: the row window comes from
    /// the body scroll's measured height (as every windowed grid's does), the
    /// column window from the horizontal scroll's measured width through the
    /// shared `ColumnWidths` prefix-sum — the same two windows the view fn
    /// paints against, so the AT tree and the painted tree cannot disagree
    /// about which cells exist.
    ///
    /// The extents both survive windowing: `aria-setsize = N` on every row,
    /// `aria-colcount = NCOLS` on the grid, and each windowed cell's absolute
    /// `aria-colindex`.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let h_scroll = use_scroll_state(H_SCROLL_KEY);
        let widths = use_column_widths(COLS_KEY, col_widths);
        let (_, measured_h) = scroll.measured_viewport();
        let (measured_w, _) = h_scroll.measured_viewport();
        let rows = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        let cols = widths.visible_columns(h_scroll.offset_x(), measured_w, OVERSCAN);
        let labels = headers();
        let header_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        windowed_grid_nodes_wide(
            TABLE_TAG,
            "Wide data grid",
            &header_refs,
            u32::try_from(N).unwrap_or(u32::MAX),
            &rows,
            &cols,
        )
    }
}

impl WidgetView for VirtualColumnsView {
    type Renderer = HelloVirtualColumnsRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<VirtualColumnsView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    /// The grid's intrinsic content width (24,000px — ~43x the window).
    ///
    /// Test-only: production never states it, because `view_virtual_table`
    /// sums the widths itself. A second statement of the total in the binding
    /// could drift from the one the grid actually lays out.
    fn total_width() -> u32 {
        col_widths().iter().copied().sum()
    }

    /// Build the a11y tree with both viewports measured, as the shell does
    /// after the first layout pass.
    fn run_access(measured_w: u32, measured_h: u32, offset_x: i32) -> Vec<AccessNode> {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(measured_w, measured_h);
            let h_scroll = use_scroll_state(H_SCROLL_KEY);
            h_scroll.set_measured_viewport(measured_w, measured_h);
            let extent = total_width().saturating_sub(measured_w);
            h_scroll.set_max(i32::try_from(extent).unwrap_or(i32::MAX), 0);
            h_scroll.scroll_to(offset_x, 0);
            VirtualColumnsView::access_node(&(), None)
        })
    }

    /// The premise: the columns overflow the window by a wide margin, and
    /// they are **not** uniform — the two properties that make this example
    /// discriminating. Asserted, not assumed: a later edit that equalises
    /// the widths would silently turn every window assertion below into a
    /// test of uniform-pitch arithmetic.
    #[test]
    fn premise_columns_are_unequal_and_overflow() {
        let total = total_width();
        assert!(
            total > WIN_W * 10,
            "columns ({total}) must overflow WIN_W ({WIN_W}) by an order of magnitude",
        );
        let widths = col_widths();
        let distinct: std::collections::BTreeSet<u32> = widths.iter().copied().collect();
        assert!(
            distinct.len() >= 3,
            "column widths must be unequal so no uniform pitch describes the axis, got {distinct:?}",
        );
    }

    #[test]
    fn row_cells_name_both_coordinates() {
        let cells = row_cells(42);
        assert_eq!(cells.len(), NCOLS, "the row builder covers every column");
        assert_eq!(cells[0], "r42c0");
        assert_eq!(cells[NCOLS - 1], format!("r42c{}", NCOLS - 1));
    }

    /// The defect this round closes, on the a11y axis: a windowed grid must
    /// expose one `gridcell` per **visible** column, not one per column.
    #[test]
    fn column_axis_is_windowed() {
        let nodes = run_access(WIN_W, 360, 0);
        let headers = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .count();
        assert!(
            headers < NCOLS / 4,
            "the header band must window: {headers} of {NCOLS} columnheaders for a {WIN_W}px \
             viewport",
        );
        assert!(headers > 0, "some columns must be present");
    }

    /// The extent survives the windowing: an AT must still learn that the
    /// grid is 200 columns wide and that a present cell is column *k* of
    /// those 200.
    #[test]
    fn column_extent_survives_windowing() {
        let nodes = run_access(WIN_W, 360, 0);
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(
            nodes[0].column_count,
            Some(u32::try_from(NCOLS).unwrap()),
            "aria-colcount conveys the FULL 200-column extent",
        );
        let cells: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::GridCell)
            .collect();
        assert!(!cells.is_empty(), "windowed cells present");
        for cell in &cells {
            assert!(
                cell.column_index.is_some(),
                "every windowed gridcell carries its absolute aria-colindex",
            );
        }
    }

    /// Scrolling the column window moves which columns exist — the witness
    /// that the window is derived from `offset_x`, not fixed at the left
    /// edge.
    #[test]
    fn scrolling_moves_the_column_window() {
        fn header_indices(nodes: &[AccessNode]) -> Vec<u32> {
            nodes
                .iter()
                .filter(|n| n.role == AriaRole::ColumnHeader)
                .filter_map(|n| n.column_index)
                .collect()
        }
        let at_left = header_indices(&run_access(WIN_W, 360, 0));
        let scrolled = header_indices(&run_access(WIN_W, 360, 6_000));
        assert!(!at_left.is_empty() && !scrolled.is_empty());
        assert_eq!(
            at_left.iter().min(),
            Some(&1),
            "at offset 0 the window starts at column 1 (1-based aria-colindex)",
        );
        assert!(
            scrolled.iter().min() > at_left.iter().max(),
            "a 6000px scroll moves the window entirely past its boot set: {at_left:?} -> \
             {scrolled:?}",
        );
    }

    /// The row axis is untouched by this round — its extent still reaches the
    /// AT through `aria-setsize`.
    #[test]
    fn row_extent_is_unchanged() {
        let nodes = run_access(WIN_W, 360, 0);
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(N).unwrap()),
            "grid setsize conveys the FULL 10,000-row dataset",
        );
    }
}
