//! `hello-virtual-sort` — R747 §5.27 §5.40 **sort / filter on a virtualized
//! list**.
//!
//! R744–R746 land virtualization (window an N-row dataset) and selection
//! held by data index. R747 adds the third Model/View axis: a **sort/filter
//! view-order proxy** ([`ViewSortFilterExternal`]) that maps *visual* row
//! positions to *source* data indices, composed with the R746
//! [`VirtualSelectExternal`] (its **second consumer**). The decisive Model/
//! View witness: selection is held by **source** index, so re-sorting moves
//! a selected row's visual position while keeping the *same data item*
//! selected — selection ⊥ ordering, both data-indexed.
//!
//! ## The three coordinators (Qt `QSortFilterProxyModel` shape)
//!
//! - **Primary** `VirtualSelectExternal` at [`LIST_TAG`] (R746 reuse) — the
//!   windowed `vlist#<source>` rows route clicks here; selection is the
//!   **source** index, never the visual position.
//! - **Extra** `ViewSortFilterExternal` at [`SORT_TAG`] — holds the
//!   `(sort, filter)` and derives the visual→source [`order`] permutation;
//!   the clicked sort header routes here (`vsort#cycle`), filter is driven
//!   by the AI-first `invoke "set_filter"`.
//! - **Extra** `ScrollBarExternal` at [`SCROLLBAR_TAG`] — shares the list's
//!   `ScrollState` (R659 lift).
//!
//! ## The witness (§2 #7 scene-as-data)
//!
//! Boot shows source order (10,000 rows, interleaved categories), small
//! window. Click row whose source index is `s` → it paints Accent and
//! reports `aria-selected`. Cycle the sort (header click or `invoke
//! cycle_sort`) → the window regroups (all "Alpha" rows first, …) and the
//! selected source row `s` is now at a *different visual position* but
//! **still selected**. Filter to a category → `view_len` shrinks, the
//! window adapts, the selected row stays selected iff it passes the filter.
//! The whole proof is data (see `tools/demos/r747_virtual_sort.py`).
//!
//! The order is recomputed in the view (memoized on `(sort, filter)` via
//! [`Owner::cache`]) from the same deterministic row keys the proxy holds,
//! so the view's window and the proxy's `query("order")` agree by
//! construction — the table reads its order back through introspect, but a
//! 10,000-row order is too large to project, so the view recomputes through
//! the shared [`compute_order`] SSOT instead.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::view_order::{
    sort_dir_str, use_view_order, ViewOrderState, ViewSortFilterExternal,
};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::VirtualSelectExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_virtual_list;
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloVirtualSortRenderer, HelloVirtualSortRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 500;
const THEME_TAG: &str = "app";
/// Total dataset size — large while the rendered node count stays small.
const N: usize = 10_000;
/// Uniform per-row vertical slot in logical pixels.
const ROW_PITCH: u32 = 32;
/// Extra rows built above + below the strict visible window.
const OVERSCAN: usize = 3;
const VIEWPORT_W: u32 = 280;
/// Scroll viewport height — exactly 12 rows tall.
const VIEWPORT_H: u32 = 12 * ROW_PITCH;
const HEADER_H: u32 = 40;

/// Paint-root + a11y `list` container tag, and the state-scene tag of the
/// primary [`VirtualSelectExternal`] (clicks on `vlist#<source>` rows route
/// here, selecting the **source** index).
const LIST_TAG: &str = "vlist";
/// The [`ViewSortFilterExternal`] anchor; the sort header routes here.
const SORT_TAG: &str = "vsort";
/// Composite region of the clickable sort header (`vsort#cycle`).
const SORT_REGION: &str = "cycle";
const SCROLL_KEY: &str = "vlist_scroll";
const SCROLLBAR_TAG: &str = "vlist_scrollbar";

/// Five filter categories. Source row `i` belongs to `CATEGORIES[i % 5]`.
const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];

/// Filter category id of source row `i`.
fn row_category(i: usize) -> usize {
    i % CATEGORIES.len()
}

/// Display label / sort key of source row `i`: `"<Category> · <iiiii>"`.
/// The category-first key means an ascending sort regroups the interleaved
/// source order into contiguous category blocks (a dramatic, verifiable
/// reordering); the zero-padded index keeps widths stable.
fn row_label(i: usize) -> String {
    format!("{} \u{00B7} {i:05}", CATEGORIES[row_category(i)])
}

/// The shared [`ViewOrderState`] — the single source of truth for the
/// list's sort/filter view order. The view, the a11y tree, and the
/// [`ViewSortFilterExternal`] all reach the same `Rc` through this hook (the
/// `use_scroll_state` pattern), so the order is computed once and the rows
/// the view paints are the rows the proxy's `query` reports. The source keys
/// + categories are materialized once, here, on first resolution.
fn use_list_order() -> Rc<ViewOrderState> {
    use_view_order(SORT_TAG, || {
        let keys = (0..N).map(row_label).collect::<Vec<String>>();
        let cats = (0..N).map(row_category).collect::<Vec<usize>>();
        (keys, cats)
    })
}

