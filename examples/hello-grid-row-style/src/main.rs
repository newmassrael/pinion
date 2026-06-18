//! `hello-grid-row-style` — R998 §5.40 **per-row style rules at scale**.
//!
//! A Wireshark packet list paints each row by the first **coloring rule** whose
//! display filter the packet matches; a dlt-viewer colours log lines by filter.
//! This binding is the grid analog over a 10 000-row virtualized data-grid:
//! declarative [`RowStyleRule`]s, each a (predicate → tint) pair, paint the
//! rows, with the **first** matching rule winning (Wireshark precedence).
//!
//! The decisive reuse: a rule's predicate **is** the R997
//! [`GridFilter`](pinion_core::widgets::grid_sort::GridFilter) conjunction — the
//! same multi-facet filter the grid's filter axis carries. Coloring and
//! filtering speak the same wire vocab; an AI client adds a rule with
//! `invoke "add_rule" "2=Done;#c82828;#ffffff"` (Status=Done rows paint
//! red-on-white), reading `2=Done` exactly as a filter would.
//!
//! Three orthogonal axes, all data-indexed (the Model/View separation):
//!
//! * **coloring** — the R998 [`RowStyleState`] (rule list), resolved per
//!   **source** row by the grid body's `row_style` resolver, so a coloured row
//!   stays coloured across a re-sort. RPC-driven (`add_rule` / `clear` /
//!   restore the whole list), and the matched rule per row is itself
//!   introspectable (`match.<row>` / `tint.<row>`) — no pixels needed.
//! * **sort** — the R778 [`GridSortExternal`]; a clicked header cycles the
//!   sort over the coloured rows (coloring ⊥ ordering).
//! * **selection** — the R746 [`VirtualSelectExternal`] (source index). A
//!   selected row keeps the selection highlight **over** any rule tint
//!   (precedence: selection > rule > zebra), so the cursor stays visible.
//!
//! No new windowing substrate: the body windows over the shared sort `order`;
//! the `row_style` resolver only recolours the strips it already builds.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `query("rule_count")` is 2 at boot (seeded); `query("match.<row>")` reports
//! which rule colours a row (a Done row → 0, a high-Score row → 1, else -1);
//! `query("matched_rows")` counts the coloured rows; `invoke "clear"` reverts
//! every row to the zebra fill and `add_rule` re-colours — all without pixels
//! (see `tools/demos/r998_grid_row_style.py`).

use pinion_a11y::{windowed_grid_nodes_sorted, AccessNode, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::grid_sort::{
    grid_sort_str, use_grid_sort, ColumnFacet, FilterOp, GridFilter, GridSortExternal, GridSortState,
};
use pinion_core::widgets::row_style::{
    use_row_style, RowStyleExternal, RowStyleRule, RowStyleState, RowTint,
};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{read_selected, VirtualSelectExternal};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::{view_virtual_table, GridScroll, TableStyle, VirtualTableData};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridRowStyleRenderer, HelloGridRowStyleRendererError);

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
/// The Score column index — the numeric `>=` rule column.
const SCORE_COL: usize = 1;
/// The Status column index — the exact `=` rule column.
const STATUS_COL: usize = 2;
const GRID_TAG: &str = "vtbl";
/// The [`GridSortExternal`] anchor + `use_grid_sort` cache key.
const SORT_TAG: &str = "vsort";
/// The [`RowStyleExternal`] anchor + `use_row_style` cache key.
const RULES_TAG: &str = "vrules";
const SCROLL_KEY: &str = "vtbl_scroll";
const H_SCROLL_KEY: &str = "vtbl_hscroll";
const STATUS_TAG: &str = "vtbl_status";

const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
const STATUS: [&str; 3] = ["Idle", "Active", "Done"];

fn table_style() -> TableStyle {
    TableStyle { col_width: COL_W, row_height: ROW_H, ..TableStyle::m3() }
}

/// Score for data row `id`: a non-monotonic pseudo-random number in `0..1000`,
/// so the numeric `>= 900` rule keeps a sparse subset.
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

/// The shared sort/data proxy (cells + sort order). The view + a11y tree + the
/// `GridSortExternal` reach the same `Rc`.
fn use_grid_data() -> Rc<GridSortState> {
    use_grid_sort(SORT_TAG, || (NCOLS, (0..N).map(row_cells).collect()))
}

/// The shared row-coloring rule list.
fn use_rules() -> Rc<RowStyleState> {
    use_row_style(RULES_TAG)
}

/// The seed coloring rules (Wireshark-style): Status=Done rows paint
/// red-on-white (rule 0), high-Score rows (≥ 900) paint green-on-black (rule 1).
/// First-match precedence: a Done row with Score ≥ 900 still paints red (rule 0
/// precedes rule 1).
fn seed_rules() -> Vec<RowStyleRule> {
    vec![
        RowStyleRule::new(
            GridFilter::eq(STATUS_COL, "Done"),
            RowTint::new(Color::rgb(200, 40, 40), Color::rgb(255, 255, 255)),
        ),
        RowStyleRule::new(
            GridFilter::single(ColumnFacet::new(SCORE_COL, FilterOp::Ge, "900")),
            RowTint::new(Color::rgb(40, 160, 60), Color::rgb(0, 0, 0)),
        ),
    ]
}

