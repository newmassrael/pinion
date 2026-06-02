//! `hello-nav-rail` — R751 §5.38 §5.40 WAI-ARIA navigation rail.
//!
//! A navigation rail is a persistent, vertical list of top-level app
//! **destinations**; selecting one navigates to it, and the active
//! destination is marked `aria-current="page"`. It is a `navigation`
//! landmark holding `link` destinations — the **2nd consumer** of the
//! `Navigation` / `Link` a11y primitives the breadcrumb (R731) introduced
//! (clearing the R731 "Link/Navigation 2nd consumer" carry, validating
//! that pair per the second-consumer rule). The active destination wears
//! the Material 3 **active-indicator pill** (a tonal rounded surface
//! behind the label) — the rail's distinguishing paint vs the breadcrumb's
//! flat colour-only current.
//!
//! ## substrate-0 interaction
//!
//! The "active destination" is exactly a **1-of-N exclusive selection**,
//! so this binding reuses the R51.44 [`RadioGroupExternal`] coordinator
//! (selected index = active destination) with **no new interaction
//! substrate** — the same composition the breadcrumb / segmented button /
//! stepper use. The hover / pressed pill tint reuses the R750
//! [`state_layer`] M3 overlay SSOT.
//!
//! ## Keyboard model (honest carry — shared with breadcrumb)
//!
//! The rail is a **single tab stop** with `ArrowUp` / `ArrowDown` roving +
//! `Home` / `End` (the [`RadioGroupExternal`] roving model), reported to AT
//! as the Navigation landmark owning the tab stop with the active
//! destination as `aria-activedescendant`. The strict per-link-Tab variant
//! (each destination in the normal Tab order) is a deferred axis; pointer /
//! RPC / AT all reach every destination regardless.
//!
//! ## AI clients (§2 invariant #2)
//!
//! `query("selected_index")` is the active destination; `scene/click
//! {path:"nav_rail#<i>"}`, `focus/set` + `scene/key`, and the AccessKit
//! tree (`navigation` > `link` with `aria-current="page"`) all converge on
//! the same `RadioGroupExternal` statechart.

use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::radio::{RadioEvent, RadioState};
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, AccessState, AriaCurrent, AriaRole, WidgetA11y,
};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::radio_composite as rc;
use pinion_widget_paint::state_layer::state_layer;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloNavRailRenderer, HelloNavRailRendererError);

const WIN_W: u32 = 220;
const WIN_H: u32 = 280;
const THEME_TAG: &str = "app";
const N: usize = 4;
const PRIMARY_TAG: &str = "nav_rail";

/// The destination labels (single source — paint label + a11y link name).
const DESTINATIONS: [&str; N] = ["Home", "Search", "Library", "Settings"];

const LABEL_FONT_PX: u32 = 16;
/// Active-indicator pill geometry.
const ITEM_H: u32 = 44;
const PILL_RADIUS: u32 = ITEM_H / 2;
const RAIL_GAP: u32 = 6;
const RAIL_PAD: u32 = 12;
const LABEL_PAD_X: u32 = 16;

/// Cached projection: one `(RadioState, selected)` per destination + the
/// §5.40 active-descendant index. `Copy` for the paint closure.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct RailState {
    rows: [(RadioState, bool); N],
    focused: Option<usize>,
}

impl RailState {
    fn idle() -> Self {
        Self {
            rows: [(RadioState::Idle, false); N],
            focused: None,
        }
    }
}

