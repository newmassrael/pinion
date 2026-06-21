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
//! ## a11y (WAI-ARIA `treegrid`)
//!
//! Built by the
//! [`grouped_grid_access_nodes`](pinion_a11y::grouped_grid_access_nodes) SSOT
//! (R847) — a `treegrid` root, a `columnheader` per column, a spanning
//! `aria-level = 1` `row` + `aria-expanded` per group header, and an
//! `aria-level = 2` `row` (`aria-selected`) + `gridcell` children per data row.
//! This grouped-grid shape is hierarchical *with columns*, so it lowers to a
//! `treegrid` (R874; R863 added the role) — distinct from the grouped-*list*'s
//! `tree`/`treeitem`
//! ([`grouped_tree_access_nodes`](pinion_a11y::grouped_tree_access_nodes)) — two
//! builders, one per shape, each shared by its two consumers.

use std::rc::Rc;

use pinion_a11y::{
    AccessFocus, AccessNode, GridColumn, GroupedGridSelection, GroupedGridSpec, WidgetA11y,
    grouped_focus_target, grouped_grid_access_nodes,
};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::group_order::{
    GroupOrderExternal, GroupOrderState, GroupRow, group_nav_key, read_cursor, use_group_order,
};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{VirtualSelectExternal, read_selected};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::group_header::group_header_row;
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::virtual_list::view_virtual_list;

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
const GRID_W: u32 = COL_W[0] + COL_W[1] + COL_W[2]; // derived, can't desync from COL_W
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

/// R848 — the grid's projected state: the selected source row **and** the
/// roving keyboard cursor's visual position. The cursor is *projected* (not
/// only held in the reactive [`GroupOrderState`]) so a cursor-only move —
/// arrowing between group headers with no selection change — repaints and the
/// focus ring tracks it. This is the datepicker `focused_day` precedent: a
/// roving cursor distinct from the selection must be projected state, because
/// the shell repaints on a *visible-state* change, and the focus ring is shell-
/// drawn from [`access_focus_target`](GroupedGridView::access_focus_target),
/// not painted by the view.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
struct GridState {
    /// The selected source row (the [`VirtualSelectExternal`] index), or `None`.
    selected: Option<usize>,
    /// The roving cursor's visual position into the flattened rows, or `None`.
    cursor: Option<usize>,
}

/// The shared [`GroupOrderState`] grouping rows by asset type (source order).
fn use_grid_groups() -> Rc<GroupOrderState> {
    use_group_order(GROUP_TAG, || {
        let groups = (0..N).map(row_group).collect::<Vec<usize>>();
        let labels = GROUPS
            .iter()
            .map(|&g| g.to_string())
            .collect::<Vec<String>>();
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
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
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
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHighest),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_size(Size::px(GRID_W, ROW_PITCH)),
            ),
    )
}

/// One group-header row (`ggrp#<group>`): chevron + type + member count,
/// spanning the whole grid width. The header skin is the [`group_header_row`]
/// SSOT (R871); this binds the example's group-label table + grid width.
fn build_header(group: usize, member_count: usize, collapsed: bool, theme: &Theme) -> Scene {
    group_header_row(
        format!("{GROUP_TAG}#{group}"),
        GROUPS[group % GROUPS.len()],
        &member_count.to_string(),
        collapsed,
        theme,
        GRID_W,
        ROW_PITCH,
    )
}

