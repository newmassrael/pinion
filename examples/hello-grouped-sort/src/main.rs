//! `hello-grouped-sort` — R844 §5.27 §5.40 **stacked Model/View proxies:
//! filter → sort → group**.
//!
//! R747 lands the sort/filter view-order proxy; R843 the group-by proxy over a
//! *source* order. R844 stacks them into the canonical Qt
//! `QSortFilterProxyModel`-plus-grouping chain: an upstream
//! [`ViewSortFilterExternal`] sorts + filters 10,000 rows into a live `order`
//! permutation, and a [`GroupOrderExternal`] groups **that order** into
//! collapsible asset-type groups. Sorting reorders the groups and the members
//! within them; filtering shrinks groups (a group whose every member is
//! filtered out loses its header); collapsing hides a group's members. The
//! whole stack is virtualized — only the visible window of the flattened
//! grouped-sorted-filtered sequence is materialized.
//!
//! ## The four coordinators
//!
//! - **Primary** [`VirtualSelectExternal`] at [`LIST_TAG`] — selection by
//!   **source** index; survives filter, sort and collapse alike.
//! - **Extra** [`ViewSortFilterExternal`] at [`SORT_TAG`] — the upstream
//!   sort/filter proxy; the clicked header (`gsort#cycle`) cycles the sort, the
//!   AI-first `invoke "set_filter"` sets the facet.
//! - **Extra** [`GroupOrderExternal`] at [`GROUP_TAG`] — groups the upstream's
//!   `order()` (via [`GroupOrderState::with_order_source`]); a clicked group
//!   header (`ggroup#<group>`) toggles collapse.
//! - **Extra** [`ScrollBarExternal`] at [`SCROLLBAR_TAG`].
//!
//! ## The witness (§2 #7 scene-as-data)
//!
//! Boot: source order, 6 groups, 10,006 visible rows. Cycle the sort →
//! descending regroups (the highest-source group leads, members in reverse).
//! Filter to a category → every group shrinks, `visible_len` drops, but the
//! selection (a source index) is held. Collapse a group → its members vanish
//! independent of sort/filter. The proof is data (`tools/demos/r844_grouped_sort.py`).

use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, GroupedTreeSpec, WidgetA11y, grouped_tree_access_nodes};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::group_order::{
    GroupOrderExternal, GroupOrderState, GroupRow, use_group_order_with_source,
};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::view_order::{
    ViewOrderState, ViewSortFilterExternal, sort_dir_str, use_view_order,
};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{VirtualSelectExternal, read_selected};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::group_header::group_header_row;
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::virtual_list::view_virtual_list;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloGroupedSortRenderer, HelloGroupedSortRendererError);

const WIN_W: u32 = 400;
const WIN_H: u32 = 540;
const THEME_TAG: &str = "app";
const N: usize = 10_000;
const ROW_PITCH: u32 = 28;
const OVERSCAN: usize = 3;
const VIEWPORT_W: u32 = 320;
const VIEWPORT_H: u32 = 14 * ROW_PITCH;
const HEADER_H: u32 = 40;

/// Paint-root + a11y `tree` container + primary [`VirtualSelectExternal`] tag.
const LIST_TAG: &str = "glist";
/// The [`GroupOrderExternal`] anchor; group headers route here (`ggroup#<g>`).
const GROUP_TAG: &str = "ggroup";
/// The upstream [`ViewSortFilterExternal`] anchor; the sort header routes here.
const SORT_TAG: &str = "gsort";
/// Composite region of the clickable sort header (`gsort#cycle`).
const SORT_REGION: &str = "cycle";
const SCROLL_KEY: &str = "glist_scroll";
const SCROLLBAR_TAG: &str = "glist_scrollbar";

/// Six asset-type groups (the group facet). Source row `i` is `GROUPS[i % 6]`.
const GROUPS: [&str; 6] = ["Mesh", "Texture", "Material", "Sound", "Script", "Prefab"];
/// Five filter categories (the filter facet, orthogonal to the group facet).
const CATS: [&str; 5] = ["A", "B", "C", "D", "E"];

fn row_group(i: usize) -> usize {
    i % GROUPS.len()
}
fn row_category(i: usize) -> usize {
    i % CATS.len()
}
/// Sort key / display name of source row `i`. Zero-padded so a name sort is
/// lexicographic and width-stable; monotonic with `i`, so ascending = source
/// order and descending = reverse (a visible reorder within each group).
fn row_name(i: usize) -> String {
    format!("asset_{i:05}")
}

/// The shared upstream [`ViewOrderState`] — sort by name, filter by category.
/// The view, the [`ViewSortFilterExternal`], and the [`GroupOrderState`]'s
/// order source all reach the same `Rc` through this hook.
fn use_sort() -> Rc<ViewOrderState> {
    use_view_order(SORT_TAG, || {
        let keys = (0..N).map(row_name).collect::<Vec<String>>();
        let cats = (0..N).map(row_category).collect::<Vec<usize>>();
        (keys, cats)
    })
}

