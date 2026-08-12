//! `hello-breadcrumb` — R731 §5.38 §5.40 WAI-ARIA breadcrumb navigation.
//!
//! A breadcrumb is a `navigation` landmark holding a trail of `link`
//! crumbs, the **current location** marked with `aria-current="page"`
//! (WAI-ARIA APG breadcrumb pattern). Selecting a crumb navigates to it.
//!
//! ## substrate-0 interaction + new a11y primitives
//!
//! The "current crumb" is exactly a **1-of-N exclusive selection**, so
//! this binding reuses the R51.44 [`RadioGroupExternal`] coordinator
//! (selected index = current location) with **no new interaction
//! substrate** — the 3rd standalone consumer after `hello-radio-group`
//! and `hello-segmented-button`. What R731 *adds* is three foundational
//! a11y primitives the catalog lacked: `AriaRole::Navigation` (the
//! landmark), `AriaRole::Link` (each crumb), and the `AriaCurrent`
//! axis (`aria-current="page"` on the current crumb).
//!
//! ## Keyboard model (honest carry)
//!
//! The trail is a **single tab stop** with `ArrowLeft` / `ArrowRight`
//! roving + `Home` / `End` (the [`RadioGroupExternal`] roving model the
//! segmented button uses), reported to AT as the Navigation landmark
//! owning the tab stop with the active crumb as `aria-activedescendant`.
//! The strict WAI-ARIA APG breadcrumb puts *each* link in the normal Tab
//! order; that per-link-Tab variant is a deferred axis (it needs
//! per-crumb focusable externals). Both are valid keyboard patterns;
//! pointer / RPC / AT all reach every crumb regardless.
//!
//! ## AI clients (§2 invariant #2)
//!
//! `query("selected_index")` is the current location; `scene/click
//! {path:"breadcrumb#<i>"}`, `focus/set` + `scene/key`, and the AccessKit
//! tree (`navigation` > `link` with `aria-current`) all converge on the
//! same `RadioGroupExternal` statechart.

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, NavLink, WidgetA11y, navigation_link_nodes,
};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::radio::{RadioEvent, RadioState};
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::radio_composite as rc;
use pinion_widget_paint::state_layer::state_layer;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloBreadcrumbRenderer, HelloBreadcrumbRendererError);

const WIN_W: u32 = 600;
const WIN_H: u32 = 120;
const THEME_TAG: &str = "app";
const N: usize = 4;
const PRIMARY_TAG: &str = "breadcrumb";

/// The trail labels (single source — paint label + a11y link name).
const CRUMBS: [&str; N] = ["Home", "Reports", "Q3 2026", "Summary"];
/// Crumb separator (ASCII slash — the common breadcrumb delimiter).
const SEP: &str = "/";

const LABEL_FONT_PX: u32 = 16;
const CRUMB_GAP: u32 = 8;

/// Cached projection: one `(RadioState, selected)` per crumb + the §5.40
/// active-descendant index. `Copy` for the paint closure.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct TrailState {
    rows: [(RadioState, bool); N],
    focused: Option<usize>,
}

impl TrailState {
    fn idle() -> Self {
        Self {
            rows: [(RadioState::Idle, false); N],
            focused: None,
        }
    }
}

/// view-fn (§6.3): a Navigation landmark holding the crumb trail.
/// `Home / Reports / Q3 2026 / Summary` with `/` separators; the current
/// crumb (selected) paints in `OnSurface`, the link crumbs in `Accent`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: TrailState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mut row: Vec<Scene> = Vec::with_capacity(N * 2 - 1);
    for i in 0..N {
        if i > 0 {
            row.push(separator(&theme));
        }
        row.push(crumb(i, state.rows[i].0, state.rows[i].1, &theme));
    }
    // PRIMARY_TAG on the trail row so `{path:"breadcrumb"}` AI routing +
    // `rect_for_tag` AT bounds attach to the Navigation landmark.
    let trail = Scene::Container(
        // (R1030 §5.39) hand-composed focus stop — composing view owns the opt-in.
        ContainerNode::new(row).with_tag(PRIMARY_TAG).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_focusable(true)
                .with_gap(CRUMB_GAP),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![trail])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// One crumb, tagged `"breadcrumb#<i>"` for R51.42 sub-index routing. The