/// One data row (`ggrid#<source>`): a flex row of `pointer_transparent` cells
/// across the columns. The row is the click target (selecting the source); the
/// cells are transparent so a click anywhere in the row reaches it.
fn build_data_row(source: usize, theme: &Theme, selected: Option<usize>) -> Scene {
    let is_selected = selected == Some(source);
    let (fill, fg) = if is_selected {
        (
            theme.resolve(ColorRole::Accent),
            theme.resolve(ColorRole::OnAccent),
        )
    } else {
        let stripe = if source % 2 == 0 {
            ColorRole::SurfaceContainerLow
        } else {
            ColorRole::Surface
        };
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
            GroupRow::Header {
                group,
                member_count,
                collapsed,
            } => build_header(group, member_count, collapsed, &theme),
            GroupRow::Data { source } => build_data_row(source, &theme, selected),
        },
    );

    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    let list_row = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    let grid_root = Scene::Container(
        ContainerNode::new(vec![header, list_row])
            .with_tag(GRID_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_focusable(true),
            ),
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
    type State = GridState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                GROUP_TAG,
                Box::new(GroupOrderExternal::new(use_grid_groups())),
            ),
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
        ]
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn read_state(scene: &Scene) -> GridState {
        let selected = scene
            .find_external_with_tag(GRID_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(read_selected);
        // The roving cursor lives on the extra GroupOrderExternal — the same
        // find → introspect → read idiom as the selection above.
        let cursor = scene
            .find_external_with_tag(GROUP_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(read_cursor);
        GridState { selected, cursor }
    }

    fn view(state: GridState, frame: &Frame) -> Scene {
        view(state.selected, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// R848 §5.27 — keyboard navigation over the grouped grid, delegated to the
    /// shared [`group_nav_key`] controller (the same one `hello-grouped-list`
    /// uses): Arrow / Home / End rove the cursor over headers + data rows,
    /// Right / Left expand / collapse a group header (or climb a data row to its
    /// header), Enter / Space toggle a header or re-affirm a data selection.
    /// Landing on a data row selects its source (selection-follows-focus) and
    /// scrolls it into view.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        group_nav_key(
            scene,
            &use_scroll_state(SCROLL_KEY),
            &use_grid_groups(),
            GRID_TAG,
            focused,
            key,
            ROW_PITCH,
        )
    }

    fn title() -> &'static str {
        "pinion hello-grouped-grid (R848 §5.27 §5.40 grouped data grid + keyboard nav)"
    }

    fn fmt_state_log(state: &GridState) -> String {
        format!(
            "selected={} cursor={}",
            state
                .selected
                .map_or_else(|| "none".to_string(), |i| format!("source {i}")),
            state
                .cursor
                .map_or_else(|| "none".to_string(), |c| c.to_string()),
        )
    }
}

impl WidgetA11y for GroupedGridView {
    /// WAI-ARIA `treegrid` via the
    /// [`grouped_grid_access_nodes`](pinion_a11y::grouped_grid_access_nodes) SSOT
    /// builder (R847): a `treegrid` root over the header row + windowed rows, a
    /// `columnheader` per column, a spanning level-1 `row` + `aria-expanded` per
    /// group header, and a level-2 `row` (`aria-selected`) + `gridcell` children
    /// per data row. The columns are static (no `aria-sort`) — passed as a
    /// [`GridColumn`] slice with `sort: None`, the same slice a sortable grid
    /// (`hello-grouped-grid-sort`) fills with the active direction. The asset
    /// grid has a real selection model, so its rows carry `aria-selected`
    /// ([`GroupedGridSelection::Single`]).
    fn access_node(state: &GridState, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let groups = use_grid_groups();
        let rows = groups.rows();
        let window = compute_visible_range(
            scroll_state.offset_y(),
            VIEWPORT_H,
            rows.len(),
            ROW_PITCH,
            OVERSCAN,
        );
        // Static (non-sortable) columns: tag + label, no aria-sort.
        let columns: Vec<GridColumn> = COLS
            .iter()
            .enumerate()
            .map(|(c, &name)| GridColumn {
                tag: format!("{GRID_TAG}_col{c}"),
                label: name.to_string(),
                sort: None,
            })
            .collect();
        let spec = GroupedGridSpec {
            grid_tag: GRID_TAG,
            name: Some("Grouped asset grid"),
            header_row_tag: HEAD_ROW_TAG,
            columns: &columns,
            selection: GroupedGridSelection::Single(state.selected),
            focused_view_pos: state.cursor,
            // R895 — row-focus (a selection-navigated grid; cell-focus is the
            // editable data grid's axis).
            focused_cell: None,
        };
        grouped_grid_access_nodes(
            &spec,
            &rows,
            window,
            |g| GROUPS[g % GROUPS.len()].to_string(),
            cell_tag,
            cell_value,
            // R895 — the rows route through one coordinator via the composite
            // scheme; the consumer owns the tag (it used to be a spec prefix).
            |r| r.composite_tag(GROUP_TAG, GRID_TAG),
        )
    }

    /// R848 / R850 — single-tab-stop roving: shell focus stays on the grid
    /// container while the ring frames the cursor's row. Delegated to the
    /// [`grouped_focus_target`] SSOT (shared with `hello-grouped-list`) so the
    /// ring policy is one source of truth across both grouped presentations.
    fn access_focus_target(state: &GridState, focused: Option<&str>) -> Option<AccessFocus> {
        grouped_focus_target(
            &use_grid_groups(),
            GRID_TAG,
            GROUP_TAG,
            state.cursor,
            focused,
        )
    }
}