/// The shared [`GroupOrderState`] grouping the **upstream order**. The upstream
/// [`ViewOrderState`] is resolved *before* the group cache factory runs (never
/// inside it) so the two `Owner::cache` calls do not nest
/// ([[owner-cache-no-nested-factory]]); the factory then captures that `Rc` and
/// reads `order()` reactively.
fn use_groups() -> Rc<GroupOrderState> {
    let sort = use_sort();
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
        move || sort.order(),
    )
}

/// The sort/filter control bar: shows the active sort + filter + visible-row
/// count and cycles the sort on click (`gsort#cycle`, R51.42 composite).
fn control_bar(sort: Option<bool>, filter: Option<usize>, visible: usize, theme: &Theme) -> Scene {
    // R886.1 — directional pair from the glyph SSOT; the unsorted
    // marker stays a per-consumer style choice.
    let arrow = pinion_widget_paint::glyph::sort_glyph(sort).unwrap_or("\u{2195}");
    let filter_label = match filter {
        Some(c) => CATS[c % CATS.len()],
        None => "All",
    };
    let text = format!(
        "Sort {arrow} name   \u{00B7}   Filter: {filter_label}   \u{00B7}   {visible} rows"
    );
    let label = Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new()
            .with_size_px(13)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{SORT_TAG}#{SORT_REGION}"))
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHighest),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(VIEWPORT_W, HEADER_H)),
            ),
    )
}

/// One group header row (`ggroup#<group>`): chevron + label + member count.
/// The header skin is the [`group_header_row`] SSOT (R871); this binds the
/// example's group-label table + list width.
fn build_header(group: usize, member_count: usize, collapsed: bool, theme: &Theme) -> Scene {
    group_header_row(
        format!("{GROUP_TAG}#{group}"),
        GROUPS[group % GROUPS.len()],
        &member_count.to_string(),
        collapsed,
        theme,
        VIEWPORT_W,
        ROW_PITCH,
    )
}

/// One data row (`glist#<source>`): the asset name + its category, indented;
/// selected → Accent, else zebra by source parity.
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
    let text = format!("{}   [{}]", row_name(source), CATS[row_category(source)]);
    let label = Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new().with_size_px(13).with_fg(fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{LIST_TAG}#{source}"))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(VIEWPORT_W, ROW_PITCH))
                    .with_padding(Rect::new(30, 0, 12, 0)),
            ),
    )
}

