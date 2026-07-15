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
//! - **Extra** `ScrollBarExternal`.
//!
//! ## a11y
//!
//! WAI-ARIA `treegrid` via the
//! [`grouped_grid_access_nodes`] SSOT
//! builder (R847; R874 lowers the hierarchical-with-columns grid to a
//! `treegrid`): the header row's `columnheader`s carry `aria-sort` on the
//! active sort column (passed in the [`GridColumn`] slice), group headers are
//! spanning `aria-level = 1` `row`s with `aria-expanded`, and data rows are
//! `aria-level = 2` `row`s (`aria-selected`) with `gridcell` children.

use std::rc::Rc;

use pinion_a11y::{
    AccessNode, GridColumn, GroupedGridSelection, GroupedGridSpec, SortDirection, WidgetA11y,
    grouped_grid_access_nodes,
};
use pinion_core::external::{
    External, IntrospectSchema, IntrospectValue, QueryOnlyIntrospect, QuerySource, SchemaArg,
    SchemaField, int_of,
};
use pinion_core::reactive::Owner;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::grid_sort::{
    GridSortExternal, GridSortState, col_sort_dir, use_grid_sort,
};
use pinion_core::widgets::group_order::{
    GroupOrderExternal, GroupOrderState, GroupRow, use_group_order_with_source,
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
vello_renderer_impl!(
    HelloGroupedGridSortRenderer,
    HelloGroupedGridSortRendererError
);

const WIN_W: u32 = 400;
const WIN_H: u32 = 540;
const THEME_TAG: &str = "app";
const N: usize = 10_000;
const ROW_PITCH: u32 = 28;
const OVERSCAN: usize = 3;

const COLS: [&str; 3] = ["Name", "Size", "Modified"];
const COL_W: [u32; 3] = [150, 90, 80];
const NCOLS: usize = 3;
const GRID_W: u32 = COL_W[0] + COL_W[1] + COL_W[2]; // derived, can't desync from COL_W
const VIEWPORT_H: u32 = 13 * ROW_PITCH;

/// Paint-root + a11y `grid` + primary [`VirtualSelectExternal`] tag.
const GRID_TAG: &str = "ggrid";
/// The [`GroupOrderExternal`] anchor (`ggrp#<group>` headers).
const GROUP_TAG: &str = "ggrp";
/// The [`GridSortExternal`] anchor (`gsort#h<col>` clickable column headers).
const SORT_TAG: &str = "gsort";
const SCROLL_KEY: &str = "ggrid_scroll";
const SCROLLBAR_TAG: &str = "ggrid_scrollbar";
/// R854 — the per-group aggregate query surface (`group_count` / `total_count`
/// / `total_size` / `group.<g>.{count,size}`), an AI-first read of the same
/// counts + total Size the group headers show. `Owner::cache` key for it.
const AGG_TAG: &str = "ggs_agg";
const AGG_KEY: &str = "ggs_aggregates";

const GROUPS: [&str; 6] = ["Mesh", "Texture", "Material", "Sound", "Script", "Prefab"];

/// Unsorted-column marker — `U+2195` (style choice per consumer; the
/// directional pair is the R886.1 `pinion_widget_paint::glyph` SSOT).
const ARROW_NONE: &str = "\u{2195}";

fn row_group(i: usize) -> usize {
    i % GROUPS.len()
}

/// The cell value of row `source` in column `col` (0=Name, 1=Size, 2=Modified).
/// Size is a bare integer so `cell_cmp` orders it numerically; Name/Modified
/// are text (lexicographic).
fn cell_value(source: usize, col: usize) -> String {
    match col {
        0 => format!("asset_{source:05}"),
        // R854 — the Size formula lives in `row_size` (the aggregate sums the
        // same value the column displays; one SSOT, no display/aggregate drift).
        1 => format!("{}", row_size(source)),
        _ => format!("day {:03}", source % 90),
    }
}

fn cell_tag(source: usize, col: usize) -> String {
    format!("gcell_{source}_{col}")
}

/// R854 — the numeric Size of row `source` (the integer [`cell_value`] formats
/// into the Size column), summed into the per-group aggregate.
fn row_size(source: usize) -> u64 {
    u64::try_from((source * 13) % 990 + 8).unwrap_or(0)
}

/// R854 §5.27 — static per-group aggregates: the member count and the total
/// Size of each group. The data is a pure function of the row index, so these
/// never change — computed once and shared (the headers display them, the
/// [`QueryOnlyIntrospect`] surfaces them as data). A real asset browser shows
/// exactly this ("Textures: 1,234 items, 5.6 MB"); the aggregate stays visible
/// when a group is collapsed because it rides the always-shown header.
struct GroupAggregates {
    /// R856 — the group membership SSOT: per-group **count** is read from here
    /// (`total_member_count`), never re-tallied. (R854 originally recomputed the
    /// count in a parallel loop — the audit found it duplicated
    /// `GroupOrderState.member_counts`; the *size* sum below is the only
    /// genuinely-new aggregate, since the group proxy does not sum cell values.)
    groups: Rc<GroupOrderState>,
    /// `total_size[g]` — the Size sum per group (a pure fn of the row index, so
    /// computed once).
    total_size: Vec<u64>,
}

impl core::fmt::Debug for GroupAggregates {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GroupAggregates")
            .field("total_size", &self.total_size)
            .finish_non_exhaustive()
    }
}

