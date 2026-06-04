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

use pinion_a11y::{windowed_list_nodes_selected, AccessNode, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::{compute_visible_range, scroll_offset_to_reveal};
use pinion_core::widgets::virtual_select::VirtualSelectExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
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

/// R776 — pure keyboard navigation arithmetic: map a key to the next
/// selected index given the current selection and a page size (rows per
/// measured viewport-ful). Single-select, **clamp** (no wrap) — a finite
/// data list has ends; the cyclic convention is `hello-listbox`'s. With no
/// current selection, every navigation key lands on the first row (the
/// W3C "first key focuses the first option" convention). Returns `None`
/// for an unhandled key so `apply_key` falls through to the shell's
/// unrecognised-keybinding swallow contract.
fn next_index(current: Option<usize>, key: &str, page: usize) -> Option<usize> {
    let last = N - 1;
    let next = match key {
        "ArrowDown" => current.map_or(0, |i| (i + 1).min(last)),
        "ArrowUp" => current.map_or(0, |i| i.saturating_sub(1)),
        "Home" => 0,
        "End" => last,
        "PageDown" => current.map_or(0, |i| i.saturating_add(page).min(last)),
        "PageUp" => current.map_or(0, |i| i.saturating_sub(page)),
        _ => return None,
    };
    Some(next)
}

/// One virtualized row. The selected row paints in the Accent tone (M3
/// selected state layer); unselected rows zebra-stripe. Tagged
/// `"vlist#<i>"` so a click routes to the [`VirtualSelectExternal`] anchor
/// (R51.42 composite protocol) and `rect_for_tag` bounds the `listitem`.
fn build_row(index: usize, theme: &Theme, width: u32, selected: Option<usize>) -> Scene {
    let is_selected = selected == Some(index);
    let (fill, fg) = if is_selected {
        (theme.resolve(ColorRole::Accent), theme.resolve(ColorRole::OnAccent))
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
    format!("Item {index:05} \u{00B7} {}", CATEGORIES[index % CATEGORIES.len()])
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
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
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
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_flex_grow(1.0)),
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
        vec![scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG)]
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

    /// The list is a single keyboard tab stop (roving by data index, the
    /// WAI-ARIA listbox convention). Tab lands on the list; Arrow / Home /
    /// End / `PageUp` / `PageDown` then move the selection via [`apply_key`].
    fn focusable_tags() -> Vec<&'static str> {
        vec![LIST_TAG]
    }

    /// R776 §5.27 — keyboard navigation over the windowed list. Keys only
    /// route when the list itself is focused (single tab stop, no
    /// sibling-widget aliasing). Each handled key moves the index-model
    /// selection (clamped) and **scrolls the new selection into view** via
    /// [`scroll_offset_to_reveal`] — so navigating to a row that was never
    /// materialized scrolls there. `PageUp`/`PageDown` step by one measured
    /// viewport-ful of rows.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(LIST_TAG) {
            return false;
        }
        // The scroll state resolves from the owner cache (CoreShell wraps
        // apply_key in `root_owner().run`), independent of the `scene`
        // borrow below. Page size = rows per measured viewport-ful (>= 1).
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let page = usize::try_from(measured_h / ROW_PITCH).unwrap_or(1).max(1);

        let Some(node) = scene.find_external_with_tag_mut(LIST_TAG) else {
            return false;
        };
        let current = node
            .handle
            .introspect()
            .and_then(|intro| match intro.query("selected") {
                Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
                _ => None,
            });
        let Some(target) = next_index(current, key, page) else {
            return false;
        };
        // Move the selection through the AI-first `invoke("select", …)`
        // path — the same wire a `scene/invoke` drives, so keyboard and RPC
        // selection are one funnel.
        if let (Some(intro), Ok(t)) = (node.handle.introspect_mut(), i64::try_from(target)) {
            let _ = intro.invoke("select", IntrospectValue::Int(t));
        }
        // Scroll the (possibly never-materialized) target row into view.
        let reveal = scroll_offset_to_reveal(target, scroll.offset_y(), measured_h, ROW_PITCH);
        scroll.scroll_to(0, reveal);
        true
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
    use pinion_core::widgets::scroll::ScrollState;
    use pinion_core::Owner;
    use std::rc::Rc;

    // ── pure keyboard navigation arithmetic ─────────────────────────

    #[test]
    fn nav_clamps_at_both_ends_no_wrap() {
        // ArrowUp from the top stays at 0 (no wrap to the last row).
        assert_eq!(next_index(Some(0), "ArrowUp", 12), Some(0));
        // ArrowDown from the last row stays at the last (no wrap to 0).
        assert_eq!(next_index(Some(N - 1), "ArrowDown", 12), Some(N - 1));
    }

    #[test]
    fn nav_arrows_step_one_row() {
        assert_eq!(next_index(Some(5), "ArrowDown", 12), Some(6));
        assert_eq!(next_index(Some(5), "ArrowUp", 12), Some(4));
    }

    #[test]
    fn nav_home_end_jump_to_extremes() {
        assert_eq!(next_index(Some(500), "Home", 12), Some(0));
        assert_eq!(next_index(Some(500), "End", 12), Some(N - 1));
    }

    #[test]
    fn nav_page_steps_by_page_and_clamps() {
        assert_eq!(next_index(Some(100), "PageDown", 12), Some(112));
        assert_eq!(next_index(Some(100), "PageUp", 12), Some(88));
        // PageUp near the top clamps to 0; PageDown near the end clamps.
        assert_eq!(next_index(Some(5), "PageUp", 12), Some(0));
        assert_eq!(next_index(Some(N - 3), "PageDown", 12), Some(N - 1));
    }

    #[test]
    fn nav_from_none_lands_on_first_row() {
        for key in ["ArrowDown", "ArrowUp", "PageDown", "PageUp"] {
            assert_eq!(next_index(None, key, 12), Some(0), "{key} from None -> first row");
        }
    }

    #[test]
    fn nav_unhandled_key_returns_none() {
        assert_eq!(next_index(Some(3), "Tab", 12), None);
        assert_eq!(next_index(Some(3), "a", 12), None);
    }

    // ── view + a11y ─────────────────────────────────────────────────

    fn run_view_with_measured(selected: Option<usize>, offset_y: i32, measured_h: u32) -> Scene {
        let owner = Owner::new();
        owner.run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_PITCH).unwrap());
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
        assert_eq!(item1.selected, Some(true), "selected row carries aria-selected=true");
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
        s.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_PITCH).unwrap());
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