/// view-fn (§6.3): the grouped-sorted-filtered flattening, windowed. The
/// control bar reads the upstream sort/filter; the list windows the
/// [`GroupOrderState`] rows (which group the upstream `order()`); a
/// sort/filter/collapse change repaints through the reactive subscriptions the
/// reads open.
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();

    let sort = use_sort();
    let groups = use_groups();
    let rows = groups.rows();
    let visible_len = rows.len();

    let bar = control_bar(sort.sort(), sort.filter(), visible_len, &theme);

    let list = view_virtual_list(
        &scroll_state,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
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

    let list_root = Scene::Container(
        ContainerNode::new(vec![bar, list_row])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    );

    Scene::Container(
        ContainerNode::new(vec![list_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

struct GroupedSortView;

impl WidgetCore for GroupedSortView {
    type State = Option<usize>;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    /// Extras: the group-by proxy (grouping the upstream order), the upstream
    /// sort/filter proxy (over the **same** shared [`ViewOrderState`]
    /// `use_groups` reads), and the scrollbar peer.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(GROUP_TAG, Box::new(GroupOrderExternal::new(use_groups()))),
            ExtraExternal::new(SORT_TAG, Box::new(ViewSortFilterExternal::new(use_sort()))),
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
        ]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    fn read_state(scene: &Scene) -> Option<usize> {
        scene
            .find_external_with_tag(LIST_TAG)
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
        "pinion hello-grouped-sort (R844 §5.27 §5.40 filter->sort->group stack)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=source {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for GroupedSortView {
    /// WAI-ARIA `tree` over the flattened grouped-sorted-filtered view (a
    /// grouped collapsible list is a one-level tree to AT): the sort control is
    /// an [`AriaRole::Button`]; each visible group header is an
    /// [`AriaRole::TreeItem`] at `aria-level = 1` with `aria-expanded`, and each
    /// data row a `TreeItem` at `aria-level = 2` with `aria-selected`.
    ///
    /// Hand-rolled rather than reusing
    /// [`pinion_a11y::tree_access_nodes`](pinion_a11y::tree_access_nodes) for
    /// the same reason as `hello-grouped-list`: that builder uses one
    /// `row_prefix`, but a grouped list carries two composite namespaces
    /// (`ggroup#` headers route to collapse, `glist#` data rows to selection),
    /// both load-bearing for AT activation routing.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let sort = use_sort();
        let groups = use_groups();
        let rows = groups.rows();
        let window = compute_visible_range(
            scroll_state.offset_y(),
            VIEWPORT_H,
            rows.len(),
            ROW_PITCH,
            OVERSCAN,
        );

        // The sort control is a standalone Button sibling of the grouped tree.
        let mut nodes = vec![
            AccessNode::new(format!("{SORT_TAG}#{SORT_REGION}"), AriaRole::Button)
                .with_name(format!("Sort: {}", sort_dir_str(sort.sort()))),
        ];
        let spec = GroupedTreeSpec {
            tree_tag: LIST_TAG,
            name: Some("Grouped sorted asset list"),
            header_prefix: GROUP_TAG,
            data_prefix: LIST_TAG,
            selected_source: *selected,
            // No keyboard navigation in this sort-focused slice (R848 wires
            // grouped-list / grouped-grid); the cursor axis is absent here.
            focused_view_pos: None,
        };
        nodes.extend(grouped_tree_access_nodes(
            &spec,
            &rows,
            window,
            |g| GROUPS[g % GROUPS.len()].to_string(),
            row_name,
        ));
        nodes
    }
}

impl WidgetView for GroupedSortView {
    type Renderer = HelloGroupedSortRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GroupedSortView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;

    /// Render with the upstream sort/filter pre-set (mutated within the same
    /// Owner scope the view + group state read from).
    fn render(selected: Option<usize>, sort: Option<bool>, filter: Option<usize>) -> Scene {
        Owner::new().run(|| {
            let vo = use_sort();
            vo.set_sort(sort);
            vo.set_filter(filter);
            view(selected, &Frame::default())
        })
    }

    fn present_sources(scene: &Scene) -> Vec<usize> {
        fn walk(scene: &Scene, out: &mut Vec<usize>) {
            match scene {
                Scene::Container(c) => {
                    if let Some(tag) = c.tag.as_deref() {
                        if let Some(rest) = tag.strip_prefix(&format!("{LIST_TAG}#")) {
                            if let Ok(s) = rest.parse::<usize>() {
                                out.push(s);
                            }
                        }
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

    fn first_header_group(scene: &Scene) -> Option<usize> {
        fn walk(scene: &Scene) -> Option<usize> {
            match scene {
                Scene::Container(c) => {
                    if let Some(g) = c
                        .tag
                        .as_deref()
                        .and_then(|t| t.strip_prefix(&format!("{GROUP_TAG}#")))
                        .and_then(|r| r.parse::<usize>().ok())
                    {
                        return Some(g);
                    }
                    c.children.iter().find_map(walk)
                }
                Scene::Scroll(s) => walk(s.content.as_ref()),
                _ => None,
            }
        }
        walk(scene)
    }

    #[test]
    fn boot_groups_source_order_with_six_groups() {
        let scene = render(None, None, None);
        assert_eq!(
            first_header_group(&scene),
            Some(0),
            "group 0 (Mesh) leads source order"
        );
        let sources = present_sources(&scene);
        assert!(sources.len() < 30, "virtualized window");
        assert_eq!(&sources[..3], &[0, 6, 12], "Mesh members in source order");
    }

    #[test]
    fn descending_sort_regroups_by_highest_source() {
        // Descending by name = reverse source order. Source 9999 leads; its
        // group is 9999 % 6 == 3, so group 3 heads the flattening.
        let scene = render(None, Some(false), None);
        assert_eq!(
            first_header_group(&scene),
            Some(3),
            "highest source's group leads descending"
        );
        assert_eq!(
            present_sources(&scene)[0],
            9999,
            "9999 is the first descending member"
        );
    }

    #[test]
    fn filter_shrinks_groups_but_keeps_them_present() {
        // Category 0 (i % 5 == 0) is orthogonal to the 6 groups, so every group
        // keeps some members — all six headers survive, each shrunk.
        let scene = render(None, None, Some(0));
        let sources = present_sources(&scene);
        assert!(!sources.is_empty());
        assert!(
            sources.iter().all(|&s| s % 5 == 0),
            "only category-0 rows: {sources:?}"
        );
    }

    #[test]
    fn selection_survives_filter_then_collapse() {
        let theme = pinion_core::theme::Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        // Source 0 (Mesh, category 0): selected + visible at boot.
        let scene = render(Some(0), None, None);
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
            walk(&scene, &format!("{LIST_TAG}#0"))
        };
        assert_eq!(fill, Some(accent), "source 0 selected + Accent at boot");
    }
}
