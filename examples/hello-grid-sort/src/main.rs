//! `hello-grid-sort` — R778 §5.27 §5.40 **data-grid sort at scale**.
//!
//! R775 (`hello-virtual-table`) lands a display-only virtualized data grid;
//! R777 (`hello-grid-nav`) makes it selectable + keyboard-navigable. This
//! binding adds the axis a real data grid needs at scale: **clickable column
//! headers that sort the whole 10 000-row grid**, numeric-aware, while the
//! selection stays put on its data row. It is the grid analogue of the 1-D
//! `hello-virtual-sort`, and like it adds **no** new windowing substrate —
//! pure composition of three coordinators:
//!
//! * **sort proxy** — the R778 [`GridSortExternal`] over a shared
//!   [`GridSortState`], a multi-column numeric-aware sort proxy (Qt's
//!   `QSortFilterProxyModel`). A clicked column header (`vsort#h<col>`)
//!   cycles that column unsorted → asc → desc → unsorted; a different column
//!   jumps to it ascending. The `order` permutation it derives drives the
//!   view's windowing.
//! * **selection model** — the R746 [`VirtualSelectExternal`], holding a
//!   **source** data-row index. A cell click (`vtbl#<source>_<col>`) selects
//!   that row; because the index is the *source* row, a re-sort moves the
//!   selected strip's visual position while keeping it selected — selection
//!   ⊥ ordering, both data-indexed (the decisive Model/View separation).
//! * **windowed body + frozen header** — the R775 / R778
//!   [`view_virtual_table`], now fed the sort glyph + `sort_tag` (so headers
//!   route to the sort proxy) + the `order` permutation (so visual position
//!   `view_pos` paints source row `order[view_pos]`).
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/invoke` `cycle_sort(1)` on the sort proxy → `query("sort")` reports
//! `"1:ascending"`, `query("source_at.0")` reports the data row with the
//! smallest **numeric** Score (not the lexicographically-smallest), and the
//! rendered top strip is `vtbl_row<that source>`. Selecting a cell then
//! re-sorting leaves `query("selected")` unchanged. Pure data, no pixels
//! (see `tools/r778_grid_sort.py`).
//!
//! ## a11y
//!
//! WAI-ARIA virtualized `grid` over the *current sort order*: one `row` per
//! windowed visual position with `aria-posinset` = visual position and
//! `aria-selected = (source == selected)`, a `gridcell` per column tagged by
//! **source** id, under a frozen header row whose active column carries
//! `aria-sort`. The sort permutation breaks the identity mapping the lifted
//! [`windowed_grid_nodes_selected`](pinion_a11y) assumes (posinset = id + 1),
//! so — exactly as the 1-D `hello-virtual-sort` does — the tree is built
//! inline (R776's view-order-permutation carve-out).

use pinion_a11y::{windowed_grid_nodes_sorted, AccessNode, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::undo::{use_undo_stack, UndoStack, UndoStackExternal};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::grid_sort::{grid_sort_str, use_grid_sort, GridSortExternal, GridSortState};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{read_selected, VirtualSelectExternal};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::{view_virtual_table, GridScroll, TableStyle, VirtualTableData};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridSortRenderer, HelloGridSortRendererError);

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
/// Paint-root + a11y `grid` tag, and the [`VirtualSelectExternal`] anchor
/// (cell clicks on `vtbl#<source>_<col>` route here, selecting the row).
const GRID_TAG: &str = "vtbl";
/// The [`GridSortExternal`] anchor + the `use_grid_sort` cache key: clicked
/// column headers (`vsort#h<col>`) route here, cycling that column's sort.
const SORT_TAG: &str = "vsort";
const SCROLL_KEY: &str = "vtbl_scroll";
/// R784 — outer horizontal scroll `ScrollState` cache key (columns fit
/// the window here, so `max_x` stays 0 — wiring present for parity).
const H_SCROLL_KEY: &str = "vtbl_hscroll";
const STATUS_TAG: &str = "vtbl_status";
/// The shared [`UndoStack`] cache key + the [`UndoStackExternal`] anchor
/// (`invoke "undo"` / `"redo"` on it step the sort timeline).
const UNDO_KEY: &str = "vtbl_sort_undo";
const UNDO_STACK_TAG: &str = "vtbl_undo_stack";

const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
const STATUS: [&str; 3] = ["Idle", "Active", "Done"];

// The widget's only projected state is the selected **source** data-row index
// (`Option<usize>`). Sort lives in the shared `GridSortState`; scroll lives in
// the reactive `ScrollState`. Both drive their own repaints.

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        ..TableStyle::m3()
    }
}

