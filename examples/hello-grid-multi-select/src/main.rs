//! `hello-grid-multi-select` — R782 §5.40 **multi-select data-grid at scale**.
//!
//! R777 (`hello-grid-nav`) lands a *single*-select virtualized data-grid:
//! exactly one row at a time, selection-follows-focus, the macOS/Windows
//! data-grid cursor model. R780 (`hello-multi-select`) lands the multi-select
//! index model over a virtualized *list*, and R781 wires keyboard-modifier
//! click into the pointer send wire. This binding brings them together: the
//! same [`VirtualSelectExternal`] in **`Multi`** mode (the Qt
//! `QItemSelectionModel` `ExtendedSelection` analogue — a selection *set*
//! plus an `anchor` for range extension) drives a 10 000-row data-grid, and
//! the selection is an arbitrary set of **data rows**:
//!
//! * **plain `ArrowDown` / `ArrowUp` / `Home` / `End` / `Page`** — move the
//!   active row and replace the selection with just it (resetting the
//!   `anchor`), exactly as R777.
//! * **`Shift` + nav** — extend the contiguous range from the `anchor` to the
//!   navigated row.
//! * **`Ctrl+A`** — select every one of the 10 000 rows.
//! * **`Ctrl+Space`** — toggle the active row's membership.
//! * **plain click** on a cell — select that cell's row (column irrelevant,
//!   WAI-ARIA / Qt `SelectRows`).
//! * **`Ctrl`-click** — toggle the clicked row's membership.
//! * **`Shift`-click** — extend the range from the `anchor` to the clicked
//!   row.
//!
//! The round adds **no** coordinator code: [`VirtualSelectExternal`] already
//! handles grid-cell sends (`vtbl#<row>_<col>` → the row, R777) and is
//! already modifier-aware on the pointer wire (R781). The grid simply
//! constructs it in `Multi` mode and projects the selection *set*. The two
//! substrate touches this round are (1) the R780/R781-carry a11y lift
//! [`windowed_grid_nodes_multiselected`] (the second windowed-grid
//! multi-select consumer after the eager `hello-table-multi`), and (2) the
//! generalization of `view_virtual_table`'s selection to a row predicate so
//! one virtualized-grid paint path serves single + multi.
//!
//! The coordinator holds the selection as a `BTreeSet` of **data indices**
//! (no per-row state, decoupled from materialization). The view projects
//! that into a Copy per-row bitmap purely as the paint snapshot the shell
//! hands into the view fn — the same shape `hello-multi-select` /
//! `hello-table-multi` use.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/key` `End` then `Shift`+`Home` → `query("selection")` reports the
//! full `[0, 1, …, 9999]` span, and `query("mode")` reports `"multi"`. A
//! `scene/modifiers` (`ctrl`) + `scene/click` on a cell toggles that row into
//! the set without disturbing the rest. Pure data, no pixels (see
//! `tools/r782_grid_multi_select.py`).
//!
//! ## a11y
//!
//! Multi-select WAI-ARIA virtualized `grid` via the R782-lifted
//! [`windowed_grid_nodes_multiselected`]: `aria-setsize = N`, the grid
//! container `aria-multiselectable`, each rendered data row an
//! `AriaRole::Row` with `aria-posinset` + `aria-selected =
//! selection.contains(id)` (so several visible rows can be `aria-selected` at
//! once) and a `gridcell` per column, under a frozen header row of
//! `columnheader`s. The grid is the focusable tab stop.

use pinion_a11y::{AccessNode, WidgetA11y, windowed_grid_nodes_multiselected};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{
    RowMetrics, VirtualSelectExternal, nav_select_key, read_selection,
};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::{
    CellIndex, GridModel, GridScroll, TableStyle, VirtualTableData, header_from_slice,
    no_decoration, no_edit, no_header_decoration, view_virtual_table,
};
use std::collections::BTreeSet;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(
    HelloGridMultiSelectRenderer,
    HelloGridMultiSelectRendererError
);

/// Initial window size — freely resizable; the grid body re-windows on every
/// `Resized` event (R774 `AutoSizer`). Wide enough that `NCOLS × COL_W` fits.
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
/// Paint-root + a11y `grid` tag, and the multi-select [`VirtualSelectExternal`]
/// anchor (cell clicks on `vtbl#<row>_<col>` route here via the R51.42
/// composite protocol, carrying the R781 modifier token).
const TABLE_TAG: &str = "vtbl";
const SCROLL_KEY: &str = "vtbl_scroll";
/// R784 — outer horizontal scroll `ScrollState` cache key (columns fit
/// the window here, so `max_x` stays 0 — wiring present for parity).
const H_SCROLL_KEY: &str = "vtbl_hscroll";
const STATUS_TAG: &str = "vtbl_status";

