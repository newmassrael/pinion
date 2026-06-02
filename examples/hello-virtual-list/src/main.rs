//! `hello-virtual-list` — R744 §5.27 **Model/View virtualization first
//! consumer**.
//!
//! A list of `N = 10_000` rows rendered over the existing R55.G
//! [`ScrollNode`](pinion_core::scene::ScrollNode) substrate, but with the
//! Phase-B Model/View twist: the view fn builds scene nodes for **only
//! the visible window** (viewport rows + overscan), not the whole
//! dataset. Scroll the wheel or drag the scrollbar peer and the window
//! slides — different row indices materialize, the off-window rows never
//! exist in the tree.
//!
//! ## Why this is the entry slice
//!
//! Every pre-R744 list (`hello-listbox`, `hello-table`, `todomvc`) builds
//! one node per item eagerly. That is fine at 12 rows and impossible at
//! the 10 000-row data grids the Phase-D editor needs. This binding is
//! the first consumer of
//! [`pinion_widget_paint::virtual_list::view_virtual_list`], which
//! assembles a windowed list whose full-height *sizer* still drives the
//! scroll bound — so the scrollbar thumb is tiny (it sizes against
//! `N × pitch`) while the scene holds ~15 rows.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` over the painted list reports ~`viewport + overscan`
//! row nodes even though `aria-setsize` on the list container is
//! `10_000`. Scroll via `scene/wheel`, snapshot again, and the row tags
//! present have shifted to a higher index band. The whole proof is
//! introspectable as data — no pixels required (see
//! `tools/r744_virtual_list.py`).
//!
//! ## a11y (WAI-ARIA virtualized list)
//!
//! The container reports [`AriaRole::List`] with
//! [`aria-setsize`](pinion_a11y::AccessNode::with_size_of_set) `= N`;
//! each *rendered* row reports [`AriaRole::ListItem`] with its
//! [`aria-posinset`](pinion_a11y::AccessNode::with_position_in_set)
//! `= index + 1`. This is exactly how mature AT stacks model a
//! virtualized list: the rendered subset is announced with its absolute
//! position in the full set. The list itself is not a tab stop and rows
//! are display-only this slice (selection is a later mini-series round),
//! so [`StubExternal`](pinion_core::external::StubExternal) is the
//! addressable list anchor and the only interactive peer is the
//! scrollbar.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_virtual_list;
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloVirtualListRenderer, HelloVirtualListRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 460;
/// Shared [`ThemeProvider`] cache key (the `"app"` convention shared
/// across the catalogue).
const THEME_TAG: &str = "app";
/// Total dataset size — the whole point is that this is large while the
/// rendered node count stays small.
const N: usize = 10_000;
/// Uniform per-row vertical slot in logical pixels. This slice supports
/// uniform pitch only (variable height is a later mini-series round), so
/// the windowing math is exact integer division.
const ROW_PITCH: u32 = 32;
/// Extra rows built above + below the strict visible window so a fast
/// wheel-flick never exposes a blank gap before the next frame.
const OVERSCAN: usize = 3;
/// Scroll viewport width (frames each row slot).
const VIEWPORT_W: u32 = 280;
/// Scroll viewport height — exactly 12 rows tall.
const VIEWPORT_H: u32 = 12 * ROW_PITCH;
/// Paint-root + a11y `list` container tag, and the state-scene tag of
/// the [`StubExternal`] list anchor.
const LIST_TAG: &str = "vlist";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "vlist_scroll";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "vlist_scrollbar";

// This is a display-only list: it has no widget state of its own
// (`type State = ()`). Every repaint trigger is a reactive `Signal`
// subscription the view opens — the theme provider, the `ScrollState`
// offset (`scroll.offset_y()`), and the scrollbar peer's
// `ScrollBarInteractionSignal` (`.get()`). The scrollbar's hover / drag
// phase belongs to the scrollbar External (queryable at
// `/vlist_scrollbar/external/state`); re-projecting it into this widget's
// state would be a redundant copy that also re-declares the canonical
// `ScrollBarState` enum.

