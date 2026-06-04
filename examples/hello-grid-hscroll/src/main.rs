//! `hello-grid-hscroll` — R784 §5.45 **horizontal scroll + frozen header**.
//!
//! A 10,000-row Material-3 data-grid whose **8 columns are wider than the
//! window**, so the grid scrolls *horizontally* as well as vertically. The
//! header band and the body share one outer
//! [`ScrollAxis::Horizontal`](pinion_core::scene::ScrollAxis) scroll, so the
//! header tracks the body's horizontal offset exactly — it can never drift
//! out of column alignment — while staying **vertically pinned** above the
//! inner vertical body scroll (the R784 nested single-axis composition:
//! outer horizontal over `[frozen header, inner vertical body]`).
//!
//! This is the consumer that motivates the R784 layout substrate: before
//! R784 the scroll layout pass only unbounded the *height* axis, so scroll
//! content could never overflow its viewport width and `max_x` was always
//! 0. `ScrollAxis::Horizontal` makes the pass unbound the *width* axis so a
//! `NCOLS × COL_W`-wide column overflows a narrower window and the layout
//! pass writes the overflow into the outer scroll's `max_x`.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/set_scroll_offset` on the outer horizontal scroll's tag
//! (`ghs_hscroll`) slides the whole column; `scene/snapshot` then reports
//! the header cells AND the body cells shifted left by the same amount
//! (frozen-header horizontal sync), while their vertical positions are
//! unchanged (the header is pinned). Vertical `scene/set_scroll_offset` on
//! the body scroll (`ghs_scroll`) slides the rows but never the header. No
//! pixels required (see `tools/r784_grid_hscroll.py`).
//!
//! ## a11y (WAI-ARIA virtualized grid)
//!
//! Identical topology to `hello-virtual-table` — one `AriaRole::Grid` over
//! the header `row` + the windowed data `row`s (`windowed_grid_nodes`); the
//! horizontal scroll is a viewport concern, orthogonal to the row/column
//! structure the a11y tree conveys.

use std::rc::Rc;

use pinion_a11y::{windowed_grid_nodes, AccessNode, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{BoxStyle, FlexDirection, LayoutStyle};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::column_widths::{
    column_resize_externals, use_column_widths, ColumnWidthExternal,
};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_widget_paint::table::{view_virtual_table, GridScroll, TableStyle, VirtualTableData};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridHscrollRenderer, HelloGridHscrollRendererError);

/// Initial window size. Deliberately narrower than `NCOLS × COL_W` so the
/// columns overflow and the grid scrolls horizontally.
const WIN_W: u32 = 520;
const WIN_H: u32 = 440;
/// Shared [`ThemeProvider`] cache key.
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 8;
/// Uniform column width; `NCOLS × COL_W = 1040 > WIN_W` so the grid scrolls
/// horizontally and the frozen header tracks the body's horizontal offset.
const COL_W: u32 = 130;
/// Data-row height (must match the windowing pitch used in `access_node`).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Column header labels — a process-table-style wide row.
const HEADERS: [&str; NCOLS] =
    ["PID", "Name", "CPU%", "Memory", "Threads", "Status", "User", "Started"];
/// Paint-root + a11y `grid` tag, and the [`StubExternal`] anchor tag.
const TABLE_TAG: &str = "ghs";
/// Cache key (and input-router tag) for the vertical body `ScrollState`.
const SCROLL_KEY: &str = "ghs_scroll";
/// R784 — cache key (and input-router tag) for the outer horizontal
/// `ScrollState`. `scene/set_scroll_offset` on this tag slides the grid
/// sideways; the header and body move together.
const H_SCROLL_KEY: &str = "ghs_hscroll";
/// R785 — cache key for the shared [`ColumnWidths`] model, and the tag of
/// the [`ColumnWidthExternal`] that drives it. `invoke set_col_width` on this
/// tag resizes a column; widening past the window grows the R784 horizontal
/// scroll extent.
const COLS_KEY: &str = "ghs_cols";

// Display-only grid: no widget state of its own (`type State = ()`).
// Repaints are driven by the theme + scroll-offset + measured-viewport
// reactive `Signal` subscriptions the view opens.

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        ..TableStyle::m3()
    }
}

/// Synthetic cell texts for a data row across all 8 columns. The
/// five-digit PID keeps every `scene/snapshot` cell unambiguous; the
/// other columns cycle so the eye (and the diff) has something to track
/// while scrolling either axis.
fn row_cells(id: usize) -> Vec<String> {
    const NAMES: [&str; 5] = ["alpha", "bravo", "charlie", "delta", "echo"];
    const STATUS: [&str; 3] = ["Idle", "Active", "Done"];
    const USERS: [&str; 4] = ["root", "daemon", "coin", "ai"];
    vec![
        format!("{id:05}"),
        format!("{}-{id}", NAMES[id % NAMES.len()]),
        format!("{}.{}", id % 100, id % 10),
        format!("{} MB", 16 + (id % 480)),
        format!("{}", 1 + (id % 32)),
        STATUS[id % STATUS.len()].to_string(),
        USERS[id % USERS.len()].to_string(),
        format!("T-{:04}", id % 1440),
    ]
}

