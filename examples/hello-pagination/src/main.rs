//! `hello-pagination` — R754 §5.38 §5.40 a Material 3 **pagination** bar:
//! numbered page links with clamping previous / next controls.
//!
//! A pagination control is a single-select group of numbered page links
//! (exactly one page is *current*) plus previous / next stepping. The page
//! cells reuse the [`RadioGroupExternal`] machinery verbatim — through the
//! [`PaginationExternal`](pinion_core::widgets::pagination::PaginationExternal)
//! coordinator, which wraps the group and adds clamping prev / next — so
//! per-cell interaction state, 1-of-N exclusion, the §5.20 `"selected"`
//! intent and the roving keyboard model all come for free.
//!
//! **3rd consumer of the lifted Navigation/Link a11y substrate.** The
//! AccessKit tree — a [`AriaRole::Navigation`](pinion_a11y::AriaRole)
//! landmark holding [`AriaRole::Link`](pinion_a11y::AriaRole) children, the
//! current page carrying `aria-current="page"` — is built by the shared
//! [`navigation_link_nodes`] substrate, exactly as `hello-breadcrumb`
//! (R731) and `hello-nav-rail` (R751) do. Pagination's previous / next are
//! themselves non-current links within the same landmark (WAI-ARIA models
//! pagination prev / next as links), so they flow through the same uniform
//! list — `aria-disabled` at the ends.
//!
//! Interaction model: clicking a page number navigates to it; clicking
//! `‹` / `›` steps one page, **clamping** at the ends (no previous on page
//! 1, no next on the last page). The clamp is the contrast with the cyclic
//! arrow roving of a radio group; the keyboard here clamps to match —
//! `ArrowLeft` / `ArrowRight` (and `ArrowUp` / `ArrowDown`) step without
//! wrap, `Home` / `End` jump to the first / last page (the shared
//! [`rc::roving_key`] shell with a clamping key map).

use pinion_a11y::{
    navigation_link_nodes, AccessAction, AccessFocus, AccessNode, NavLink, WidgetA11y,
};
use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::pagination::PaginationExternal;
use pinion_core::widgets::radio::RadioState;
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::radio_composite as rc;
use pinion_widget_paint::state_layer::state_layer;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloPaginationRenderer, HelloPaginationRendererError);

const WIN_W: u32 = 460;
const WIN_H: u32 = 120;
/// [`ThemeProvider`] cache key — the `"app"` convention shared across the
/// example gallery.
const THEME_TAG: &str = "app";

/// Page count. Five reads as a realistic pager and gives the boot frame
/// (current = page 3) both an enabled previous and an enabled next.
const N: usize = 5;

/// The coordinator / Navigation-landmark tag. Page cells are
/// `pagination#{i}` (R51.42 sub-index routing); prev / next are
/// `pagination#prev` / `pagination#next`.
const PRIMARY_TAG: &str = "pagination";

/// Boot current page (0-based): page 3 of 5 — a middle page so both prev
/// and next are enabled in the live boot frame.
const BOOT_CURRENT: usize = 2;

const CELL: u32 = 40;
const GAP: u32 = 8;
/// Fully-rounded page cells (M3 pagination indicators are circular).
const CELL_RADIUS: u32 = CELL / 2;
const LABEL_FONT_PX: u32 = 16;
const ARROW_FONT_PX: u32 = 22;
/// U+2039 / U+203A SINGLE LEFT / RIGHT-POINTING ANGLE QUOTATION MARK — the
/// previous / next chevrons. Named const + escape per the non-ASCII-source
/// rule (raw glyph only in doc strings).
const PREV_GLYPH: &str = "\u{2039}";
const NEXT_GLYPH: &str = "\u{203A}";

/// Cached projection: per-page `(RadioState, current)` plus the roving
/// focus index and the clamped prev / next availability.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct PageState {
    pages: [(RadioState, bool); N],
    focused: Option<usize>,
    can_prev: bool,
    can_next: bool,
}

impl PageState {
    fn idle() -> Self {
        Self {
            pages: [(RadioState::Idle, false); N],
            focused: None,
            can_prev: false,
            can_next: false,
        }
    }
}

/// view-fn (§6.3): a Navigation landmark holding `‹  1 2 3 4 5  ›`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: PageState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mut row: Vec<Scene> = Vec::with_capacity(N + 2);
    row.push(arrow("prev", PREV_GLYPH, state.can_prev, &theme));
    for i in 0..N {
        row.push(page_cell(i, state.pages[i].0, state.pages[i].1, &theme));
    }
    row.push(arrow("next", NEXT_GLYPH, state.can_next, &theme));
    // PRIMARY_TAG on the pager row so `{path:"pagination"}` AI routing +
    // `rect_for_tag` AT bounds attach to the Navigation landmark.
    let bar = Scene::Container(
        ContainerNode::new(row)
            .with_tag(PRIMARY_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(GAP),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![bar])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// One page cell, tagged `"pagination#<i>"` for R51.42 sub-index routing.
/// The current page is a filled `Accent` circle (`OnAccent` numeral); other
/// pages are transparent (`OnSurface` numeral), with the shared
/// hover / pressed [`state_layer`] overlay.
fn page_cell(index: usize, st: RadioState, current: bool, theme: &Theme) -> Scene {
    let base = if current {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::rgba(0, 0, 0, 0)
    };
    let fill = state_layer(base, st, theme);
    let ink = if current {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            PAGE_LABELS[index],
            Rect::default(),
            TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(ink),
        ))])
        .with_tag(format!("{PRIMARY_TAG}#{index}"))
        .with_style(BoxStyle::filled(fill).with_corner_radius(CELL_RADIUS))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(CELL, CELL)),
        ),
    )
}