/// One virtualized row. `source` is the data index (the row's identity); the
/// row is tagged `"vlist#<source>"` so a click routes to the
/// [`VirtualSelectExternal`] anchor and selects the **source** index. The
/// selected source row paints Accent; others zebra-stripe by source parity
/// (so the stripe travels with the data, making a re-sort visible).
fn build_row(source: usize, theme: &Theme, selected: Option<usize>) -> Scene {
    let is_selected = selected == Some(source);
    let (fill, fg) = if is_selected {
        (theme.resolve(ColorRole::Accent), theme.resolve(ColorRole::OnAccent))
    } else {
        let stripe = if source % 2 == 0 {
            ColorRole::SurfaceContainerLow
        } else {
            ColorRole::SurfaceContainer
        };
        (theme.resolve(stripe), theme.resolve(ColorRole::OnSurface))
    };
    let label = Scene::Text(TextNode::styled(
        row_label(source),
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(fg),
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
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// The clickable sort header: shows the active sort + filter and cycles the
/// sort on click. Tagged `"vsort#cycle"` (R51.42 composite) so a click
/// routes to the [`ViewSortFilterExternal`] anchor.
fn sort_header(sort: Option<bool>, filter: Option<usize>, theme: &Theme) -> Scene {
    let arrow = match sort {
        None => "\u{2195}",        // ↕ unsorted
        Some(true) => "\u{25B2}",  // ▲ ascending
        Some(false) => "\u{25BC}", // ▼ descending
    };
    let filter_label = match filter {
        Some(c) => CATEGORIES[c % CATEGORIES.len()],
        None => "All",
    };
    let text = format!("Sort {arrow}   \u{00B7}   Filter: {filter_label}");
    let label = Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{SORT_TAG}#{SORT_REGION}"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(VIEWPORT_W, HEADER_H)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping `selected -> Scene`. The dataset is
/// virtual — `view_virtual_list` windows over the *view* length
/// (`order.len()`), and each visual position resolves to its source index
/// through the shared [`ViewOrderState`]. Sort/filter live in that reactive
/// holder (read here, not projected into `State`), so a sort/filter change
/// repaints through the `Signal` subscription exactly as scroll does.
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();

    let view_order = use_list_order();
    let sort = view_order.sort();
    let filter = view_order.filter();
    let order = view_order.order();
    let view_len = order.len();

    let header = sort_header(sort, filter, &theme);

    let list = view_virtual_list(
        &scroll_state,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        view_len,
        ROW_PITCH,
        OVERSCAN,
        |view_pos| build_row(order[view_pos], &theme, selected),
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
        ContainerNode::new(vec![header, list_row])
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

struct VirtualSortView;

impl WidgetCore for VirtualSortView {
    /// The widget's projected state is just the selected **source** index
    /// (like `hello-virtual-select`). Sort/filter are an auxiliary reactive
    /// axis held in the shared [`ViewOrderState`] — read in the view, not
    /// projected here (the order is not state to copy; a config change
    /// repaints through its `Signal`, like scroll offset).
    type State = Option<usize>;
    type Event = ();

    /// Primary = the index-held selection coordinator (R746), at
    /// [`LIST_TAG`]. The windowed `vlist#<source>` rows route here.
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    /// Extras: the sort/filter proxy (a thin adapter over the **same**
    /// shared [`ViewOrderState`] the view reads via [`use_list_order`]) and
    /// the scrollbar peer.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(SORT_TAG, Box::new(ViewSortFilterExternal::new(use_list_order()))),
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
        ]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// Project the selected source index off the primary coordinator. A
    /// selection change raises the §5.20 transition and repaints; sort /
    /// filter / scroll repaint via their own reactive `Signal` subscriptions
    /// the view opens.
    fn read_state(scene: &Scene) -> Option<usize> {
        scene
            .find_external_with_tag(LIST_TAG)
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

    /// Pointer / RPC selection + sort this slice; neither the rows nor the
    /// sort header are keyboard tab stops (windowed-list roving is a later
    /// axis). Empty so Tab never lands.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-virtual-sort (R747 §5.27 §5.40 sort/filter virtualization)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=source {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for VirtualSortView {
    /// Single-select WAI-ARIA virtualized `list` over the *current view
    /// order*: one [`AriaRole::List`] (`aria-setsize = view_len`) over the
    /// rendered window; each visible row is an [`AriaRole::ListItem`] at its
    /// **visual** `aria-posinset` with `aria-selected = (source == selected)`.
    /// The sort header is a [`AriaRole::Button`] naming the active sort.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let selected = *selected;
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let view_order = use_list_order();
        let sort = view_order.sort();
        let order = view_order.order();
        let view_len = order.len();
        let window =
            compute_visible_range(scroll_state.offset_y(), VIEWPORT_H, view_len, ROW_PITCH, OVERSCAN);

        let total = u32::try_from(view_len).unwrap_or(u32::MAX);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count + 2);

        nodes.push(
            AccessNode::new(format!("{SORT_TAG}#{SORT_REGION}"), AriaRole::Button)
                .with_name(format!("Sort: {}", sort_dir_str(sort))),
        );

        let mut list = AccessNode::new(LIST_TAG, AriaRole::List)
            .with_name("Sortable filterable item list")
            .with_size_of_set(total);
        for view_pos in window.indices() {
            list = list.with_child(format!("{LIST_TAG}#{}", order[view_pos]));
        }
        nodes.push(list);

        for view_pos in window.indices() {
            let source = order[view_pos];
            let posinset = u32::try_from(view_pos + 1).unwrap_or(u32::MAX);
            nodes.push(
                AccessNode::new(format!("{LIST_TAG}#{source}"), AriaRole::ListItem)
                    .with_position_in_set(posinset)
                    .with_size_of_set(total)
                    .with_selected(selected == Some(source)),
            );
        }
        nodes
    }
}

impl WidgetView for VirtualSortView {
    type Renderer = HelloVirtualSortRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<VirtualSortView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// Render the view with the shared [`ViewOrderState`] pre-set to `sort` /
    /// `filter` (mutated within the same Owner scope the view reads from).
    fn render(selected: Option<usize>, sort: Option<bool>, filter: Option<usize>) -> Scene {
        Owner::new().run(|| {
            let vo = use_list_order();
            vo.set_sort(sort);
            vo.set_filter(filter);
            view(selected, &Frame::default())
        })
    }

    /// Find the `vlist#<source>` row container and return its fill color.
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

    /// Collect the `vlist#<source>` tags present in the scene, in tree order
    /// (= visual order, since the windowed slots are appended in view-pos
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

    #[test]
    fn boot_renders_small_window_in_source_order() {
        let scene = render(None, None, None);
        let sources = present_sources(&scene);
        assert!(sources.len() < 30, "virtualized: small window, got {}", sources.len());
        // Unsorted: visual order == source order at the top.
        assert_eq!(&sources[..5], &[0, 1, 2, 3, 4]);
    }

    #[test]
    fn ascending_sort_regroups_window_by_category() {
        let scene = render(None, Some(true), None);
        let sources = present_sources(&scene);
        // Ascending by "Alpha · …": the first visible sources are the
        // Alpha rows 0,5,10,15,… (category 0 = every 5th source index).
        assert_eq!(&sources[..4], &[0, 5, 10, 15], "Alpha block leads the ascending view");
    }

    #[test]
    fn selected_source_follows_the_sort_and_stays_accent() {
        let theme = pinion_core::theme::Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        // Select source 5 (an Alpha row). Unsorted it sits at visual pos 5;
        // ascending it jumps to visual pos 1 — but it is the SAME data row,
        // still Accent in both.
        let unsorted = render(Some(5), None, None);
        assert_eq!(row_fill(&unsorted, 5), Some(accent), "source 5 selected unsorted");
        let sorted = render(Some(5), Some(true), None);
        assert_eq!(row_fill(&sorted, 5), Some(accent), "source 5 still selected after sort");
        // Its visual position moved: now second in the Alpha block.
        assert_eq!(present_sources(&sorted)[1], 5);
    }

    #[test]
    fn filter_shrinks_the_view_to_one_category() {
        // Filter to category 1 (Bravo = sources 1,6,11,…). Every rendered
        // row must be a Bravo source (≡ 1 mod 5).
        let scene = render(None, None, Some(1));
        let sources = present_sources(&scene);
        assert!(!sources.is_empty());
        assert!(
            sources.iter().all(|&s| s % 5 == 1),
            "every visible row is a Bravo (category 1) source: {sources:?}",
        );
    }

    #[test]
    fn a11y_marks_selected_source_and_visual_posinset() {
        // Source 5 selected, ascending: it is visual position 2 (posinset 2)
        // in the Alpha block and carries aria-selected.
        let nodes = Owner::new().run(|| {
            let vo = use_list_order();
            vo.set_sort(Some(true));
            VirtualSortView::access_node(&Some(5), None)
        });
        // nodes[0] = sort button, nodes[1] = list, rest = items.
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert_eq!(nodes[1].role, AriaRole::List);
        let item = nodes[2..]
            .iter()
            .find(|n| n.tag == format!("{LIST_TAG}#5"))
            .expect("rendered listitem for source 5");
        assert_eq!(item.selected, Some(true), "selected source carries aria-selected");
        assert_eq!(item.position_in_set, Some(2), "source 5 is visual position 2 ascending");
    }
}
