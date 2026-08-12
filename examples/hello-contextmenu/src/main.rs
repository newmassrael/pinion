// R772 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (ContextMenu, WAI-ARIA, ContextMenuExternal, …).
#![allow(clippy::doc_markdown)]

//! `hello-contextmenu` — R772 §5.53 §5.38 first consumer of the
//! `pinion_widget_paint::view_context_menu` substrate.
//!
//! ## Why this binding exists
//!
//! Phase B widget-catalog entry, and the second consumer of the R691
//! command-menu contract. A right-click command popup
//! (`Cut` / `Copy` / `Paste` / `Delete`) — the primitive every editor /
//! DCC / IDE ships, drawn by pinion's own scene-graph renderer on every
//! platform (R771.1 §5.53: the own-renderer menu is 1st-class; native OS
//! menus are an optional desktop-app projection, not the substrate).
//!
//! ## Context menu vs menubar dropdown
//!
//! Both are command-class popups (WAI-ARIA `menuitem`: no
//! `aria-selected` / `aria-checked`, activation fires a command and
//! dismisses). The *only* axis a context menu adds over a
//! [`MenuBar`](pinion_core::widgets::menu) dropdown is the **arbitrary
//! cursor anchor** — a menubar dropdown opens flush under its title; a
//! context menu opens wherever the secondary-button press landed. So the
//! shared substrate is reused, not recopied: the active-item navigation
//! ([`menu_nav`](pinion_core::widgets), lifted at this second consumer),
//! the floating-panel paint, and the R715 dismiss barrier.
//!
//! ## Open path (R772 §5.53)
//!
//! A secondary-button press routes through
//! [`WidgetCore::apply_secondary_click`] carrying the window-space click
//! point; the override `invoke("open_at", …)`s the
//! [`ContextMenuExternal`], anchoring the popup. Since R887 the
//! universal AI peer is `scene/click {button: "right"}` — the exact
//! injected mirror of the physical press, reaching this same override
//! on ANY binding (§2 invariant #2). `scene/invoke` `open_at` remains
//! the binding-specific programmatic action (open without a synthetic
//! pointer arc).
//!
//! ## Keyboard model (WAI-ARIA 1.2 §3.5 Menu)
//!
//! The whole keyboard model lives in `ContextMenuExternal::apply_key`
//! (reached via `invoke("key", <name>)`), so the human keyboard, the AT
//! action layer, and an RPC headless client converge on one statechart
//! (§2 invariant #2). When the menu owns focus and is open: Arrow
//! Up/Down move the active item (wrap), Home/End jump, Enter/Space
//! activate (command + close), Escape closes.
//!
//! ## Dismiss paths
//!
//! Escape, activating an item, and (R715) **clicking outside** the popup
//! all dismiss it. The click-outside path is the shared light-dismiss
//! barrier ([`dismiss_barrier`]): a transparent click-catcher over the
//! whole window, tagged `ctx#barrier`, whose outside `PointerUp` routes
//! to the one [`ContextMenuExternal`] (R51.42), which closes the popup.
//!
//! ## AI clients
//!
//! `query("open")` / `query("open_x")` / `query("open_y")` /
//! `query("active")` read the structure; `scene/click
//! {button: "right"}` (R887, the universal input arc) or
//! `invoke("open_at", "<x>,<y>")` (the programmatic action) opens it, `invoke("send", "i<i>:PointerUp")` activates an item,
//! `invoke("send", "barrier:PointerUp")` dismisses, and
//! `invoke("key", "<W3CKeyName>")` drives the keyboard model. The popup
//! is queryable via `scene/snapshot` on the `ctx` panel node (§2
//! invariant #7).

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, MenuItemCell, WidgetA11y, menu_item_nodes,
};
// R817 §5.40 — `AriaRole` is now only referenced by the test asserts (the
// lifted `menu_item_nodes` builder owns role + state tagging in prod).
#[cfg(test)]
use pinion_a11y::AriaRole;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::context_menu::{ContextMenuExternal, read_open_state};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::barrier::dismiss_barrier;
use pinion_widget_paint::menu::{
    ContextMenuPlacement, MenuStyle, composite_item_tag, parse_item_sub_tag, view_context_menu,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloContextMenuRenderer, HelloContextMenuRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";
/// The [`ContextMenuExternal`] scope tag — the External handle, the
/// painted popup panel (the widget's paint root + a11y `Menu` node), and
/// the snapshot anchor all share it. Item rows paint as the R51.42
/// composite `ctx#i<i>`, routing left-clicks to this one handle.
const CTX_TAG: &str = "ctx";
/// R715 §5.16 — the transparent click-outside dismiss barrier. The
/// R51.42 composite form `ctx#barrier` routes its outside `PointerUp` to
/// the one [`ContextMenuExternal`] (tag `ctx`), whose barrier arm closes
/// the popup.
const BARRIER_TAG: &str = "ctx#barrier";
const BODY_FONT_PX: u32 = 15;
const FOOTER_FONT_PX: u32 = 12;

/// The context-menu command items. Index `i` becomes the row tagged
/// `ctx#i<i>`; the label is each item's WAI-ARIA name-from-contents. The
/// count seeds the [`ContextMenuExternal`] structure.
const ITEMS: [&str; 4] = ["Cut", "Copy", "Paste", "Delete"];

/// Cached projection of the context menu: the open anchor (window-space)
/// and the active (highlighted) item. `Copy` so the shell hands the
/// snapshot into the paint closure without lifetime gymnastics.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
struct ContextState {
    /// Window-space anchor when open, `None` when closed.
    open_at: Option<(f32, f32)>,
    /// Highlighted item (WAI-ARIA active descendant), or `None`.
    active: Option<usize>,
}

/// view-fn (§6.3): pure sync mapping `ContextState -> Scene`. The body is
/// a static instruction panel; when the menu is open,
/// [`view_context_menu`] paints an absolutely-positioned floating command
/// list (panel tagged `ctx`, per-item `ctx#i<i>`) placed **last** so it
/// paints over everything, behind which the R715 [`dismiss_barrier`]
/// (tagged `ctx#barrier`) catches the click-outside that closes it.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ContextState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = MenuStyle::m3_default();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let body = Scene::Text(TextNode::styled(
        "Right-click anywhere to open the context menu.",
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(on_surface),
    ));
    let footer = Scene::Text(TextNode::styled(
        "right-click open   \u{2191}/\u{2193} item   Enter activate   Esc close",
        Rect::default(),
        TextStyle::new()
            .with_size_px(FOOTER_FONT_PX)
            .with_fg(on_surface_muted),
    ));
    let content = Scene::Container(
        ContainerNode::new(vec![body, footer]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_justify(JustifyContent::Start)
                .with_flex_grow(1.0)
                .with_gap(10)
                .with_padding(Rect::new(16, 16, 16, 16)),
        ),
    );

    let mut children = vec![content];
    if let Some(anchor) = state.open_at {
        // R715 §5.16 — transparent click-outside dismiss barrier over the
        // whole window. A context menu has no always-visible anchor strip
        // (unlike a menubar), so the barrier covers everything; an outside
        // click lands on `ctx#barrier` -> close. Pushed before the popup so
        // the popup hit-tests above it.
        children.push(dismiss_barrier(BARRIER_TAG, (0, 0), (WIN_W, WIN_H)));
        // Placed last so the absolutely-positioned popup paints over the
        // content and the barrier below it.
        children.push(view_context_menu(
            CTX_TAG,
            CTX_TAG,
            &ITEMS,
            state.active,
            ContextMenuPlacement {
                anchor,
                window: (WIN_W, WIN_H),
            },
            &theme,
            &style,
        ));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

struct ContextMenuView;

impl WidgetCore for ContextMenuView {
    type State = ContextState;
    // Every state change flows through `apply_secondary_click` (open),
    // `apply_key` (`invoke("key", …)`), and the InputRouter composite
    // `send` dispatch, so no keybinding-channel events flow through
    // `event_name`. `()` keeps the trait's `Copy` bound satisfied without
    // an unused variant (mirrors hello-menu).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ContextMenuExternal::new(ITEMS.len()))
    }

    fn tag() -> &'static str {
        CTX_TAG
    }

    fn read_state(scene: &Scene) -> ContextState {
        let mut out = ContextState::default();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // R986 §5.53 — the menu open/active projection is the shared
        // `read_open_state` reader (the menu peer of `read_selected`), so a
        // host that composes the menu as an extra external reuses it.
        let (open_at, active) = read_open_state(intro);
        out.open_at = open_at;
        out.active = active;
        out
    }

    fn view(state: ContextState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-contextmenu (R772 §5.53 §5.38)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        // Single source of truth for the keymap lives in the External's
        // `apply_key` (WAI-ARIA §3.5 menu model), reached below.
        None
    }

    /// R772 §5.53 — a secondary-button press opens (or re-anchors) the
    /// popup at the click point. The whole open contract lives in the
    /// External (`open_at`), so the binding just forwards the
    /// window-space coordinates and reports the External's open verdict.
    fn apply_secondary_click(scene: &mut Scene, x: f32, y: f32) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        matches!(
            intro.invoke("open_at", ContextMenuExternal::open_at_args(x, y)),
            Ok(IntrospectValue::Bool(true))
        )
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // The whole WAI-ARIA §3.5 keyboard model lives in the External;
        // forwarding the focused key to its `"key"` wire is the shared
        // command-menu pattern (R772.1; a closed popup swallows nothing, so
        // unhandled keys fall through).
        Self::forward_key_to_external(scene, focused, key)
    }

    fn fmt_state_log(state: &ContextState) -> String {
        match (state.open_at, state.active) {
            (Some((x, y)), Some(a)) => format!("open=({x},{y}) active={a}"),
            (Some((x, y)), None) => format!("open=({x},{y})"),
            (None, _) => "closed".to_string(),
        }
    }
}