impl GroupAggregates {
    /// Sum the Size column per group once. Counts are NOT tallied here — they
    /// live in the [`GroupOrderState`] SSOT (R856 audit fix).
    fn new(groups: Rc<GroupOrderState>) -> Self {
        let mut total_size = vec![0u64; GROUPS.len()];
        for i in 0..N {
            total_size[row_group(i)] += row_size(i);
        }
        Self { groups, total_size }
    }
}

impl QuerySource for GroupAggregates {
    fn introspect_schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("group_count", "int"),
                    SchemaField::new("total_count", "int"),
                    SchemaField::new("total_size", "int"),
                    SchemaField::parametric(
                        "group.<g>.count",
                        "int",
                        const { &[SchemaArg::open("g", "int")] },
                    ),
                    SchemaField::parametric(
                        "group.<g>.size",
                        "int",
                        const { &[SchemaArg::open("g", "int")] },
                    ),
                ]
            },
        )
    }

    fn introspect_query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "group_count" => Some(IntrospectValue::Int(int_of(self.groups.group_count()))),
            "total_count" => Some(IntrospectValue::Int(int_of(self.groups.count()))),
            "total_size" => Some(IntrospectValue::Int(
                i64::try_from(self.total_size.iter().sum::<u64>()).unwrap_or(i64::MAX),
            )),
            _ => {
                let (g_str, field) = path.strip_prefix("group.")?.split_once('.')?;
                let g: usize = g_str.parse().ok()?;
                match field {
                    // Count from the membership SSOT. `total_member_count` returns
                    // 0 for an out-of-range id, so gate on `group_count` to keep
                    // the out-of-range path a None (an RPC UnknownPath error).
                    "count" => (g < self.groups.group_count())
                        .then(|| IntrospectValue::Int(int_of(self.groups.total_member_count(g)))),
                    "size" => self
                        .total_size
                        .get(g)
                        .map(|&s| IntrospectValue::Int(i64::try_from(s).unwrap_or(i64::MAX))),
                    _ => None,
                }
            }
        }
    }
}

/// R854 — the shared aggregate cache (the headers + the [`QueryOnlyIntrospect`]
/// read the same instance). The group proxy is pre-resolved (it is itself
/// `Owner::cache`'d) BEFORE this factory, never inside it (R666 no-nested-cache).
fn use_group_aggregates() -> Rc<GroupAggregates> {
    let groups = use_grid_groups();
    Owner::current()
        .expect("use_group_aggregates requires an active Owner scope")
        .cache(AGG_KEY, move || GroupAggregates::new(groups))
}

/// The shared column-sort proxy over the materialized cell grid.
fn use_grid_data() -> Rc<GridSortState> {
    use_grid_sort(SORT_TAG, || {
        let cells: Vec<Vec<String>> = (0..N)
            .map(|i| (0..NCOLS).map(|c| cell_value(i, c)).collect())
            .collect();
        (NCOLS, cells)
    })
}

/// The shared group-by proxy grouping the **sorted** order. The
/// [`GridSortState`] is resolved before the group cache factory (R666
/// no-nested-cache); the factory captures it and groups its live `order()`
/// (the 2nd consumer of [`GroupOrderState::with_order_source`]).
fn use_grid_groups() -> Rc<GroupOrderState> {
    let grid = use_grid_data();
    use_group_order_with_source(
        GROUP_TAG,
        || {
            let groups = (0..N).map(row_group).collect::<Vec<usize>>();
            let labels = GROUPS
                .iter()
                .map(|&g| g.to_string())
                .collect::<Vec<String>>();
            (groups, labels)
        },
        move || grid.order(),
    )
}

