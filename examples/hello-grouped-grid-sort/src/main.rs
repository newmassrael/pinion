//! `hello-grouped-grid-sort` — R846 §5.27 §5.40 **sortable grouped data grid**
//! (the asset table).
//!
//! R845 renders the R843 group-by flatten as a multi-column grid; R844 makes
//! grouping compose over an upstream proxy's live order. R846 ties them into
//! the production asset table: **clickable column headers** sort the 10,000
//! rows through a [`GridSortState`] (R778), and that sorted order feeds
//! [`GroupOrderState::with_order_source`] — so rows sort **within** their
//! asset-type groups. This is the **second consumer** of the R844 order-source
//! (a *grid* sort proxy upstream where R844 used a 1-D list proxy), proving the
//! order-source abstraction is not `ViewOrderState`-specific.
//!
//! Three click semantics coexist over one grid: a **column header**
//! (`gsort#h<col>`) cycles that column's sort; a **group header**
//! (`ggrp#<group>`) toggles collapse; a **data row** (`ggrid#<source>`) selects
//! the row — sort ⊥ grouping ⊥ selection, all data-indexed.
//!
//! ## Coordinators
//!
//! - **Primary** [`VirtualSelectExternal`] at [`GRID_TAG`] — row selection by
//!   source index.
//! - **Extra** [`GridSortExternal`] at [`SORT_TAG`] — the column-sort proxy;
//!   the clicked column header cycles its sort (`cell_cmp` is numeric-aware
//!   with a lexicographic fallback, so Name sorts lexically, Size numerically).
//! - **Extra** [`GroupOrderExternal`] at [`GROUP_TAG`] — groups the *sorted*
//!   order by asset type; a clicked group header toggles collapse.
//! - **Extra** [`ScrollBarExternal`].
//!
//! ## a11y
//!
//! A hand-rolled WAI-ARIA [`AriaRole::Grid`] (as in `hello-grouped-grid`): the
//! header row's [`AriaRole::ColumnHeader`]s carry `aria-sort` on the active
//! sort column; group headers are spanning [`AriaRole::Row`]s with
//! `aria-expanded`; data rows are [`AriaRole::Row`]s (`aria-selected`) with
//! [`AriaRole::GridCell`] children.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, SortDirection, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::grid_sort::{use_grid_sort, GridSortExternal, GridSortState};
use pinion_core::widgets::group_order::{GroupOrderExternal, GroupOrderState, GroupRow};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{read_selected, VirtualSelectExternal};
use pinion_core::reactive::Owner;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_virtual_list;
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGroupedGridSortRenderer, HelloGroupedGridSortRendererError);

const WIN_W: u32 = 400;
const WIN_H: u32 = 540;
const THEME_TAG: &str = "app";
const N: usize = 10_000;
const ROW_PITCH: u32 = 28;
const OVERSCAN: usize = 3;

const COLS: [&str; 3] = ["Name", "Size", "Modified"];
const COL_W: [u32; 3] = [150, 90, 80];
const NCOLS: usize = 3;
const GRID_W: u32 = 320;
const VIEWPORT_H: u32 = 13 * ROW_PITCH;

/// Paint-root + a11y `grid` + primary [`VirtualSelectExternal`] tag.
const GRID_TAG: &str = "ggrid";
/// The [`GroupOrderExternal`] anchor (`ggrp#<group>` headers).
const GROUP_TAG: &str = "ggrp";
/// The [`GridSortExternal`] anchor (`gsort#h<col>` clickable column headers).
const SORT_TAG: &str = "gsort";
const SCROLL_KEY: &str = "ggrid_scroll";
const SCROLLBAR_TAG: &str = "ggrid_scrollbar";

const GROUPS: [&str; 6] = ["Mesh", "Texture", "Material", "Sound", "Script", "Prefab"];

const CHEVRON_EXPANDED: &str = "\u{25BC}";
const CHEVRON_COLLAPSED: &str = "\u{25B6}";
const ARROW_ASC: &str = "\u{25B2}"; // ▲
const ARROW_DESC: &str = "\u{25BC}"; // ▼
const ARROW_NONE: &str = "\u{2195}"; // ↕ sortable, unsorted

fn row_group(i: usize) -> usize {
    i % GROUPS.len()
}

/// The cell value of row `source` in column `col` (0=Name, 1=Size, 2=Modified).
/// Size is a bare integer so `cell_cmp` orders it numerically; Name/Modified
/// are text (lexicographic).
fn cell_value(source: usize, col: usize) -> String {
    match col {
        0 => format!("asset_{source:05}"),
        1 => format!("{}", (source * 13) % 990 + 8),
        _ => format!("day {:03}", source % 90),
    }
}