/// Copy paint-snapshot of the coordinator's selection: a per-row bitmap
/// indexed by data row, plus the running total. The coordinator itself holds
/// the selection as a `BTreeSet` of indices (no per-row state); this
/// projection exists only so the shell can hand a `Copy` snapshot into the
/// view fn without lifetime gymnastics — exactly the shape `hello-multi-select`
/// projects for its virtualized list (and `hello-table-multi` for its eager
/// grid).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct MultiSelection {
    /// Per-row selection bit, indexed by absolute data row.
    selected: [bool; N],
    /// Total selected rows (the status readout).
    total: usize,
}

impl MultiSelection {
    fn empty() -> Self {
        Self {
            selected: [false; N],
            total: 0,
        }
    }

    /// Build the bitmap from a list of selected data indices (out-of-range
    /// dropped, duplicates idempotent).
    fn from_indices(indices: &[usize]) -> Self {
        let mut s = Self::empty();
        for &i in indices {
            if i < N && !s.selected[i] {
                s.selected[i] = true;
                s.total += 1;
            }
        }
        s
    }

    fn is_selected(&self, index: usize) -> bool {
        index < N && self.selected[index]
    }
}

fn table_style() -> TableStyle {
    TableStyle {
        col_width: COL_W,
        row_height: ROW_H,
        // R1020 §5.39 — the grid is a single Tab stop; opt the table into the
        // scene-derived focus enumeration so its TABLE_TAG is collected.
        focusable: true,
        ..TableStyle::m3()
    }
}

/// Synthetic cell texts for a data row (same dataset as `hello-grid-nav`).
fn cell_text(c: CellIndex) -> String {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    const STATUS: [&str; 3] = ["Idle", "Active", "Done"];
    let id = c.row;
    match c.col {
        0 => format!("{id:05}"),
        1 => CATEGORIES[id % CATEGORIES.len()].to_string(),
        _ => STATUS[id % STATUS.len()].to_string(),
    }
}

/// Status bar above the grid: a literal scene-as-data readout of the
/// selection cardinality + measured viewport. `Ctrl+A` and it reports
/// `selected 10000 rows`, proving the set survives rows that were never
/// materialized.
fn status_bar(
    scroll: &std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
    theme: &Theme,
    total: usize,
) -> Scene {
    let (mw, mh) = scroll.measured_viewport();
    let noun = if total == 1 { "row" } else { "rows" };
    let text = Scene::Text(
        TextNode::styled(
            format!("selected {total} {noun} \u{00B7} viewport {mw}\u{00D7}{mh}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG),
    );
    Scene::Container(
        ContainerNode::new(vec![text])
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

/// view-fn (§6.3): pure sync mapping `selection bitmap -> Scene`. The dataset
/// is virtual — `view_virtual_table` invokes [`cell_text`] only for the
/// indices in the current window, whose *size* is the runtime-measured
/// viewport height. Every selected row's strip washes accent; several
/// visible rows can wash at once in multi-select.
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(selection: &MultiSelection, _frame: &Frame) -> Scene {
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
            delegate: None,
            editing: None,
        },
        &theme,
        &style,
        |id| selection.is_selected(id),
        GridModel {
            cell: cell_text,
            header: header_from_slice(&HEADERS),
            header_decoration: no_header_decoration,
            decoration: no_decoration,
            edit: no_edit,
        },
    );

    Scene::Container(
        ContainerNode::new(vec![status_bar(&scroll, &theme, selection.total), grid])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct GridMultiSelectView;

impl WidgetCore for GridMultiSelectView {
    /// The selection bitmap — the widget's entire projected paint state.
    type State = MultiSelection;
    type Event = ();

    /// The primary External is the R746 index-held coordinator, built in
    /// R780 **`Multi`** mode at [`TABLE_TAG`]. Cell clicks on the windowed
    /// `vtbl#<id>_<col>` cells route here via the R51.42 composite protocol
    /// (selecting / toggling / extending the row per the R781 modifier
    /// token); `apply_key` drives it from the keyboard.
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new_multi(N))
    }

    fn tag() -> &'static str {
        TABLE_TAG
    }

    /// Project the selection set off the primary coordinator's `selection`
    /// JSON-array query into the Copy paint bitmap. A selection change
    /// repaints; scroll offset repaints via its own reactive `Signal`
    /// subscription the view opens.
    fn read_state(scene: &Scene) -> MultiSelection {
        let indices = scene
            .find_external_with_tag(TABLE_TAG)
            .and_then(|node| node.handle.introspect())
            .map(read_selection)
            .unwrap_or_default();
        MultiSelection::from_indices(&indices)
    }

    fn view(state: MultiSelection, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// R782 §5.40 — keyboard multi-select over the windowed grid, delegated
    /// to the shared `nav_select_key` controller (the same one
    /// `hello-virtual-nav` / `hello-grid-nav` / `hello-multi-select` use).
    /// Because the coordinator is in `Multi` mode, the `modifiers` now
    /// matter: `Shift`+nav extends a range, `Ctrl+A` selects all,
    /// `Ctrl+Space` toggles the active row. Each handled key scrolls the new
    /// active row into view; the pitch is the data-row height.
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
            RowMetrics {
                item_count: N,
                row_pitch: ROW_H,
            },
        )
    }

    fn title() -> &'static str {
        "pinion hello-grid-multi-select (R782 §5.40 multi-select data-grid at scale)"
    }

    fn fmt_state_log(state: &MultiSelection) -> String {
        format!("selected={} rows", state.total)
    }
}

