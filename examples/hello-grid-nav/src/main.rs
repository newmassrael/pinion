//! `hello-grid-nav` — R777 §5.27 **data-grid keyboard navigation at scale**.
//!
//! R775 (`hello-virtual-table`) lands a *display-only* virtualized
//! data-grid: a frozen header above a flex-viewport (`AutoSizer`)
//! virtualized body, 10 000 rows, only the window materialized. R776
//! (`hello-virtual-nav`) lands selectable keyboard navigation over a
//! virtualized *list*. This binding brings the two together: the grid
//! becomes **selectable and keyboard-navigable at scale**, the interactive
//! Model/View grid every Phase-B DCC / IDE inspector needs.
//!
//! It is pure composition — the round adds **no** new substrate:
//!
//! * selection model — the R746 [`VirtualSelectExternal`], an index-held
//!   single-select coordinator. The *same* coordinator drives the list and
//!   the grid: a grid cell click (`vtbl#<row>_<col>`) selects the **row**
//!   (WAI-ARIA / Qt `QItemSelectionModel` `SelectRows`; the column is
//!   irrelevant to a row selection).
//! * windowed body + frozen header — the R775
//!   [`view_virtual_table`](pinion_widget_paint::table::view_virtual_table),
//!   now forwarding a `selected` row so the selected strip paints accent.
//! * scroll-into-view — the R776
//!   [`scroll_offset_to_reveal`](pinion_core::widgets::virtual_list::scroll_offset_to_reveal),
//!   here getting its **second consumer**: navigating to a row that was
//!   never materialized scrolls there.
//!
//! ## Keyboard model (single-select, selection-follows-focus)
//!
//! The grid is a single tab stop (roving by data-row index). Selection is
//! the cursor — the macOS/Windows data-grid model:
//!
//! * `ArrowDown` / `ArrowUp` — move the selected row one step (clamped, no
//!   wrap).
//! * `Home` / `End` — first / last row.
//! * `PageDown` / `PageUp` — move by one measured viewport-ful of rows.
//!
//! Every move scrolls the new selection into view.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/key` `End` → `query("selected")` reports `9999`, and the
//! `scene/snapshot` window has scrolled so `vtbl_row9999` is a rendered
//! node — a row that did not exist at offset 0. A `scene/click` on a cell
//! selects its row. Pure data, no pixels (see `tools/r777_grid_nav.py`).
//!
//! ## a11y
//!
//! Single-select WAI-ARIA virtualized `grid` via the R777-lifted
//! [`windowed_grid_nodes_selected`] (shared with the display-only
//! `hello-virtual-table`): `aria-setsize = N`, one `row` per windowed index
//! with `aria-posinset` + `aria-selected = (id == selected)` and a
//! `gridcell` per column, under a frozen header row of `columnheader`s.

use pinion_a11y::{windowed_grid_nodes_selected, AccessNode, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{nav_select_key, RowMetrics, VirtualSelectExternal};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::{view_virtual_table, TableStyle, VirtualTableData};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGridNavRenderer, HelloGridNavRendererError);

/// Initial window size — freely resizable; the grid body re-windows on
/// every `Resized` event. Wide enough that `NCOLS × COL_W` fits.
const WIN_W: u32 = 400;
const WIN_H: u32 = 480;
const THEME_TAG: &str = "app";
/// Total data-row count — large while the rendered node count stays small.
const N: usize = 10_000;
/// Column count (matches `HEADERS.len()`).
const NCOLS: usize = 3;
/// Uniform column width; `NCOLS × COL_W = 330 < WIN_W` so no h-scroll.
const COL_W: u32 = 110;
/// Data-row height (the windowing pitch + the scroll-into-view pitch).
const ROW_H: u32 = 36;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Status-bar height above the grid.
const STATUS_H: u32 = 40;
/// Column header labels.
const HEADERS: [&str; NCOLS] = ["Index", "Name", "Status"];
/// Paint-root + a11y `grid` tag, and the [`VirtualSelectExternal`] anchor
/// (cell clicks on `vtbl#<id>_<col>` route here via the R51.42 composite
/// protocol).
const TABLE_TAG: &str = "vtbl";
const SCROLL_KEY: &str = "vtbl_scroll";
const STATUS_TAG: &str = "vtbl_status";

