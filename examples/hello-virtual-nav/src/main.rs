//! `hello-virtual-nav` — R776 §5.27 **keyboard navigation at scale**.
//!
//! R746 (`hello-virtual-select`) lands an index-model selection over a
//! *fixed-viewport* virtualized list, driven by pointer + RPC. It
//! explicitly defers the keyboard axis: "focus-by-index + scroll-into-view
//! is a later additive axis". R774 (`hello-flex-virtual-list`) lands the
//! `AutoSizer` flex viewport but is display-only. This binding composes
//! the two and closes the deferred axis: a **selectable, keyboard-
//! navigable, flex-viewport** virtualized list.
//!
//! The decisive new capability is **scroll-into-view by index**. When the
//! user presses `End` the selection jumps to row 9 999 — a row that was
//! never materialized — and the list must *scroll there*. A virtualized
//! list cannot `scrollIntoView` a DOM node (there is no node); the offset
//! is computed from the row's known slot by
//! [`scroll_offset_to_reveal`](pinion_core::widgets::virtual_list::scroll_offset_to_reveal)
//! (the canonical `react-window` `scrollToItem` / Qt `scrollTo` / Flutter
//! `ensureVisible` arithmetic, landed this round). Everything else is
//! composition: the selection model is the unchanged R746
//! [`VirtualSelectExternal`], the windowing is the unchanged R774
//! [`view_flex_virtual_list`], and the scroll offset is the unchanged
//! [`ScrollState`].
//!
//! ## Keyboard model (single-select, selection-follows-focus)
//!
//! The list is a **single tab stop** (roving by data index, the WAI-ARIA
//! listbox convention). With no separate focus cursor, the selection *is*
//! the cursor — the macOS/Windows file-list model where Arrow moves the
//! selection directly:
//!
//! * `ArrowDown` / `ArrowUp` — move the selection one row (clamped, no
//!   wrap — a 10 000-row data list has ends, unlike the cyclic 4-item
//!   fruit listbox).
//! * `Home` / `End` — first / last row.
//! * `PageDown` / `PageUp` — move by one measured viewport-ful of rows.
//!
//! Every move scrolls the new selection into view. The clamp-vs-wrap
//! choice is the binding's (`hello-listbox` wraps; pagination clamps) — an
//! opinionated nav convention, not a shared SSOT.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/key` `End` → `query("selected")` reports `9999`, and the
//! `scene/snapshot` window has scrolled so `vlist#9999` is now a rendered
//! node, even though it did not exist before the key. Pure data, no pixels
//! (see `tools/r776_virtual_nav.py`).
//!
//! ## a11y
//!
//! Single-select WAI-ARIA virtualized `list` via the R776-lifted
//! [`windowed_list_nodes_selected`] (the decorated peer of the display-only
//! [`windowed_list_nodes`](pinion_a11y::windowed_list_nodes), shared with
//! `hello-virtual-select`): `aria-setsize = N`, each rendered row an
//! `AriaRole::ListItem` with `aria-posinset` + `aria-selected = (index ==
//! selected)`. The list container is the focusable tab stop.

use pinion_a11y::{AccessNode, WidgetA11y, windowed_list_nodes_selected};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{
    RowMetrics, VirtualSelectExternal, nav_select_key, read_selected,
};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::virtual_list::view_flex_virtual_list;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloVirtualNavRenderer, HelloVirtualNavRendererError);

/// Initial window size — freely resizable; the list re-windows on every
/// `Resized` event (R774 `AutoSizer`).
const WIN_W: u32 = 380;
const WIN_H: u32 = 480;
const THEME_TAG: &str = "app";
/// Total dataset size — large while the rendered node count stays small.
const N: usize = 10_000;
/// Uniform per-row vertical slot in logical pixels.
const ROW_PITCH: u32 = 32;
/// Extra rows built above + below the strict visible window.
const OVERSCAN: usize = 3;
/// Fixed header-bar height; the flex list fills the window below it.
const HEADER_H: u32 = 44;
/// Gutter width reserved for the scrollbar peer.
const GUTTER_W: u32 = 12;
/// Paint-root + a11y `list` container tag, and the state-scene tag of the
/// [`VirtualSelectExternal`] anchor (clicks on `vlist#<i>` rows route here
/// via the R51.42 composite protocol).
const LIST_TAG: &str = "vlist";
const SCROLL_KEY: &str = "vlist_scroll";
const SCROLLBAR_TAG: &str = "vlist_scrollbar";
const STATUS_TAG: &str = "vlist_status";

