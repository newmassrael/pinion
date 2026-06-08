//! `hello-grouped-grid` — R845 §5.27 §5.40 **grouped multi-column data grid**.
//!
//! R843 lands the group-by [`GroupRow`] flatten and proves it in a 1-D list
//! (`hello-grouped-list`); R844 stacks it over a sort/filter order. R845 is the
//! flatten's **second structural consumer**: a multi-column data grid (Name /
//! Size / Modified) grouped by asset type, with collapsible **group-header rows
//! spanning the grid width** between the data rows. The same
//! [`GroupOrderState::rows`] flattening drives a different *rendering* — data
//! rows paint cells across the columns, header rows span them — validating that
//! the R843 proxy is not list-specific (abstraction-needs-a-second-consumer).
//! No new framework primitive: the grid is pure composition over
//! [`view_virtual_list`] + the existing [`GroupOrderExternal`] +
//! [`VirtualSelectExternal`].
//!
//! ## The coordinators
//!
//! - **Primary** [`VirtualSelectExternal`] at [`GRID_TAG`] — **row** selection
//!   by source index. Each data row is the click target; its cells are
//!   `pointer_transparent` so a click anywhere in the row selects it.
//! - **Extra** [`GroupOrderExternal`] at [`GROUP_TAG`] — the group-by proxy
//!   (group id = the row's asset type); a clicked group header (`ggrp#<group>`)
//!   toggles collapse.
//! - **Extra** [`ScrollBarExternal`].
//!
//! ## a11y (WAI-ARIA `grid`)
//!
//! Hand-rolled rather than reusing
//! [`pinion_a11y::grid_table_nodes`](pinion_a11y::grid_table_nodes): that
//! builder models a *flat* grid (header row + uniform data rows) with no
//! collapsible group-header-row concept, and pinion's a11y has no `treegrid`
//! role. So this emits an [`AriaRole::Grid`] with an
//! [`AriaRole::ColumnHeader`] header row, a spanning [`AriaRole::Row`] +
//! `aria-expanded` per group header, and an [`AriaRole::Row`] +
//! [`AriaRole::GridCell`] children per data row (`aria-selected` on the row).
//! The grouped-grid a11y shape (group-header rows interleaved with cell rows)
//! is distinct from the grouped-*list*'s `tree`/`treeitem`
//! (`hello-grouped-list`/`hello-grouped-sort`), so the two do not share a
//! builder.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::group_order::{use_group_order, GroupOrderExternal, GroupOrderState, GroupRow};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{read_selected, VirtualSelectExternal};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_virtual_list;
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGroupedGridRenderer, HelloGroupedGridRendererError);

const WIN_W: u32 = 400;
const WIN_H: u32 = 540;
const THEME_TAG: &str = "app";
const N: usize = 10_000;
const ROW_PITCH: u32 = 28;
const OVERSCAN: usize = 3;

/// Three columns: Name / Size / Modified. Their widths sum to the grid width.
const COLS: [&str; 3] = ["Name", "Size", "Modified"];
const COL_W: [u32; 3] = [150, 90, 80];
const GRID_W: u32 = 320; // COL_W sum
const VIEWPORT_H: u32 = 13 * ROW_PITCH;

/// Paint-root + a11y `grid` container + primary [`VirtualSelectExternal`] tag.
const GRID_TAG: &str = "ggrid";
/// The [`GroupOrderExternal`] anchor; group headers route here (`ggrp#<group>`).
const GROUP_TAG: &str = "ggrp";
/// The a11y column-header row tag (not a paint click target).
const HEAD_ROW_TAG: &str = "ggrid_head";
const SCROLL_KEY: &str = "ggrid_scroll";
const SCROLLBAR_TAG: &str = "ggrid_scrollbar";

/// Six asset-type groups (the group facet). Row `i` is `GROUPS[i % 6]`.
const GROUPS: [&str; 6] = ["Mesh", "Texture", "Material", "Sound", "Script", "Prefab"];

/// `\u{25BC}` ▼ — an expanded group's disclosure chevron.
const CHEVRON_EXPANDED: &str = "\u{25BC}";
/// `\u{25B6}` ▶ — a collapsed group's disclosure chevron.
const CHEVRON_COLLAPSED: &str = "\u{25B6}";

fn row_group(i: usize) -> usize {
    i % GROUPS.len()
}

