//! `hello-grouped-list` — R843 §5.27 §5.40 **group-by on a virtualized list**.
//!
//! R747 lands the sort/filter view-order proxy and R821 the tree filter proxy.
//! R843 adds the fourth Model/View axis the DCC asset browser / scene outliner
//! needs: a **group-by proxy with collapsible group headers**
//! ([`GroupOrderExternal`]) that partitions 10,000 source rows into 6
//! asset-type groups, each fronted by a collapsible header, and flattens the
//! whole thing into one windowable [`GroupRow`] sequence
//! ([`GroupOrderState::rows`]). The list is **virtualized** — headers and data
//! rows share one uniform-pitch [`view_virtual_list`] window, so only the
//! visible window plus overscan is materialized however many groups expand.
//!
//! ## The three coordinators (Qt grouping-model shape)
//!
//! - **Primary** [`VirtualSelectExternal`] at [`LIST_TAG`] (R746 reuse) — the
//!   windowed `glist#<source>` data rows route clicks here; selection is the
//!   **source** index, never the visual position.
//! - **Extra** [`GroupOrderExternal`] at [`GROUP_TAG`] — holds the per-group
//!   collapse set and flattens the grouped rows; a clicked group header routes
//!   here (`ggroup#<group>`), collapse/expand is driven by the AI-first
//!   `invoke "toggle_group"` / `"collapse_all"` / `"expand_all"`.
//! - **Extra** [`ScrollBarExternal`] at [`SCROLLBAR_TAG`] — shares the list's
//!   `ScrollState` (R659 lift).
//!
//! ## The witness (§2 #7 scene-as-data)
//!
//! Boot shows 6 group headers + 10,000 data rows = 10,006 visible rows, small
//! window. Click a data row whose source index is `s` → it paints Accent and
//! reports `aria-selected`. Collapse the group `s` belongs to (header click or
//! `invoke toggle_group`) → its data rows vanish from the flattening
//! (`visible_len` shrinks by the group's member count) **but `s` stays
//! selected** — selection is held by source index, orthogonal to grouping.
//! Expand the group → row `s` re-appears, still Accent. `collapse_all` leaves
//! only the 6 headers; `expand_all` restores 10,006. The whole proof is data
//! (see `tools/demos/r843_grouped_list.py`).

use std::rc::Rc;

