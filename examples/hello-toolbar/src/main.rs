// R692 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates; the architectural narrative carries many
// proper-noun identifiers (Toolbar, WAI-ARIA, ToolbarExternal, …).
#![allow(clippy::doc_markdown)]

//! `hello-toolbar` — R692 §5.16 §5.40 §5.50 first consumer of the
//! `pinion_widget_paint::toolbar` substrate.
//!
//! ## Why this binding exists
//!
//! Phase B widget-catalog entry. A classic editor format toolbar —
//! Bold / Italic / Underline toggle buttons next to Undo / Redo
//! command buttons — the control strip every pro DCC / IDE / CAD tool
//! ships, and a direct step toward the northern-star "Unreal-class
//! editor self-hosted in pinion".
//!
//! ## Two control classes, one container
//!
//! A toolbar groups two kinds of control (WAI-ARIA 1.2 §3.4):
//!
//! - **Command** (Undo / Redo) — a one-shot button. Activation fires
//!   a `"command"` intent (payload `"<i>"`); no persistent state.
//! - **Toggle** (Bold / Italic / Underline) — a button carrying
//!   `aria-pressed`. Activation flips a persistent pressed bit and
//!   fires a `"toggle"` intent (payload `"<i>.<pressed>"`). The
//!   `aria-pressed` axis is the `button` role + a toggled state (vs the
//!   `aria-checked` a `checkbox` carries), so a toolbar toggle stays
//!   distinct from a [`CheckBox`](pinion_core::widgets::checkbox) at the
//!   AT surface even though both lower to the AccessKit `Toggled`
//!   attribute.
//!
//! Unlike the R691 [`MenuBar`](pinion_core::widgets::menu::MenuBarExternal)
//! (titles that open dropdown menus), a toolbar's controls are directly
//! actionable — there is no open/closed structural state, only the
//! roving cursor + the per-toggle pressed bits.
//!
//! ## Keyboard model (WAI-ARIA 1.2 §3.4 Toolbar)
//!
//! The whole keyboard model lives in `ToolbarExternal::apply_key`
//! (reached via `invoke("key", <name>)`), so the human keyboard, the
//! AT action layer, and an RPC headless client converge on one
//! statechart (§2 invariant #2). The toolbar is a single Tab stop:
//!
//! - **Arrow Left/Right** — move the roving cursor (wrap).
//! - **Home/End** — jump to the first/last control.
//! - **Enter/Space** — activate the focused control (command fires;
//!   toggle flips).
//! - **Tab** — unhandled here, so the shell advances focus past the
//!   toolbar to the next widget.
//!
//! ## Roving-cursor visual (R694 shell-focus gating)
//!
//! The paint draws the roving cursor's focus ring on the
//! `ToolbarExternal`'s `focus` control (mirroring
//! [`pinion_widget_paint::tree_view::view_tree_focused`]), gated on
//! `group_focused` so the ring shows only while the strip owns the shell
//! focus and dims when focus moves elsewhere. R692 deferred this gating
//! for want of a "does my widget own shell focus" channel into the pure
//! `view(state, frame)`; R694 §5.39 supplies it the same way hover /
//! pressed flow — the shell mirrors focus onto the `ToolbarExternal` via
//! `External::on_focus_change`, the external surfaces it on the `focused`
//! introspect slot, and `read_state` threads it into `ToolbarState`. The
//! a11y walker is independently shell-focus aware (`access_node`
//! receives the focused tag) so AT clients see the same state.
//!
//! ## AI clients
//!
//! `query("focus")` reads the roving cursor; `query("pressed.<i>")`
//! reads each toggle; `query("kind.<i>")` reads a control's class.
//! `invoke("send", "<i>:PointerUp")` activates a control,
//! `invoke("key", "<W3CKeyName>")` drives the keyboard model.