/// A previous / next chevron, tagged `"pagination#prev"` / `"pagination#next"`.
/// `enabled` chevrons paint in `OnSurface`; clamped (disabled) chevrons in
/// the muted on-surface tone (the `aria-disabled` end state).
fn arrow(which: &str, glyph: &str, enabled: bool, theme: &Theme) -> Scene {
    let ink = if enabled {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            glyph,
            Rect::default(),
            TextStyle::new().with_size_px(ARROW_FONT_PX).with_fg(ink),
        ))])
        .with_tag(format!("{PRIMARY_TAG}#{which}"))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(CELL, CELL)),
        ),
    )
}

/// Visible page numerals — single source of truth for the painted
/// `TextNode`; the accessible names are `"Page <n>"` (built in
/// `access_node`).
const PAGE_LABELS: [&str; N] = ["1", "2", "3", "4", "5"];

struct PaginationView;

impl WidgetCore for PaginationView {
    type State = PageState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(PaginationExternal::new(N, BOOT_CURRENT))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> PageState {
        let mut out = PageState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        rc::read_rows(intro, &mut out.pages);
        out.focused = rc::focused_index(intro);
        out.can_prev = matches!(intro.query("can_prev"), Some(IntrospectValue::Bool(true)));
        out.can_next = matches!(intro.query("can_next"), Some(IntrospectValue::Bool(true)));
        out
    }

    fn view(state: PageState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-pagination (R754 §5.38 §5.40 pagination)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Navigate to the resolved page through the shared roving-key shell
    /// (1-of-N exclusion + `"selected"` intent); the key map *clamps*
    /// (no wrap) to match the prev / next buttons.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        rc::roving_key(scene, focused, Self::tag(), key, resolve_target_index)
    }

    fn fmt_state_log(state: &PageState) -> String {
        let cur = state
            .pages
            .iter()
            .position(|(_, sel)| *sel)
            .map_or_else(|| "none".to_string(), |i| i.to_string());
        format!("current={cur} prev={} next={}", state.can_prev, state.can_next)
    }
}

impl WidgetA11y for PaginationView {
    /// §5.40 — a Navigation landmark whose children are `‹ Previous`, the
    /// numbered page links (current = `aria-current="page"`), and `Next ›`,
    /// built by the shared [`navigation_link_nodes`] substrate. Previous /
    /// next carry `aria-disabled` (a `RadioState::Disabled` posture) at the
    /// clamped ends.
    fn access_node(state: &PageState, focused: Option<&str>) -> Vec<AccessNode> {
        let nav_focused = focused == Some(<Self as WidgetCore>::tag());
        let active = rc::active_index(&state.pages, state.focused);
        let prev_tag = format!("{PRIMARY_TAG}#prev");
        let next_tag = format!("{PRIMARY_TAG}#next");
        let page_tags: Vec<String> = (0..N).map(|i| format!("{PRIMARY_TAG}#{i}")).collect();
        let page_labels: Vec<String> = (0..N).map(|i| format!("Page {}", i + 1)).collect();
        let end_state = |enabled: bool| {
            if enabled {
                RadioState::Idle
            } else {
                RadioState::Disabled
            }
        };
        let mut links: Vec<NavLink<'_>> = Vec::with_capacity(N + 2);
        links.push(NavLink {
            tag: &prev_tag,
            label: "Previous page",
            state: end_state(state.can_prev),
            current: false,
            focused: false,
        });
        for i in 0..N {
            links.push(NavLink {
                tag: &page_tags[i],
                label: &page_labels[i],
                state: state.pages[i].0,
                current: state.pages[i].1,
                focused: nav_focused && i == active,
            });
        }
        links.push(NavLink {
            tag: &next_tag,
            label: "Next page",
            state: end_state(state.can_next),
            current: false,
            focused: false,
        });
        navigation_link_nodes(<Self as WidgetCore>::tag(), "Pagination", &links)
    }

