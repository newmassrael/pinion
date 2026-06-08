//! `hello-virtual-table` — R775 §5.27 **virtualized data-grid**.
//!
//! A 10,000-row Material-3 data table with a **frozen header** above a
//! flex-viewport (`AutoSizer`) **virtualized body**: the body's
//! [`ScrollNode`](pinion_core::scene::ScrollNode) flex-grows to fill the
//! window below the header and materializes cell rows for **only** the
//! window the runtime-measured viewport height exposes. Resize the window
//! taller and more rows appear; scroll the wheel and the band slides —
//! the same `AutoSizer` windowing as `hello-flex-virtual-list` (R774), but
//! each slot is a multi-column data row.
//!
//! This composes the R774 windowing substrate with the R730 column model
//! ([`pinion_widget_paint::table::view_virtual_table`]) — the DCC / IDE
//! data-grid every Phase-B inspector needs, at a scale eager rendering
//! cannot reach.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` over the painted grid reports the header row plus
//! ~`measured_viewport / row_height` data-row strips — even though
//! `N = 10_000`. Drive `scene/resize` taller, snapshot again, and the
//! rendered row count has grown; `scene/wheel` slides the band to a higher
//! index range while the header stays put. No pixels required (see
//! `tools/r775_virtual_table.py`).
//!
//! ## a11y (WAI-ARIA virtualized grid)
//!
//! One `AriaRole::Grid` claims the header `row` + the windowed data `row`
//! tags; each column header is a `columnheader`; each windowed data row is
//! a `row` carrying its absolute `aria-posinset` + `aria-setsize = N`, with
//! one `gridcell` per column. The rendered subset tracks the measured
//! viewport — exactly the visible set a sighted user sees.
//!
//! First slice (R775): display-only, columns fit the window width (no
//! horizontal scroll), no sort / selection / scrollbar peer — those are
//! follow-ups, mirroring the R744 → R746 list arc.

use pinion_a11y::{windowed_grid_nodes, AccessNode, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{BoxStyle, FlexDirection, LayoutStyle};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_widget_paint::table::{view_virtual_table, GridScroll, TableStyle, VirtualTableData};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloVirtualTableRenderer, HelloVirtualTableRendererError);

/// Initial window size — freely resizable; the grid body re-windows on
/// every `Resized` event. Wide enough that `NCOLS × COL_W` fits without
/// horizontal scroll.
const WIN_W: u32 = 400;
const WIN_H: u32 = 480;
/// Shared [`ThemeProvider`] cache key.
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small
/// and tracks the window height.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 3;
/// Uniform column width; `NCOLS × COL_W = 330 < WIN_W` so no h-scroll.
const COL_W: u32 = 110;
/// Data-row height (must match the windowing pitch used in `access_node`).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Column header labels.
const HEADERS: [&str; NCOLS] = ["Index", "Name", "Status"];
/// Paint-root + a11y `grid` tag, and the [`StubExternal`] anchor tag.
const TABLE_TAG: &str = "vtbl";
/// Cache key for the body scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "vtbl_scroll";
/// R784 — the outer horizontal scroll's `ScrollState` cache key. The
/// three columns here fit the window, so `max_x` stays 0 (no horizontal
/// scroll); the wiring is present for parity with the wide-grid demo.
const H_SCROLL_KEY: &str = "vtbl_hscroll";

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

/// Synthetic cell texts for a data row. The five-digit index keeps every
/// `scene/snapshot` cell unambiguous; the category cycles so the eye has
/// something to track while scrolling.
fn row_cells(id: usize) -> Vec<String> {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    const STATUS: [&str; 3] = ["Idle", "Active", "Done"];
    vec![
        format!("{id:05}"),
        CATEGORIES[id % CATEGORIES.len()].to_string(),
        STATUS[id % STATUS.len()].to_string(),
    ]
}