impl WidgetA11y for ContextMenuView {
    /// R772 §5.40 — WAI-ARIA 1.2 §3.5 menu / menuitem tree. The menu is a
    /// transient popup: while closed it contributes nothing; while open it
    /// emits an `AriaRole::Menu` root (tag `ctx`) plus one
    /// `AriaRole::MenuItem` per command, the active one carrying the AT
    /// focus flag. Per-item names are derived by `enrich_names_from_scene`
    /// from each label `TextNode`; the menu container name has no single
    /// paint equivalent so it stays an explicit override.
    fn access_node(state: &ContextState, focused: Option<&str>) -> Vec<AccessNode> {
        // R817 §5.40 — lifted `menu_item_nodes` builder (one of two
        // consumers). The closed menu emits nothing; every item is a plain
        // command menuitem (no checkbox / disabled / separator here).
        if state.open_at.is_none() {
            return Vec::new();
        }
        let group_focused = focused == Some(<Self as WidgetCore>::tag());
        let tags: Vec<String> = (0..ITEMS.len())
            .map(|i| composite_item_tag(CTX_TAG, i))
            .collect();
        let items: Vec<MenuItemCell<'_>> = (0..ITEMS.len())
            .map(|i| MenuItemCell {
                tag: &tags[i],
                label: None,
                checked: None,
                disabled: false,
                focused: group_focused && state.active == Some(i),
                ..MenuItemCell::default()
            })
            .collect();
        menu_item_nodes(<Self as WidgetCore>::tag(), "Context menu", &items)
    }

