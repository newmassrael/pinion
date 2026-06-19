//! `hello-grid-search` — R1004 §5.27 §5.40 **search-and-jump cursor at scale**.
//!
//! A code editor's results pane, a Wireshark "Find Packet", a dlt-viewer log
//! search: a query selects matching rows, but the data set never shrinks — you
//! navigate the matches in place, each scrolling into view. This binding is
//! that pattern over a 10 000-row virtualized data-grid: a [`RowSearchState`]
//! cursor walks the rows a [`GridFilter`] accepts (next / previous / jump),
//! and every cursor move reveals the match through the body scroll. **All
//! 10 000 rows stay visible** — that is what distinguishes search from the
//! filter axis (`hello-grid-filter`), which removes the rows it rejects.
//!
//! The decisive reuse: a search query **is** the R997
//! [`GridFilter`](pinion_core::widgets::grid_sort::GridFilter) — the same
//! multi-facet predicate the grid's filter axis carries and the R998 coloring
//! engine matches against. Search is the **third** consumer of that match
//! vocabulary (filter removes, coloring tints, search navigates): an AI client
//! searches with `invoke "set_query" "1>=900"` (Score ≥ 900) exactly as it
//! would filter or add a coloring rule. Two more reuses make the binding
//! almost free of new wiring: `reveal_row` performs the jump, and the R998
//! `row_style` resolver highlights the matches (the current one strongly, the
//! rest lightly) with **no** new paint substrate.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `query("query")` is the active filter wire form, `query("match_count")` the
//! number of matches, `query("current")` the cursor position and
//! `query("current_row")` the source row it points at; `query("match.<i>")`
//! resolves the i-th match. `invoke "next"` / `"prev"` / `"jump"` walk the
//! cursor and return the new source row — the whole search is observable and
//! driveable without a pixel (see `tools/demos/r1004_grid_search.py`).

use pinion_a11y::{windowed_grid_nodes, AccessNode, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::grid_sort::{grid_filter_str, ColumnFacet, FilterOp, GridFilter};
use pinion_core::widgets::row_search::{use_row_search, RowSearchExternal, RowSearchState};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::{view_virtual_table, GridScroll, TableStyle, VirtualTableData};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridSearchRenderer, HelloGridSearchRendererError);

const WIN_W: u32 = 400;
const WIN_H: u32 = 480;
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small.
const N: usize = 10_000;
const NCOLS: usize = 3;
const COL_W: u32 = 110;
const ROW_H: u32 = 36;
const OVERSCAN: usize = 3;
const STATUS_H: u32 = 40;
const HEADERS: [&str; NCOLS] = ["Name", "Score", "Status"];
/// The Name column — the substring (`~`) search column.
const NAME_COL: usize = 0;
const GRID_TAG: &str = "vtbl";
/// The [`RowSearchExternal`] anchor + `use_row_search` cache key.
const SEARCH_TAG: &str = "vsearch";
const SCROLL_KEY: &str = "vtbl_scroll";
const H_SCROLL_KEY: &str = "vtbl_hscroll";
const STATUS_TAG: &str = "vtbl_status";

const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
const STATUS: [&str; 3] = ["Idle", "Active", "Done"];

/// The current match's highlight (the jump target) — a strong amber strip.
const CURRENT_BG: Color = Color::rgb(255, 193, 7);
const CURRENT_FG: Color = Color::rgb(0, 0, 0);
/// The other matches' highlight — a faint amber wash, so the eye sees the whole
/// result set while the cursor stands out.
const MATCH_BG: Color = Color::rgb(255, 243, 205);
const MATCH_FG: Color = Color::rgb(0, 0, 0);

fn table_style() -> TableStyle {
    TableStyle { col_width: COL_W, row_height: ROW_H, ..TableStyle::m3() }
}

/// Score for data row `id`: a non-monotonic pseudo-random number in `0..1000`,
/// so a numeric `>=` query keeps a sparse subset.
fn score(id: usize) -> usize {
    (id * 7919) % 1000
}

/// Synthetic cell texts: Name `<Category><id>`, numeric Score, cyclic Status.
fn row_cells(id: usize) -> Vec<String> {
    vec![
        format!("{}{id:04}", CATEGORIES[id % CATEGORIES.len()]),
        score(id).to_string(),
        STATUS[id % STATUS.len()].to_string(),
    ]
}

/// The shared search proxy (cells + query + cursor). The view + the
/// `RowSearchExternal` reach the same `Rc`.
fn use_search() -> Rc<RowSearchState> {
    use_row_search(SEARCH_TAG, || (NCOLS, (0..N).map(row_cells).collect()))
}

/// The seed query (Name contains "Delta") — a substring search over a large
/// match set the cursor then walks. Reuses the R997 `Contains` op.
fn seed_query() -> GridFilter {
    GridFilter::single(ColumnFacet::new(NAME_COL, FilterOp::Contains, "Delta"))
}