/// view-fn (§6.3): pure sync `() -> Scene`. The dataset is virtual —
/// `view_virtual_table` invokes [`row_cells`] only for the indices in the
/// current vertical window; the horizontal axis is a viewport offset (no
/// column virtualization at 8 columns).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    // R785 — the shared per-column widths, seeded uniform so the boot layout
    // matches the R784 slice; `set_col_width` (via the external) widens a
    // column and this view re-runs with the new widths.
    let widths = use_column_widths(COLS_KEY, || vec![COL_W; NCOLS]);
    let col_widths = widths.widths();
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let grid = view_virtual_table(
        TABLE_TAG,
        GridScroll { body: &scroll, horizontal: &h_scroll },
        VirtualTableData {
            headers: &HEADERS,
            item_count: N,
            overscan: OVERSCAN,
            sort: None,
            sort_tag: None,
            order: None,
            col_widths: Some(&col_widths),
            // R786 — drag a column border to resize it live (the grabber is the
            // header cell's trailing edge); widening past the window grows the
            // R784 horizontal scroll. Driven by the per-column
            // `ColumnResizeExternal`s registered in `create_extra_externals`.
            resizable: true,
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

struct GridHscrollView;

impl WidgetCore for GridHscrollView {
    type State = ();
    type Event = ();

    /// Display-only: the only addressable anchor is the no-op
    /// [`StubExternal`] at [`TABLE_TAG`] (input router + a11y `grid`
    /// bounds). Wheel input routes to the body / horizontal `ScrollNode`s
    /// via their `ScrollState`s, no extra External needed.
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// R785 / R786 — the column-width axis surfaces, all over the **same**
    /// shared [`ColumnWidths`] the view reads via [`use_column_widths`]:
    ///
    /// * The [`ColumnWidthExternal`] at [`COLS_KEY`] — the AI-first RPC path
    ///   (`invoke set_col_width "<col>=<width>"` widens a column).
    /// * One [`ColumnResizeExternal`](pinion_core::widgets::column_widths::ColumnResizeExternal)
    ///   per column (via
    ///   [`column_resize_externals`]) registered at the header-cell tags
    ///   (`ghs_ch<col>`) — the **live-drag** path: grabbing a header border and
    ///   dragging it widens that column. Both mutate the one width `Signal`, so
    ///   a drag and an RPC `set_col_width` agree; the new total content width
    ///   grows the R784 horizontal scroll.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let widths = use_column_widths(COLS_KEY, || vec![COL_W; NCOLS]);
        // The resize handles normalize their pixel drag against the horizontal
        // scroll viewport (a stable width — the dragged cell resizes, the
        // viewport does not), so they share the grid's h-scroll state + tag.
        let h_scroll = use_scroll_state(H_SCROLL_KEY);
        let mut externals =
            vec![ExtraExternal::new(COLS_KEY, Box::new(ColumnWidthExternal::new(Rc::clone(&widths))))];
        externals.extend(column_resize_externals(TABLE_TAG, &widths, &h_scroll, H_SCROLL_KEY));
        externals
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

    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-grid-hscroll (R784 §5.45 horizontal scroll + frozen header)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for GridHscrollView {
    /// WAI-ARIA virtualized `grid` — same topology as `hello-virtual-table`
    /// (the horizontal scroll is a viewport concern, orthogonal to the
    /// row/column structure). Built by the shared
    /// `pinion_a11y::windowed_grid_nodes` so the virtualized-grid topology
    /// stays one source of truth.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        windowed_grid_nodes(
            TABLE_TAG,
            "Horizontally scrolling data grid",
            &HEADERS,
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
        )
    }
}

impl WidgetView for GridHscrollView {
    type Renderer = HelloGridHscrollRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridHscrollView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    #[test]
    fn columns_overflow_the_window() {
        // The premise of this example: the column total exceeds the window
        // width, so the grid genuinely scrolls horizontally.
        let total_w = u32::try_from(NCOLS).unwrap() * COL_W;
        assert!(
            total_w > WIN_W,
            "NCOLS×COL_W ({total_w}) must exceed WIN_W ({WIN_W})",
        );
    }

    #[test]
    fn row_cells_fill_all_columns() {
        let cells = row_cells(42);
        assert_eq!(cells.len(), NCOLS, "one cell per column");
        assert_eq!(cells[0], "00042", "PID is the zero-padded index");
    }

    #[test]
    fn grid_a11y_setsize_is_full_dataset() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 360);
            GridHscrollView::access_node(&(), None)
        });
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(N).unwrap()),
            "grid setsize conveys the FULL 10_000-row dataset",
        );
        let columnheaders = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .count();
        assert_eq!(columnheaders, NCOLS, "one columnheader per column");
    }
}
