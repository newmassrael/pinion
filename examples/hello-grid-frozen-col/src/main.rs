//! `hello-grid-frozen-col` — R859 §5.27 **frozen-left-column data-grid**.
//!
//! A 10,000-row Material-3 data-grid whose **first two columns are pinned**
//! against the horizontal scroll — the spreadsheet "frozen row-headers"
//! pattern a DCC asset-browser / scene-outliner needs: the identity columns
//! (PID, Name) stay visible while the metadata columns (CPU%, Memory, …)
//! scroll sideways. The whole grid still scrolls vertically; both the frozen
//! pane and the scrolling pane move in **vertical lockstep**.
//!
//! ## The substrate (R859 linked-scroll follower)
//!
//! The grid splits into two side-by-side panes:
//!
//! ```text
//! Row[
//!   FROZEN pane:  Column[ frozen_header(0..2),  Scroll(V, body, FOLLOWER)[ rows ] ]   // fixed width, no h-scroll
//!   SCROLLED pane: Scroll(H, h)[ Column[ header(2..),  Scroll(V, body, PRIMARY)[ rows ] ] ]  // R784 grid for cols 2..
//! ]
//! ```
//!
//! Both panes' vertical scrolls reference the **same** body
//! [`ScrollState`](pinion_core::widgets::scroll::ScrollState), so they slide
//! together. Only the scrolling pane's vertical scroll is the *primary* that
//! publishes its measured viewport / max bounds; the frozen pane's is a
//! [follower](pinion_core::scene::ScrollNode::as_follower). Without that
//! distinction the two panes' mismatched widths would flip-flop the shared
//! `measured_w` every frame and spin a perpetual scroll-dirty re-pass — the
//! reason real data grids (Qt `QTableView`, AG-Grid, Excel) link one
//! scrollbar to a passive follower rather than running two publishers.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/scroll` on the outer horizontal scroll's tag (`gfz_hscroll`)
//! slides the scrolling pane; `scene/snapshot` then reports the **scrolled**
//! columns shifted left while the **frozen** columns stay at their boot x —
//! the freeze, witnessed without pixels. `scene/scroll` on the body scroll
//! (`gfz_scroll`) slides BOTH panes' rows by the same amount (vertical
//! lockstep) while neither header moves. See `tools/demos/r859_frozen_col.py`.
//!
//! ## a11y (WAI-ARIA virtualized grid)
//!
//! The shared `pinion_a11y::windowed_grid_nodes_frozen` builds the
//! `AriaRole::Grid` over the (scrolling-pane) header `row` + the windowed data
//! `row`s; every `gridcell` (`gfz#<id>_<col>`) resolves its own bounds across
//! the split, so cell-level navigation is correct for all columns. R863 — each
//! header / data `row` additionally lists its frozen-pane strip
//! (`gfz_fhrow` / `gfz_frow<id>`) as a `bounds_union_tags` fragment, so the
//! shell resolves the **row container** bounds across both panes (the frozen
//! identity columns are inside the AT row bounds, not just the scrolling
//! pane's strip). This is the first `bounds_union_tags` substrate consumer
//! (the R860 tree-grid `row` is the second).

use pinion_a11y::{AccessNode, WidgetA11y, windowed_grid_nodes_frozen};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{BoxStyle, FlexDirection, LayoutStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{GridScroll, TableStyle, VirtualTableData, view_virtual_table};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridFrozenColRenderer, HelloGridFrozenColRendererError);