use pinion_a11y::{
    AccessFocus, AccessNode, GroupedTreeSpec, WidgetA11y, grouped_focus_target,
    grouped_tree_access_nodes,
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
vello_renderer_impl!(HelloGroupedListRenderer, HelloGroupedListRendererError);

const WIN_W: u32 = 380;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";
/// Total dataset size — large while the rendered node count stays small.
const N: usize = 10_000;
/// Uniform per-row vertical slot in logical pixels (headers + data rows alike).
const ROW_PITCH: u32 = 28;
/// Extra rows built above + below the strict visible window.
const OVERSCAN: usize = 3;
const VIEWPORT_W: u32 = 300;
/// Scroll viewport height — exactly 14 rows tall.
const VIEWPORT_H: u32 = 14 * ROW_PITCH;

/// Paint-root + a11y `tree` container tag, and the state-scene tag of the
/// primary [`VirtualSelectExternal`] (clicks on `glist#<source>` data rows
/// route here, selecting the **source** index).
const LIST_TAG: &str = "glist";
/// The [`GroupOrderExternal`] anchor; a clicked group header routes here
/// (`ggroup#<group>`).
const GROUP_TAG: &str = "ggroup";
const SCROLL_KEY: &str = "glist_scroll";
const SCROLLBAR_TAG: &str = "glist_scrollbar";

/// Six asset-type groups. Source row `i` belongs to `GROUPS[i % 6]`.
const GROUPS: [&str; 6] = ["Mesh", "Texture", "Material", "Sound", "Script", "Prefab"];

/// Group id of source row `i` (an index into [`GROUPS`]).
fn row_group(i: usize) -> usize {
    i % GROUPS.len()
}

/// R848 — the list's projected state: the selected source row **and** the
/// roving keyboard cursor's visual position. The cursor is *projected* (not
/// only held in the reactive [`GroupOrderState`]) so a cursor-only move —
/// arrowing between group headers with no selection change — repaints and the
/// focus ring tracks it (the datepicker `focused_day` precedent: a roving
/// cursor distinct from the selection must be projected state, because the
/// shell repaints on a *visible-state* change and the ring is shell-drawn from
/// [`access_focus_target`](GroupedListView::access_focus_target)).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
struct ListState {
    /// The selected source row (the [`VirtualSelectExternal`] index), or `None`.
    selected: Option<usize>,
    /// The roving cursor's visual position into the flattened rows, or `None`.
    cursor: Option<usize>,
}

/// Display label of data row `i`: a stable, width-fixed asset name.
fn row_label(i: usize) -> String {
    format!("asset_{i:05}")
}

/// The shared [`GroupOrderState`] — the single source of truth for the list's
/// grouping + collapse state. The view, the a11y tree, and the
/// [`GroupOrderExternal`] all reach the same `Rc` through this hook (the
/// `use_scroll_state` pattern), so the flattening the view paints is the
/// flattening the proxy's `query` reports. The source group keys + label table
/// are materialized once, here, on first resolution.
fn use_list_groups() -> Rc<GroupOrderState> {
    use_group_order(GROUP_TAG, || {
        let groups = (0..N).map(row_group).collect::<Vec<usize>>();
        let labels = GROUPS
            .iter()
            .map(|&g| g.to_string())
            .collect::<Vec<String>>();
        (groups, labels)
    })
}

/// One group header row. `group` is the group id; the row is tagged
/// `"ggroup#<group>"` (R51.42 composite) so a click routes to the
/// [`GroupOrderExternal`] anchor and toggles the group. The header *skin*
/// (twisty + label + count, the `SurfaceContainerHigh` flex row) is the
/// [`group_header_row`] SSOT (R871); this binds the example's group-label table
/// and list width to it.
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

/// One data row. `source` is the data index (the row's identity); the row is
/// tagged `"glist#<source>"` so a click routes to the [`VirtualSelectExternal`]
/// anchor and selects the **source** index. The selected source row paints
/// Accent; others zebra-stripe by source parity (so the stripe travels with the
/// data). Indented under its group header.
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
    let label = Scene::Text(TextNode::styled(
        row_label(source),
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

/// view-fn (§6.3): pure sync mapping `selected -> Scene`. The dataset is
/// virtual — `view_virtual_list` windows over the *flattened* length
/// (`rows.len()`, headers + expanded-group data rows), and each visual position
/// is a [`GroupRow`] resolved through the shared [`GroupOrderState`]. The
/// collapse state lives in that reactive holder (read here, not projected into
/// `State`), so a collapse/expand repaints through the `Signal` subscription
/// exactly as scroll does.
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();

    let groups = use_list_groups();
    let rows = groups.rows();
    let visible_len = rows.len();

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
        ContainerNode::new(vec![list_row])
            .with_tag(LIST_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_focusable(true),
            ),
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

struct GroupedListView;

impl WidgetCore for GroupedListView {
    /// The widget's projected state is just the selected **source** index
    /// (like `hello-virtual-sort`). Grouping/collapse are an auxiliary reactive
    /// axis held in the shared [`GroupOrderState`] — read in the view, not
    /// projected here (a collapse change repaints through its `Signal`, like
    /// scroll offset).
    type State = ListState;
    type Event = ();

    /// Primary = the index-held selection coordinator (R746), at [`LIST_TAG`].
    /// The windowed `glist#<source>` data rows route here.
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    /// Extras: the group-by proxy (a thin adapter over the **same** shared
    /// [`GroupOrderState`] the view reads via [`use_list_groups`]) and the
    /// scrollbar peer.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                GROUP_TAG,
                Box::new(GroupOrderExternal::new(use_list_groups())),
            ),
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
        ]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// Project the selected source index off the primary coordinator. A
    /// selection change raises the §5.20 transition and repaints; grouping /
    /// scroll repaint via their own reactive `Signal` subscriptions the view
    /// opens.
    fn read_state(scene: &Scene) -> ListState {
        let selected = scene
            .find_external_with_tag(LIST_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(read_selected);
        // The roving cursor lives on the extra GroupOrderExternal — the same
        // find → introspect → read idiom as the selection above.
        let cursor = scene
            .find_external_with_tag(GROUP_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(read_cursor);
        ListState { selected, cursor }
    }

    fn view(state: ListState, frame: &Frame) -> Scene {
        view(state.selected, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// R848 §5.27 — keyboard navigation over the grouped list, delegated to the
    /// shared [`group_nav_key`] controller (the same one `hello-grouped-grid`
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
            &use_list_groups(),
            LIST_TAG,
            focused,
            key,
            ROW_PITCH,
        )
    }

    fn title() -> &'static str {
        "pinion hello-grouped-list (R848 §5.27 §5.40 group-by + keyboard nav)"
    }

    fn fmt_state_log(state: &ListState) -> String {
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

impl WidgetA11y for GroupedListView {
    /// WAI-ARIA `tree` over the *flattened* grouped view (a grouped collapsible
    /// list is a one-level tree to AT), via the
    /// [`grouped_tree_access_nodes`](pinion_a11y::grouped_tree_access_nodes) SSOT
    /// builder (R847): group headers are `treeitem`s at `aria-level = 1` with
    /// `aria-expanded`, data rows are `treeitem`s at `aria-level = 2` with
    /// `aria-selected`. The two composite namespaces — group headers route to the
    /// `GroupOrderExternal` (`ggroup#`), data rows to the `VirtualSelectExternal`
    /// (`glist#`) — are the spec's `header_prefix` / `data_prefix`; only the
    /// labels differ per consumer.
    fn access_node(state: &ListState, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let groups = use_list_groups();
        let rows = groups.rows();
        let window = compute_visible_range(
            scroll_state.offset_y(),
            VIEWPORT_H,
            rows.len(),
            ROW_PITCH,
            OVERSCAN,
        );
        let spec = GroupedTreeSpec {
            tree_tag: LIST_TAG,
            name: Some("Grouped asset list"),
            header_prefix: GROUP_TAG,
            data_prefix: LIST_TAG,
            selected_source: state.selected,
            focused_view_pos: state.cursor,
        };
        grouped_tree_access_nodes(
            &spec,
            &rows,
            window,
            |g| GROUPS[g % GROUPS.len()].to_string(),
            row_label,
        )
    }

    /// R848 / R850 — single-tab-stop roving: shell focus stays on the tree
    /// container while the ring frames the cursor's row. Delegated to the
    /// [`grouped_focus_target`] SSOT (shared with `hello-grouped-grid`) so the
    /// ring policy is one source of truth across both grouped presentations.
    fn access_focus_target(state: &ListState, focused: Option<&str>) -> Option<AccessFocus> {
        grouped_focus_target(
            &use_list_groups(),
            LIST_TAG,
            GROUP_TAG,
            state.cursor,
            focused,
        )
    }
}

impl WidgetView for GroupedListView {
    type Renderer = HelloGroupedListRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<GroupedListView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    /// Render the view with the shared [`GroupOrderState`] pre-set to a
    /// collapse configuration (mutated within the same Owner scope the view
    /// reads from).
    fn render(selected: Option<usize>, collapse: impl FnOnce(&GroupOrderState)) -> Scene {
        Owner::new().run(|| {
            let g = use_list_groups();
            collapse(&g);
            view(selected, &Frame::default())
        })
    }

    /// Collect the `glist#<source>` data-row tags present in the scene, in tree
    /// order (= visual order, since the windowed slots are appended in view-pos
    /// order).
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

    /// Collect the `ggroup#<group>` header tags present in the scene, in tree
    /// order.
    fn present_headers(scene: &Scene) -> Vec<usize> {
        fn walk(scene: &Scene, out: &mut Vec<usize>) {
            match scene {
                Scene::Container(c) => {
                    if let Some(tag) = c.tag.as_deref() {
                        if let Some(rest) = tag.strip_prefix(&format!("{GROUP_TAG}#")) {
                            if let Ok(g) = rest.parse::<usize>() {
                                out.push(g);
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

    /// Find the `glist#<source>` data row container and return its fill color.
    fn row_fill(scene: &Scene, source: usize) -> Option<pinion_core::style::Color> {
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
        walk(scene, &format!("{LIST_TAG}#{source}"))
    }

    #[test]
    fn boot_renders_all_six_headers_in_a_small_window() {
        let scene = render(None, |_| {});
        // The top of the flattening is group 0's header + its first data rows.
        let headers = present_headers(&scene);
        assert_eq!(headers.first(), Some(&0), "group 0 header leads the view");
        let sources = present_sources(&scene);
        assert!(
            sources.len() < 30,
            "virtualized: small window, got {}",
            sources.len()
        );
        // group 0 = source indices 0,6,12,… (every 6th); the first data rows
        // under the Mesh header are those.
        assert_eq!(&sources[..3], &[0, 6, 12], "Mesh group's first members");
    }

    #[test]
    fn collapse_all_leaves_only_headers_no_data_rows() {
        let scene = render(None, GroupOrderState::collapse_all);
        assert_eq!(
            present_headers(&scene),
            vec![0, 1, 2, 3, 4, 5],
            "all six headers, no data"
        );
        assert!(
            present_sources(&scene).is_empty(),
            "collapsed: no data rows rendered"
        );
    }

    #[test]
    fn collapsing_group_zero_hides_its_data_but_keeps_selection() {
        let theme = pinion_core::theme::Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        // Select source 6 (a Mesh = group 0 row). Expanded it is rendered Accent.
        let expanded = render(Some(6), |_| {});
        assert_eq!(
            row_fill(&expanded, 6),
            Some(accent),
            "source 6 selected, group expanded"
        );
        // Collapse group 0: source 6's row vanishes from the window, but the
        // selection (held by source index) is untouched.
        let collapsed = render(Some(6), |g| g.set_collapsed(0, true));
        assert!(
            !present_sources(&collapsed).contains(&6),
            "collapsed group 0 hides source 6's row",
        );
        // Re-expanding reveals it, still Accent (selection survived the collapse).
        let reexpanded = render(Some(6), |g| g.set_collapsed(0, false));
        assert_eq!(
            row_fill(&reexpanded, 6),
            Some(accent),
            "source 6 still selected after expand"
        );
    }

    #[test]
    fn a11y_marks_tree_with_expandable_headers_and_selected_data_rows() {
        // Selection on data source 6; cursor on visual pos 0 (the group-0
        // header) — cursor ⊥ selection.
        let nodes = Owner::new().run(|| {
            GroupedListView::access_node(
                &ListState {
                    selected: Some(6),
                    cursor: Some(0),
                },
                None,
            )
        });
        // nodes[0] = tree container.
        assert_eq!(nodes[0].role, AriaRole::Tree);
        // The group 0 header is a level-1 expanded TreeItem, and the cursor
        // rests on it → focused (the active descendant).
        let header = nodes[1..]
            .iter()
            .find(|n| n.tag == format!("{GROUP_TAG}#0"))
            .expect("rendered group 0 header");
        assert_eq!(header.role, AriaRole::TreeItem);
        assert_eq!(header.level, Some(1));
        assert_eq!(
            header.expanded,
            Some(true),
            "expanded group header carries aria-expanded"
        );
        assert!(header.state.focused, "the cursor rests on the header");
        // The selected data row (source 6) is a level-2 TreeItem with
        // aria-selected, and is *not* the cursor (cursor ⊥ selection).
        let item = nodes[1..]
            .iter()
            .find(|n| n.tag == format!("{LIST_TAG}#6"))
            .expect("rendered data row for source 6");
        assert_eq!(item.level, Some(2));
        assert_eq!(
            item.selected,
            Some(true),
            "selected data row carries aria-selected"
        );
        assert!(
            !item.state.focused,
            "the selected row is not the cursor here"
        );
    }

    #[test]
    fn access_focus_target_rings_the_cursor_row() {
        Owner::new().run(|| {
            let _ = use_list_groups(); // build the shared holder (all groups expanded)
            // Cursor on visual pos 0 = the group-0 header.
            let af = GroupedListView::access_focus_target(
                &ListState {
                    selected: None,
                    cursor: Some(0),
                },
                Some(LIST_TAG),
            )
            .expect("list focused returns Some");
            assert_eq!(
                af.active_descendant.as_deref(),
                Some(&*format!("{GROUP_TAG}#0"))
            );
            // Cursor on visual pos 1 = the first Mesh data row (source 0).
            let af1 = GroupedListView::access_focus_target(
                &ListState {
                    selected: None,
                    cursor: Some(1),
                },
                Some(LIST_TAG),
            )
            .expect("list focused returns Some");
            assert_eq!(
                af1.active_descendant.as_deref(),
                Some(&*format!("{LIST_TAG}#0"))
            );
            // No cursor → ring the container (no active descendant).
            let af_none =
                GroupedListView::access_focus_target(&ListState::default(), Some(LIST_TAG))
                    .expect("list focused returns Some");
            assert_eq!(af_none.active_descendant, None);
        });
    }
}
