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
//! ## What the binding is asked for (R1524, R1530)
//!
//! Windowing the *tree* is not windowing the *work*. Two readouts publish what
//! this frame's model was asked for, because neither is visible in the pixels:
//! `vcol_status` counts cell requests ([`CellIndex`]-addressed, R1524) and
//! `vcol_hstatus` counts header-section requests (R1530). Each is held against
//! an independent observable — the cells and the header cells actually in the
//! tree — and their equality is what "asks for what it paints" means on that
//! axis (`tools/demos/r1524_cells_asked_for.py`,
//! `tools/demos/r1530_headers_asked_for.py`).
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
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::column_widths::use_column_widths;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{
    CellIndex, GridModel, GridScroll, TableStyle, VirtualTableData, no_decoration,
    view_virtual_table,
};
use std::cell::Cell;

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
/// Height of the R1524 request-count status band.
const STATUS_H: u32 = 40;
/// Tag of the status readout — the `scene/snapshot` slot the demo reads the
/// per-frame cell-request count from.
const STATUS_TAG: &str = "vcol_status";
/// R1530 — tag of the header-request readout, the `scene/snapshot` slot the
/// demo reads the per-frame **section**-request count from.
const HEADER_STATUS_TAG: &str = "vcol_hstatus";

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

/// The label of **one column** — the R1530 per-section contract (Qt
/// `headerData(section, Qt::Horizontal, Qt::DisplayRole)`), and the SSOT for
/// this example's column names.
///
/// Generated rather than a literal array: 200 hand written labels would be
/// noise, and the zero-padded index makes every `scene/snapshot` header
/// unambiguous about *which* column survived the window.
///
/// Pre-R1530 this was a `Vec<String>` of all [`NCOLS`] labels, built twice a
/// frame — once for the paint and once for the a11y tree — of which the grid
/// kept the ~5 the viewport exposes.
fn header_text(col: usize) -> String {
    HEADER_REQUESTS.with(|n| n.set(n.get() + 1));
    format!("C{col:03}")
}

fn table_style() -> TableStyle {
    TableStyle {
        row_height: ROW_H,
        ..TableStyle::m3()
    }
}

thread_local! {
    /// R1524 — how many cells the grid asked for while building the current
    /// frame.
    ///
    /// This round changes *who builds what*, not what is painted, so the
    /// painted scene alone cannot witness it: the same cells appear on screen
    /// either way. The count is therefore published into the scene, where
    /// `scene/snapshot` reads it with no pixels (§2 #7) — the readout pattern
    /// `hello-grid-multi-select` uses for its selection cardinality.
    ///
    /// It does not compromise view-fn purity (§6.3): [`view`] zeroes it on
    /// entry and reads it after the grid — its only writer — has been built,
    /// so the Scene stays a deterministic function of state and a repeated
    /// pass (a `dry_run`, an introspection pass) yields the identical value.
    static CELL_REQUESTS: Cell<usize> = const { Cell::new(0) };

    /// R1530 — how many column headers the grid asked for while building the
    /// current frame.
    ///
    /// A separate counter from [`CELL_REQUESTS`], not a sum: the two axes cost
    /// differently (cells scale with both windows, headers with one), and a
    /// single total would let a regression on either hide inside the other.
    static HEADER_REQUESTS: Cell<usize> = const { Cell::new(0) };
}

/// The synthetic text of **one cell** — the R1524 per-cell contract
/// ([`CellIndex`]), and the SSOT for this example's dataset.
///
/// `r<row>c<col>` names both coordinates, so a snapshot cell proves which
/// (row, column) pair the tree actually holds — the assertion the demo needs
/// on both axes at once. Pre-R1524 this was a `Vec<String>` of all [`NCOLS`]
/// columns per row, of which the grid kept the ~5 the viewport exposed.
fn cell_text(c: CellIndex) -> String {
    CELL_REQUESTS.with(|n| n.set(n.get() + 1));
    format!("r{}c{}", c.row, c.col)
}