/// The cell value of row `source` in column `col` (0=Name, 1=Size, 2=Modified).
fn cell_value(source: usize, col: usize) -> String {
    match col {
        0 => format!("asset_{source:05}"),
        1 => format!("{} KB", (source * 13) % 990 + 8),
        _ => format!("day {}", source % 90),
    }
}

/// The a11y tag of cell `(source, col)` — a non-composite tag (no `#`) on a
/// `pointer_transparent` container, so it carries an a11y bound without
/// intercepting the row's selection click.
fn cell_tag(source: usize, col: usize) -> String {
    format!("gcell_{source}_{col}")
}

/// The shared [`GroupOrderState`] grouping rows by asset type (source order).
fn use_grid_groups() -> Rc<GroupOrderState> {
    use_group_order(GROUP_TAG, || {
        let groups = (0..N).map(row_group).collect::<Vec<usize>>();
        let labels = GROUPS.iter().map(|&g| g.to_string()).collect::<Vec<String>>();
        (groups, labels)
    })
}

/// The fixed column-header row (Name / Size / Modified), width-matched to the
/// data cells so the columns line up.
fn column_header_row(theme: &Theme) -> Scene {
    let mut cells: Vec<Scene> = Vec::with_capacity(COLS.len());
    for (c, &name) in COLS.iter().enumerate() {
        let label = Scene::Text(TextNode::styled(
            name.to_string(),
            Rect::default(),
            TextStyle::new().with_size_px(13).with_fg(theme.resolve(ColorRole::OnSurface)),
        ));
        cells.push(Scene::Container(
            ContainerNode::new(vec![label])
                .with_tag(format!("{GRID_TAG}_col{c}"))
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
            .with_tag(HEAD_ROW_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_size(Size::px(GRID_W, ROW_PITCH)),
            ),
    )
}

/// One group-header row (`ggrp#<group>`): chevron + type + member count,
/// spanning the whole grid width.
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

/// One data row (`ggrid#<source>`): a flex row of `pointer_transparent` cells
/// across the columns. The row is the click target (selecting the source); the
/// cells are transparent so a click anywhere in the row reaches it.
fn build_data_row(source: usize, theme: &Theme, selected: Option<usize>) -> Scene {
    let is_selected = selected == Some(source);
    let (fill, fg) = if is_selected {
        (theme.resolve(ColorRole::Accent), theme.resolve(ColorRole::OnAccent))
    } else {
        let stripe = if source % 2 == 0 { ColorRole::SurfaceContainerLow } else { ColorRole::Surface };
        (theme.resolve(stripe), theme.resolve(ColorRole::OnSurface))
    };
    let mut cells: Vec<Scene> = Vec::with_capacity(COLS.len());
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
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_size(Size::px(GRID_W, ROW_PITCH)),
            ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();

    let groups = use_grid_groups();
    let rows = groups.rows();
    let visible_len = rows.len();

    let header = column_header_row(&theme);

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

struct GroupedGridView;

impl WidgetCore for GroupedGridView {
    type State = Option<usize>;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
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
        "pinion hello-grouped-grid (R845 §5.27 §5.40 grouped data grid)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=source {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GroupedGridView {
    /// WAI-ARIA `grid`: the [`AriaRole::Grid`] root references the header row +
    /// the windowed rows; the header row carries the three
    /// [`AriaRole::ColumnHeader`]s; each visible group header is a spanning
    /// [`AriaRole::Row`] + `aria-expanded`, and each data row an
    /// [`AriaRole::Row`] (`aria-selected`) with three [`AriaRole::GridCell`]
    /// children named `"Column: value"`. See the module note on why this is
    /// hand-rolled rather than reusing `grid_table_nodes`.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let selected = *selected;
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let groups = use_grid_groups();
        let rows = groups.rows();
        let visible_len = rows.len();
        let window =
            compute_visible_range(scroll_state.offset_y(), VIEWPORT_H, visible_len, ROW_PITCH, OVERSCAN);
        let total = u32::try_from(visible_len).unwrap_or(u32::MAX);

        let mut nodes: Vec<AccessNode> = Vec::new();

        // Grid root: header row then the windowed rows.
        let mut grid = AccessNode::new(GRID_TAG, AriaRole::Grid).with_name("Grouped asset grid");
        grid = grid.with_child(HEAD_ROW_TAG);
        for view_pos in window.indices() {
            let tag = match rows[view_pos] {
                GroupRow::Header { group, .. } => format!("{GROUP_TAG}#{group}"),
                GroupRow::Data { source } => format!("{GRID_TAG}#{source}"),
            };
            grid = grid.with_child(tag);
        }
        nodes.push(grid);

        // Header row + its column headers.
        let mut header_row = AccessNode::new(HEAD_ROW_TAG, AriaRole::Row);
        for c in 0..COLS.len() {
            header_row = header_row.with_child(format!("{GRID_TAG}_col{c}"));
        }
        nodes.push(header_row);
        for (c, &name) in COLS.iter().enumerate() {
            nodes.push(AccessNode::new(format!("{GRID_TAG}_col{c}"), AriaRole::ColumnHeader).with_name(name));
        }

        // Windowed rows: spanning group headers + cell data rows.
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
                    let row_tag = format!("{GRID_TAG}#{source}");
                    let mut row = AccessNode::new(row_tag.clone(), AriaRole::Row)
                        .with_position_in_set(posinset)
                        .with_size_of_set(total)
                        .with_selected(selected == Some(source));
                    for c in 0..COLS.len() {
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

impl WidgetView for GroupedGridView {
    type Renderer = HelloGroupedGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<GroupedGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;

    fn render(selected: Option<usize>, collapse: impl FnOnce(&GroupOrderState)) -> Scene {
        Owner::new().run(|| {
            let g = use_grid_groups();
            collapse(&g);
            view(selected, &Frame::default())
        })
    }

    /// All tagged containers present in the scene (depth-first).
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

    fn data_sources(scene: &Scene) -> Vec<usize> {
        present_tags(scene)
            .iter()
            .filter_map(|t| t.strip_prefix(&format!("{GRID_TAG}#")).and_then(|r| r.parse::<usize>().ok()))
            .collect()
    }

    #[test]
    fn boot_renders_columns_and_grouped_cells() {
        let scene = render(None, |_| {});
        let tags = present_tags(&scene);
        // The three column headers and the group-0 header are present.
        for c in 0..COLS.len() {
            assert!(tags.contains(&format!("{GRID_TAG}_col{c}")), "column header {c} present");
        }
        assert!(tags.contains(&format!("{GROUP_TAG}#0")), "Mesh group header present");
        // First data rows are Mesh members 0, 6, 12, each with three cells.
        let mut sources = data_sources(&scene);
        sources.sort_unstable();
        assert_eq!(&sources[..3], &[0, 6, 12], "Mesh members lead the grid");
        for c in 0..COLS.len() {
            assert!(tags.contains(&cell_tag(0, c)), "row 0 cell {c} present");
        }
    }

    #[test]
    fn collapsing_a_group_hides_its_cell_rows() {
        let expanded = render(None, |_| {});
        assert!(data_sources(&expanded).contains(&0), "source 0 visible when expanded");
        let collapsed = render(None, |g| g.set_collapsed(0, true));
        assert!(!data_sources(&collapsed).contains(&0), "collapsed Mesh hides its cell rows");
        // The header itself stays.
        assert!(present_tags(&collapsed).contains(&format!("{GROUP_TAG}#0")), "header stays on collapse");
    }

    #[test]
    fn a11y_grid_has_columnheaders_expandable_group_rows_and_gridcells() {
        let nodes = Owner::new().run(|| GroupedGridView::access_node(&Some(0), None));
        // Grid root.
        assert_eq!(nodes[0].role, AriaRole::Grid);
        // Three column headers.
        let col_headers = nodes.iter().filter(|n| n.role == AriaRole::ColumnHeader).count();
        assert_eq!(col_headers, COLS.len(), "one columnheader per column");
        // Group-0 header is an expandable Row.
        let gh = nodes.iter().find(|n| n.tag == format!("{GROUP_TAG}#0")).expect("group header node");
        assert_eq!(gh.role, AriaRole::Row);
        assert_eq!(gh.expanded, Some(true), "expanded group header carries aria-expanded");
        // The selected data row is a Row with aria-selected and three gridcells.
        let dr = nodes.iter().find(|n| n.tag == format!("{GRID_TAG}#0")).expect("data row node");
        assert_eq!(dr.role, AriaRole::Row);
        assert_eq!(dr.selected, Some(true), "selected row carries aria-selected");
        let cells = nodes.iter().filter(|n| n.role == AriaRole::GridCell).count();
        assert!(cells >= COLS.len(), "data rows expose gridcells");
    }
}
