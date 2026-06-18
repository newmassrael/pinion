//! `hello-grid-multifilter` — R997 §5.40 **multi-facet predicate filter at scale**.
//!
//! R783 (`hello-grid-filter`) lands a **single** exact-match column filter over
//! a 10 000-row virtualized data-grid (`set_filter "2=Active"`). A
//! Wireshark / dlt-class viewer needs more: a **conjunction** of column
//! predicates with comparison operators — *Name contains "Alpha" AND Score ≥
//! 500 AND Status = Active*. This binding closes that Model/View-at-scale
//! campaign slice: the R783 `GridFilter` is now a list of
//! [`ColumnFacet`]s (col + [`FilterOp`] + value) combined with logical AND, and the
//! AI-first wire carries the whole conjunction in one string —
//! `set_filter "0~Alpha&1>=500&2=Active"`:
//!
//! * **filter** — the R997 [`GridSortState`] filter axis, now multi-facet. A
//!   row passes iff it satisfies *every* facet; the ordered ops (`<`..`>=`) are
//!   numeric-aware (the same `cell_cmp` SSOT the sort uses, so `500 ≤ 999` is a
//!   number comparison, not `"500" ≤ "999"` text). `view_len` shrinks to the
//!   survivors. Filter is RPC-driven (no clickable chip — exactly as
//!   `hello-grid-filter`); the headers stay clickable for sort.
//! * **sort** — the R778 [`GridSortExternal`]; a clicked column header
//!   (`vsort#h<col>`) cycles that column's sort over the **filtered** rows
//!   (filter-then-sort composes, unchanged from R783).
//! * **selection** — the R746 [`VirtualSelectExternal`], a **source** data-row
//!   index; selection ⊥ filter ⊥ ordering, all three data-indexed (the
//!   Model/View separation), so a selected row stays selected even when the
//!   conjunction scrolls it out of the view.
//!
//! The round adds no new windowing substrate: the body windows over the shared
//! `order` permutation, which is now `multi-facet-filter`-then-`sort`.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/invoke` `set_filter("0~Alpha&1>=500&2=Active")` returns the new
//! `view_len`; `query("view_len")` confirms it; every rendered row's Name
//! contains "Alpha", Score ≥ 500, and Status = Active. Relaxing a facet
//! (`set_filter "0~Alpha&2=Active"`) grows the view; clearing it
//! (`set_filter` Null) restores the full 10 000. Pure data, no pixels (see
//! `tools/demos/r997_grid_multifilter.py`).
//!
//! ## a11y
//!
//! WAI-ARIA virtualized `grid` over the *current filtered + sorted order*: one
//! `row` per windowed visual position with `aria-posinset` = visual position
//! and `aria-selected = (source == selected)`, a `gridcell` per column tagged
//! by **source** id, under a frozen header row whose active column carries
//! `aria-sort`. The conjunction + sort permutation breaks the identity mapping
//! the lifted `windowed_grid_nodes_sorted` assumes, so the tree is built inline
//! (the view-order-permutation carve-out shared with `hello-grid-filter`).

use pinion_a11y::{windowed_grid_nodes_sorted, AccessNode, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::grid_sort::{
    grid_filter_str, grid_sort_str, use_grid_sort, GridFilter, GridSortExternal, GridSortState,
};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{read_selected, VirtualSelectExternal};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::{view_virtual_table, GridScroll, TableStyle, VirtualTableData};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridMultiFilterRenderer, HelloGridMultiFilterRendererError);

/// Initial window size — freely resizable; the grid body re-windows on every
/// `Resized` event. Wide enough that `NCOLS × COL_W` fits.
const WIN_W: u32 = 400;
const WIN_H: u32 = 480;
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 3;
/// Uniform column width; `NCOLS × COL_W = 330 < WIN_W` so no h-scroll.
const COL_W: u32 = 110;
/// Data-row height (the windowing pitch).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Status-bar height above the grid.
const STATUS_H: u32 = 40;
/// Column header labels.
const HEADERS: [&str; NCOLS] = ["Name", "Score", "Status"];
/// The Name column index — the `~` (substring) facet column.
#[cfg(test)]
const NAME_COL: usize = 0;
/// The Score column index — the numeric `>=` facet column.
#[cfg(test)]
const SCORE_COL: usize = 1;
/// The Status column index — the `=` (exact category) facet column.
#[cfg(test)]
const STATUS_COL: usize = 2;
/// Paint-root + a11y `grid` tag, and the [`VirtualSelectExternal`] anchor
/// (cell clicks on `vtbl#<source>_<col>` route here, selecting the row).
const GRID_TAG: &str = "vtbl";
/// The [`GridSortExternal`] anchor + the `use_grid_sort` cache key: clicked
/// column headers (`vsort#h<col>`) route here, and `invoke "set_filter"`
/// drives the multi-facet conjunction.
const SORT_TAG: &str = "vsort";
const SCROLL_KEY: &str = "vtbl_scroll";
/// Outer horizontal scroll `ScrollState` cache key (columns fit the window
/// here, so `max_x` stays 0 — wiring present for parity).
const H_SCROLL_KEY: &str = "vtbl_hscroll";
const STATUS_TAG: &str = "vtbl_status";