/// Status bar: a scene-as-data readout of the cursor position + query + the
/// invariant that every row stays visible.
fn status_bar(theme: &Theme, search: &RowSearchState) -> Scene {
    let q = grid_filter_str(search.query().as_ref());
    let total = search.match_count();
    let label = match search.current_index() {
        Some(i) => format!("match {} of {total} \u{00B7} {q} \u{00B7} {N} rows (all visible)", i + 1),
        None if total == 0 && search.query().is_some() => {
            format!("no matches \u{00B7} {q} \u{00B7} {N} rows")
        }
        None => format!("no search \u{00B7} {N} rows"),
    };
    let text = Scene::Text(
        TextNode::styled(
            label,
            Rect::default(),
            TextStyle::new().with_size_px(13).with_fg(theme.resolve(ColorRole::OnSurface)),
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

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let search = use_search();
    // Snapshot the match list + cursor ONCE per frame (subscribes → repaints on
    // a query/cursor change), so the per-row resolver never recomputes.
    let matches = search.matches();
    let current = search.current_row();

    // The per-source-row search highlight: the current match paints a strong
    // strip (the jump target), the other matches a faint wash — search keeps
    // every row, so non-matches stay on the zebra fill. Reuses the R998
    // `row_style` paint hook with no new substrate; matches() is ascending, so
    // membership is a binary search.
    let resolve = |source: usize| {
        if Some(source) == current {
            Some((CURRENT_BG, CURRENT_FG))
        } else if matches.binary_search(&source).is_ok() {
            Some((MATCH_BG, MATCH_FG))
        } else {
            None
        }
    };

    let table = view_virtual_table(
        GRID_TAG,
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
            row_style: Some(&resolve),
        },
        &theme,
        &style,
        |_| false, // search has its own cursor highlight; selection is a separate axis
        row_cells,
    );

    Scene::Container(
        ContainerNode::new(vec![status_bar(&theme, &search), table])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct GridSearchView;

impl WidgetCore for GridSearchView {
    type State = ();
    type Event = ();

    /// No widget statechart of its own — the addressable anchor is the search
    /// proxy. A [`StubExternal`](pinion_core::external::StubExternal) holds the
    /// grid `GRID_TAG` (input router + a11y `grid` bounds).
    fn create_external() -> Box<dyn External> {
        Box::new(pinion_core::external::StubExternal::new())
    }

    /// Extras: the search proxy, seeded once (boot) and attached to the body
    /// scroll so a cursor move scrolls the match into view (the "jump").
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let search = use_search();
        if search.query().is_none() {
            search.set_query(Some(seed_query()));
        }
        let scroll = use_scroll_state(SCROLL_KEY);
        vec![ExtraExternal::new(
            SEARCH_TAG,
            Box::new(RowSearchExternal::new(search).with_reveal(scroll, ROW_H)),
        )]
    }

    fn tag() -> &'static str {
        GRID_TAG
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
        "pinion hello-grid-search (R1004 §5.27 search-and-jump cursor)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (search state in the proxy)".to_string()
    }
}

impl WidgetA11y for GridSearchView {
    /// WAI-ARIA virtualized `grid` over the identity row order (the search
    /// highlight is a presentation overlay, orthogonal to the a11y structure).
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        windowed_grid_nodes(
            GRID_TAG,
            "Searchable data grid",
            &HEADERS,
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
        )
    }
}

impl WidgetView for GridSearchView {
    type Renderer = HelloGridSearchRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<GridSearchView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// Independent oracle: data row `id` matches the seed query (Name contains
    /// "Delta") iff its category is Delta (index 3 in the 5-category cycle).
    fn oracle_match(id: usize) -> bool {
        id % CATEGORIES.len() == 3
    }

    #[test]
    fn seed_query_matches_the_delta_rows() {
        Owner::new().run(|| {
            let search = use_search();
            search.set_query(Some(seed_query()));
            for id in 0..200 {
                let hit = search.matches().binary_search(&id).is_ok();
                assert_eq!(hit, oracle_match(id), "row {id} match membership");
            }
            // Cursor lands on the first Delta row (id 3).
            assert_eq!(search.current_row(), Some(3));
        });
    }

    #[test]
    fn next_walks_matches_and_keeps_all_rows() {
        Owner::new().run(|| {
            let search = use_search();
            search.set_query(Some(seed_query()));
            // Delta rows are 3, 8, 13, ... (stride 5).
            assert_eq!(search.next_match(), Some(8));
            assert_eq!(search.next_match(), Some(13));
            // The dataset never shrinks — every row is still addressable.
            assert_eq!(search.count(), N, "search keeps all rows (unlike a filter)");
        });
    }

    #[test]
    fn numeric_query_reuses_grid_filter_ops() {
        Owner::new().run(|| {
            let search = use_search();
            // Score >= 900 — the R997 numeric `>=` op over the live cells.
            search.set_query(Some(GridFilter::single(ColumnFacet::new(1, FilterOp::Ge, "900"))));
            let n = search.match_count();
            assert!(n > 0 && n < N, "a sparse numeric match set: {n}");
            // Every match really has Score >= 900.
            for &row in search.matches().iter().take(50) {
                assert!(score(row) >= 900, "row {row} score {} >= 900", score(row));
            }
        });
    }

    #[test]
    fn row_cells_shape() {
        assert_eq!(
            row_cells(3),
            vec!["Delta0003".to_string(), score(3).to_string(), "Idle".to_string()],
        );
    }
}