/// current crumb (selected) is `OnSurface`; link crumbs are `Accent`,
/// with the shared hover / pressed state-layer overlay.
fn crumb(index: usize, state: RadioState, selected: bool, theme: &Theme) -> Scene {
    let base = if selected {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::Accent)
    };
    let color = state_layer(base, state, theme);
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            CRUMBS[index],
            Rect::default(),
            TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(color),
        ))])
        .with_tag(format!("{PRIMARY_TAG}#{index}"))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center),
        ),
    )
}

fn separator(theme: &Theme) -> Scene {
    Scene::Text(TextNode::styled(
        SEP,
        Rect::default(),
        TextStyle::new()
            .with_size_px(LABEL_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ))
}

struct BreadcrumbView;

impl WidgetCore for BreadcrumbView {
    type State = TrailState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut ext = RadioGroupExternal::new(N);
        // Default current location = the last crumb ("Summary"): a
        // breadcrumb always has a current page. KeyboardActivate selects
        // without a hover / pressed residue (state stays Idle).
        ext.send(N - 1, RadioEvent::KeyboardActivate);
        Box::new(ext)
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> TrailState {
        let mut out = TrailState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        rc::read_rows(intro, &mut out.rows);
        out.focused = rc::focused_index(intro);
        out
    }

    fn view(state: TrailState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-breadcrumb (R731 §5.38 §5.40 navigation)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // Navigate to the resolved crumb through the shared roving-key shell
        // (1-of-N exclusion + `"selected"` intent); `resolve_target_index`
        // is this trail's opinionated ArrowLeft/Right + Home/End key map.
        rc::roving_key(scene, focused, Self::tag(), key, resolve_target_index)
    }

    fn fmt_state_log(state: &TrailState) -> String {
        let cur = state
            .rows
            .iter()
            .position(|(_, sel)| *sel)
            .map_or_else(|| "none".to_string(), |i| i.to_string());
        format!("current={cur}")
    }
}

impl WidgetA11y for BreadcrumbView {
    /// §5.40 — a `AriaRole::Navigation` landmark whose children are the
    /// crumb `AriaRole::Link` nodes; the current crumb (selected)
    /// carries `aria-current="page"`. Link names come from [`CRUMBS`].
    fn access_node(state: &TrailState, focused: Option<&str>) -> Vec<AccessNode> {
        let nav_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = rc::active_index(&state.rows, state.focused);
        let tags: Vec<String> = (0..N).map(|i| format!("{PRIMARY_TAG}#{i}")).collect();
        let links: Vec<NavLink<'_>> = state
            .rows
            .iter()
            .copied()
            .enumerate()
            .map(|(i, (crumb_state, selected))| NavLink {
                tag: &tags[i],
                label: CRUMBS[i],
                state: crumb_state,
                current: selected,
                focused: nav_focused && i == active_idx,
            })
            .collect();
        navigation_link_nodes(<Self as WidgetCore>::tag(), "Breadcrumb", &links)
    }

    /// §5.40 composite focus model — when the trail owns focus, the
    /// Navigation landmark is the `TreeUpdate::focus` target and the
    /// active crumb is the `aria-activedescendant` (the roving model; see
    /// the module docs for the per-link-Tab alternative).
    fn access_focus_target(state: &TrailState, focused: Option<&str>) -> Option<AccessFocus> {
        rc::composite_focus_target(
            <Self as WidgetCore>::tag(),
            focused,
            rc::active_index(&state.rows, state.focused),
        )
    }

    /// §5.40 composite child action dispatch — an AT `Click` / `Default`
    /// on a crumb navigates to it; `Focus` pins the active descendant.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        rc::composite_child_invoke(scene, sub_tag, action, N)
    }
}