/// Status bar: a scene-as-data readout of the active rule count + sort.
fn status_bar(theme: &Theme, sort: Option<(usize, bool)>, rule_count: usize) -> Scene {
    let text = Scene::Text(
        TextNode::styled(
            format!("{rule_count} coloring rules \u{00B7} sort {} \u{00B7} {N} rows", grid_sort_str(sort)),
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
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let grid = use_grid_data();
    let rules = use_rules();
    let sort = grid.sort();
    let order = grid.order();

    // R998 — the per-source-row coloring resolver: the first rule whose
    // GridFilter predicate the row's cells satisfy yields its tint. Reads the
    // rule `Signal` (subscribes → repaints on a rule change) and the grid cells.
    let resolve = |source: usize| {
        rules.resolve(|col| grid.cell(source, col)).map(|tint| (tint.bg, tint.fg))
    };

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
            row_style: Some(&resolve),
        },
        &theme,
        &style,
        |id| selected == Some(id),
        row_cells,
    );

    Scene::Container(
        ContainerNode::new(vec![status_bar(&theme, sort, rules.rule_count()), table])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct GridRowStyleView;

impl WidgetCore for GridRowStyleView {
    type State = Option<usize>;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    /// Extras: the sort proxy + the row-coloring rule proxy. The rule proxy is
    /// seeded once (boot) and given a cell source so `match.<row>` /
    /// `matched_rows` are RPC-introspectable.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let grid = use_grid_data();
        let rules = use_rules();
        if rules.rule_count() == 0 {
            rules.set_rules(seed_rules());
        }
        let row_source = {
            let grid = Rc::clone(&grid);
            move |row: usize| (0..NCOLS).map(|c| grid.cell(row, c).to_string()).collect()
        };
        vec![
            ExtraExternal::new(SORT_TAG, Box::new(GridSortExternal::new(Rc::clone(&grid)))),
            ExtraExternal::new(
                RULES_TAG,
                Box::new(RowStyleExternal::new(rules).with_source(N, row_source)),
            ),
        ]
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

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

    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-grid-row-style (R998 §5.40 per-row coloring rules)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=source {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GridRowStyleView {
    /// WAI-ARIA virtualized `grid` over the sort order (the coloring is a
    /// presentation overlay, orthogonal to the a11y structure).
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let grid = use_grid_data();
        let sort = grid.sort();
        let order = grid.order();
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, order.len(), ROW_H, OVERSCAN);
        windowed_grid_nodes_sorted(
            GRID_TAG,
            "Colour-ruled data grid",
            &HEADERS,
            order.as_slice(),
            sort,
            *selected,
            &window,
        )
    }
}

impl WidgetView for GridRowStyleView {
    type Renderer = HelloGridRowStyleRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<GridRowStyleView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// Independent oracle: which seed rule (if any) colours data row `id`
    /// (first-match: Done → 0, else Score ≥ 900 → 1, else none).
    fn oracle_rule(id: usize) -> Option<usize> {
        if STATUS[id % 3] == "Done" {
            Some(0)
        } else if score(id) >= 900 {
            Some(1)
        } else {
            None
        }
    }

    #[test]
    fn resolver_matches_the_seed_rules_first_match_wins() {
        Owner::new().run(|| {
            let grid = use_grid_data();
            let rules = use_rules();
            rules.set_rules(seed_rules());
            for id in 0..400 {
                let got = rules.match_index(|c| grid.cell(id, c));
                assert_eq!(got, oracle_rule(id), "row {id} matches the expected rule");
            }
        });
    }

    #[test]
    fn done_rule_precedes_high_score_rule() {
        Owner::new().run(|| {
            let grid = use_grid_data();
            let rules = use_rules();
            rules.set_rules(seed_rules());
            // Find a Done row that also has Score >= 900 (matches both rules).
            let both = (0..N).find(|&id| STATUS[id % 3] == "Done" && score(id) >= 900);
            if let Some(id) = both {
                assert_eq!(
                    rules.match_index(|c| grid.cell(id, c)),
                    Some(0),
                    "row {id} matches both → the earlier Done rule wins",
                );
                let tint = rules.resolve(|c| grid.cell(id, c)).unwrap();
                assert_eq!(tint.bg, Color::rgb(200, 40, 40), "painted red (Done), not green");
            }
        });
    }

    #[test]
    fn clearing_rules_reverts_to_no_tint() {
        Owner::new().run(|| {
            let grid = use_grid_data();
            let rules = use_rules();
            rules.set_rules(seed_rules());
            let done = (0..N).find(|&id| STATUS[id % 3] == "Done").unwrap();
            assert!(rules.resolve(|c| grid.cell(done, c)).is_some(), "Done row coloured");
            rules.clear();
            assert!(rules.resolve(|c| grid.cell(done, c)).is_none(), "cleared → no tint");
        });
    }
}