    /// §5.40 composite focus model — the Navigation landmark is the focus
    /// target and the active page is the `aria-activedescendant`.
    fn access_focus_target(state: &PageState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(<Self as WidgetCore>::tag()) {
            let idx = rc::active_index(&state.pages, state.focused);
            Some(rc::composite_focus(<Self as WidgetCore>::tag(), idx))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// §5.40 composite child action dispatch — an AT `Click` / `Default` on
    /// a page navigates to it (`Focus` pins the active descendant); on
    /// `prev` / `next` it steps one page (clamped).
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match sub_tag {
            "prev" | "next" => {
                if matches!(action, AccessAction::Click | AccessAction::Default) {
                    let _ =
                        intro.invoke("send", IntrospectValue::Text(format!("{sub_tag}:PointerUp")));
                    true
                } else {
                    false
                }
            }
            _ => rc::child_invoke(intro, sub_tag, action, N),
        }
    }
}

impl WidgetView for PaginationView {
    type Renderer = HelloPaginationRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

/// Resolve a key to a target page index, **clamping** at the ends (no
/// wrap — matching the prev / next buttons). `Home` / `End` jump to the
/// first / last page.
fn resolve_target_index(
    intro: Option<&dyn ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    let cur = intro.and_then(rc::selected_index).unwrap_or(0);
    match key {
        "Home" => Some(0),
        "End" => Some(N - 1),
        "ArrowRight" | "ArrowDown" => Some((cur + 1).min(N - 1)),
        "ArrowLeft" | "ArrowUp" => Some(cur.saturating_sub(1)),
        _ => None,
    }
}

fn main() {
    pinion_shell::run::<PaginationView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::{AriaCurrent, AriaRole};

    fn state_with(current: usize) -> PageState {
        let mut s = PageState::idle();
        s.pages[current].1 = true;
        s.can_prev = current > 0;
        s.can_next = current + 1 < N;
        s
    }

    #[test]
    fn view_carries_primary_tag_every_page_and_both_arrows() {
        let scene = pinion_core::Owner::new().run(|| view(state_with(BOOT_CURRENT), &Frame::new()));
        assert!(scene.contains_tag(PRIMARY_TAG), "pager row carries the landmark tag");
        for i in 0..N {
            assert!(scene.contains_tag(&format!("{PRIMARY_TAG}#{i}")), "page {i} present");
        }
        assert!(scene.contains_tag(&format!("{PRIMARY_TAG}#prev")), "prev arrow present");
        assert!(scene.contains_tag(&format!("{PRIMARY_TAG}#next")), "next arrow present");
    }

    /// Recursively find the container tagged `tag`.
    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(c);
                }
                c.children.iter().find_map(|ch| find(ch, tag))
            }
            _ => None,
        }
    }

    #[test]
    fn current_page_is_a_filled_accent_circle_others_transparent() {
        let scene = pinion_core::Owner::new().run(|| view(state_with(2), &Frame::new()));
        let cur = find(&scene, &format!("{PRIMARY_TAG}#2")).expect("page 2");
        let other = find(&scene, &format!("{PRIMARY_TAG}#0")).expect("page 0");
        assert_eq!(cur.style.fill.a, 255, "current page is opaque (Accent)");
        assert_eq!(other.style.fill.a, 0, "non-current page is transparent");
    }

    #[test]
    fn access_node_landmark_links_and_aria_current() {
        let nodes = PaginationView::access_node(&state_with(2), None);
        // landmark + prev + N pages + next.
        assert_eq!(nodes.len(), N + 3, "landmark + prev + {N} pages + next");
        assert_eq!(nodes[0].role, AriaRole::Navigation);
        assert_eq!(nodes[0].name.as_deref(), Some("Pagination"));
        // nodes[1] = prev, nodes[2..2+N] = pages, last = next.
        assert_eq!(nodes[1].name.as_deref(), Some("Previous page"));
        assert_eq!(nodes[1].role, AriaRole::Link);
        let page2 = &nodes[2 + 2]; // page index 2
        assert_eq!(page2.current, Some(AriaCurrent::Page), "current page = aria-current");
        assert_eq!(page2.name.as_deref(), Some("Page 3"));
        let last = nodes.last().unwrap();
        assert_eq!(last.name.as_deref(), Some("Next page"));
    }

    #[test]
    fn prev_disabled_on_first_page_next_disabled_on_last() {
        let first = PaginationView::access_node(&state_with(0), None);
        assert!(first[1].state.disabled, "prev disabled on page 0");
        assert!(!first.last().unwrap().state.disabled, "next enabled on page 0");
        let last = PaginationView::access_node(&state_with(N - 1), None);
        assert!(!last[1].state.disabled, "prev enabled on last page");
        assert!(last.last().unwrap().state.disabled, "next disabled on last page");
    }

    #[test]
    fn keymap_clamps_without_wrapping() {
        // No live external here — resolve from the key alone (cur defaults
        // to 0 when intro is None), exercising the clamp arithmetic.
        assert_eq!(resolve_target_index(None, "ArrowLeft"), Some(0), "clamp at first");
        assert_eq!(resolve_target_index(None, "ArrowRight"), Some(1));
        assert_eq!(resolve_target_index(None, "Home"), Some(0));
        assert_eq!(resolve_target_index(None, "End"), Some(N - 1));
        assert_eq!(resolve_target_index(None, "Tab"), None);
    }
}