use pinion_a11y::{AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::toolbar::{ToolItem, ToolbarExternal};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::toolbar::{composite_item_tag, view_toolbar, ToolbarStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloToolbarRenderer, HelloToolbarRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 220;
const THEME_TAG: &str = "app";
const TOOLBAR_TAG: &str = "toolbar";
const BODY_FONT_PX: u32 = 15;
const FOOTER_FONT_PX: u32 = 12;

/// Number of toolbar controls (Bold / Italic / Underline / Undo /
/// Redo). A const so [`ToolbarState`] can store a `Copy` pressed array.
const CONTROL_COUNT: usize = 5;

/// Control labels. Index `i` becomes the control tagged `toolbar#<i>`;
/// the label is each control's WAI-ARIA name-from-contents.
const LABELS: [&str; CONTROL_COUNT] = ["B", "I", "U", "Undo", "Redo"];

/// Control classes, parallel to [`LABELS`]. The first three are toggle
/// buttons (`aria-pressed`); the last two are one-shot commands. Drives
/// both the `ToolbarExternal` structure and the a11y `checked` axis.
const KINDS: [ToolItem; CONTROL_COUNT] = [
    ToolItem::Toggle,
    ToolItem::Toggle,
    ToolItem::Toggle,
    ToolItem::Command,
    ToolItem::Command,
];

/// Cached projection of the toolbar: the roving keyboard cursor + the
/// per-control pressed bits. `Copy` so the shell hands the snapshot
/// into the paint closure without lifetime gymnastics.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct ToolbarState {
    /// Roving keyboard cursor (the control Arrow Left/Right moves).
    focus: usize,
    /// Per-control pressed bits; a command control's bit stays `false`.
    pressed: [bool; CONTROL_COUNT],
    /// R694 §5.39 — whether the toolbar owns the shell focus (read from
    /// the `ToolbarExternal`'s `focused` introspect slot, mirrored by the
    /// shell via `on_focus_change`). Gates the roving focus ring so it
    /// shows only while the strip is the focused widget.
    group_focused: bool,
}

/// view-fn (§6.3): pure sync mapping `ToolbarState -> Scene`. The
/// [`view_toolbar`] substrate paints the horizontal control strip
/// (tagged `toolbar`, per-control `toolbar#<i>`): pressed toggles carry
/// a tonal fill, the roving cursor control carries the focus ring.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToolbarState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = ToolbarStyle::m3_default();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    // R694 §5.39 — the roving ring tracks the External's `focus`, gated
    // on `group_focused` so it paints only while the strip owns shell
    // focus (the R692 deferred axis, now sourced from the External's
    // `focused` introspect slot via `read_state`).
    let toolbar = view_toolbar(
        TOOLBAR_TAG,
        &LABELS,
        &state.pressed,
        state.focus,
        state.group_focused,
        &theme,
        &style,
    );

    let body = Scene::Text(TextNode::styled(
        "Arrow between controls, Enter/Space to toggle or run; or click a control.",
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(on_surface),
    ));
    let footer = Scene::Text(TextNode::styled(
        "\u{2190}/\u{2192} move   Home/End jump   Enter/Space activate",
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

    Scene::Container(
        ContainerNode::new(vec![toolbar, content])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

struct ToolbarView;

impl WidgetCore for ToolbarView {
    type State = ToolbarState;
    // Every state change flows through `apply_key` (`invoke("key", …)`)
    // and the InputRouter composite `send` dispatch, so no
    // keybinding-channel events flow through `event_name`. `()` keeps
    // the trait's `Copy` bound satisfied (mirrors hello-menu /
    // hello-tabs).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ToolbarExternal::new(KINDS.to_vec()))
    }

    fn tag() -> &'static str {
        TOOLBAR_TAG
    }

    fn read_state(scene: &Scene) -> ToolbarState {
        let mut out = ToolbarState::default();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        out.focus = match intro.query("focus") {
            Some(IntrospectValue::Int(i)) => usize::try_from(i).unwrap_or(0),
            _ => 0,
        };
        for (i, slot) in out.pressed.iter_mut().enumerate() {
            *slot = matches!(intro.query(&format!("pressed.{i}")), Some(IntrospectValue::Bool(true)));
        }
        // R694 §5.39 — whether the strip owns shell focus, for the ring gate.
        out.group_focused =
            matches!(intro.query("focused"), Some(IntrospectValue::Bool(true)));
        out
    }

    fn view(state: ToolbarState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-toolbar (R692 §5.16 §5.40 §5.50)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        // Single source of truth for the keymap lives in the External's
        // `apply_key` (WAI-ARIA §3.4 toolbar model), reached below.
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // The toolbar is one tab stop; keys route only when the bar
        // itself owns focus (sibling controls keep their own keys).
        if focused != Some(Self::tag()) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        // The whole WAI-ARIA §3.4 keyboard model lives in the External;
        // the binding forwards the W3C key name and reports the
        // External's handled verdict so the shell swallow contract is
        // exact (unhandled keys like Tab fall through).
        matches!(
            intro.invoke("key", IntrospectValue::Text(key.to_string())),
            Ok(IntrospectValue::Bool(true))
        )
    }

    fn fmt_state_log(state: &ToolbarState) -> String {
        format!("focus={} pressed={:?}", state.focus, state.pressed)
    }
}

impl WidgetA11y for ToolbarView {
    /// R692 §5.40 — WAI-ARIA 1.2 §3.4 toolbar / button tree. Emits the
    /// [`AriaRole::Toolbar`] root + one [`AriaRole::Button`] per
    /// control. Toggle controls carry `aria-pressed` via
    /// [`AccessState::checked`] (which lowers to the AccessKit `Toggled`
    /// attribute); command controls leave `checked = None`. The roving
    /// cursor control carries the AT focus flag when the toolbar owns
    /// shell focus.
    fn access_node(state: &ToolbarState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(<Self as WidgetCore>::tag());
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(1 + CONTROL_COUNT);

        let mut toolbar = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Toolbar)
            .with_name("Formatting toolbar");
        for i in 0..CONTROL_COUNT {
            toolbar = toolbar.with_child(composite_item_tag(TOOLBAR_TAG, i));
        }
        nodes.push(toolbar);

        let setsize = u32::try_from(CONTROL_COUNT).unwrap_or(u32::MAX);
        for (i, kind) in KINDS.iter().enumerate() {
            let checked = match kind {
                ToolItem::Toggle => Some(state.pressed[i]),
                ToolItem::Command => None,
            };
            nodes.push(
                AccessNode::new(composite_item_tag(TOOLBAR_TAG, i), AriaRole::Button)
                    .with_state(AccessState {
                        focused: group_focused && state.focus == i,
                        checked,
                        ..AccessState::default()
                    })
                    .with_position_in_set(u32::try_from(i + 1).unwrap_or(u32::MAX))
                    .with_size_of_set(setsize),
            );
        }
        nodes
    }

    /// R51.71 §5.40 — composite focus model. While the toolbar owns
    /// focus, the parent (`toolbar`) stays focused and the active
    /// descendant points at the roving-cursor control; otherwise the
    /// shell focus is reported atomically.
    fn access_focus_target(state: &ToolbarState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused != Some(<Self as WidgetCore>::tag()) {
            return focused.map(AccessFocus::atomic);
        }
        Some(AccessFocus::composite(
            <Self as WidgetCore>::tag(),
            composite_item_tag(TOOLBAR_TAG, state.focus),
        ))
    }

    /// R51.70 §5.40 — composite child action dispatch. `Click` /
    /// `Default` on a control (`toolbar#<i>`) activates it (command
    /// fires / toggle flips). `Focus` moves the roving cursor without
    /// activating.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Ok(idx) = sub_tag.parse::<usize>() else {
            return false;
        };
        if idx >= CONTROL_COUNT {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                let _ = intro.invoke("send", IntrospectValue::Text(format!("{idx}:PointerUp")));
                true
            }
            AccessAction::Focus => {
                if let Ok(i) = i64::try_from(idx) {
                    let _ = intro.intervene("focus", IntrospectValue::Int(i));
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for ToolbarView {
    type Renderer = HelloToolbarRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ToolbarView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn state(focus: usize, pressed: [bool; CONTROL_COUNT]) -> ToolbarState {
        ToolbarState {
            focus,
            pressed,
            group_focused: false,
        }
    }

    #[test]
    fn r692_emits_toolbar_plus_buttons() {
        let nodes = ToolbarView::access_node(&ToolbarState::default(), None);
        assert_eq!(nodes.len(), 1 + CONTROL_COUNT);
        assert_eq!(nodes[0].role, AriaRole::Toolbar);
        assert_eq!(nodes[0].name.as_deref(), Some("Formatting toolbar"));
        for i in 0..CONTROL_COUNT {
            assert_eq!(nodes[i + 1].role, AriaRole::Button);
        }
    }

    #[test]
    fn r692_toolbar_lists_all_controls_in_order() {
        let nodes = ToolbarView::access_node(&ToolbarState::default(), None);
        assert_eq!(nodes[0].children.len(), CONTROL_COUNT);
        for i in 0..CONTROL_COUNT {
            assert_eq!(nodes[0].children[i], format!("toolbar#{i}"));
        }
    }

    #[test]
    fn r692_toggle_carries_pressed_command_does_not() {
        let nodes = ToolbarView::access_node(&state(0, [true, false, false, false, false]), None);
        // Control 0 = toggle, pressed → checked Some(true).
        assert_eq!(nodes[1].state.checked, Some(true));
        // Control 1 = toggle, unpressed → checked Some(false).
        assert_eq!(nodes[2].state.checked, Some(false));
        // Control 3 = command → no aria-pressed axis.
        assert_eq!(nodes[4].state.checked, None);
    }

    #[test]
    fn r692_posinset_setsize() {
        let nodes = ToolbarView::access_node(&ToolbarState::default(), None);
        for i in 0..CONTROL_COUNT {
            assert_eq!(nodes[i + 1].position_in_set, Some(u32::try_from(i + 1).unwrap()));
            assert_eq!(nodes[i + 1].size_of_set, Some(5));
        }
    }

    #[test]
    fn r692_focused_control_marked_when_group_focused() {
        let nodes = ToolbarView::access_node(&state(2, [false; CONTROL_COUNT]), Some("toolbar"));
        assert!(nodes[3].state.focused, "roving control 2 is focused");
        assert!(!nodes[1].state.focused);
    }

    #[test]
    fn r692_no_focus_when_unfocused() {
        let nodes = ToolbarView::access_node(&state(2, [false; CONTROL_COUNT]), None);
        for i in 0..CONTROL_COUNT {
            assert!(!nodes[i + 1].state.focused);
        }
    }

    #[test]
    fn r692_controls_enriched_from_paint() {
        let st = ToolbarState::default();
        let scene = pinion_core::Owner::new().run(|| view(st, &Frame::new()));
        let mut nodes = ToolbarView::access_node(&st, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[1].name.as_deref(), Some("B"));
        assert_eq!(nodes[4].name.as_deref(), Some("Undo"));
        assert_eq!(nodes[5].name.as_deref(), Some("Redo"));
    }

    #[test]
    fn r692_focus_target_composite_points_to_roving_control() {
        let target = ToolbarView::access_focus_target(&state(3, [false; CONTROL_COUNT]), Some("toolbar"))
            .expect("focused toolbar returns Some");
        assert_eq!(target.focus_tag, "toolbar");
        assert_eq!(target.active_descendant.as_deref(), Some("toolbar#3"));
    }

    #[test]
    fn r692_focus_target_none_when_unfocused() {
        assert!(ToolbarView::access_focus_target(&ToolbarState::default(), None).is_none());
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn scene() -> Scene {
        Scene::External(ExternalNode::new(Box::new(ToolbarExternal::new(KINDS.to_vec()))).with_tag(TOOLBAR_TAG))
    }

    fn focus_of(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        match node.handle.introspect()?.query("focus") {
            Some(IntrospectValue::Int(i)) => Some(i),
            _ => None,
        }
    }

    fn pressed_of(scene: &Scene, i: usize) -> Option<bool> {
        let Scene::External(node) = scene else {
            return None;
        };
        match node.handle.introspect()?.query(&format!("pressed.{i}")) {
            Some(IntrospectValue::Bool(b)) => Some(b),
            _ => None,
        }
    }

    fn press(s: &mut Scene, key: &str) -> bool {
        ToolbarView::apply_key(s, Some(TOOLBAR_TAG), key, pinion_core::Modifiers::empty())
    }

    #[test]
    fn r692_arrow_right_moves_roving_cursor() {
        let mut s = scene();
        assert!(press(&mut s, "ArrowRight"));
        assert_eq!(focus_of(&s), Some(1));
    }

    #[test]
    fn r692_end_jumps_last() {
        let mut s = scene();
        assert!(press(&mut s, "End"));
        assert_eq!(focus_of(&s), Some(4));
    }

    #[test]
    fn r692_enter_activates_focused_toggle() {
        let mut s = scene();
        press(&mut s, "ArrowRight"); // focus 1 (toggle)
        assert!(press(&mut s, "Enter"));
        assert_eq!(pressed_of(&s, 1), Some(true));
        let Scene::External(node) = &mut s else {
            panic!("external");
        };
        let mut got = Vec::new();
        node.handle.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tag_str(), "toggle");
        assert_eq!(got[0].payload, IntrospectValue::Text("1.true".to_string()));
    }

    #[test]
    fn r692_unfocused_bar_swallows_keys() {
        let mut s = scene();
        assert!(!ToolbarView::apply_key(
            &mut s,
            Some("other"),
            "ArrowRight",
            pinion_core::Modifiers::empty()
        ));
        assert_eq!(focus_of(&s), Some(0));
    }

    #[test]
    fn r692_tab_is_not_handled() {
        let mut s = scene();
        assert!(!press(&mut s, "Tab"));
    }

    #[test]
    fn r692_access_child_invoke_click_command_activates() {
        let mut s = scene();
        assert!(ToolbarView::access_child_invoke(
            &mut s,
            TOOLBAR_TAG,
            "3",
            AccessAction::Click
        ));
        assert_eq!(focus_of(&s), Some(3), "click focuses the control");
        let Scene::External(node) = &mut s else {
            panic!("external");
        };
        let mut got = Vec::new();
        node.handle.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tag_str(), "command");
        assert_eq!(got[0].payload, IntrospectValue::Text("3".to_string()));
    }

    #[test]
    fn r692_access_child_invoke_focus_moves_cursor() {
        let mut s = scene();
        assert!(ToolbarView::access_child_invoke(
            &mut s,
            TOOLBAR_TAG,
            "2",
            AccessAction::Focus
        ));
        assert_eq!(focus_of(&s), Some(2));
    }

    #[test]
    fn r692_access_child_invoke_bad_sub_tag_declines() {
        let mut s = scene();
        assert!(!ToolbarView::access_child_invoke(
            &mut s,
            TOOLBAR_TAG,
            "9",
            AccessAction::Click
        ));
        assert!(!ToolbarView::access_child_invoke(
            &mut s,
            TOOLBAR_TAG,
            "x",
            AccessAction::Click
        ));
    }

    #[test]
    fn r692_view_contains_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ToolbarView>(
            ToolbarState::default(),
            &Frame::default(),
        );
    }
}