fn cell_tag(source: usize, col: usize) -> String {
    format!("gcell_{source}_{col}")
}

/// The shared column-sort proxy over the materialized cell grid.
fn use_grid_data() -> Rc<GridSortState> {
    use_grid_sort(SORT_TAG, || {
        let cells: Vec<Vec<String>> =
            (0..N).map(|i| (0..NCOLS).map(|c| cell_value(i, c)).collect()).collect();
        (NCOLS, cells)
    })
}

/// The shared group-by proxy grouping the **sorted** order. The
/// [`GridSortState`] is resolved before the group cache factory (R666
/// no-nested-cache); the factory captures it and groups its live `order()`
/// (the 2nd consumer of [`GroupOrderState::with_order_source`]).
fn use_grid_groups() -> Rc<GroupOrderState> {
    let grid = use_grid_data();
    Owner::current()
        .expect("use_grid_groups requires an active Owner scope")
        .cache(GROUP_TAG, move || {
            let groups = (0..N).map(row_group).collect::<Vec<usize>>();
            let labels = GROUPS.iter().map(|&g| g.to_string()).collect::<Vec<String>>();
            GroupOrderState::with_tag(GROUP_TAG, groups, labels).with_order_source(move || grid.order())
        })
}

/// The active sort direction of column `col`, or `None` when it is not the
/// sort column (drives the header glyph + `aria-sort`).
fn col_sort_dir(sort: Option<(usize, bool)>, col: usize) -> Option<bool> {
    match sort {
        Some((c, dir)) if c == col => Some(dir),
        _ => None,
    }
}

/// The clickable column-header row: each header (`gsort#h<col>`) shows its
/// label + a sort glyph and cycles that column's sort on click.
fn column_header_row(sort: Option<(usize, bool)>, theme: &Theme) -> Scene {
    let mut cells: Vec<Scene> = Vec::with_capacity(NCOLS);
    for (c, &name) in COLS.iter().enumerate() {
        let glyph = match col_sort_dir(sort, c) {
            Some(true) => ARROW_ASC,
            Some(false) => ARROW_DESC,
            None => ARROW_NONE,
        };
        let label = Scene::Text(TextNode::styled(
            format!("{name} {glyph}"),
            Rect::default(),
            TextStyle::new().with_size_px(13).with_fg(theme.resolve(ColorRole::OnSurface)),
        ));
        cells.push(Scene::Container(
            ContainerNode::new(vec![label])
                .with_tag(format!("{SORT_TAG}#h{c}"))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(COL_W[c], ROW_PITCH))
                        .with_padding(Rect::new(10, 0, 6, 0)),
                ),
        ));
    }
    Scene::Container(
        ContainerNode::new(cells)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_size(Size::px(GRID_W, ROW_PITCH))),
    )
}

fn build_header(group: usize, member_count: usize, collapsed: bool, theme: &Theme) -> Scene {
    let chevron = if collapsed { CHEVRON_COLLAPSED } else { CHEVRON_EXPANDED };
    let text = format!("{chevron}  {}  ({member_count})", GROUPS[group % GROUPS.len()]);
    let label = Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{GROUP_TAG}#{group}"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(GRID_W, ROW_PITCH))
                    .with_padding(Rect::new(10, 0, 10, 0)),
            ),
    )
}

