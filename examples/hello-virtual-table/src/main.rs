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

use pinion_a11y::{AccessNode, WidgetA11y, windowed_grid_nodes};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::{ContainerNode, Rect, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{
    CellDecoration, CellIndex, CellPainter, CellRender, GridModel, GridScroll, TableStyle,
    VirtualTableData, header_from_slice, view_virtual_table,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloVirtualTableRenderer, HelloVirtualTableRendererError);

/// Initial window size — freely resizable; the grid body re-windows on
/// every `Resized` event. Wide enough that `NCOLS × COL_W` fits without
/// horizontal scroll.
const WIN_W: u32 = 400;
const WIN_H: u32 = 480;
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key.
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small
/// and tracks the window height.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 4;
/// Uniform column width; `NCOLS × COL_W = 360 < WIN_W` so no h-scroll.
const COL_W: u32 = 90;
/// Data-row height (must match the windowing pitch used in `access_node`).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Column header labels. `Load` is the R1532 **delegated** column, `Status`
/// the R1535 **decorated** one.
const HEADERS: [&str; NCOLS] = ["Index", "Name", "Status", "Load"];
/// R1535 — the absolute index of the column whose cells answer the
/// `Qt::DecorationRole` with a colour keyed to the row's status.
const STATUS_COL: usize = 2;
/// R1532 — the absolute index of the column whose cells are painted by a
/// delegate rather than as a label.
const LOAD_COL: usize = 3;
/// R1532 — the load bar's track height; the rest of the row is padding, so
/// the bar reads as a gauge inside the cell rather than as a filled cell.
const BAR_H: u32 = 10;
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

/// How many distinct values the `Status` column cycles through — the modulus
/// both [`cell_text`] and [`cell_decoration`] read the row's status with, so
/// the label and its mark cannot describe different states.
const STATUS_KINDS: usize = 3;

/// Synthetic cell texts for a data row. The five-digit index keeps every
/// `scene/snapshot` cell unambiguous; the category cycles so the eye has
/// something to track while scrolling.
fn cell_text(c: CellIndex) -> String {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    const STATUS: [&str; STATUS_KINDS] = ["Idle", "Active", "Done"];
    match c.col {
        0 => format!("{:05}", c.row),
        1 => CATEGORIES[c.row % CATEGORIES.len()].to_string(),
        LOAD_COL => format!("{}%", load_percent(c.row)),
        _ => STATUS[c.row % STATUS_KINDS].to_string(),
    }
}

/// R1535 §5.27 — the grid's `Qt::DecorationRole`: a colour swatch on every
/// `Status` cell, keyed to that **row**'s status.
///
/// This is the column R1532's delegate could not paint. A delegate belongs to a
/// column, and a column's painter is resolved once for every row it draws — so
/// a mark whose colour is a function of the row could only be had by delegating
/// the cell wholesale and re-deriving the status from the text the model had
/// already answered with. Asked per cell, the role simply answers.
///
/// The colour comes from the same `row % STATUS_KINDS` the label does, so the
/// swatch and the word can never disagree.
fn cell_decoration(c: CellIndex, theme: &Theme) -> Option<CellDecoration> {
    if c.col != STATUS_COL {
        return None;
    }
    let role = match c.row % STATUS_KINDS {
        0 => ColorRole::OnSurfaceMuted,
        1 => ColorRole::Accent,
        _ => ColorRole::Outline,
    };
    Some(CellDecoration::Swatch(theme.resolve(role)))
}

/// R1532 — the `Load` column's datum, `0..=100`. Synthetic but not uniform,
/// so a delegated cell's fill width is visibly a function of its row.
fn load_percent(row: usize) -> u32 {
    u32::try_from((row * 7) % 101).unwrap_or(0)
}

/// R1532 §5.27 — the `Load` column's paint delegate (Qt
/// `QStyledItemDelegate::paint`): a proportionally filled track instead of a
/// label.
///
/// This is the column a text-only grid cannot have, and the reason the
/// delegate seam exists. Before R1532 a binding wanting it had to stop using
/// the grid's cell path and build the row itself — which is exactly what
/// `hello-property-grid`'s `ranged_slider_cell` does.
///
/// **The model's string is still painted.** A bar encodes the value in pixels
/// and `scene/snapshot` reads text, so a delegate that dropped the label would
/// make the column invisible to §2 #7 introspection and to a screen reader —
/// the same reason Qt's `QProgressBar` carries `text()`. `c.text` is the
/// model's own answer, so the number beside the bar cannot disagree with the
/// number the bar is drawn from.
fn load_bar(c: &CellRender<'_>) -> Scene {
    let pct = c.text.trim_end_matches('%').parse::<u32>().unwrap_or(0);
    let inner = c.width.saturating_sub(2 * c.style.cell_pad_x);
    // Half the interior is the track, the rest carries the label.
    let track_w = inner / 2;
    let fill_w = track_w * pct.min(100) / 100;
    let bar = |w: u32, fill| {
        Scene::Container(
            ContainerNode::new(Vec::new())
                .with_style(BoxStyle::filled(fill).with_corner_radius(BAR_H / 2))
                .with_layout(
                    LayoutStyle::new()
                        .with_size(Size::px(w, BAR_H))
                        .with_absolute_position(0, 0)
                        .with_pointer_transparent(true),
                ),
        )
    };
    Scene::Container(
        ContainerNode::new(vec![
            Scene::Container(
                ContainerNode::new(vec![
                    bar(track_w, c.theme.resolve(ColorRole::SurfaceContainerHighest)),
                    bar(fill_w, c.theme.resolve(ColorRole::Accent)),
                ])
                .with_layout(LayoutStyle::new().with_size(Size::px(track_w, BAR_H))),
            ),
            Scene::Text(
                TextNode::styled(
                    c.text.to_string(),
                    Rect::default(),
                    TextStyle::new()
                        .with_size_px(c.style.label_size_px)
                        .with_fg(c.fg),
                )
                .with_role(TextRole::Presentational),
            ),
        ])
        // R1532 — the cell's own tag. A painter that omits it drops the cell
        // out of pointer routing and out of every tag-addressed RPC.
        .with_tag(c.tag.to_string())
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Start)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(c.width, c.height))
                .with_padding(Rect::new(c.style.cell_pad_x, 0, c.style.cell_pad_x, 0)),
        ),
    )
}