// The widget's only projected state is the selected data index
// (`Option<usize>`, `Copy`). The scroll offset, the measured viewport, and
// the scrollbar peer's interaction phase each drive their own repaints
// through the reactive `Signal` subscriptions the view opens — re-
// projecting them would duplicate state those externals already own.

/// One virtualized row. The selected row paints in the Accent tone (M3
/// selected state layer); unselected rows zebra-stripe. Tagged
/// `"vlist#<i>"` so a click routes to the [`VirtualSelectExternal`] anchor
/// (R51.42 composite protocol) and `rect_for_tag` bounds the `listitem`.
fn build_row(index: usize, theme: &Theme, width: u32, selected: Option<usize>) -> Scene {
    let is_selected = selected == Some(index);
    let (fill, fg) = if is_selected {
        (
            theme.resolve(ColorRole::Accent),
            theme.resolve(ColorRole::OnAccent),
        )
    } else {
        let stripe = if index % 2 == 0 {
            ColorRole::SurfaceContainerLow
        } else {
            ColorRole::SurfaceContainer
        };
        (theme.resolve(stripe), theme.resolve(ColorRole::OnSurface))
    };
    let label = Scene::Text(TextNode::styled(
        row_label(index),
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(fg),
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

/// Synthetic row content; zero-padded for stable width + unambiguous
/// `scene/snapshot` readout.
fn row_label(index: usize) -> String {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    format!(
        "Item {index:05} \u{00B7} {}",
        CATEGORIES[index % CATEGORIES.len()]
    )
}

/// Header-bar status line: the current selection + the measured viewport
/// extent. A literal scene-as-data readout — press `End` and the text
/// reports `selected 9999`, proving the selection survives even though the
/// row was never materialized at boot.
fn header(
    scroll: &std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
    theme: &Theme,
    selected: Option<usize>,
) -> Scene {
    let (mw, mh) = scroll.measured_viewport();
    let sel = selected.map_or_else(|| "none".to_string(), |i| i.to_string());
    let text = Scene::Text(
        TextNode::styled(
            format!("selected {sel} \u{00B7} viewport {mw}\u{00D7}{mh}"),
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
                    .with_size(Size::auto().with_height(SizeValue::Px(HEADER_H)))
                    .with_flex_grow(0.0)
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping `selected index -> Scene`. The dataset
/// is virtual — `view_flex_virtual_list` invokes [`build_row`] only for the
/// indices in the current window, whose *size* is the runtime-measured
/// viewport height (R774 `AutoSizer`).
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(selected: Option<usize>, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let (measured_w, measured_h) = scroll.measured_viewport();

    let list = view_flex_virtual_list(&scroll, N, ROW_PITCH, OVERSCAN, |index| {
        build_row(index, &theme, measured_w, selected)
    });

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

    let band = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_flex_grow(1.0)
                    .with_focusable(true),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![header(&scroll, &theme, selected), band])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct VirtualNavView;

impl WidgetCore for VirtualNavView {
    /// The selected data index — the widget's entire projected state.
    type State = Option<usize>;
    type Event = ();

    /// The primary External is the R746 index-held selection coordinator,
    /// addressable at [`LIST_TAG`]. Clicks on the windowed `vlist#<i>` rows
    /// route here via the R51.42 composite protocol; `apply_key` drives it
    /// from the keyboard.
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    /// Sibling `ScrollBarExternal` sharing the list's `Rc<ScrollState>`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![scrollbar_extra_external(
            use_scroll_state(SCROLL_KEY),
            SCROLLBAR_TAG,
        )]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// Project the selected data index off the primary coordinator. A
    /// selection change repaints; scroll offset + scrollbar phase repaint
    /// via their own reactive `Signal` subscriptions the view opens.
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

    /// R776/R777 §5.27 — keyboard navigation over the windowed list,
    /// delegated to the shared `nav_select_key` controller (the same one
    /// `hello-grid-nav` uses): keys only route when the list is focused
    /// (single tab stop); each handled key moves the index-model selection
    /// (linear clamp, no wrap) and scrolls the new selection into view, so
    /// navigating to a never-materialized row scrolls there.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        // The scroll state resolves from the owner cache (CoreShell wraps
        // apply_key in `root_owner().run`), independent of the `scene`
        // borrow inside the controller.
        nav_select_key(
            scene,
            &use_scroll_state(SCROLL_KEY),
            LIST_TAG,
            focused,
            key,
            modifiers,
            RowMetrics {
                item_count: N,
                row_pitch: ROW_PITCH,
            },
        )
    }

    fn title() -> &'static str {
        "pinion hello-virtual-nav (R776 §5.27 keyboard navigation at scale)"
    }

    fn fmt_state_log(state: &Option<usize>) -> String {
        match state {
            Some(i) => format!("selected=row {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for VirtualNavView {
    /// Single-select WAI-ARIA virtualized `list` via the R776-lifted
    /// [`windowed_list_nodes_selected`] (shared with `hello-virtual-select`
    /// so the selectable-windowed topology is one source of truth):
    /// `aria-setsize = N`, each rendered row an `AriaRole::ListItem` with
    /// `aria-posinset` + `aria-selected = (index == selected)`. The window
    /// is computed from the same measured viewport the view fn windows
    /// against, so the a11y tree and the painted tree never diverge.
    fn access_node(selected: &Option<usize>, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_PITCH, OVERSCAN);
        windowed_list_nodes_selected(
            LIST_TAG,
            "Navigable item list",
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
            *selected,
        )
    }
}

impl WidgetView for VirtualNavView {
    type Renderer = HelloVirtualNavRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<VirtualNavView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;
    use pinion_core::widgets::scroll::ScrollState;
    use pinion_core::widgets::virtual_list::scroll_offset_to_reveal;
    use std::rc::Rc;

    // The keyboard navigation policy + controller (`clamp_nav` /
    // `nav_select_key`) are unit-tested in `pinion_core::widgets::virtual_select`;
    // this binding's apply_key is a thin delegation. The end-to-end
    // keyboard drive is covered by `tools/r776_virtual_nav.py`.

    fn run_view_with_measured(selected: Option<usize>, offset_y: i32, measured_h: u32) -> Scene {
        let owner = Owner::new();
        owner.run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(
                0,
                i32::try_from(N).unwrap() * i32::try_from(ROW_PITCH).unwrap(),
            );
            scroll.set_measured_viewport(360, measured_h);
            scroll.scroll_to(0, offset_y);
            view(selected, &Frame::default())
        })
    }

    /// Find the `vlist#<i>` row container and return its fill color.
    fn row_fill(scene: &Scene, index: usize) -> Option<pinion_core::style::Color> {
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
        walk(scene, &format!("{LIST_TAG}#{index}"))
    }

    #[test]
    fn selected_row_paints_accent() {
        let theme = pinion_core::theme::Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        // Row 2 selected, viewport tall enough to render it from the top.
        let scene = run_view_with_measured(Some(2), 0, 384);
        assert_eq!(row_fill(&scene, 2), Some(accent), "selected row is Accent");
        assert_ne!(row_fill(&scene, 3), Some(accent), "neighbor is not Accent");
    }

    #[test]
    fn a11y_marks_selected_and_tracks_window() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(360, 384);
            VirtualNavView::access_node(&Some(1), None)
        });
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(nodes[0].size_of_set, Some(u32::try_from(N).unwrap()));
        let item1 = nodes[1..]
            .iter()
            .find(|n| n.position_in_set == Some(2))
            .expect("rendered listitem for index 1");
        assert_eq!(
            item1.selected,
            Some(true),
            "selected row carries aria-selected=true"
        );
    }

    #[test]
    fn scroll_node_flex_grows() {
        fn find_scroll(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
            match scene {
                Scene::Scroll(s) => Some(s),
                Scene::Container(c) => c.children.iter().find_map(find_scroll),
                _ => None,
            }
        }
        let scene = run_view_with_measured(None, 0, 384);
        let scroll = find_scroll(&scene).expect("view must contain a Scene::Scroll");
        assert!(
            (scroll.layout.flex_grow - 1.0).abs() < f32::EPSILON,
            "flex list ScrollNode must flex-grow to fill its parent",
        );
    }

    #[test]
    fn reveal_scrolls_a_deep_target_into_view() {
        // The headline composition: selecting row 9 999 and revealing it
        // moves the scroll offset deep, so the windowed range now includes
        // 9 999 — a row that did not exist at offset 0.
        let s = Rc::new(ScrollState::new());
        s.set_max(
            0,
            i32::try_from(N).unwrap() * i32::try_from(ROW_PITCH).unwrap(),
        );
        let measured_h = 384;
        let reveal = scroll_offset_to_reveal(N - 1, 0, measured_h, ROW_PITCH);
        s.scroll_to(0, reveal);
        let window = compute_visible_range(s.offset_y(), measured_h, N, ROW_PITCH, OVERSCAN);
        assert!(
            window.indices().any(|i| i == N - 1),
            "after reveal, the last row is inside the window {window:?}",
        );
    }
}