/// The clickable column-header row: each header (`gsort#h<col>`) shows its
/// label + a sort glyph and cycles that column's sort on click.
fn column_header_row(sort: Option<(usize, bool)>, theme: &Theme) -> Scene {
    let mut cells: Vec<Scene> = Vec::with_capacity(NCOLS);
    for (c, &name) in COLS.iter().enumerate() {
        let glyph =
            pinion_widget_paint::glyph::sort_glyph(col_sort_dir(sort, c)).unwrap_or(ARROW_NONE);
        let label = Scene::Text(TextNode::styled(
            format!("{name} {glyph}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
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

/// R854 — the per-group aggregate (member count + total Size) rides the
/// always-shown header, so it stays visible when the group is collapsed. The
/// header skin is the [`group_header_row`] SSOT (R871); this variant passes the
/// aggregate as the parenthesized detail (the only divergence from the three
/// count-only grouped headers).
fn build_header(
    group: usize,
    member_count: usize,
    total_size: u64,
    collapsed: bool,
    theme: &Theme,
) -> Scene {
    group_header_row(
        format!("{GROUP_TAG}#{group}"),
        GROUPS[group % GROUPS.len()],
        &format!("{member_count}, {total_size} B"),
        collapsed,
        theme,
        GRID_W,
        ROW_PITCH,
    )
}

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

    let grid = use_grid_data();
    let groups = use_grid_groups();
    let aggregates = use_group_aggregates();
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
            GroupRow::Header {
                group,
                member_count,
                collapsed,
            } => {
                let total_size = aggregates.total_size.get(group).copied().unwrap_or(0);
                build_header(group, member_count, total_size, collapsed, &theme)
            }
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
            ExtraExternal::new(
                GROUP_TAG,
                Box::new(GroupOrderExternal::new(use_grid_groups())),
            ),
            // R854 — the AI-first per-group aggregate surface, query-only via the
            // QueryOnlyIntrospect substrate (the modal/snackbar R810.1 pattern).
            ExtraExternal::new(
                AGG_TAG,
                Box::new(QueryOnlyIntrospect::new(use_group_aggregates())),
            ),
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
    /// WAI-ARIA `treegrid` via [`grouped_grid_access_nodes`]:
    /// the active sort column's `aria-sort` is the one datum that differs from
    /// the static `hello-grouped-grid`, carried in the [`GridColumn`] slice.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let sort = use_grid_data().sort();
        let groups = use_grid_groups();
        let rows = groups.rows();
        let window = compute_visible_range(
            scroll_state.offset_y(),
            VIEWPORT_H,
            rows.len(),
            ROW_PITCH,
            OVERSCAN,
        );
        // Sortable columns: the active column carries its `aria-sort` in the
        // GridColumn slice (the one datum that differs from the static grid).
        let columns: Vec<GridColumn> = COLS
            .iter()
            .enumerate()
            .map(|(c, &name)| GridColumn {
                tag: format!("{SORT_TAG}#h{c}"),
                label: name.to_string(),
                sort: col_sort_dir(sort, c).map(SortDirection::from_ascending),
            })
            .collect();
        let header_row_tag = format!("{SORT_TAG}#hrow");
        let spec = GroupedGridSpec {
            grid_tag: GRID_TAG,
            name: Some("Sortable grouped asset grid"),
            header_row_tag: &header_row_tag,
            columns: &columns,
            selection: GroupedGridSelection::Single(*selected),
            // No keyboard navigation in this sort-focused slice (R848 wires
            // grouped-list / grouped-grid); the cursor axis is absent here.
            focused_view_pos: None,
            // R895 — row-focus (read-only selection grid).
            focused_cell: None,
        };
        grouped_grid_access_nodes(
            &spec,
            &rows,
            window,
            |g| GROUPS[g % GROUPS.len()].to_string(),
            cell_tag,
            cell_value,
            |r| r.composite_tag(GROUP_TAG, GRID_TAG),
        )
    }
}

impl WidgetView for GroupedGridSortView {
    type Renderer = HelloGroupedGridSortRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
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
            .filter_map(|t| {
                t.strip_prefix(&format!("{GRID_TAG}#"))
                    .and_then(|r| r.parse::<usize>().ok())
            })
            .collect()
    }

    #[test]
    fn boot_has_sortable_column_headers_and_source_order() {
        let scene = render(None, None);
        let tags = present_tags(&scene);
        for c in 0..NCOLS {
            assert!(
                tags.contains(&format!("{SORT_TAG}#h{c}")),
                "sortable column header {c} present"
            );
        }
        // Unsorted: the first Mesh members are in source order.
        let mut srcs = data_sources_ordered(&scene);
        srcs.sort_unstable();
        assert_eq!(
            &srcs[..3],
            &[0, 6, 12],
            "unsorted Mesh members in source order"
        );
    }

    #[test]
    fn descending_name_sort_reverses_within_groups() {
        // Sort Name (col 0) descending = reverse source order. Source 9999
        // leads (group 9999 % 6 == 3); within each group rows go high -> low.
        let scene = render(None, Some((0, false)));
        let ordered = data_sources_ordered(&scene);
        assert_eq!(
            ordered[0], 9999,
            "descending Name puts the highest source first"
        );
        // The first group's members descend (all share a group; strictly
        // decreasing source indices).
        let g0 = 9999 % GROUPS.len();
        let first_group: Vec<usize> = ordered
            .iter()
            .copied()
            .take_while(|&s| s % GROUPS.len() == g0)
            .collect();
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
        assert_eq!(
            col1.sort,
            Some(SortDirection::Ascending),
            "active column carries aria-sort"
        );
        let col0 = nodes
            .iter()
            .find(|n| n.tag == format!("{SORT_TAG}#h0"))
            .expect("Name column header");
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
        assert_eq!(
            fill,
            Some(accent),
            "selected source 9999 stays Accent after the sort"
        );
    }

    // ── R854 per-group aggregates ──────────────────────────────────

    /// Every Text node's content, in walk order.
    fn all_texts(scene: &Scene) -> Vec<String> {
        fn walk(s: &Scene, out: &mut Vec<String>) {
            match s {
                Scene::Container(c) => c.children.iter().for_each(|ch| walk(ch, out)),
                Scene::Scroll(sc) => walk(sc.content.as_ref(), out),
                Scene::Text(t) => out.push(t.content.clone()),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    #[test]
    fn r854_group_aggregates_tally_every_row_once() {
        Owner::new().run(|| {
            let agg = use_group_aggregates();
            // Independent recomputation of the per-group Size sums.
            let mut expected = vec![0u64; GROUPS.len()];
            for i in 0..N {
                expected[row_group(i)] += row_size(i);
            }
            assert_eq!(
                agg.total_size, expected,
                "per-group Size sums match a fresh tally"
            );
            // R856 — counts come from the GroupOrderState membership SSOT, not a
            // 2nd tally in GroupAggregates.
            assert_eq!(
                agg.groups.group_count(),
                GROUPS.len(),
                "one entry per group"
            );
            assert_eq!(agg.groups.count(), N, "every row counted once");
            assert_eq!(
                agg.groups.total_member_count(0),
                (0..N).filter(|&i| row_group(i) == 0).count(),
                "group 0 count via the membership SSOT",
            );
        });
    }

    #[test]
    fn r854_aggregate_query_source_paths() {
        Owner::new().run(|| {
            let agg = use_group_aggregates();
            assert_eq!(
                agg.introspect_query("group_count"),
                Some(IntrospectValue::Int(int_of(GROUPS.len()))),
            );
            assert_eq!(
                agg.introspect_query("total_count"),
                Some(IntrospectValue::Int(int_of(N)))
            );
            let total: u64 = agg.total_size.iter().sum();
            assert_eq!(
                agg.introspect_query("total_size"),
                Some(IntrospectValue::Int(i64::try_from(total).unwrap())),
            );
            // group count delegates to the membership SSOT (R856).
            let g0 = agg.groups.total_member_count(0);
            assert_eq!(
                agg.introspect_query("group.0.count"),
                Some(IntrospectValue::Int(int_of(g0)))
            );
            assert_eq!(
                agg.introspect_query("group.0.size"),
                Some(IntrospectValue::Int(
                    i64::try_from(agg.total_size[0]).unwrap()
                )),
            );
            assert_eq!(
                agg.introspect_query("group.99.count"),
                None,
                "out-of-range group is None"
            );
            assert_eq!(agg.introspect_query("bogus"), None, "unknown path is None");
        });
    }

    #[test]
    fn r854_aggregate_reaches_rpc_via_query_only_introspect() {
        Owner::new().run(|| {
            let agg = use_group_aggregates();
            let ext = QueryOnlyIntrospect::new(Rc::clone(&agg));
            let intro = External::introspect(&ext).expect("query-only introspectable");
            assert_eq!(
                intro.query("group_count"),
                Some(IntrospectValue::Int(int_of(GROUPS.len()))),
            );
            assert_eq!(
                intro.query("total_count"),
                Some(IntrospectValue::Int(int_of(N)))
            );
            assert_eq!(
                intro.query("group.0.size"),
                Some(IntrospectValue::Int(
                    i64::try_from(agg.total_size[0]).unwrap()
                )),
            );
        });
    }

    #[test]
    fn r854_group_header_displays_the_aggregate() {
        // The leading group's header text carries its member count + total Size.
        // Independent expected aggregate for group 0 (no filter at boot, so the
        // header's member_count equals the dataset-total).
        let count0 = (0..N).filter(|&i| row_group(i) == 0).count();
        let size0: u64 = (0..N).filter(|&i| row_group(i) == 0).map(row_size).sum();
        let want = format!("({count0}, {size0} B)");
        let scene = render(None, None);
        let texts = all_texts(&scene);
        assert!(
            texts.iter().any(|t| t.contains(&want)),
            "a group header shows its aggregate {want:?}; texts = {texts:?}",
        );
    }
}