const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
const STATUS: [&str; 3] = ["Idle", "Active", "Done"];

// The widget's only projected state is the selected **source** data-row index
// (`Option<usize>`). Sort + filter live in the shared `GridSortState`; scroll
// lives in the reactive `ScrollState`. All drive their own repaints.

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        ..TableStyle::m3()
    }
}

/// The Score column value for data row `id`: a non-monotonic pseudo-random
/// number in `0..1000`, so a numeric `>=` facet keeps a non-trivial subset and
/// a numeric-aware sort over the survivors visibly differs from a lexicographic
/// one.
fn score(id: usize) -> usize {
    (id * 7919) % 1000
}

/// Synthetic cell texts for a data row. Column 0 (Name) is `<Category><id>`
/// (so a `~` substring facet on the category name keeps that category); column
/// 1 (Score) is numeric (the `>=` facet column); column 2 (Status) is a cyclic
/// category (the `=` facet column).
fn row_cells(id: usize) -> Vec<String> {
    vec![
        format!("{}{id:04}", CATEGORIES[id % CATEGORIES.len()]),
        score(id).to_string(),
        STATUS[id % STATUS.len()].to_string(),
    ]
}

/// The shared [`GridSortState`] — the single source of truth for the grid's
/// `(sort, filter)` view order. The view, the a11y tree, and the
/// [`GridSortExternal`] all reach the same `Rc` through this hook, so the
/// order is computed once.
fn use_grid_data() -> Rc<GridSortState> {
    use_grid_sort(SORT_TAG, || (NCOLS, (0..N).map(row_cells).collect()))
}