    /// R772 §5.40 — composite focus model. While the menu owns focus and
    /// is open, the parent (`ctx`) stays focused and the active descendant
    /// points at the highlighted item; with nothing highlighted yet, focus
    /// rests on the menu container itself (atomic). A closed menu exposes
    /// no focusable surface.
    fn access_focus_target(state: &ContextState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused != Some(<Self as WidgetCore>::tag()) {
            return focused.map(AccessFocus::atomic);
        }
        state.open_at?;
        Some(match state.active {
            Some(i) => {
                AccessFocus::composite(<Self as WidgetCore>::tag(), composite_item_tag(CTX_TAG, i))
            }
            None => AccessFocus::atomic(<Self as WidgetCore>::tag()),
        })
    }

    /// R772 §5.40 — composite child action dispatch. `Click` / `Default`
    /// on an item (`ctx#i<i>`) activates it (command + close); `Focus`
    /// records the AT-side active descendant without committing.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Some(idx) = parse_item_sub_tag(sub_tag) else {
            return false;
        };
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                // PointerUp is the activation edge: command + close.
                let _ = intro.invoke("send", IntrospectValue::Text(format!("i{idx}:PointerUp")));
                true
            }
            AccessAction::Focus => {
                if let Ok(i) = i64::try_from(idx) {
                    let _ = intro.intervene("active", IntrospectValue::Int(i));
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for ContextMenuView {
    type Renderer = HelloContextMenuRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ContextMenuView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn closed() -> ContextState {
        ContextState::default()
    }

    fn open(active: Option<usize>) -> ContextState {
        ContextState {
            open_at: Some((100.0, 80.0)),
            active,
        }
    }

    #[test]
    fn r772_closed_emits_no_menu() {
        assert!(ContextMenuView::access_node(&closed(), None).is_empty());
    }

    #[test]
    fn r772_open_emits_menu_and_items() {
        let nodes = ContextMenuView::access_node(&open(Some(2)), None);
        // 1 menu + 4 items.
        assert_eq!(nodes.len(), 1 + ITEMS.len());
        assert_eq!(nodes[0].role, AriaRole::Menu);
        assert_eq!(nodes[0].tag, CTX_TAG);
        assert_eq!(nodes[0].name.as_deref(), Some("Context menu"));
        assert_eq!(nodes[0].children.len(), ITEMS.len());
        for i in 0..ITEMS.len() {
            assert_eq!(nodes[i + 1].role, AriaRole::MenuItem);
        }
    }

    #[test]
    fn r772_items_carry_posinset_setsize() {
        let nodes = ContextMenuView::access_node(&open(None), None);
        let items: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::MenuItem)
            .collect();
        assert_eq!(items.len(), ITEMS.len());
        for (i, item) in items.iter().enumerate() {
            assert_eq!(item.position_in_set, Some(u32::try_from(i + 1).unwrap()));
            assert_eq!(item.size_of_set, Some(u32::try_from(ITEMS.len()).unwrap()));
        }
    }

    #[test]
    fn r772_focused_open_marks_active_item() {
        let nodes = ContextMenuView::access_node(&open(Some(1)), Some(CTX_TAG));
        let item1 = nodes
            .iter()
            .find(|n| n.tag == "ctx#i1")
            .expect("item 1 node");
        assert!(item1.state.focused);
        assert!(!nodes[1].state.focused, "item 0 not focused");
    }

    #[test]
    fn r772_items_enriched_from_paint_when_open() {
        let state = open(None);
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = ContextMenuView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        let item0 = nodes.iter().find(|n| n.tag == "ctx#i0").unwrap();
        assert_eq!(item0.name.as_deref(), Some("Cut"));
        let item3 = nodes.iter().find(|n| n.tag == "ctx#i3").unwrap();
        assert_eq!(item3.name.as_deref(), Some("Delete"));
    }

    #[test]
    fn r772_focus_target_open_points_to_active_item() {
        let target = ContextMenuView::access_focus_target(&open(Some(2)), Some(CTX_TAG))
            .expect("focused open menu returns Some");
        assert_eq!(target.focus_tag, CTX_TAG);
        assert_eq!(target.active_descendant.as_deref(), Some("ctx#i2"));
    }

    #[test]
    fn r772_focus_target_open_no_active_rests_on_menu() {
        let target = ContextMenuView::access_focus_target(&open(None), Some(CTX_TAG))
            .expect("focused open menu returns Some");
        assert_eq!(target.focus_tag, CTX_TAG);
        assert_eq!(target.active_descendant, None);
    }

    #[test]
    fn r772_focus_target_none_when_closed_or_unfocused() {
        assert!(ContextMenuView::access_focus_target(&closed(), Some(CTX_TAG)).is_none());
        assert!(ContextMenuView::access_focus_target(&open(None), None).is_none());
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn scene() -> Scene {
        Scene::External(
            ExternalNode::new(Box::new(ContextMenuExternal::new(ITEMS.len()))).with_tag(CTX_TAG),
        )
    }

    fn open_at(s: &mut Scene, x: f32, y: f32) -> bool {
        ContextMenuView::apply_secondary_click(s, x, y)
    }

    fn is_open(scene: &Scene) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        matches!(
            node.handle.introspect().and_then(|i| i.query("open").ok()),
            Some(IntrospectValue::Bool(true))
        )
    }

    fn press(s: &mut Scene, key: &str) -> bool {
        ContextMenuView::apply_key(s, Some(CTX_TAG), key, pinion_core::Modifiers::empty())
    }

    #[test]
    fn r772_secondary_click_opens_at_point() {
        let mut s = scene();
        assert!(open_at(&mut s, 120.0, 64.0));
        assert!(is_open(&s));
        let Scene::External(node) = &s else {
            panic!("external");
        };
        let intro = node.handle.introspect().unwrap();
        assert_eq!(intro.query("open_x"), Ok(IntrospectValue::Float(120.0)));
        assert_eq!(intro.query("open_y"), Ok(IntrospectValue::Float(64.0)));
    }

    #[test]
    fn r772_keys_ignored_while_closed() {
        let mut s = scene();
        assert!(!press(&mut s, "ArrowDown"), "closed popup swallows nothing");
    }

    #[test]
    fn r772_arrow_then_enter_activates_and_closes() {
        let mut s = scene();
        open_at(&mut s, 0.0, 0.0);
        assert!(press(&mut s, "ArrowDown")); // active = 0
        assert!(press(&mut s, "Enter")); // command + close
        assert!(!is_open(&s));
        let Scene::External(node) = &mut s else {
            panic!("external");
        };
        let mut got = Vec::new();
        node.handle.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tag_str(), "command");
        assert_eq!(got[0].payload, IntrospectValue::Text("0".to_string()));
    }

    #[test]
    fn r772_escape_closes_without_command() {
        let mut s = scene();
        open_at(&mut s, 0.0, 0.0);
        assert!(press(&mut s, "Escape"));
        assert!(!is_open(&s));
        let Scene::External(node) = &mut s else {
            panic!("external");
        };
        let mut got = Vec::new();
        node.handle.drain_intents(&mut |i| got.push(i));
        assert!(got.is_empty(), "Escape fires no command");
    }

    #[test]
    fn r772_unfocused_swallows_keys() {
        let mut s = scene();
        open_at(&mut s, 0.0, 0.0);
        assert!(!ContextMenuView::apply_key(
            &mut s,
            Some("other"),
            "ArrowDown",
            pinion_core::Modifiers::empty()
        ));
    }

    #[test]
    fn r772_access_child_invoke_click_item_activates() {
        let mut s = scene();
        open_at(&mut s, 0.0, 0.0);
        assert!(ContextMenuView::access_child_invoke(
            &mut s,
            CTX_TAG,
            "i2",
            AccessAction::Click
        ));
        assert!(!is_open(&s), "activation dismisses the popup");
        let Scene::External(node) = &mut s else {
            panic!("external");
        };
        let mut got = Vec::new();
        node.handle.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].payload, IntrospectValue::Text("2".to_string()));
    }

    #[test]
    fn r772_access_child_invoke_bad_sub_tag_declines() {
        let mut s = scene();
        open_at(&mut s, 0.0, 0.0);
        assert!(!ContextMenuView::access_child_invoke(
            &mut s,
            CTX_TAG,
            "x9",
            AccessAction::Click
        ));
    }

    #[test]
    fn r772_view_contains_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ContextMenuView>(
            ContextState {
                open_at: Some((100.0, 80.0)),
                active: None,
            },
            &Frame::default(),
        );
    }

    #[test]
    fn r772_closed_view_omits_barrier_and_panel() {
        let scene = pinion_core::Owner::new().run(|| view(ContextState::default(), &Frame::new()));
        assert!(!scene.contains_tag(BARRIER_TAG), "closed: no barrier");
        assert!(!scene.contains_tag(CTX_TAG), "closed: no popup");
    }

    #[test]
    fn r772_open_view_paints_barrier_and_panel() {
        let state = ContextState {
            open_at: Some((100.0, 80.0)),
            active: None,
        };
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        assert!(scene.contains_tag(BARRIER_TAG), "open: barrier painted");
        assert!(scene.contains_tag(CTX_TAG), "open: popup panel painted");
        assert!(scene.contains_tag("ctx#i0"), "open: item rows route to ctx");
    }
}