/// view-fn (§6.3): pure sync `() -> Scene`. The dataset is virtual —
/// `view_virtual_table` invokes [`row_cells`] only for the indices in the
/// current window, whose *size* is the runtime-measured viewport height.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
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
            col_widths: None,
            resizable: false,
            frozen_cols: 0,
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

struct VirtualTableView;

impl WidgetCore for VirtualTableView {
    type State = ();
    type Event = ();

    /// The grid is display-only this slice — the only addressable anchor
    /// is the no-op [`StubExternal`] at [`TABLE_TAG`] (input router + a11y
    /// `grid` bounds). Wheel scroll routes to the body `ScrollNode` via its
    /// `ScrollState`, no extra External needed.
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

    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-virtual-table (R775 §5.27 virtualized data-grid)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for VirtualTableView {
    /// WAI-ARIA virtualized `grid`: one [`AriaRole::Grid`] claiming the
    /// header `row` + the windowed data `row` tags; per-column
    /// `columnheader`; each windowed data row a `row` with its absolute
    /// `aria-posinset` + `aria-setsize = N` and one `gridcell` per column.
    /// The windowing source is the same `compute_visible_range` over the
    /// measured viewport the view fn uses, so the a11y tree and the painted
    /// tree never disagree on which rows exist. Built by the shared
    /// `pinion_a11y::windowed_grid_nodes` (R777 lift — the display-only
    /// peer of `windowed_grid_nodes_selected`), shared with `hello-grid-nav`
    /// so the virtualized-grid topology is one source of truth.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        windowed_grid_nodes(
            TABLE_TAG,
            "Virtual data grid",
            &HEADERS,
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
        )
    }
}

impl WidgetView for VirtualTableView {
    type Renderer = HelloVirtualTableRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<VirtualTableView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    fn run_access(measured_h: u32) -> Vec<AccessNode> {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, measured_h);
            VirtualTableView::access_node(&(), None)
        })
    }

    #[test]
    fn grid_setsize_is_full_dataset() {
        let nodes = run_access(360);
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(N).unwrap()),
            "grid setsize conveys the FULL 10_000-row dataset",
        );
    }

    #[test]
    fn header_row_then_columnheaders_present() {
        let nodes = run_access(360);
        assert_eq!(nodes[1].role, AriaRole::Row, "header row follows the grid");
        assert_eq!(nodes[1].tag, format!("{TABLE_TAG}_hrow"));
        let columnheaders = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .count();
        assert_eq!(columnheaders, NCOLS, "one columnheader per column");
    }

    #[test]
    fn rendered_rows_track_measured_viewport_and_window_dataset() {
        let short = run_access(360);
        let tall = run_access(720);
        let count_rows = |nodes: &[AccessNode]| {
            nodes.iter().filter(|n| n.role == AriaRole::Row).count()
        };
        // Header row + data rows; subtract the 1 header row for the body.
        let short_body = count_rows(&short) - 1;
        let tall_body = count_rows(&tall) - 1;
        assert!(tall_body > short_body, "taller viewport => more body rows");
        assert!(tall_body < 40, "windowed, not {N}: {tall_body}");
        // Every body row carries gridcells.
        let cells = tall.iter().filter(|n| n.role == AriaRole::GridCell).count();
        assert_eq!(cells, tall_body * NCOLS, "NCOLS gridcells per windowed row");
    }

    #[test]
    fn row_cells_are_indexed_and_categorized() {
        assert_eq!(row_cells(0), vec!["00000", "Alpha", "Idle"]);
        assert_eq!(row_cells(42), vec!["00042", "Charlie", "Idle"]);
    }

    #[test]
    fn set_measured_viewport_seeds_a_nonempty_window() {
        // Sanity: a measured viewport yields a posinset-1 first body row.
        let nodes = run_access(360);
        let first_body = nodes
            .iter()
            .find(|n| n.role == AriaRole::Row && n.tag != format!("{TABLE_TAG}_hrow"));
        let first = first_body.expect("at least one body row");
        assert_eq!(first.position_in_set, Some(1), "top window starts at posinset 1");
    }
}
