//! `hello-grid-filter` — R783 §5.40 **data-grid filter at scale**.
//!
//! R778 (`hello-grid-sort`) lands clickable numeric-aware **sort** over a
//! 10 000-row virtualized data-grid. The 1-D `hello-virtual-sort` pairs its
//! sort with a **filter** facet; the grid's `GridSortState` doc flagged the
//! same filter axis as the additive follow-up. This binding closes it: an
//! AI-first **column filter** (`invoke "set_filter" "<col>=<value>"`) that
//! shrinks the view to the matching rows, composing orthogonally with the
//! clickable sort headers and the data-indexed selection:
//!
//! * **filter** — the R783 [`GridSortState`] filter axis (a single-facet
//!   column filter, the grid peer of `ViewOrderState`'s category facet). It
//!   keeps only rows whose Status column equals the active value; `view_len`
//!   shrinks. Filter is driven by the AI-first `invoke "set_filter"` (no
//!   clickable chip — exactly as `hello-virtual-sort` drives its filter).
//! * **sort** — the R778 [`GridSortExternal`]; a clicked column header
//!   (`vsort#h<col>`) cycles that column's sort over the **filtered** rows
//!   (filter-then-sort composes).
//! * **selection** — the R746 [`VirtualSelectExternal`], a **source**
//!   data-row index; a cell click selects that row, and it stays selected
//!   even when a filter scrolls it out of the view (selection ⊥ filter ⊥
//!   ordering, all three data-indexed — the Model/View separation).
//!
//! The round adds no new windowing substrate: the body windows over the
//! shared `order` permutation, which is now `filter`-then-`sort`.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/invoke` `set_filter("2=Active")` returns the new `view_len`
//! (~3 333 of 10 000); `query("view_len")` confirms it; the rendered window
//! holds only Status=Active rows; and `cycle_sort(1)` then orders those
//! survivors by numeric Score. Clearing the filter (`set_filter` Null)
//! restores the full 10 000. Pure data, no pixels (see
//! `tools/r783_grid_filter.py`).
//!
//! ## a11y
//!
//! WAI-ARIA virtualized `grid` over the *current filtered + sorted order*:
//! one `row` per windowed visual position with `aria-posinset` = visual
//! position and `aria-selected = (source == selected)`, a `gridcell` per
//! column tagged by **source** id, under a frozen header row whose active
//! column carries `aria-sort`. The filter + sort permutation breaks the
//! identity mapping the lifted `windowed_grid_nodes_selected` assumes, so the
//! tree is built inline (R776's view-order-permutation carve-out, shared with
//! `hello-grid-sort`).

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
use pinion_widget_paint::table::{view_virtual_table, TableStyle, VirtualTableData};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridFilterRenderer, HelloGridFilterRendererError);

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
/// The Status column index — the categorical column the filter facet matches
/// (the filter is RPC-driven, so this is referenced only by the unit tests).
#[cfg(test)]
const STATUS_COL: usize = 2;
/// Paint-root + a11y `grid` tag, and the [`VirtualSelectExternal`] anchor
/// (cell clicks on `vtbl#<source>_<col>` route here, selecting the row).
const GRID_TAG: &str = "vtbl";
/// The [`GridSortExternal`] anchor + the `use_grid_sort` cache key: clicked
/// column headers (`vsort#h<col>`) route here, and `invoke "set_filter"`
/// drives the filter facet.
const SORT_TAG: &str = "vsort";
const SCROLL_KEY: &str = "vtbl_scroll";
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
/// number with varying digit-length, so a numeric-aware sort over the filtered
/// survivors visibly differs from a lexicographic one.
fn score(id: usize) -> usize {
    (id * 7919) % 997
}

/// Synthetic cell texts for a data row. Column 0 (Name) is unique; column 1
/// (Score) is numeric; column 2 (Status) is a cyclic category — the filter
/// facet (each of Idle / Active / Done covers ~`N / 3` rows).
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
/// filter, sort, and the resulting view size — `filter 2=Active · showing
/// 3334 of 10000` after filtering, proving the view shrank.
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
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let grid = use_grid_data();
    let sort = grid.sort();
    let filter = grid.filter();
    let order = grid.order();

    let table = view_virtual_table(
        GRID_TAG,
        &scroll,
        VirtualTableData {
            headers: &HEADERS,
            item_count: N,
            overscan: OVERSCAN,
            sort,
            sort_tag: Some(SORT_TAG),
            order: Some(order.as_slice()),
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

struct GridFilterView;

impl WidgetCore for GridFilterView {
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
    /// clicks cycle its sort; `invoke "set_filter"` drives its filter facet
    /// (sort ⊥ filter ⊥ selection).
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
        "pinion hello-grid-filter (R783 §5.40 data-grid filter at scale)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=source {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GridFilterView {
    /// WAI-ARIA virtualized `grid` over the current `(filter, sort)` order via
    /// the R783-lifted [`windowed_grid_nodes_sorted`] (the permuted-grid peer,
    /// shared with `hello-grid-sort`): the filter + sort permutation makes
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
            "Filterable data grid",
            &HEADERS,
            order.as_slice(),
            sort,
            *selected,
            &window,
        )
    }
}

impl WidgetView for GridFilterView {
    type Renderer = HelloGridFilterRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridFilterView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    // The filter transition + filter-then-sort composition + RPC are
    // unit-tested in `pinion_core::widgets::grid_sort`; these cover the binding
    // wiring (filtered body windows over the view-len, a11y setsize tracks the
    // filtered count). End-to-end RPC drive is covered by
    // `tools/r783_grid_filter.py`.

    fn active_rows_in_view(order: &[usize]) -> bool {
        order.iter().all(|&id| row_cells(id)[STATUS_COL] == "Active")
    }

    #[test]
    fn filter_shrinks_the_view_to_matching_status() {
        Owner::new().run(|| {
            let grid = use_grid_data();
            assert_eq!(grid.view_len(), N, "unfiltered view is the full dataset");
            let kept =
                grid.set_filter(Some(GridFilter { col: STATUS_COL, value: "Active".into() }));
            assert!(kept > 0 && kept < N, "filter keeps a strict subset, got {kept}");
            let order = grid.order();
            assert_eq!(order.len(), kept, "view len equals the order length");
            assert!(active_rows_in_view(&order), "every row in the view is Status=Active");
        });
    }

    #[test]
    fn filter_then_sort_orders_only_the_survivors() {
        Owner::new().run(|| {
            let grid = use_grid_data();
            grid.set_filter(Some(GridFilter { col: STATUS_COL, value: "Active".into() }));
            grid.set_sort(Some((1, true))); // Score ascending over the filtered rows
            let order = grid.order();
            assert!(active_rows_in_view(&order), "survivors are still all Active after sort");
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
            let full = GridFilterView::access_node(&None, None)[0].size_of_set;
            grid.set_filter(Some(GridFilter { col: STATUS_COL, value: "Active".into() }));
            let filtered = GridFilterView::access_node(&None, None)[0].size_of_set;
            (full, filtered)
        });
        assert_eq!(full, Some(u32::try_from(N).unwrap()), "unfiltered grid setsize = N");
        let filtered = filtered.unwrap();
        assert!(filtered > 0 && filtered < u32::try_from(N).unwrap(), "setsize shrinks under filter");
    }
}