/// One virtualized row: a zebra-striped strip carrying its index label.
/// Tagged `"vlist#<i>"` so `enrich_names_from_scene` derives the AT name
/// from the label and `rect_for_tag` can bound the matching `listitem`
/// a11y node. The strip fills the `ROW_PITCH` slot the
/// [`view_virtual_list`] positioning wrapper frames it into.
fn build_row(index: usize, theme: &Theme) -> Scene {
    // Zebra stripe makes the windowing visible: scrolling shifts which
    // indices (and so which stripe parity at a given y) are painted.
    let fill = if index % 2 == 0 {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else {
        theme.resolve(ColorRole::SurfaceContainer)
    };
    let label = Scene::Text(TextNode::styled(
        row_label(index),
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{LIST_TAG}#{index}"))
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

/// Synthetic row content. The five-digit zero-pad keeps every row the
/// same width and makes the index unambiguous in a `scene/snapshot`
/// readout; the category suffix gives the eye something to track while
/// scrolling.
fn row_label(index: usize) -> String {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    format!("Item {index:05} \u{00B7} {}", CATEGORIES[index % CATEGORIES.len()])
}

/// view-fn (§6.3): pure sync mapping (stateless) `() -> Scene`. The dataset
/// is virtual — `view_virtual_list` invokes [`build_row`] only for the
/// indices in the current scroll window.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();

    // The windowed list. `view_virtual_list` reads `scroll_state`'s
    // offset, resolves the visible window, and builds only those rows.
    // `Theme` is `Copy`; the closure borrows it for the duration of the
    // call, after which it is reused for the scrollbar + panel below.
    let list = view_virtual_list(
        &scroll_state,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        N,
        ROW_PITCH,
        OVERSCAN,
        |index| build_row(index, &theme),
    );

    // Scrollbar peer — sized against the *total* extent (the sizer height
    // the layout pass wrote into `ScrollState::max_y`), so the thumb is
    // tiny on a 10 000-row list. Shares the same `Rc<ScrollState>`.
    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    // Composite paint root tagged `LIST_TAG` so `scene/wheel` /
    // `scene/snapshot` `{path: "vlist"}` resolves and the `list` a11y
    // bounds attach to the visible list + gutter. Flex Row lays the
    // windowed list beside the scrollbar peer.
    let list_root = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
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

struct VirtualListView;

impl WidgetCore for VirtualListView {
    /// Display-only: no widget state of its own. Repaints are driven by the
    /// theme / scroll-offset / scrollbar-interaction `Signal` subscriptions
    /// the view opens, not by state change-detection.
    type State = ();
    type Event = ();

    /// The list has no per-item or aggregate widget statechart — the
    /// scroll position lives in [`ScrollState`] and the only interactive
    /// peer is the scrollbar (an extra External). The primary External is
    /// the no-op [`StubExternal`] anchor, addressable at [`LIST_TAG`] for
    /// the input router and the a11y `list` bounds.
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// Sibling `ScrollBarExternal` sharing the list's
    /// `Rc<ScrollState>` — drag the gutter to scroll. Mirrors the
    /// `hello-listbox` multi-External composition (R55.D.5).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG)]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// No widget state to project — the scroll offset and the scrollbar
    /// peer's interaction phase each drive their own repaints through the
    /// reactive `Signal` subscriptions the view opens.
    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Display-only list: nothing here is a keyboard tab stop (the
    /// scrollbar is pointer / RPC driven). Empty so Tab never lands.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-virtual-list (R744 §5.27 Model/View virtualization)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for VirtualListView {
    /// WAI-ARIA virtualized `list`: one [`AriaRole::List`] parent with
    /// `aria-setsize = N` claiming the **rendered** row tags as children,
    /// plus one [`AriaRole::ListItem`] per visible row carrying its
    /// absolute `aria-posinset`. The rendered subset shifts as the user
    /// scrolls — the canonical AT model for a virtualized list (the full
    /// extent is conveyed by `aria-setsize`, the rendered window by the
    /// present `listitem` nodes).
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        // Same windowing source as the view fn (runs owner-wrapped, so
        // `use_scroll_state` resolves the live offset) — the a11y tree
        // and the painted tree never diverge on which rows exist.
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let window = compute_visible_range(scroll_state.offset_y(), VIEWPORT_H, N, ROW_PITCH, OVERSCAN);

        let total = u32::try_from(N).unwrap_or(u32::MAX);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count + 1);
        let mut list = AccessNode::new(LIST_TAG, AriaRole::List)
            .with_name("Virtual item list")
            .with_size_of_set(total);
        for index in window.indices() {
            list = list.with_child(format!("{LIST_TAG}#{index}"));
        }
        nodes.push(list);
        for index in window.indices() {
            let posinset = u32::try_from(index + 1).unwrap_or(u32::MAX);
            nodes.push(
                AccessNode::new(format!("{LIST_TAG}#{index}"), AriaRole::ListItem)
                    .with_position_in_set(posinset)
                    .with_size_of_set(total),
            );
        }
        nodes
    }
}

impl WidgetView for VirtualListView {
    type Renderer = HelloVirtualListRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<VirtualListView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn run_view() -> Scene {
        Owner::new().run(|| view((), &Frame::default()))
    }

    fn find_scroll(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
        match scene {
            Scene::Scroll(s) => Some(s),
            Scene::Container(c) => c.children.iter().find_map(find_scroll),
            _ => None,
        }
    }

    /// Count `vlist#<i>` row containers anywhere in the scene.
    fn count_row_tags(scene: &Scene) -> usize {
        fn walk(scene: &Scene, n: &mut usize) {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref().is_some_and(|t| t.starts_with("vlist#")) {
                        *n += 1;
                    }
                    for child in &c.children {
                        walk(child, n);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), n),
                _ => {}
            }
        }
        let mut n = 0;
        walk(scene, &mut n);
        n
    }

    #[test]
    fn renders_a_small_window_not_the_whole_dataset() {
        let scene = run_view();
        let rendered = count_row_tags(&scene);
        // 12 visible rows + 2×3 overscan, clamped at the top → 12 + 3 =
        // 15 at offset 0. Crucially << N = 10_000.
        assert!(
            rendered < 30,
            "virtualized list must render a small window, got {rendered} of {N}",
        );
        assert!(rendered >= 12, "must at least cover the 12-row viewport");
    }

    #[test]
    fn view_wraps_the_window_in_a_scroll_node() {
        let scene = run_view();
        let scroll = find_scroll(&scene).expect("view must contain a Scene::Scroll");
        assert_eq!(scroll.viewport.w, VIEWPORT_W);
        assert_eq!(scroll.viewport.h, VIEWPORT_H);
        assert!(scroll.state.is_some(), "scroll must carry the state Rc");
    }

    #[test]
    fn a11y_list_reports_full_setsize_with_windowed_items() {
        // `access_node` reads the live scroll offset via `use_scroll_state`,
        // which the substrate wraps in `root_owner.run` (substrate.rs);
        // mirror that wrap here.
        let nodes = Owner::new().run(|| VirtualListView::access_node(&(), None));
        // First node = the list container.
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(N).unwrap()),
            "aria-setsize conveys the FULL dataset size",
        );
        // The rest are the rendered listitems — a small window, each
        // with an absolute posinset.
        assert!(nodes.len() - 1 < 30, "only the rendered window has listitem nodes");
        for item in &nodes[1..] {
            assert_eq!(item.role, AriaRole::ListItem);
            assert!(item.position_in_set.is_some(), "each row carries aria-posinset");
            assert_eq!(item.size_of_set, Some(u32::try_from(N).unwrap()));
        }
        // Posinset is 1-based and matches the first rendered index + 1.
        assert_eq!(nodes[1].position_in_set, Some(1), "top window starts at posinset 1");
    }

    #[test]
    fn row_label_is_stable_width_and_indexed() {
        assert_eq!(row_label(0), "Item 00000 \u{00B7} Alpha");
        assert_eq!(row_label(42), "Item 00042 \u{00B7} Charlie");
    }
}