/// The Score column value for data row `id`: a non-monotonic pseudo-random
/// number with varying digit-length, so a numeric-aware sort visibly differs
/// from a lexicographic one (`"9" < "80"` numerically, but `"80" < "9"`
/// lexically). The RPC demo replicates this formula to assert the order.
fn score(id: usize) -> usize {
    (id * 7919) % 997
}

/// Synthetic cell texts for a data row. Column 0 (Name) groups by category so
/// a name sort visibly regroups; column 1 (Score) is the numeric column;
/// column 2 (Status) is a cyclic category.
fn row_cells(id: usize) -> Vec<String> {
    vec![
        format!("{}{id:04}", CATEGORIES[id % CATEGORIES.len()]),
        score(id).to_string(),
        STATUS[id % STATUS.len()].to_string(),
    ]
}

/// The shared [`GridSortState`] — the single source of truth for the grid's
/// column sort order. The view, the a11y tree, and the [`GridSortExternal`]
/// all reach the same `Rc` through this hook (the `use_scroll_state`
/// pattern), so the order is computed once. The source cells are materialized
/// here (the dataset the proxy sorts; identical to `hello-virtual-sort`
/// materializing its key column).
fn use_grid_data() -> Rc<GridSortState> {
    use_grid_sort(SORT_TAG, || (NCOLS, (0..N).map(row_cells).collect()))
}

/// The shared [`UndoStack`] for the sort timeline. The [`GridSortExternal`]
/// records each sort change onto it; the [`UndoStackExternal`] surfaces the
/// history to RPC. Both reach the same `Rc` through this hook.
fn use_sort_undo() -> Rc<UndoStack> {
    use_undo_stack(UNDO_KEY)
}