impl WidgetA11y for GridMultiSelectView {
    /// Multi-select WAI-ARIA virtualized `grid` via the R782-lifted
    /// [`windowed_grid_nodes_multiselected`] (shared with `hello-virtual-table`
    /// / `hello-grid-nav` so the virtualized-grid topology is one source of
    /// truth): the grid container is `aria-multiselectable`; each windowed
    /// data row carries `aria-posinset` + `aria-selected =
    /// selection.contains(id)` (several visible rows at once); one `gridcell`
    /// per column; a frozen header row of `columnheader`s. The window is the
    /// same `compute_visible_range` over the measured viewport the view fn
    /// uses, so the a11y tree and the painted tree never disagree on which
    /// rows exist.
    fn access_node(selection: &MultiSelection, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_H, OVERSCAN);
        let windowed_set: BTreeSet<usize> = window
            .indices()
            .filter(|&i| selection.is_selected(i))
            .collect();
        windowed_grid_nodes_multiselected(
            TABLE_TAG,
            "Multi-selectable data grid",
            HEADERS.len(),
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
            &windowed_set,
        )
    }
}

impl WidgetView for GridMultiSelectView {
    type Renderer = HelloGridMultiSelectRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GridMultiSelectView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    // Keyboard multi-select policy + controller (`clamp_nav` / `nav_select_key`)
    // and modifier-aware click are unit-tested in
    // `pinion_core::widgets::virtual_select`; this binding's apply_key is a
    // thin delegation. End-to-end keyboard + pointer drive is covered by
    // `tools/r782_grid_multi_select.py`.

    fn run_view_with_measured(indices: &[usize], offset_y: i32, measured_h: u32) -> Scene {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_H).unwrap());
            scroll.set_measured_viewport(WIN_W, measured_h);
            scroll.scroll_to(0, offset_y);
            view(&MultiSelection::from_indices(indices), &Frame::default())
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
    fn several_selected_rows_wash_accent_at_once() {
        // The grid selection tint is a 16% accent wash over Surface (the
        // shared `table::row_fill` selection path). In multi-select several
        // visible rows wash at once.
        let theme = pinion_core::theme::Theme::light();
        let wash = theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16);
        let scene = run_view_with_measured(&[1, 3], 0, 384);
        assert_eq!(row_fill(&scene, 1), Some(wash), "row 1 washes accent");
        assert_eq!(row_fill(&scene, 3), Some(wash), "row 3 washes accent too");
        assert_ne!(
            row_fill(&scene, 2),
            Some(wash),
            "row 2 between them does not"
        );
    }

    #[test]
    fn a11y_marks_multiselectable_and_every_member() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 384);
            GridMultiSelectView::access_node(&MultiSelection::from_indices(&[0, 2]), None)
        });
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(nodes[0].size_of_set, Some(u32::try_from(N).unwrap()));
        assert!(
            nodes[0].multiselectable,
            "container is aria-multiselectable"
        );
        let selected_count = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.selected == Some(true))
            .count();
        assert_eq!(selected_count, 2, "both members are aria-selected at once");
    }
}