fn build_data_row(source: usize, theme: &Theme, selected: Option<usize>) -> Scene {
    let is_selected = selected == Some(source);
    let (fill, fg) = if is_selected {
        (theme.resolve(ColorRole::Accent), theme.resolve(ColorRole::OnAccent))
    } else {
        let stripe = if source % 2 == 0 { ColorRole::SurfaceContainerLow } else { ColorRole::Surface };
        (theme.resolve(stripe), theme.resolve(ColorRole::OnSurface))
    };
    let mut cells: Vec<Scene> = Vec::with_capacity(NCOLS);
    for (c, &width) in COL_W.iter().enumerate() {
        let text = Scene::Text(TextNode::styled(
            cell_value(source, c),
            Rect::default(),
            TextStyle::new().with_size_px(13).with_fg(fg),
        ));
        cells.push(Scene::Container(
            ContainerNode::new(vec![text])
                .with_tag(cell_tag(source, c))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(width, ROW_PITCH))
                        .with_padding(Rect::new(if c == 0 { 24 } else { 10 }, 0, 6, 0))
                        .with_pointer_transparent(true),
                ),
        ));
    }
    Scene::Container(
        ContainerNode::new(cells)
            .with_tag(format!("{GRID_TAG}#{source}"))
            .with_style(BoxStyle::filled(fill))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_size(Size::px(GRID_W, ROW_PITCH))),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();

    let grid = use_grid_data();
    let groups = use_grid_groups();
    let rows = groups.rows();
    let visible_len = rows.len();

    let header = column_header_row(grid.sort(), &theme);

    let list = view_virtual_list(
        &scroll_state,
        Rect::new(0, 0, GRID_W, VIEWPORT_H),
        visible_len,
        ROW_PITCH,
        OVERSCAN,
        |view_pos| match rows[view_pos] {
            GroupRow::Header { group, member_count, collapsed } => {
                build_header(group, member_count, collapsed, &theme)
            }
            GroupRow::Data { source } => build_data_row(source, &theme, selected),
        },
    );

    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual =
        view_vertical_scrollbar(&scroll_state, &theme, &scrollbar_style, scrollbar_interaction.get());

    let list_row = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    let grid_root = Scene::Container(
        ContainerNode::new(vec![header, list_row])
            .with_tag(GRID_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    );

    Scene::Container(
        ContainerNode::new(vec![grid_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

struct GroupedGridSortView;

impl WidgetCore for GroupedGridSortView {
    type State = Option<usize>;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(SORT_TAG, Box::new(GridSortExternal::new(use_grid_data()))),
            ExtraExternal::new(GROUP_TAG, Box::new(GroupOrderExternal::new(use_grid_groups()))),
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
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
        "pinion hello-grouped-grid-sort (R846 §5.27 §5.40 sortable grouped grid)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=source {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GroupedGridSortView {
    /// WAI-ARIA `grid` (see `hello-grouped-grid`): the header row's
    /// [`AriaRole::ColumnHeader`]s carry `aria-sort` on the active sort column;
    /// group headers are spanning expandable [`AriaRole::Row`]s; data rows are
    /// [`AriaRole::Row`]s with [`AriaRole::GridCell`] children.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let selected = *selected;
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let grid = use_grid_data();
        let sort = grid.sort();
        let groups = use_grid_groups();
        let rows = groups.rows();
        let visible_len = rows.len();
        let window =
            compute_visible_range(scroll_state.offset_y(), VIEWPORT_H, visible_len, ROW_PITCH, OVERSCAN);
        let total = u32::try_from(visible_len).unwrap_or(u32::MAX);

        let mut nodes: Vec<AccessNode> = Vec::new();

        let mut g = AccessNode::new(GRID_TAG, AriaRole::Grid).with_name("Sortable grouped asset grid");
        g = g.with_child(format!("{SORT_TAG}#hrow"));
        for view_pos in window.indices() {
            let tag = match rows[view_pos] {
                GroupRow::Header { group, .. } => format!("{GROUP_TAG}#{group}"),
                GroupRow::Data { source } => format!("{GRID_TAG}#{source}"),
            };
            g = g.with_child(tag);
        }
        nodes.push(g);

        // Header row + its (sortable) column headers.
        let mut header_row = AccessNode::new(format!("{SORT_TAG}#hrow"), AriaRole::Row);
        for c in 0..NCOLS {
            header_row = header_row.with_child(format!("{SORT_TAG}#h{c}"));
        }
        nodes.push(header_row);
        for (c, &name) in COLS.iter().enumerate() {
            let mut col = AccessNode::new(format!("{SORT_TAG}#h{c}"), AriaRole::ColumnHeader).with_name(name);
            col = match col_sort_dir(sort, c) {
                Some(true) => col.with_sort(SortDirection::Ascending),
                Some(false) => col.with_sort(SortDirection::Descending),
                None => col,
            };
            nodes.push(col);
        }

        for view_pos in window.indices() {
            let posinset = u32::try_from(view_pos + 1).unwrap_or(u32::MAX);
            match rows[view_pos] {
                GroupRow::Header { group, member_count, collapsed } => {
                    nodes.push(
                        AccessNode::new(format!("{GROUP_TAG}#{group}"), AriaRole::Row)
                            .with_name(format!("{} ({member_count})", GROUPS[group % GROUPS.len()]))
                            .with_position_in_set(posinset)
                            .with_size_of_set(total)
                            .with_expanded(!collapsed),
                    );
                }
                GroupRow::Data { source } => {
                    let mut row = AccessNode::new(format!("{GRID_TAG}#{source}"), AriaRole::Row)
                        .with_position_in_set(posinset)
                        .with_size_of_set(total)
                        .with_selected(selected == Some(source));
                    for c in 0..NCOLS {
                        row = row.with_child(cell_tag(source, c));
                    }
                    nodes.push(row);
                    for (c, &name) in COLS.iter().enumerate() {
                        nodes.push(
                            AccessNode::new(cell_tag(source, c), AriaRole::GridCell)
                                .with_name(format!("{name}: {}", cell_value(source, c))),
                        );
                    }
                }
            }
        }
        nodes
    }
}

impl WidgetView for GroupedGridSortView {
    type Renderer = HelloGroupedGridSortRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<GroupedGridSortView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render with the column sort pre-set (mutated within the same Owner scope
    /// the view + group state read from).
    fn render(selected: Option<usize>, sort: Option<(usize, bool)>) -> Scene {
        Owner::new().run(|| {
            let grid = use_grid_data();
            grid.set_sort(sort);
            view(selected, &Frame::default())
        })
    }

    fn present_tags(scene: &Scene) -> Vec<String> {
        fn walk(scene: &Scene, out: &mut Vec<String>) {
            match scene {
                Scene::Container(c) => {
                    if let Some(t) = c.tag.as_deref() {
                        out.push(t.to_string());
                    }
                    c.children.iter().for_each(|ch| walk(ch, out));
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    /// The data-row sources present, in tree (= visual) order.
    fn data_sources_ordered(scene: &Scene) -> Vec<usize> {
        present_tags(scene)
            .iter()
            .filter_map(|t| t.strip_prefix(&format!("{GRID_TAG}#")).and_then(|r| r.parse::<usize>().ok()))
            .collect()
    }

    #[test]
    fn boot_has_sortable_column_headers_and_source_order() {
        let scene = render(None, None);
        let tags = present_tags(&scene);
        for c in 0..NCOLS {
            assert!(tags.contains(&format!("{SORT_TAG}#h{c}")), "sortable column header {c} present");
        }
        // Unsorted: the first Mesh members are in source order.
        let mut srcs = data_sources_ordered(&scene);
        srcs.sort_unstable();
        assert_eq!(&srcs[..3], &[0, 6, 12], "unsorted Mesh members in source order");
    }

    #[test]
    fn descending_name_sort_reverses_within_groups() {
        // Sort Name (col 0) descending = reverse source order. Source 9999
        // leads (group 9999 % 6 == 3); within each group rows go high -> low.
        let scene = render(None, Some((0, false)));
        let ordered = data_sources_ordered(&scene);
        assert_eq!(ordered[0], 9999, "descending Name puts the highest source first");
        // The first group's members descend (all share a group; strictly
        // decreasing source indices).
        let g0 = 9999 % GROUPS.len();
        let first_group: Vec<usize> = ordered.iter().copied().take_while(|&s| s % GROUPS.len() == g0).collect();
        assert!(
            first_group.windows(2).all(|w| w[0] > w[1]),
            "members descend within the leading group: {first_group:?}",
        );
    }

    #[test]
    fn a11y_active_sort_column_carries_aria_sort() {
        let nodes = Owner::new().run(|| {
            let grid = use_grid_data();
            grid.set_sort(Some((1, true))); // Size ascending
            GroupedGridSortView::access_node(&None, None)
        });
        let col1 = nodes
            .iter()
            .find(|n| n.tag == format!("{SORT_TAG}#h1"))
            .expect("Size column header node");
        assert_eq!(col1.sort, Some(SortDirection::Ascending), "active column carries aria-sort");
        let col0 = nodes.iter().find(|n| n.tag == format!("{SORT_TAG}#h0")).expect("Name column header");
        assert_eq!(col0.sort, None, "inactive column has no aria-sort");
    }

    #[test]
    fn selection_survives_a_column_sort() {
        let theme = pinion_core::theme::Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        // Select source 9999, sort Name descending: 9999 leads the view (its
        // group heads the descending flatten), so its row stays visible and
        // Accent — selection (by source index) is orthogonal to the sort.
        let scene = render(Some(9999), Some((0, false)));
        let fill = {
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
            walk(&scene, &format!("{GRID_TAG}#9999"))
        };
        assert_eq!(fill, Some(accent), "selected source 9999 stays Accent after the sort");
    }
}