impl WidgetView for BreadcrumbView {
    type Renderer = HelloBreadcrumbRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve a key to a target crumb index. `ArrowRight` / `ArrowLeft` step
/// (wrapping); `Home` / `End` jump to the first / last crumb.
fn resolve_target_index(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    match key {
        "Home" => Some(0),
        "End" => Some(N - 1),
        "ArrowRight" | "ArrowDown" => Some(rc::step(intro, 1, N)),
        "ArrowLeft" | "ArrowUp" => Some(rc::step(intro, -1, N)),
        _ => None,
    }
}

fn main() {
    pinion_shell::run::<BreadcrumbView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::{AriaCurrent, AriaRole};
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::ExternalNode;

    fn selected_state(idx: usize) -> TrailState {
        let mut s = TrailState::idle();
        s.rows[idx].1 = true;
        s
    }

    #[test]
    fn nav_landmark_with_n_link_children() {
        let nodes = BreadcrumbView::access_node(&selected_state(3), None);
        assert_eq!(nodes.len(), N + 1);
        assert_eq!(nodes[0].role, AriaRole::Navigation);
        assert_eq!(nodes[0].name.as_deref(), Some("Breadcrumb"));
        for i in 0..N {
            assert_eq!(nodes[i + 1].role, AriaRole::Link);
            assert_eq!(nodes[i + 1].name.as_deref(), Some(CRUMBS[i]));
        }
    }

    #[test]
    fn current_crumb_carries_aria_current_page() {
        let nodes = BreadcrumbView::access_node(&selected_state(2), None);
        assert_eq!(
            nodes[3].current,
            Some(AriaCurrent::Page),
            "crumb 2 is current"
        );
        assert!(
            nodes[1].current.is_none(),
            "non-current crumb has no aria-current"
        );
        assert!(nodes[2].current.is_none());
        assert!(nodes[4].current.is_none());
    }

    #[test]
    fn focused_nav_marks_active_crumb() {
        let nodes = BreadcrumbView::access_node(&selected_state(1), Some(PRIMARY_TAG));
        assert!(nodes[2].state.focused);
        assert!(!nodes[1].state.focused);
    }

    #[test]
    fn focus_target_is_composite_nav_plus_active_descendant() {
        let target = BreadcrumbView::access_focus_target(&selected_state(2), Some(PRIMARY_TAG))
            .expect("nav focused returns Some");
        assert_eq!(target.focus_tag, PRIMARY_TAG);
        assert_eq!(target.active_descendant.as_deref(), Some("breadcrumb#2"));
    }

    fn scene() -> Scene {
        let ext = RadioGroupExternal::new(N);
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag(PRIMARY_TAG))
    }

    fn current_index(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        node.handle
            .introspect()?
            .query("selected_index")
            .ok()
            .and_then(|v| match v {
                IntrospectValue::Int(i) => Some(i),
                _ => None,
            })
    }

    #[test]
    fn arrow_navigates_and_wraps_when_focused() {
        let mut s = scene();
        for expected in [0_i64, 1, 2, 3, 0] {
            assert!(BreadcrumbView::apply_key(
                &mut s,
                Some(PRIMARY_TAG),
                "ArrowRight",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(current_index(&s), Some(expected));
        }
    }

    #[test]
    fn home_end_jump_to_edges() {
        let mut s = scene();
        assert!(BreadcrumbView::apply_key(
            &mut s,
            Some(PRIMARY_TAG),
            "End",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(current_index(&s), Some(i64::try_from(N - 1).unwrap()));
        assert!(BreadcrumbView::apply_key(
            &mut s,
            Some(PRIMARY_TAG),
            "Home",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(current_index(&s), Some(0));
    }

    #[test]
    fn unfocused_swallows_arrow() {
        let mut s = scene();
        assert!(!BreadcrumbView::apply_key(
            &mut s,
            None,
            "ArrowRight",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(current_index(&s), None);
    }

    #[test]
    fn at_click_navigates_to_crumb() {
        let mut s = scene();
        assert!(BreadcrumbView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "1",
            AccessAction::Click
        ));
        assert_eq!(current_index(&s), Some(1));
    }

    #[test]
    fn view_carries_navigation_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<BreadcrumbView>(
            TrailState::idle(),
            &Frame::default(),
        );
    }
}