impl WidgetView for GroupedGridView {
    type Renderer = HelloGroupedGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GroupedGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
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
            .filter_map(|t| {
                t.strip_prefix(&format!("{GRID_TAG}#"))
                    .and_then(|r| r.parse::<usize>().ok())
            })
            .collect()
    }

    #[test]
    fn boot_renders_columns_and_grouped_cells() {
        let scene = render(None, |_| {});
        let tags = present_tags(&scene);
        // The three column headers and the group-0 header are present.
        for c in 0..COLS.len() {
            assert!(
                tags.contains(&format!("{GRID_TAG}_col{c}")),
                "column header {c} present"
            );
        }
        assert!(
            tags.contains(&format!("{GROUP_TAG}#0")),
            "Mesh group header present"
        );
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
        assert!(
            data_sources(&expanded).contains(&0),
            "source 0 visible when expanded"
        );
        let collapsed = render(None, |g| g.set_collapsed(0, true));
        assert!(
            !data_sources(&collapsed).contains(&0),
            "collapsed Mesh hides its cell rows"
        );
        // The header itself stays.
        assert!(
            present_tags(&collapsed).contains(&format!("{GROUP_TAG}#0")),
            "header stays on collapse"
        );
    }

    #[test]
    fn a11y_treegrid_has_columnheaders_expandable_group_rows_and_gridcells() {
        // Cursor on visual pos 1 (the first data row, source 0).
        let nodes = Owner::new().run(|| {
            GroupedGridView::access_node(
                &GridState {
                    selected: Some(0),
                    cursor: Some(1),
                },
                None,
            )
        });
        // R874 — treegrid root (hierarchical + columns), not a flat grid.
        assert_eq!(nodes[0].role, AriaRole::TreeGrid);
        // Three column headers.
        let col_headers = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .count();
        assert_eq!(col_headers, COLS.len(), "one columnheader per column");
        // Group-0 header is an expandable level-1 Row.
        let gh = nodes
            .iter()
            .find(|n| n.tag == format!("{GROUP_TAG}#0"))
            .expect("group header node");
        assert_eq!(gh.role, AriaRole::Row);
        assert_eq!(gh.level, Some(1), "group header is aria-level 1");
        assert_eq!(
            gh.expanded,
            Some(true),
            "expanded group header carries aria-expanded"
        );
        // The selected data row is a level-2 Row with aria-selected and three
        // gridcells, and it is the cursor row (active descendant) → focused.
        let dr = nodes
            .iter()
            .find(|n| n.tag == format!("{GRID_TAG}#0"))
            .expect("data row node");
        assert_eq!(dr.role, AriaRole::Row);
        assert_eq!(dr.level, Some(2), "data row is aria-level 2");
        assert_eq!(
            dr.selected,
            Some(true),
            "selected row carries aria-selected"
        );
        assert!(dr.state.focused, "the cursor row is the active descendant");
        let cells = nodes
            .iter()
            .filter(|n| n.role == AriaRole::GridCell)
            .count();
        assert!(cells >= COLS.len(), "data rows expose gridcells");
    }

    #[test]
    fn access_focus_target_rings_the_cursor_row() {
        Owner::new().run(|| {
            let _ = use_grid_groups(); // build the shared holder (all groups expanded)
            // Cursor on visual pos 0 = the group-0 header.
            let af = GroupedGridView::access_focus_target(
                &GridState {
                    selected: None,
                    cursor: Some(0),
                },
                Some(GRID_TAG),
            )
            .expect("grid focused returns Some");
            assert_eq!(
                af.active_descendant.as_deref(),
                Some(&*format!("{GROUP_TAG}#0"))
            );
            // Cursor on visual pos 1 = the first Mesh data row (source 0).
            let af1 = GroupedGridView::access_focus_target(
                &GridState {
                    selected: None,
                    cursor: Some(1),
                },
                Some(GRID_TAG),
            )
            .expect("grid focused returns Some");
            assert_eq!(
                af1.active_descendant.as_deref(),
                Some(&*format!("{GRID_TAG}#0"))
            );
            // No cursor → ring the container (no active descendant).
            let af_none =
                GroupedGridView::access_focus_target(&GridState::default(), Some(GRID_TAG))
                    .expect("grid focused returns Some");
            assert_eq!(af_none.active_descendant, None);
        });
    }
}