/// The R1524 readout: how many cells the grid asked for, against the extent it
/// asked them from.
///
/// `asked` is deliberately the only *measured* number here — the window sizes
/// are not restated, because the grid already derives them and a second
/// statement could drift from it (the reason this example's `total_width` is
/// test-only). The demo instead cross-checks `asked` against an independent
/// observable: how many cells the snapshot actually holds. Equality of those
/// two is exactly what "the grid asks for the cells it paints" means.
fn status_bar(asked: usize, headers: usize, theme: &Theme) -> Scene {
    let readout = |content: String, tag: &'static str| {
        Scene::Text(
            TextNode::styled(
                content,
                Rect::default(),
                TextStyle::new()
                    .with_size_px(13)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            )
            .with_tag(tag),
        )
    };
    // Two readouts, not one string: the cell count is R1524's observable and
    // the header count is R1530's, and a demo that had to parse both out of one
    // line would couple the two rounds' gates to each other's formatting.
    let text = readout(
        format!("asked {asked} cells \u{00B7} table {N}\u{00D7}{NCOLS}"),
        STATUS_TAG,
    );
    let htext = readout(
        format!("\u{00B7} asked {headers} headers"),
        HEADER_STATUS_TAG,
    );
    Scene::Container(
        ContainerNode::new(vec![text, htext])
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

/// view-fn (§6.3): pure sync `() -> Scene`. Both axes are virtual —
/// `view_virtual_table` builds cells only for the windowed rows *and* the
/// windowed columns, both windows derived from the runtime-measured
/// viewport, and R1524 — it *asks* [`cell_text`] for only those.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let widths = use_column_widths(COLS_KEY, col_widths);
    let width_snapshot = widths.widths();
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    CELL_REQUESTS.with(|n| n.set(0));
    HEADER_REQUESTS.with(|n| n.set(0));
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
            col_widths: Some(&width_snapshot),
            resizable: false,
            frozen_cols: 0,
            row_style: None,
            delegate: None,
        },
        &theme,
        &style,
        |_| false, // display-only grid: no selection
        GridModel {
            cell: cell_text,
            header: header_text,
            decoration: no_decoration,
        },
    );
    // Read AFTER the grid is built: `cell_text` / `header_text` are their
    // counters' only writers, so these are this frame's counts, not the
    // previous frame's.
    let asked = CELL_REQUESTS.with(Cell::get);
    let asked_headers = HEADER_REQUESTS.with(Cell::get);

    Scene::Container(
        ContainerNode::new(vec![status_bar(asked, asked_headers, &theme), grid])
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
        // R1530 — the AT pass asks the same per-section accessor the paint pass
        // does, for the same window. It used to build all 200 labels here too,
        // so an introspection frame carried the defect a second time; and the
        // labels an AT reads now come from the function the pixels came from.
        let labels: Vec<String> = cols.indices().map(header_text).collect();
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        windowed_grid_nodes_wide(
            TABLE_TAG,
            "Wide data grid",
            &label_refs,
            NCOLS,
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
    fn cell_text_names_both_coordinates() {
        assert_eq!(cell_text(CellIndex { row: 42, col: 0 }), "r42c0");
        assert_eq!(
            cell_text(CellIndex {
                row: 42,
                col: NCOLS - 1,
            }),
            format!("r42c{}", NCOLS - 1),
        );
    }

    /// What one frame asked its model for, on each axis.
    ///
    /// A struct, not a `(usize, usize)`: the two counts are the same type, so a
    /// positional pair silently accepts them swapped — the reason
    /// [`CellIndex`] names its coordinates.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Asked {
        cells: usize,
        sections: usize,
    }

    /// Render the view with both viewports measured, as the shell does after
    /// the first layout pass, and report what it asked for.
    fn run_view(measured_w: u32, measured_h: u32, offset_x: i32) -> (Scene, Asked) {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(measured_w, measured_h);
            let h_scroll = use_scroll_state(H_SCROLL_KEY);
            h_scroll.set_measured_viewport(measured_w, measured_h);
            let extent = total_width().saturating_sub(measured_w);
            h_scroll.set_max(i32::try_from(extent).unwrap_or(i32::MAX), 0);
            h_scroll.scroll_to(offset_x, 0);
            let scene = view((), &Frame::default());
            let asked = Asked {
                cells: CELL_REQUESTS.with(Cell::get),
                sections: HEADER_REQUESTS.with(Cell::get),
            };
            (scene, asked)
        })
    }

    /// The text of the readout tagged `tag`, or `None`.
    fn readout(scene: &Scene, tag: &str) -> Option<String> {
        match scene {
            Scene::Text(t) if t.tag.as_deref() == Some(tag) => Some(t.content.to_string()),
            Scene::Container(c) => c.children.iter().find_map(|ch| readout(ch, tag)),
            Scene::Scroll(s) => readout(s.content.as_ref(), tag),
            _ => None,
        }
    }

    /// The label painted under header section `col`, or `None` if that section
    /// is not in the tree.
    fn section_label(scene: &Scene, col: usize) -> Option<String> {
        fn first_text(scene: &Scene) -> Option<String> {
            match scene {
                Scene::Text(t) => Some(t.content.to_string()),
                Scene::Container(c) => c.children.iter().find_map(first_text),
                Scene::Scroll(s) => first_text(s.content.as_ref()),
                _ => None,
            }
        }
        fn walk(scene: &Scene, want: &str) -> Option<String> {
            match scene {
                Scene::Container(c) if c.tag.as_deref() == Some(want) => first_text(scene),
                Scene::Container(c) => c.children.iter().find_map(|ch| walk(ch, want)),
                Scene::Scroll(s) => walk(s.content.as_ref(), want),
                _ => None,
            }
        }
        walk(scene, &format!("{TABLE_TAG}_ch{col}"))
    }

    /// The **header** cells in the painted tree, as absolute column indices.
    fn painted_sections(scene: &Scene) -> Vec<usize> {
        fn col_of(tag: &str) -> Option<usize> {
            tag.strip_prefix(&format!("{TABLE_TAG}_ch"))?.parse().ok()
        }
        fn walk(scene: &Scene, out: &mut Vec<usize>) {
            match scene {
                Scene::Container(c) => {
                    if let Some(col) = c.tag.as_deref().and_then(col_of) {
                        out.push(col);
                    }
                    for child in &c.children {
                        walk(child, out);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    /// Every **data** cell in the painted tree, as `(row, col)`.
    ///
    /// The `{tag}#…` composite space also holds the header band's cells
    /// (`{tag}#h{col}`), which no data request corresponds to, so membership is
    /// decided by the `<row>_<col>` coordinate shape rather than by the `#`
    /// prefix alone — a prefix-only filter counts the header row as a 13th data
    /// row and reports a 7-cell surplus that is really a units mismatch.
    fn painted_cells(scene: &Scene) -> Vec<(usize, usize)> {
        fn coords(tag: &str) -> Option<(usize, usize)> {
            let (row, col) = tag
                .strip_prefix(&format!("{TABLE_TAG}#"))?
                .split_once('_')?;
            Some((row.parse().ok()?, col.parse().ok()?))
        }
        fn walk(scene: &Scene, out: &mut Vec<(usize, usize)>) {
            match scene {
                Scene::Container(c) => {
                    if let Some(rc) = c.tag.as_deref().and_then(coords) {
                        out.push(rc);
                    }
                    for child in &c.children {
                        walk(child, out);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    /// **The defect R1524 closes.** R1523 windowed which cells reach the scene
    /// tree; the grid still *asked* for every column of every windowed row and
    /// discarded all but the window. So the two counts disagreed by the same
    /// ~40x the tree had just been relieved of.
    ///
    /// The assertion is their **equality**, not a threshold: "the grid asks for
    /// the cells it paints" is exactly that, and equality also cannot be
    /// satisfied by asking for too *few* (a window bug), which a `<=` bound
    /// would wave through.
    #[test]
    fn r1524_asks_for_exactly_the_cells_it_paints() {
        let (scene, asked) = run_view(WIN_W, 360, 0);
        let painted = painted_cells(&scene);
        assert!(!painted.is_empty(), "the window must hold some cells");
        assert_eq!(
            asked.cells,
            painted.len(),
            "asked for {} cells but painted {} — every requested cell must be a \
             painted one and vice versa",
            asked.cells,
            painted.len(),
        );
    }

    /// The same equality at a scrolled offset and a resized viewport: the
    /// contract is not an artifact of the boot window (where the column window
    /// starts at 0 and a lead pad is absent).
    #[test]
    fn r1524_equality_holds_across_offsets_and_sizes() {
        for (w, h, offset) in [(WIN_W, 360, 6_000), (WIN_W, 200, 12_345), (1_100, 500, 0)] {
            let (scene, asked) = run_view(w, h, offset);
            let painted = painted_cells(&scene).len();
            assert!(painted > 0, "cells present at {w}x{h} offset {offset}");
            assert_eq!(
                asked.cells, painted,
                "asked {} != painted {painted} at {w}x{h} offset {offset}",
                asked.cells,
            );
        }
    }

    /// The magnitude the equality buys, stated against the pre-R1524 cost: the
    /// grid asks for a small multiple of the *window*, not of [`NCOLS`]. Guards
    /// the case where a future change reintroduces full-row production while
    /// keeping the tree windowed — the counts would still be equal only if the
    /// discarded cells were also painted, so this pins the absolute scale.
    #[test]
    fn r1524_requests_scale_with_the_window_not_the_table() {
        let (scene, asked) = run_view(WIN_W, 360, 0);
        let rows: std::collections::BTreeSet<usize> =
            painted_cells(&scene).iter().map(|&(row, _)| row).collect();
        let row_count = rows.len();
        assert!(row_count > 0, "some rows are windowed");
        let full_row_cost = row_count * NCOLS;
        assert!(
            asked.cells * 10 < full_row_cost,
            "asking {} for {row_count} windowed rows must be an order of magnitude \
             below the {full_row_cost} a per-row builder cost",
            asked.cells,
        );
    }

    /// The AI-first witness (§2 #7): the count reaches `scene/snapshot`, so an
    /// agent can observe the round's claim with no pixels and no profiler.
    #[test]
    fn r1524_status_readout_publishes_the_request_count() {
        let (scene, asked) = run_view(WIN_W, 360, 0);
        let text = readout(&scene, STATUS_TAG).expect("status readout present");
        assert_eq!(
            text,
            format!(
                "asked {} cells \u{00B7} table {N}\u{00D7}{NCOLS}",
                asked.cells
            ),
            "the readout states this frame's count and the full extent it came from",
        );
    }

    // ── R1530 per-section header contract ───────────────────────────

    #[test]
    fn header_text_names_its_section() {
        assert_eq!(header_text(0), "C000");
        assert_eq!(header_text(137), "C137");
        assert_eq!(header_text(NCOLS - 1), format!("C{:03}", NCOLS - 1));
    }

    /// **The defect R1530 closes.** R1523 windowed which header cells reach the
    /// tree, but the labels arrived as a slice of every column, because
    /// `VirtualTableData` read its column count off that slice's length — so
    /// this binding built 200 `String`s a frame to paint five, and built them
    /// again for the a11y pass.
    ///
    /// Equality, like its R1524 cell peer, and for the same reason: a `<=`
    /// bound would pass a grid that asked for too few and painted blanks.
    #[test]
    fn r1530_asks_for_exactly_the_sections_it_paints() {
        let (scene, asked) = run_view(WIN_W, 360, 0);
        let painted = painted_sections(&scene);
        assert!(!painted.is_empty(), "the window must hold header cells");
        assert_eq!(
            asked.sections,
            painted.len(),
            "asked for {} sections but painted {} of them",
            asked.sections,
            painted.len(),
        );
    }

    /// The same equality at a scrolled offset and a resized viewport, and the
    /// label under each painted section is the one that section was asked for —
    /// which the counts alone cannot see (a pane asking with a pane-relative
    /// index would ask for exactly as many as it paints).
    #[test]
    fn r1530_sections_are_answered_by_address_across_offsets_and_sizes() {
        for (w, h, offset) in [(WIN_W, 360, 6_000), (WIN_W, 200, 12_345), (1_100, 500, 0)] {
            let (scene, asked) = run_view(w, h, offset);
            let painted = painted_sections(&scene);
            assert!(
                !painted.is_empty(),
                "headers present at {w}x{h} offset {offset}"
            );
            assert_eq!(
                asked.sections,
                painted.len(),
                "asked {} != painted {} at {w}x{h} offset {offset}",
                asked.sections,
                painted.len(),
            );
            for col in painted {
                assert_eq!(
                    section_label(&scene, col).as_deref(),
                    Some(header_text(col).as_str()),
                    "section {col} carries its own label at {w}x{h} offset {offset}",
                );
            }
        }
    }

    /// The magnitude, stated against the pre-R1530 cost: one label per column
    /// in the *table* was what this binding produced at every viewport.
    #[test]
    fn r1530_section_requests_scale_with_the_window_not_the_table() {
        let (_, asked) = run_view(WIN_W, 360, 0);
        assert!(
            asked.sections * 10 < NCOLS,
            "asking for {} of {NCOLS} sections must be an order of magnitude below \
             the whole-table slice",
            asked.sections,
        );
    }

    /// The two axes are counted separately, and the header axis tracks only the
    /// column window: a shorter viewport changes the cell count and leaves the
    /// section count alone. A single summed counter could not show this.
    #[test]
    fn r1530_section_count_tracks_the_column_window_only() {
        let (_, tall) = run_view(WIN_W, 360, 0);
        let (_, short) = run_view(WIN_W, 200, 0);
        assert!(
            short.cells < tall.cells,
            "a shorter viewport paints fewer cells: {} -> {}",
            tall.cells,
            short.cells,
        );
        assert_eq!(
            short.sections, tall.sections,
            "but the same columns, so the same sections",
        );
        let (_, wide) = run_view(1_100, 360, 0);
        assert!(
            wide.sections > tall.sections,
            "a wider viewport exposes more columns: {} -> {}",
            tall.sections,
            wide.sections,
        );
    }

    /// The AI-first witness (§2 #7) for this axis: the section count reaches
    /// `scene/snapshot` in its own readout, so the two rounds' claims are
    /// observable independently.
    #[test]
    fn r1530_status_readout_publishes_the_section_count() {
        let (scene, asked) = run_view(WIN_W, 360, 0);
        let text = readout(&scene, HEADER_STATUS_TAG).expect("header readout present");
        assert_eq!(
            text,
            format!("\u{00B7} asked {} headers", asked.sections),
            "the readout states this frame's section count",
        );
    }

    /// The introspection pass carries the same contract as the paint pass: it
    /// asks for its window and no more. Before R1530 it built all 200 labels
    /// here too — the defect a paint-only counter would not have seen.
    #[test]
    fn r1530_access_pass_asks_only_for_its_window() {
        HEADER_REQUESTS.with(|n| n.set(0));
        let nodes = run_access(WIN_W, 360, 6_000);
        let asked = HEADER_REQUESTS.with(Cell::get);
        let columnheaders = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .count();
        assert!(columnheaders > 0, "the AT tree holds columnheaders");
        assert_eq!(
            asked, columnheaders,
            "the AT pass asked for {asked} sections and describes {columnheaders}",
        );
        assert!(
            asked * 10 < NCOLS,
            "and that is an order of magnitude below the {NCOLS} it used to build",
        );
        assert_eq!(
            nodes[0].column_count,
            Some(u32::try_from(NCOLS).unwrap()),
            "while the declared extent is still the whole table",
        );
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