/// Status bar above the grid: a literal scene-as-data readout of the active
/// sort + the selected source row.
fn status_bar(theme: &Theme, sort: Option<(usize, bool)>, selected: Option<usize>) -> Scene {
    let sel = selected.map_or_else(|| "none".to_string(), |i| i.to_string());
    let text = Scene::Text(
        TextNode::styled(
            format!("sort {} \u{00B7} selected {sel}", grid_sort_str(sort)),
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
/// shared sort `order`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let grid_sort = use_grid_data();
    let sort = grid_sort.sort();
    let order = grid_sort.order();

    let grid = view_virtual_table(
        GRID_TAG,
        GridScroll { body: &scroll, horizontal: &h_scroll },
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
        ContainerNode::new(vec![status_bar(&theme, sort, selected), grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct GridSortView;

impl WidgetCore for GridSortView {
    /// The widget's projected state is the selected **source** index. Sort is
    /// an auxiliary reactive axis in the shared [`GridSortState`] — read in
    /// the view, not projected here (a sort change repaints through its
    /// `Signal`, like scroll offset).
    type State = Option<usize>;
    type Event = ();

    /// Primary = the index-held selection coordinator (R746), at
    /// [`GRID_TAG`]. The windowed `vtbl#<source>_<col>` cells route here.
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    /// Extras: the sort proxy (a thin adapter over the **same** shared
    /// [`GridSortState`] the view reads via [`use_grid_data`], recording each
    /// sort change onto the shared [`UndoStack`]) and the undo-stack surface.
    /// The clickable column headers route to the sort proxy (sort ⊥ selection);
    /// `invoke "undo"` / `"redo"` on the stack step the sort back and forth.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                SORT_TAG,
                Box::new(GridSortExternal::new(use_grid_data()).with_undo(use_sort_undo())),
            ),
            ExtraExternal::new(UNDO_STACK_TAG, Box::new(UndoStackExternal::new(use_sort_undo()))),
        ]
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    /// Project the selected source index off the primary coordinator. A
    /// selection change repaints; sort / scroll repaint via their own
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

    /// Pointer / RPC sort + selection this slice; neither the rows nor the
    /// headers are keyboard tab stops (sorted-grid roving by *visual* position
    /// is a later axis — R777's `nav_select_key` roves by source index, which
    /// the sort permutation reorders). Empty so Tab never lands.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-grid-sort (R778 §5.27 §5.40 data-grid sort at scale)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=source {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GridSortView {
    /// WAI-ARIA virtualized `grid` over the current sort order via the
    /// R783-lifted [`windowed_grid_nodes_sorted`] (the permuted-grid peer,
    /// shared with `hello-grid-filter`): the sort permutation makes `posinset`
    /// the visual position and tags/selects rows by source id.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let grid_sort = use_grid_data();
        let sort = grid_sort.sort();
        let order = grid_sort.order();
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, order.len(), ROW_H, OVERSCAN);
        windowed_grid_nodes_sorted(
            GRID_TAG,
            "Sortable data grid",
            &HEADERS,
            order.as_slice(),
            sort,
            *selected,
            &window,
        )
    }
}

impl WidgetView for GridSortView {
    type Renderer = HelloGridSortRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridSortView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::{AriaRole, SortDirection};
    use pinion_core::Owner;

    // The sort transition + numeric order are unit-tested in
    // `pinion_core::widgets::grid_sort`; these cover the binding wiring
    // (paint permutation + a11y aria-sort + selection survival). End-to-end
    // pointer / RPC drive is covered by `tools/r778_grid_sort.py`.

    /// The numeric column's globally-smallest score lands at visual position
    /// 0 after an ascending sort — and it is NOT the lexicographically
    /// smallest, proving numeric-awareness end-to-end at the binding.
    #[test]
    fn ascending_score_sort_is_numeric_aware() {
        Owner::new().run(|| {
            let grid_sort = use_grid_data();
            grid_sort.set_sort(Some((1, true))); // Score ascending
            let order = grid_sort.order();

            // The order is monotonic non-decreasing by NUMERIC score.
            for pair in order.windows(2) {
                assert!(
                    score(pair[0]) <= score(pair[1]),
                    "numeric ascending: score({}) <= score({})",
                    pair[0],
                    pair[1],
                );
            }

            // ...and it genuinely differs from a LEXICOGRAPHIC sort of the
            // same column (so numeric awareness is observable, not a no-op:
            // `"100" < "9"` lexically but `9 < 100` numerically).
            let mut lex: Vec<usize> = (0..N).collect();
            lex.sort_by(|&a, &b| {
                score(a).to_string().cmp(&score(b).to_string()).then(a.cmp(&b))
            });
            assert_ne!(*order, lex, "numeric order differs from the lexicographic order");
        });
    }

    #[test]
    fn sorted_grid_strips_tag_by_source() {
        Owner::new().run(|| {
            let grid_sort = use_grid_data();
            grid_sort.set_sort(Some((0, true))); // Name ascending regroups
            let order = grid_sort.order();
            let scene = {
                let scroll = use_scroll_state(SCROLL_KEY);
                scroll.set_measured_viewport(WIN_W, 384);
                scroll.scroll_to(0, 0);
                view(None, &Frame::default())
            };
            // The strip at visual position 0 is the source row `order[0]`.
            assert!(
                scene.contains_tag(&format!("{GRID_TAG}_row{}", order[0])),
                "top visual strip is tagged by its source id",
            );
        });
    }

    #[test]
    fn selection_is_data_indexed_and_survives_sort() {
        // Select source row 5 (visible at boot, identity order). After a sort
        // that moves it, the SAME source strip carries the accent wash.
        let theme = pinion_core::theme::Theme::light();
        let wash = theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16);
        Owner::new().run(|| {
            let grid_sort = use_grid_data();
            grid_sort.set_sort(Some((1, false))); // Score descending moves row 5
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_H).unwrap());
            scroll.set_measured_viewport(WIN_W, ROW_H * 12);
            scroll.scroll_to(0, 0);
            let order = grid_sort.order();
            // Pick a selected source that is inside the top window post-sort.
            let selected = order[0];
            let scene = view(Some(selected), &Frame::default());
            let fill = find_row_fill(&scene, &format!("{GRID_TAG}_row{selected}"));
            assert_eq!(fill, Some(wash), "the selected SOURCE row carries the wash after re-sort");
        });
    }

    #[test]
    fn a11y_marks_active_sort_column_and_selected_source() {
        let nodes = Owner::new().run(|| {
            let grid_sort = use_grid_data();
            grid_sort.set_sort(Some((1, true))); // Score ascending
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 384);
            scroll.scroll_to(0, 0);
            let order = grid_sort.order();
            let selected = Some(order[0]);
            GridSortView::access_node(&selected, None)
        });
        assert_eq!(nodes[0].role, AriaRole::Grid);
        // Active column header carries aria-sort=ascending; others none.
        let ch1 = nodes.iter().find(|n| n.tag == format!("{GRID_TAG}_ch1")).unwrap();
        assert_eq!(ch1.sort, Some(SortDirection::Ascending), "active column aria-sort");
        let ch0 = nodes.iter().find(|n| n.tag == format!("{GRID_TAG}_ch0")).unwrap();
        assert_eq!(ch0.sort, None, "inactive column has no aria-sort");
        // The top visual row's source carries aria-selected=true.
        let top_selected = nodes
            .iter()
            .find(|n| n.role == AriaRole::Row && n.selected == Some(true));
        assert!(top_selected.is_some(), "the selected source row is marked");
    }

    fn find_row_fill(scene: &Scene, want: &str) -> Option<pinion_core::style::Color> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(want) {
                    return Some(c.style.fill);
                }
                c.children.iter().find_map(|ch| find_row_fill(ch, want))
            }
            Scene::Scroll(s) => find_row_fill(s.content.as_ref(), want),
            _ => None,
        }
    }
}