/// view-fn (§6.3): pure sync `() -> Scene`. The dataset is virtual —
/// `view_virtual_table` invokes [`cell_text`] only for the indices in the
/// current window, whose *size* is the runtime-measured viewport height.
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
            column_count: NCOLS,
            item_count: N,
            overscan: OVERSCAN,
            sort: None,
            sort_tag: None,
            order: None,
            col_widths: None,
            resizable: false,
            frozen_cols: 0,
            row_style: None,
            // R1532 — Qt `setItemDelegateForColumn`: one column paints as a
            // gauge, every other takes the built-in text painter.
            delegate: Some(&|col| (col == LOAD_COL).then_some(&load_bar as CellPainter<'_>)),
        },
        &theme,
        &style,
        |_| false, // display-only grid: no selection
        GridModel {
            cell: cell_text,
            header: header_from_slice(&HEADERS),
            // R1535 — Qt `data(index, Qt::DecorationRole)`: one column answers
            // with a mark whose colour varies by row, which is the axis a
            // per-column delegate cannot express.
            decoration: |c: CellIndex| cell_decoration(c, &theme),
        },
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

    fn title() -> &'static str {
        "pinion hello-virtual-table (R775 §5.27 virtualized data-grid)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for VirtualTableView {
    /// WAI-ARIA virtualized `grid`: one `AriaRole::Grid` claiming the
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
        let count_rows =
            |nodes: &[AccessNode]| nodes.iter().filter(|n| n.role == AriaRole::Row).count();
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
    fn cells_are_indexed_and_categorized() {
        assert_eq!(row_of(0), vec!["00000", "Alpha", "Idle", "0%"]);
        assert_eq!(row_of(42), vec!["00042", "Charlie", "Idle", "92%"]);
    }

    /// R1532 — the delegated column's cells are painted by [`load_bar`] and
    /// every other column's by the grid's built-in text painter.
    ///
    /// The negative half is the load-bearing one: a delegate wired for one
    /// column that captured all of them would paint a plausible-looking grid
    /// and fail only here.
    #[test]
    fn r1532_only_the_load_column_paints_through_the_delegate() {
        let scene = run_view(360);
        let gauges = count_tag_prefix(&scene, &format!("{TABLE_TAG}#"));
        assert!(gauges > 0, "premise: the grid painted cells");
        // The bar's track + fill are the two empty containers a text cell
        // never has, and they exist only under the delegated column.
        let bars = count_bar_tracks(&scene);
        let rows = count_tag_prefix(&scene, &format!("{TABLE_TAG}_row"));
        assert!(rows > 1, "premise: more than one row painted, got {rows}");
        assert_eq!(
            bars,
            rows * 2,
            "one track + one fill per painted row, in the one delegated \
             column — a delegate claiming every column would report {}",
            rows * 2 * NCOLS,
        );
    }

    /// R1535 — the `Status` column's mark is a function of the **row**, which
    /// is the property that makes it a model role rather than a delegate.
    ///
    /// Asserted against the label the same row carries, not against a colour
    /// literal: the claim is that the two roles describe one status, and a test
    /// comparing the swatch to a hard-coded palette entry would keep passing if
    /// the label and the mark drifted apart.
    #[test]
    fn r1535_the_status_mark_agrees_with_the_status_label() {
        let theme = pinion_core::theme::Theme::light();
        let mark = |row: usize| {
            cell_decoration(
                CellIndex {
                    row,
                    col: STATUS_COL,
                },
                &theme,
            )
        };
        let label = |row: usize| {
            cell_text(CellIndex {
                row,
                col: STATUS_COL,
            })
        };
        let rows: Vec<usize> = (0..STATUS_KINDS * 2).collect();
        for &r in &rows {
            assert_eq!(
                mark(r),
                mark(r % STATUS_KINDS),
                "row {r}'s mark is its status's mark",
            );
            assert_eq!(label(r), label(r % STATUS_KINDS), "and so is its label");
        }
        // Distinct statuses get distinct marks — otherwise "agrees with the
        // label" is satisfied by one colour for everything.
        let distinct: std::collections::BTreeSet<_> = (0..STATUS_KINDS)
            .map(|r| format!("{:?}", mark(r)))
            .collect();
        assert_eq!(distinct.len(), STATUS_KINDS, "one mark per status");
        assert_eq!(
            mark(0),
            mark(STATUS_KINDS),
            "premise: the fixture's statuses really do cycle",
        );
        // Every other column answers the role with nothing.
        for col in 0..NCOLS {
            if col != STATUS_COL {
                assert_eq!(
                    cell_decoration(CellIndex { row: 1, col }, &theme),
                    None,
                    "column {col} carries no mark",
                );
            }
        }
    }

    /// R1532 — the delegated cell keeps the composite tag a text cell has, so
    /// it stays addressable by pointer routing and by every tag-addressed RPC.
    ///
    /// Written because a counterfactual found the gap: deleting
    /// `.with_tag(c.tag)` from [`load_bar`] left every other test in this file
    /// green and was caught only by the demo. The contract has exactly one
    /// rule a painter must follow, and nothing here was checking it.
    #[test]
    fn r1532_the_delegated_cell_keeps_its_hit_tag() {
        let scene = run_view(360);
        let per_column = |col: usize| {
            let mut n = 0;
            count_col(&scene, col, &mut n);
            n
        };
        let text_col = per_column(0);
        assert!(
            text_col > 1,
            "premise: the body windowed rows, got {text_col}"
        );
        assert_eq!(
            per_column(LOAD_COL),
            text_col,
            "the delegated column carries one tagged cell per row, exactly as \
             an undelegated one does",
        );
    }

    /// R1532 — the model's own string is painted beside the bar, so the
    /// value a sighted user reads off the pixels and the value
    /// `scene/snapshot` reports cannot disagree.
    #[test]
    fn r1532_the_delegated_cell_still_carries_its_text() {
        let scene = run_view(360);
        let mut texts = Vec::new();
        collect_text(&scene, &mut texts);
        assert!(
            texts.iter().any(|t| t.ends_with('%')),
            "the load column's percentage is in the scene, got {texts:?}",
        );
    }

    /// R1532 — paint the view with a measured viewport, the state the runtime
    /// is always in when it paints. Without it the body windows nothing (the
    /// correct pre-measurement boot state, and useless as a fixture).
    fn run_view(measured_h: u32) -> Scene {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, measured_h);
            use_scroll_state(H_SCROLL_KEY).set_measured_viewport(WIN_W, 0);
            view((), &Frame::default())
        })
    }

    /// R1532 — how many cells the scene carries for one absolute column,
    /// counted off the composite `vtbl#<row>_<col>` tags.
    fn count_col(scene: &Scene, col: usize, n: &mut usize) {
        match scene {
            Scene::Container(c) => {
                if let Some(t) = c.tag.as_deref()
                    && let Some(sub) = t.strip_prefix(&format!("{TABLE_TAG}#"))
                    && sub.split('_').nth(1) == Some(&col.to_string())
                {
                    *n += 1;
                }
                for ch in &c.children {
                    count_col(ch, col, n);
                }
            }
            Scene::Scroll(s) => count_col(&s.content, col, n),
            _ => {}
        }
    }

    fn collect_text(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for child in &c.children {
                    collect_text(child, out);
                }
            }
            Scene::Scroll(s) => collect_text(&s.content, out),
            _ => {}
        }
    }

    fn count_tag_prefix(scene: &Scene, prefix: &str) -> usize {
        match scene {
            Scene::Container(c) => {
                let here = usize::from(c.tag.as_deref().is_some_and(|t| t.starts_with(prefix)));
                here + c
                    .children
                    .iter()
                    .map(|ch| count_tag_prefix(ch, prefix))
                    .sum::<usize>()
            }
            Scene::Scroll(s) => count_tag_prefix(&s.content, prefix),
            _ => 0,
        }
    }

    /// Untagged, childless containers with an explicit `BAR_H` height: the
    /// two bars [`load_bar`] draws, and nothing the built-in painter makes.
    fn count_bar_tracks(scene: &Scene) -> usize {
        match scene {
            Scene::Container(c) => {
                // R1535 — a bar is identified by its **absolute positioning**
                // as well as its height. Height alone was enough while the bar
                // was the only 10px-tall untagged leaf in the grid; the R1535
                // decoration swatch is one too (`decoration_px` is also 10), so
                // this probe counted swatches as bars until it was told what a
                // bar actually is. The stacked track/fill pair is the only
                // thing here that overlays, so the property is the real one.
                let here = usize::from(
                    c.tag.is_none()
                        && c.children.is_empty()
                        && c.layout.absolute_position.is_some()
                        && c.layout.size.height == pinion_core::style::SizeValue::Px(BAR_H),
                );
                here + c.children.iter().map(count_bar_tracks).sum::<usize>()
            }
            Scene::Scroll(s) => count_bar_tracks(&s.content),
            _ => 0,
        }
    }

    /// R1524 — data row `id` across every column, assembled from the per-cell
    /// SSOT. Test-only: production asks for the columns it paints, so nothing
    /// there ever wants a whole row.
    fn row_of(id: usize) -> Vec<String> {
        (0..HEADERS.len())
            .map(|col| cell_text(CellIndex { row: id, col }))
            .collect()
    }

    #[test]
    fn set_measured_viewport_seeds_a_nonempty_window() {
        // Sanity: a measured viewport yields a posinset-1 first body row.
        let nodes = run_access(360);
        let first_body = nodes
            .iter()
            .find(|n| n.role == AriaRole::Row && n.tag != format!("{TABLE_TAG}_hrow"));
        let first = first_body.expect("at least one body row");
        assert_eq!(
            first.position_in_set,
            Some(1),
            "top window starts at posinset 1"
        );
    }
}