/// Status bar above the grid: a literal scene-as-data readout of the active
/// multi-facet filter, sort, and the resulting view size — e.g.
/// `filter 0~Alpha&1>=500&2=Active · sort none · showing 432 of 10000`.
fn status_bar(
    theme: &Theme,
    sort: Option<(usize, bool)>,
    filter: Option<&GridFilter>,
    view_len: usize,
) -> Scene {
    let text = Scene::Text(
        TextNode::styled(
            format!(
                "filter {} \u{00B7} sort {} \u{00B7} showing {view_len} of {N}",
                grid_filter_str(filter),
                grid_sort_str(sort),
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
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
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

/// view-fn (§6.3): pure sync mapping `selected source row -> Scene`. The
/// dataset is virtual — `view_virtual_table` invokes [`row_cells`] only for
/// the windowed visual positions, each resolved to its source row through the
/// shared `(filter, sort)` `order`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let grid = use_grid_data();
    let sort = grid.sort();
    let filter = grid.filter();
    let order = grid.order();

    let table = view_virtual_table(
        GRID_TAG,
        GridScroll { body: &scroll, horizontal: &h_scroll },
        VirtualTableData {
            headers: &HEADERS,
            item_count: N,
            overscan: OVERSCAN,
            sort,
            sort_tag: Some(SORT_TAG),
            order: Some(order.as_slice()),
            col_widths: None,
            resizable: false,
            frozen_cols: 0,
            row_style: None,
        },
        &theme,
        &style,
        |id| selected == Some(id),
        row_cells,
    );

    Scene::Container(
        ContainerNode::new(vec![status_bar(&theme, sort, filter.as_ref(), order.len()), table])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct GridMultiFilterView;

impl WidgetCore for GridMultiFilterView {
    /// The widget's projected state is the selected **source** index. Sort +
    /// filter are auxiliary reactive axes in the shared [`GridSortState`] —
    /// read in the view, not projected here (they repaint through their
    /// `Signal`s, like scroll offset).
    type State = Option<usize>;
    type Event = ();

    /// Primary = the index-held selection coordinator (R746), at
    /// [`GRID_TAG`]. The windowed `vtbl#<source>_<col>` cells route here.
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    /// Extra: the sort/filter proxy (a thin adapter over the **same** shared
    /// [`GridSortState`] the view reads via [`use_grid_data`]). Column-header
    /// clicks cycle its sort; `invoke "set_filter"` drives its multi-facet
    /// conjunction (sort ⊥ filter ⊥ selection).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(SORT_TAG, Box::new(GridSortExternal::new(use_grid_data())))]
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    /// Project the selected source index off the primary coordinator. A
    /// selection change repaints; sort / filter / scroll repaint via their own
    /// reactive `Signal` subscriptions the view opens.
    fn read_state(scene: &Scene) -> Option<usize> {
        scene
            .find_external_with_tag(GRID_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(read_selected)
    }

    fn view(state: Option<usize>, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Pointer / RPC sort + filter + selection this slice; neither rows nor
    /// headers are keyboard tab stops (sorted/filtered-grid roving by *visual*
    /// position is a later axis). Empty so Tab never lands.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-grid-multifilter (R997 §5.40 multi-facet predicate filter)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=source {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GridMultiFilterView {
    /// WAI-ARIA virtualized `grid` over the current `(filter, sort)` order via
    /// the [`windowed_grid_nodes_sorted`] permuted-grid peer (shared with
    /// `hello-grid-filter`): the conjunction + sort permutation makes
    /// `posinset` the visual position and tags/selects rows by source id, and
    /// the grid `aria-setsize` is the filtered view length.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let grid = use_grid_data();
        let sort = grid.sort();
        let order = grid.order();
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, order.len(), ROW_H, OVERSCAN);
        windowed_grid_nodes_sorted(
            GRID_TAG,
            "Multi-facet filterable data grid",
            &HEADERS,
            order.as_slice(),
            sort,
            *selected,
            &window,
        )
    }
}

impl WidgetView for GridMultiFilterView {
    type Renderer = HelloGridMultiFilterRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridMultiFilterView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::grid_sort::{ColumnFacet, FilterOp};
    use pinion_core::Owner;

    // The facet ops, conjunction, and wire round-trip are unit-tested in
    // `pinion_core::widgets::grid_sort`; these cover the binding's data model
    // (the synthetic rows satisfy the conjunction the way the painted window
    // and the a11y setsize report). End-to-end RPC drive is covered by
    // `tools/demos/r997_grid_multifilter.py`.

    /// The demo conjunction: Name contains "Alpha" AND Score >= 500 AND
    /// Status == Active.
    fn demo_filter() -> GridFilter {
        GridFilter::all(vec![
            ColumnFacet::new(NAME_COL, FilterOp::Contains, "Alpha"),
            ColumnFacet::new(SCORE_COL, FilterOp::Ge, "500"),
            ColumnFacet::new(STATUS_COL, FilterOp::Eq, "Active"),
        ])
    }

    /// Whether data row `id` satisfies the demo conjunction, computed
    /// independently of the proxy (the test oracle).
    fn satisfies_demo(id: usize) -> bool {
        let cells = row_cells(id);
        cells[NAME_COL].contains("Alpha") && score(id) >= 500 && cells[STATUS_COL] == "Active"
    }

    #[test]
    fn multi_facet_filter_keeps_only_rows_satisfying_all_facets() {
        Owner::new().run(|| {
            let grid = use_grid_data();
            assert_eq!(grid.view_len(), N, "unfiltered view is the full dataset");
            let kept = grid.set_filter(Some(demo_filter()));
            assert!(kept > 0 && kept < N, "the conjunction keeps a strict subset, got {kept}");
            let order = grid.order();
            assert_eq!(order.len(), kept, "view len equals the order length");
            for &id in order.iter() {
                assert!(satisfies_demo(id), "row {id} satisfies every facet");
            }
            // The survivor count equals the independently-computed oracle.
            let oracle = (0..N).filter(|&id| satisfies_demo(id)).count();
            assert_eq!(kept, oracle, "every satisfying row is kept, no extras");
        });
    }

    #[test]
    fn relaxing_a_facet_grows_the_view() {
        Owner::new().run(|| {
            let grid = use_grid_data();
            let strict = grid.set_filter(Some(demo_filter()));
            // Drop the numeric `Score >= 500` facet: a superset survives.
            let relaxed = grid.set_filter(Some(GridFilter::all(vec![
                ColumnFacet::new(NAME_COL, FilterOp::Contains, "Alpha"),
                ColumnFacet::new(STATUS_COL, FilterOp::Eq, "Active"),
            ])));
            assert!(relaxed > strict, "relaxing a facet grows the view: {relaxed} > {strict}");
            // A back-compat single Eq facet ("2=Active") is the widest of the three.
            let single = grid.set_filter(Some(GridFilter::eq(STATUS_COL, "Active")));
            assert!(single >= relaxed, "fewer facets keep at least as many rows");
        });
    }

    #[test]
    fn multi_facet_filter_composes_with_sort() {
        Owner::new().run(|| {
            let grid = use_grid_data();
            grid.set_filter(Some(demo_filter()));
            grid.set_sort(Some((SCORE_COL, true))); // numeric Score ascending over survivors
            let order = grid.order();
            for &id in order.iter() {
                assert!(satisfies_demo(id), "survivors still satisfy the conjunction after sort");
            }
            for pair in order.windows(2) {
                assert!(
                    score(pair[0]) <= score(pair[1]),
                    "filtered survivors sort by numeric Score: {} <= {}",
                    pair[0],
                    pair[1],
                );
            }
        });
    }

    #[test]
    fn a11y_setsize_tracks_the_filtered_view() {
        let (full, filtered) = Owner::new().run(|| {
            let grid = use_grid_data();
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 384);
            scroll.scroll_to(0, 0);
            let full = GridMultiFilterView::access_node(&None, None)[0].size_of_set;
            grid.set_filter(Some(demo_filter()));
            let filtered = GridMultiFilterView::access_node(&None, None)[0].size_of_set;
            (full, filtered)
        });
        assert_eq!(full, Some(u32::try_from(N).unwrap()), "unfiltered grid setsize = N");
        let filtered = filtered.unwrap();
        assert!(filtered > 0 && filtered < u32::try_from(N).unwrap(), "setsize shrinks under filter");
    }
}