/// Initial window size. Narrower than `NCOLS × COL_W` so the metadata
/// columns overflow and the scrolling pane scrolls horizontally; wide
/// enough that the frozen pane (`FROZEN_COLS × COL_W`) plus a couple of
/// scrolling columns are on-screen at rest.
const WIN_W: u32 = 560;
const WIN_H: u32 = 440;
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key.
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 8;
/// R859 — number of leading columns to freeze (PID + Name = the identity
/// columns a row-header pin keeps visible).
const FROZEN_COLS: usize = 2;
/// Uniform column width; `NCOLS × COL_W = 1040 > WIN_W` so the scrolling
/// pane scrolls horizontally while the frozen pane stays put.
const COL_W: u32 = 130;
/// Data-row height (must match the windowing pitch used in `access_node`).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Column header labels — a process-table-style wide row; the first two
/// (PID, Name) are the frozen identity columns.
const HEADERS: [&str; NCOLS] = [
    "PID", "Name", "CPU%", "Memory", "Threads", "Status", "User", "Started",
];
/// Paint-root + a11y `grid` tag, and the [`StubExternal`] anchor tag.
const TABLE_TAG: &str = "gfz";
/// Cache key (and input-router tag) for the vertical body `ScrollState`,
/// shared by the frozen (follower) + scrolling (primary) panes.
const SCROLL_KEY: &str = "gfz_scroll";
/// Cache key (and input-router tag) for the scrolling pane's horizontal
/// `ScrollState`. `scene/scroll` on this tag slides the metadata columns;
/// the frozen columns never move.
const H_SCROLL_KEY: &str = "gfz_hscroll";

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
/// five-digit PID + the `Name` column are the frozen identity columns; the
/// scrolling columns cycle so the eye (and the diff) has something to track
/// while scrolling sideways.
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
/// current vertical window (per pane); the horizontal axis is a viewport
/// offset on the scrolling pane (no column virtualization at 8 columns).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
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
            headers: &HEADERS,
            item_count: N,
            overscan: OVERSCAN,
            sort: None,
            sort_tag: None,
            order: None,
            col_widths: None,
            resizable: false,
            // R859 — freeze the first two (identity) columns; columns 2..
            // scroll horizontally under the shared `h_scroll`.
            frozen_cols: FROZEN_COLS,
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

struct GridFrozenColView;

impl WidgetCore for GridFrozenColView {
    type State = ();
    type Event = ();

    /// Display-only: the only addressable anchor is the no-op
    /// [`StubExternal`] at [`TABLE_TAG`] (input router + a11y `grid`
    /// bounds). Wheel / arrow input routes to the body / horizontal
    /// `ScrollNode`s via their `ScrollState`s, no extra External needed.
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
        "pinion hello-grid-frozen-col (R859 §5.27 frozen-left-column)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for GridFrozenColView {
    /// R863 — WAI-ARIA virtualized frozen-split `grid` (the shared
    /// `pinion_a11y::windowed_grid_nodes_frozen`) over the header `row` + the
    /// windowed data `row`s. Each `gridcell` (`gfz#<id>_<col>`) resolves its own
    /// bounds per-pane, and each header / data `row` now spans **both** panes
    /// via `bounds_union_tags` (the scrolling strip ∪ the `gfz_fhrow` /
    /// `gfz_frow<id>` frozen strip), so the AT row bounds cover the frozen
    /// identity columns too.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        windowed_grid_nodes_frozen(
            TABLE_TAG,
            "Frozen-column data grid",
            &HEADERS,
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
        )
    }
}

impl WidgetView for GridFrozenColView {
    type Renderer = HelloGridFrozenColRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridFrozenColView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    #[test]
    fn columns_overflow_the_window() {
        // The premise: the column total exceeds the window width, so the
        // scrolling pane genuinely scrolls horizontally past the freeze.
        let total_w = u32::try_from(NCOLS).unwrap() * COL_W;
        assert!(
            total_w > WIN_W,
            "NCOLS×COL_W ({total_w}) must exceed WIN_W ({WIN_W})",
        );
    }

    #[test]
    fn frozen_pane_fits_inside_the_window() {
        // The frozen pane must not itself overflow — at least one scrolling
        // column has to be on-screen beside it for the demo to mean anything.
        let frozen_w = u32::try_from(FROZEN_COLS).unwrap() * COL_W;
        assert!(
            frozen_w < WIN_W,
            "frozen pane ({frozen_w}) must leave room for scrolling columns in WIN_W ({WIN_W})",
        );
    }

    #[test]
    fn row_cells_fill_all_columns() {
        let cells = row_cells(42);
        assert_eq!(cells.len(), NCOLS, "one cell per column");
        assert_eq!(
            cells[0], "00042",
            "PID is the zero-padded index (frozen col 0)"
        );
    }

    #[test]
    fn grid_a11y_setsize_is_full_dataset() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 360);
            GridFrozenColView::access_node(&(), None)
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
        assert_eq!(
            columnheaders, NCOLS,
            "one columnheader per column across the split"
        );
    }
}