// The widget's only projected state is the selected data-row index
// (`Option<usize>`). The scroll offset + measured viewport drive their own
// repaints through the reactive `ScrollState` subscriptions the view opens.

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        ..TableStyle::m3()
    }
}

/// Synthetic cell texts for a data row (same dataset as `hello-virtual-table`).
fn row_cells(id: usize) -> Vec<String> {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    const STATUS: [&str; 3] = ["Idle", "Active", "Done"];
    vec![
        format!("{id:05}"),
        CATEGORIES[id % CATEGORIES.len()].to_string(),
        STATUS[id % STATUS.len()].to_string(),
    ]
}

/// Status bar above the grid: a literal scene-as-data readout of the
/// selected row + measured viewport. Press `End` and it reports
/// `selected 9999`, proving the selection survives a row that was never
/// materialized at boot.
fn status_bar(
    scroll: &std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
    theme: &Theme,
    selected: Option<usize>,
) -> Scene {
    let (mw, mh) = scroll.measured_viewport();
    let sel = selected.map_or_else(|| "none".to_string(), |i| i.to_string());
    let text = Scene::Text(
        TextNode::styled(
            format!("selected {sel} \u{00B7} viewport {mw}\u{00D7}{mh}"),
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

/// view-fn (§6.3): pure sync mapping `selected row -> Scene`. The dataset
/// is virtual — `view_virtual_table` invokes [`row_cells`] only for the
/// indices in the current window, whose *size* is the runtime-measured
/// viewport height.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = table_style();

    let grid = view_virtual_table(
        TABLE_TAG,
        &scroll,
        VirtualTableData {
            headers: &HEADERS,
            item_count: N,
            overscan: OVERSCAN,
            sort: None,
            sort_tag: None,
            order: None,
        },
        &theme,
        &style,
        selected,
        row_cells,
    );

    Scene::Container(
        ContainerNode::new(vec![status_bar(&scroll, &theme, selected), grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct GridNavView;

impl WidgetCore for GridNavView {
    /// The selected data-row index — the widget's entire projected state.
    type State = Option<usize>;
    type Event = ();

    /// The primary External is the R746 index-held selection coordinator,
    /// addressable at [`TABLE_TAG`]. Cell clicks on the windowed
    /// `vtbl#<id>_<col>` cells route here via the R51.42 composite protocol
    /// (selecting the row); `apply_key` drives it from the keyboard.
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    fn tag() -> &'static str {
        TABLE_TAG
    }

    /// Project the selected row off the primary coordinator. A selection
    /// change repaints; scroll offset repaints via its own reactive
    /// `Signal` subscription the view opens.
    fn read_state(scene: &Scene) -> Option<usize> {
        scene
            .find_external_with_tag(TABLE_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(|intro| match intro.query("selected") {
                Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
                _ => None,
            })
    }

    fn view(state: Option<usize>, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// The grid is a single keyboard tab stop (roving by data-row index).
    fn focusable_tags() -> Vec<&'static str> {
        vec![TABLE_TAG]
    }

    /// R777 §5.27 — keyboard navigation over the windowed grid, delegated
    /// to the shared `nav_select_key` controller (the same one
    /// `hello-virtual-nav` uses for the list): keys only route when the grid
    /// is focused (single tab stop); each handled key moves the index-model
    /// row selection (linear clamp, no wrap) and scrolls the new selection
    /// into view. The pitch is the data-row height, the pitch the body
    /// windows against.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        nav_select_key(
            scene,
            &use_scroll_state(SCROLL_KEY),
            TABLE_TAG,
            focused,
            key,
            modifiers,
            RowMetrics { item_count: N, row_pitch: ROW_H },
        )
    }

    fn title() -> &'static str {
        "pinion hello-grid-nav (R777 §5.27 data-grid keyboard navigation at scale)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=row {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GridNavView {
    /// Single-select WAI-ARIA virtualized `grid` via the R777-lifted
    /// [`windowed_grid_nodes_selected`] (shared with `hello-virtual-table`
    /// so the virtualized-grid topology is one source of truth): each
    /// windowed data row carries `aria-posinset` + `aria-selected = (id ==
    /// selected)`; one `gridcell` per column; a frozen header row of
    /// `columnheader`s. The window is the same `compute_visible_range` over
    /// the measured viewport the view fn uses, so the a11y tree and the
    /// painted tree never disagree on which rows exist.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        windowed_grid_nodes_selected(
            TABLE_TAG,
            "Navigable data grid",
            &HEADERS,
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
            *selected,
        )
    }
}

impl WidgetView for GridNavView {
    type Renderer = HelloGridNavRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridNavView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::widgets::scroll::ScrollState;
    use pinion_core::widgets::virtual_list::scroll_offset_to_reveal;
    use pinion_core::Owner;
    use std::rc::Rc;

    // Keyboard nav policy + controller (`clamp_nav` / `nav_select_key`) are
    // unit-tested in `pinion_core::widgets::virtual_select`; this binding's
    // apply_key is a thin delegation. End-to-end keyboard drive is covered
    // by `tools/r777_grid_nav.py`.

    fn run_view_with_measured(selected: Option<usize>, offset_y: i32, measured_h: u32) -> Scene {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_H).unwrap());
            scroll.set_measured_viewport(WIN_W, measured_h);
            scroll.scroll_to(0, offset_y);
            view(selected, &Frame::default())
        })
    }

    /// Find the `vtbl_row<id>` strip and return its fill color.
    fn row_fill(scene: &Scene, id: usize) -> Option<pinion_core::style::Color> {
        fn walk(scene: &Scene, want: &str) -> Option<pinion_core::style::Color> {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref() == Some(want) {
                        return Some(c.style.fill);
                    }
                    c.children.iter().find_map(|ch| walk(ch, want))
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), want),
                _ => None,
            }
        }
        walk(scene, &format!("{TABLE_TAG}_row{id}"))
    }

    #[test]
    fn selected_row_paints_accent_wash() {
        // The grid selection tint is a 16% accent wash over Surface (the
        // shared `table::row_fill` selection path), distinct from any
        // unselected row.
        let theme = pinion_core::theme::Theme::light();
        let wash = theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16);
        let scene = run_view_with_measured(Some(2), 0, 384);
        assert_eq!(row_fill(&scene, 2), Some(wash), "selected row strip is the accent wash");
        assert_ne!(row_fill(&scene, 3), Some(wash), "an unselected neighbor differs");
    }

    #[test]
    fn a11y_marks_selected_row_and_tracks_window() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 384);
            GridNavView::access_node(&Some(1), None)
        });
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(nodes[0].size_of_set, Some(u32::try_from(N).unwrap()));
        let row1 = nodes.iter().find(|n| n.tag == format!("{TABLE_TAG}_row1")).unwrap();
        assert_eq!(row1.selected, Some(true), "selected row carries aria-selected=true");
        let row0 = nodes.iter().find(|n| n.tag == format!("{TABLE_TAG}_row0")).unwrap();
        assert_eq!(row0.selected, Some(false));
    }

    #[test]
    fn reveal_scrolls_a_deep_target_into_view() {
        // Selecting the last row and revealing it moves the offset deep so
        // the window now includes row 9999 — never materialized at offset 0.
        let s = Rc::new(ScrollState::new());
        s.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_H).unwrap());
        let measured_h = 384;
        let reveal = scroll_offset_to_reveal(N - 1, 0, measured_h, ROW_H);
        s.scroll_to(0, reveal);
        let window = compute_visible_range(s.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        assert!(
            window.indices().any(|i| i == N - 1),
            "after reveal, the last row is inside the window {window:?}",
        );
    }
}
