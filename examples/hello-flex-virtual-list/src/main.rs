//! `hello-flex-virtual-list` — R774 §5.27 **flex-viewport
//! (`AutoSizer`) virtualization**.
//!
//! The R744 `hello-virtual-list` renders only the visible window of a
//! 10 000-row dataset, but its viewport height is a **const**
//! (`VIEWPORT_H`): the rendered row count is fixed no matter how big the
//! window is. This binding closes that gap. The list's
//! [`ScrollNode`](pinion_core::scene::ScrollNode) is laid out with
//! `flex_grow = 1.0`, so it fills whatever vertical space the window
//! grants it; the windowing math reads the **runtime-measured** viewport
//! height back from
//! [`ScrollState::measured_viewport`](pinion_core::widgets::scroll::ScrollState::measured_viewport)
//! and materializes exactly the rows that fit. Resize the window taller
//! and more rows appear; shrink it and the rendered count drops — the
//! whole list "auto-sizes" to its container.
//!
//! This is react-window's `AutoSizer` pattern: the container measures
//! itself during layout, publishes the extent into the reactive
//! `ScrollState`, and the next view re-run windows against the true
//! height. The first-paint chicken-and-egg (the view fn runs *before*
//! layout) is resolved by the same scroll-dirty same-frame re-pass the
//! `set_max` scroll-bound write already uses.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` over the painted list reports ~`measured_viewport / pitch`
//! row nodes. Drive `scene/resize` to a taller window, snapshot again,
//! and the rendered row count has *grown* — even though `N = 10_000` and
//! the dataset never changed. The proof is introspectable as data, no
//! pixels required (see `tools/r774_flex_virtual_list.py`).
//!
//! ## a11y (WAI-ARIA virtualized list)
//!
//! Identical model to `hello-virtual-list`: the container reports
//! `AriaRole::List` with `aria-setsize = N`; each *rendered* row reports
//! `AriaRole::ListItem` with its absolute `aria-posinset`. The rendered
//! subset now tracks the measured viewport, so resizing the window
//! changes how many `listitem` nodes the AT tree exposes — exactly the
//! visible set, exactly as a sighted user sees.

use pinion_a11y::{windowed_list_nodes, AccessNode, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_flex_virtual_list;
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFlexVirtualListRenderer, HelloFlexVirtualListRendererError);

/// Initial window size — the window is freely resizable (OS chrome +
/// `scene/resize`), and the list re-windows on every `Resized` event.
const WIN_W: u32 = 380;
const WIN_H: u32 = 480;
/// Shared [`ThemeProvider`] cache key (the `"app"` convention shared
/// across the catalogue).
const THEME_TAG: &str = "app";
/// Total dataset size — large while the rendered node count stays small
/// and tracks the window height.
const N: usize = 10_000;
/// Uniform per-row vertical slot in logical pixels.
const ROW_PITCH: u32 = 32;
/// Extra rows built above + below the strict visible window so a fast
/// wheel-flick never exposes a blank gap before the next frame.
const OVERSCAN: usize = 3;
/// Fixed header-bar height; the flex list fills the window below it.
const HEADER_H: u32 = 44;
/// Gutter width reserved for the scrollbar peer.
const GUTTER_W: u32 = 12;
/// Paint-root + a11y `list` container tag, and the state-scene tag of
/// the [`StubExternal`] list anchor.
const LIST_TAG: &str = "flexlist";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "flexlist_scroll";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "flexlist_scrollbar";
/// Tag for the header-bar status text (rendered row count + measured
/// viewport) — a scene-as-data witness that the count tracks the window.
const STATUS_TAG: &str = "flexlist_status";

// Display-only list: it has no widget state of its own
// (`type State = ()`). Every repaint trigger is a reactive `Signal`
// subscription the view opens — the theme provider, the `ScrollState`
// offset, the scrollbar peer's interaction phase, and (new this round)
// the `ScrollState` measured viewport, which the runtime layout pass
// writes from the flex-computed clip-window rect.