/// view-fn (§6.3): a Navigation landmark holding the vertical destination
/// rail. The active destination wears the M3 active-indicator pill.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RailState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let items: Vec<Scene> = (0..N)
        .map(|i| destination(i, state.rows[i].0, state.rows[i].1, &theme))
        .collect();
    // PRIMARY_TAG on the rail column so `{path:"nav_rail"}` AI routing +
    // `rect_for_tag` AT bounds attach to the Navigation landmark.
    let rail = Scene::Container(
        ContainerNode::new(items)
            .with_tag(PRIMARY_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_gap(RAIL_GAP)
                    .with_padding(Rect::new(RAIL_PAD, RAIL_PAD, RAIL_PAD, RAIL_PAD)),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![rail])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

/// One destination, tagged `"nav_rail#<i>"` for R51.42 sub-index routing.
/// The active destination paints the tonal active-indicator pill
/// (`SurfaceContainerHighest`, an M3-approximate of `secondaryContainer`
/// which pinion's palette does not carry) with an emphasised `OnSurface`
/// label; inactive destinations are transparent with an `OnSurfaceMuted`
/// label. The hover / pressed tint reuses the shared [`state_layer`].
fn destination(index: usize, state: RadioState, active: bool, theme: &Theme) -> Scene {
    let pill_base = if active {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    } else {
        Color::rgba(0, 0, 0, 0)
    };
    let pill_fill = state_layer(pill_base, state, theme);
    let label_color = if active {
        theme.resolve(ColorRole::OnSurface)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            DESTINATIONS[index],
            Rect::default(),
            TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(label_color),
        ))])
        .with_tag(format!("{PRIMARY_TAG}#{index}"))
        .with_style(BoxStyle::filled(pill_fill).with_corner_radius(PILL_RADIUS))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_padding(Rect::new(0, LABEL_PAD_X, 0, LABEL_PAD_X))
                .with_size(Size::auto().with_height(SizeValue::Px(ITEM_H))),
        ),
    )
}

struct NavRailView;

impl WidgetCore for NavRailView {
    type State = RailState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut ext = RadioGroupExternal::new(N);
        // Default active destination = the first ("Home"): a navigation
        // rail always has one current destination. KeyboardActivate selects
        // without a hover / pressed residue (state stays Idle).
        ext.send(0, RadioEvent::KeyboardActivate);
        Box::new(ext)
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> RailState {
        let mut out = RailState::idle();
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

    fn view(state: RailState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-nav-rail (R751 §5.38 §5.40 navigation)"
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
        // Navigate to the resolved destination through the shared roving-key
        // shell (1-of-N exclusion + `"selected"` intent); `resolve_target_index`
        // is this rail's opinionated ArrowUp/Down + Home/End key map.
        rc::roving_key(scene, focused, Self::tag(), key, resolve_target_index)
    }

    fn fmt_state_log(state: &RailState) -> String {
        let cur = state
            .rows
            .iter()
            .position(|(_, sel)| *sel)
            .map_or_else(|| "none".to_string(), |i| i.to_string());
        format!("active={cur}")
    }
}

impl WidgetA11y for NavRailView {
    /// §5.40 — a [`AriaRole::Navigation`] landmark whose children are the
    /// destination [`AriaRole::Link`] nodes; the active destination
    /// (selected) carries `aria-current="page"`. Link names come from
    /// [`DESTINATIONS`].
    fn access_node(state: &RailState, focused: Option<&str>) -> Vec<AccessNode> {
        let nav_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = rc::active_index(&state.rows, state.focused);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(N + 1);
        let mut nav = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Navigation)
            .with_name("Primary");
        for i in 0..N {
            nav = nav.with_child(format!("{PRIMARY_TAG}#{i}"));
        }
        nodes.push(nav);
        for (i, (dest_state, selected)) in state.rows.iter().copied().enumerate() {
            let dest_tag = format!("{PRIMARY_TAG}#{i}");
            let mut link = AccessNode::new(&dest_tag, AriaRole::Link)
                .with_name(DESTINATIONS[i])
                .with_state(AccessState {
                    focused: nav_focused && i == active_idx,
                    disabled: matches!(dest_state, RadioState::Disabled),
                    hovered: matches!(dest_state, RadioState::Hover),
                    pressed: matches!(dest_state, RadioState::Pressed),
                    checked: None,
                });
            if selected {
                link = link.with_current(AriaCurrent::Page);
            }
            nodes.push(link);
        }
        nodes
    }