/// One virtualized row: a zebra-striped strip carrying its index label,
/// framed to the measured viewport width so it fills the resizing pane.
fn build_row(index: usize, theme: &Theme, width: u32) -> Scene {
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
                    .with_size(Size::px(width, ROW_PITCH))
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// Synthetic row content; the five-digit zero-pad keeps every row the
/// same width and makes the index unambiguous in a `scene/snapshot`.
fn row_label(index: usize) -> String {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    format!("Item {index:05} \u{00B7} {}", CATEGORIES[index % CATEGORIES.len()])
}

/// Header-bar status line: the rendered row count + the measured
/// viewport extent. Both are read from the same `ScrollState` the list
/// windows against, so this text is a literal readout of the `AutoSizer`
/// state — resize the window and the numbers move in lockstep with the
/// rendered rows.
fn header(scroll: &std::rc::Rc<pinion_core::widgets::scroll::ScrollState>, theme: &Theme) -> Scene {
    let (mw, mh) = scroll.measured_viewport();
    let window = compute_visible_range(scroll.offset_y(), mh, N, ROW_PITCH, OVERSCAN);
    let text = Scene::Text(
        TextNode::styled(
            format!("rows {} / {N} \u{00B7} viewport {mw}\u{00D7}{mh}", window.count),
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
                    // Auto width (stretch to the column) + fixed height,
                    // so the bar spans the window at any resize while the
                    // flex list below claims the remaining height.
                    .with_size(Size::auto().with_height(SizeValue::Px(HEADER_H)))
                    .with_flex_grow(0.0)
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping (stateless) `() -> Scene`. The
/// dataset is virtual — `view_flex_virtual_list` invokes [`build_row`]
/// only for the indices in the current window, whose *size* is the
/// runtime-measured viewport height.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let (measured_w, measured_h) = scroll.measured_viewport();

    // The windowed list — flex-grow ScrollNode, height resolved by the
    // runtime layout pass. `build_row` frames each row to the measured
    // width so rows fill the pane as it resizes.
    let list = view_flex_virtual_list(&scroll, N, ROW_PITCH, OVERSCAN, |index| {
        build_row(index, &theme, measured_w)
    });

    // Scrollbar peer — sized against the *measured* viewport height (so
    // the track matches the laid-out list) and the *total* extent (so
    // the thumb is tiny on a 10 000-row list). Shares the same
    // `Rc<ScrollState>`.
    let scrollbar_style = {
        let mut s = VerticalScrollbarStyle::material(measured_h, SCROLLBAR_TAG);
        s.gutter_w = GUTTER_W;
        s
    };
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    // Row band: the flex list beside the scrollbar peer, flex-grow so it
    // fills the window below the header. The list grows along the row's
    // main axis (width) and stretches the cross axis (height) to the
    // band — which `flex_grow` makes equal to `WIN_H - HEADER_H`.
    let band = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_flex_grow(1.0),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![header(&scroll, &theme), band])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct FlexVirtualListView;

impl WidgetCore for FlexVirtualListView {
    /// Display-only: no widget state of its own. Repaints are driven by
    /// the theme / scroll-offset / scrollbar-interaction / measured-
    /// viewport `Signal` subscriptions the view opens.
    type State = ();
    type Event = ();

    /// The only interactive peer is the scrollbar; the primary External
    /// is the no-op [`StubExternal`] anchor, addressable at [`LIST_TAG`]
    /// for the input router and the a11y `list` bounds.
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// Sibling `ScrollBarExternal` sharing the list's `Rc<ScrollState>`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG)]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// No widget state to project — the scroll offset, the scrollbar
    /// peer's interaction phase, and the measured viewport each drive
    /// their own repaints through reactive `Signal` subscriptions.
    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Display-only list: nothing here is a keyboard tab stop (the
    /// scrollbar is pointer / RPC driven).
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-flex-virtual-list (R774 §5.27 AutoSizer virtualization)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for FlexVirtualListView {
    /// WAI-ARIA virtualized `list`: one `AriaRole::List` parent with
    /// `aria-setsize = N` claiming the **rendered** row tags as children,
    /// plus one `AriaRole::ListItem` per visible row carrying its
    /// absolute `aria-posinset`. The rendered subset tracks the measured
    /// viewport height — resizing the window changes how many `listitem`
    /// nodes the AT tree exposes, mirroring the painted window.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        // Same windowing source as the view fn (runs owner-wrapped, so
        // `use_scroll_state` resolves the live offset + measured
        // viewport) — the a11y tree and the painted tree never diverge.
        // The windowed `list` + `listitem` topology is the shared
        // display-only shape lifted to `pinion_a11y::windowed_list_nodes`
        // (R774; shared with hello-virtual-list + hello-variable-list).
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_PITCH, OVERSCAN);
        windowed_list_nodes(LIST_TAG, "Virtual item list", u32::try_from(N).unwrap_or(u32::MAX), &window)
    }
}

impl WidgetView for FlexVirtualListView {
    type Renderer = HelloFlexVirtualListRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<FlexVirtualListView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::widgets::scroll::ScrollState;
    use pinion_core::Owner;
    use std::rc::Rc;

    /// Seed a `ScrollState` with a measured viewport (the runtime layout
    /// pass writes this in production; here we drive it directly for a
    /// deterministic unit shape) and run the view inside an Owner whose
    /// cache resolves `use_scroll_state(SCROLL_KEY)` to that same state.
    fn run_view_with_measured(measured_h: u32) -> Scene {
        let owner = Owner::new();
        owner.run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(360, measured_h);
            view((), &Frame::default())
        })
    }

    fn find_scroll(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
        match scene {
            Scene::Scroll(s) => Some(s),
            Scene::Container(c) => c.children.iter().find_map(find_scroll),
            _ => None,
        }
    }

    /// Count `flexlist#<i>` row containers anywhere in the scene.
    fn count_row_tags(scene: &Scene) -> usize {
        fn walk(scene: &Scene, n: &mut usize) {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref().is_some_and(|t| t.starts_with("flexlist#")) {
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
    fn scroll_node_is_flex_grow_not_fixed_size() {
        // The AutoSizer contract: the ScrollNode fills its parent via
        // flex_grow rather than a caller-supplied fixed clip size.
        let scene = run_view_with_measured(384);
        let scroll = find_scroll(&scene).expect("view must contain a Scene::Scroll");
        assert!(
            (scroll.layout.flex_grow - 1.0).abs() < f32::EPSILON,
            "flex list ScrollNode must flex-grow to fill its parent",
        );
        assert!(scroll.state.is_some(), "scroll must carry the state Rc");
    }

    #[test]
    fn rendered_row_count_grows_with_measured_viewport() {
        // The whole point: a taller measured viewport materializes more
        // rows. 384 px / 32 px pitch = 12 rows; 768 px = 24 rows. Both
        // far below N = 10_000.
        let short = count_row_tags(&run_view_with_measured(384));
        let tall = count_row_tags(&run_view_with_measured(768));
        assert!(
            tall > short,
            "taller viewport must render more rows: {tall} (768px) vs {short} (384px)",
        );
        assert!(tall < 40, "still a small window, not the whole dataset: {tall}");
    }

    #[test]
    fn zero_measured_viewport_renders_no_rows() {
        // First-paint warmup: before the layout pass measures the flex
        // extent, the viewport is 0 and the window is empty. The shell's
        // scroll-dirty same-frame re-pass then re-runs with the true
        // height (set_measured_viewport flips the dirty bit).
        let scene = run_view_with_measured(0);
        assert_eq!(
            count_row_tags(&scene),
            0,
            "an unmeasured viewport windows to an empty list",
        );
    }

    #[test]
    fn a11y_setsize_is_full_dataset_items_track_window() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(360, 384);
            FlexVirtualListView::access_node(&(), None)
        });
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(N).unwrap()),
            "aria-setsize conveys the FULL dataset size",
        );
        assert!(nodes.len() - 1 < 40, "only the rendered window has listitem nodes");
        for item in &nodes[1..] {
            assert_eq!(item.role, AriaRole::ListItem);
            assert!(item.position_in_set.is_some(), "each row carries aria-posinset");
        }
    }

    #[test]
    fn a11y_item_count_tracks_measured_viewport() {
        let count_items = |h: u32| {
            Owner::new().run(|| {
                let scroll = use_scroll_state(SCROLL_KEY);
                scroll.set_measured_viewport(360, h);
                FlexVirtualListView::access_node(&(), None).len() - 1
            })
        };
        assert!(
            count_items(768) > count_items(384),
            "taller viewport exposes more listitem nodes",
        );
    }

    #[test]
    fn set_measured_viewport_dirty_bit_mirrors_set_max() {
        // The dirty bit is what folds into the shell's first-paint
        // warmup: first non-zero write flips it, an idempotent re-write
        // does not.
        let s = Rc::new(ScrollState::new());
        assert!(s.set_measured_viewport(360, 384), "first measure flips dirty");
        assert!(!s.set_measured_viewport(360, 384), "idempotent re-write is a no-op");
        assert!(s.set_measured_viewport(360, 768), "a resize flips dirty again");
        assert_eq!(s.measured_viewport(), (360, 768));
    }

    #[test]
    fn row_label_is_stable_width_and_indexed() {
        assert_eq!(row_label(0), "Item 00000 \u{00B7} Alpha");
        assert_eq!(row_label(42), "Item 00042 \u{00B7} Charlie");
    }
}