    /// §5.40 composite focus model — when the rail owns focus, the
    /// Navigation landmark is the `TreeUpdate::focus` target and the active
    /// destination is the `aria-activedescendant` (the roving model; see
    /// the module docs for the per-link-Tab alternative).
    fn access_focus_target(state: &RailState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(<Self as WidgetCore>::tag()) {
            let idx = rc::active_index(&state.rows, state.focused);
            Some(rc::composite_focus(<Self as WidgetCore>::tag(), idx))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// §5.40 composite child action dispatch — an AT `Click` / `Default` on
    /// a destination navigates to it; `Focus` pins the active descendant.
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
        rc::child_invoke(intro, sub_tag, action, N)
    }
}

impl WidgetView for NavRailView {
    type Renderer = HelloNavRailRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve a key to a target destination index. `ArrowDown` / `ArrowUp`
/// step (wrapping); `Home` / `End` jump to the first / last destination.
fn resolve_target_index(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    match key {
        "Home" => Some(0),
        "End" => Some(N - 1),
        "ArrowDown" | "ArrowRight" => Some(rc::step(intro, 1, N)),
        "ArrowUp" | "ArrowLeft" => Some(rc::step(intro, -1, N)),
        _ => None,
    }
}

fn main() {
    pinion_shell::run::<NavRailView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::ExternalNode;

    fn selected_state(idx: usize) -> RailState {
        let mut s = RailState::idle();
        s.rows[idx].1 = true;
        s
    }

    #[test]
    fn nav_landmark_with_n_link_children() {
        let nodes = NavRailView::access_node(&selected_state(0), None);
        assert_eq!(nodes.len(), N + 1);
        assert_eq!(nodes[0].role, AriaRole::Navigation);
        assert_eq!(nodes[0].name.as_deref(), Some("Primary"));
        for i in 0..N {
            assert_eq!(nodes[i + 1].role, AriaRole::Link);
            assert_eq!(nodes[i + 1].name.as_deref(), Some(DESTINATIONS[i]));
        }
    }

    #[test]
    fn active_destination_carries_aria_current_page() {
        let nodes = NavRailView::access_node(&selected_state(2), None);
        assert_eq!(nodes[3].current, Some(AriaCurrent::Page), "destination 2 is active");
        assert!(nodes[1].current.is_none(), "inactive destination has no aria-current");
        assert!(nodes[2].current.is_none());
        assert!(nodes[4].current.is_none());
    }

    #[test]
    fn focused_nav_marks_active_destination() {
        let nodes = NavRailView::access_node(&selected_state(1), Some(PRIMARY_TAG));
        assert!(nodes[2].state.focused);
        assert!(!nodes[1].state.focused);
    }

    #[test]
    fn focus_target_is_composite_nav_plus_active_descendant() {
        let target = NavRailView::access_focus_target(&selected_state(2), Some(PRIMARY_TAG))
            .expect("nav focused returns Some");
        assert_eq!(target.focus_tag, PRIMARY_TAG);
        assert_eq!(target.active_descendant.as_deref(), Some("nav_rail#2"));
    }

    fn scene() -> Scene {
        let ext = RadioGroupExternal::new(N);
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag(PRIMARY_TAG))
    }

    fn active_index(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else { return None };
        node.handle.introspect()?.query("selected_index").and_then(|v| match v {
            IntrospectValue::Int(i) => Some(i),
            _ => None,
        })
    }

    #[test]
    fn arrow_navigates_and_wraps_when_focused() {
        let mut s = scene();
        for expected in [0_i64, 1, 2, 3, 0] {
            assert!(NavRailView::apply_key(
                &mut s,
                Some(PRIMARY_TAG),
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(active_index(&s), Some(expected));
        }
    }

    #[test]
    fn home_end_jump_to_edges() {
        let mut s = scene();
        assert!(NavRailView::apply_key(&mut s, Some(PRIMARY_TAG), "End", pinion_core::Modifiers::empty()));
        assert_eq!(active_index(&s), Some(i64::try_from(N - 1).unwrap()));
        assert!(NavRailView::apply_key(&mut s, Some(PRIMARY_TAG), "Home", pinion_core::Modifiers::empty()));
        assert_eq!(active_index(&s), Some(0));
    }

    #[test]
    fn unfocused_swallows_arrow() {
        let mut s = scene();
        assert!(!NavRailView::apply_key(&mut s, None, "ArrowDown", pinion_core::Modifiers::empty()));
        assert_eq!(active_index(&s), None);
    }

    #[test]
    fn at_click_navigates_to_destination() {
        let mut s = scene();
        assert!(NavRailView::access_child_invoke(&mut s, PRIMARY_TAG, "1", AccessAction::Click));
        assert_eq!(active_index(&s), Some(1));
    }

    #[test]
    fn view_carries_navigation_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<NavRailView>(
            RailState::idle(),
            &Frame::default(),
        );
    }
}
