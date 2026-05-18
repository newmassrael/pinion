//! `hello-button` — §4 first dogfood, R51.30 pinion-shell migration.
//!
//! Pre-R51.30 this binary carried ~650 LOC of `App` / `RenderState` /
//! `dispatch_rpc` / `spawn_stdin_rpc_reader` boilerplate identical to
//! hello-toggle. R51.29 surfaced that duplication as
//! [[substrate-incompleteness-signal]] evidence; R51.30 moved every
//! one of those primitives into `pinion_shell` and reduced each
//! visual binary to its widget-specific diff:
//!
//! * the [`view`] fn (pure sync §6.3, `(state, frame) -> Scene`),
//! * the [`ButtonView`] [`WidgetView`] impl (state shape, event enum,
//!   `Scene::External` factory, introspect parser, keybindings),
//! * the [`vello_renderer_impl!`] macro bridging the
//!   pinion-forge-emitted [`HelloButtonRenderer`] to the shell's
//!   [`VelloRenderer`] trait,
//! * one-line [`main`] calling `pinion_shell::run::<ButtonView>()`.
//!
//! Architecture (R17 bidirectional RPC live dogfood) — unchanged
//! from the pre-shell version, just relocated to `pinion_shell`:
//!
//!   * The app owns the state scene
//!     `Scene::External(Box<ButtonExternal>)`. Live SCXML reachable
//!     via §5.15 introspect — no duplicate state.
//!   * winit pointer events + JSON-RPC frames both hit
//!     `invoke("send", Text(<name>))` — §2 invariant #2 (RPC headless
//!     as AI primary path) is literal.
//!   * §5.20 intents flow via `walk_scene_and_drain` after every
//!     winit / RPC dispatch; stderr-logged and also reachable through
//!     `scene/intents` RPC.
//!   * Paint scene is derived from cached `ButtonState` via [`view`];
//!     `paint_adapter::to_vello` (called from the shell) walks the
//!     tree into a `vello::Scene` and `HelloButtonRenderer` submits it.

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Scene};
use pinion_a11y::{AccessNode, AccessState, AriaRole};
use pinion_shell::{vello_renderer_impl, WidgetView};

// pinion-forge codegen output. Defines `pub struct HelloButtonRenderer`
// + `pub enum HelloButtonRendererError` + async `new<W: Into<wgpu::
// SurfaceTarget<'static>>>` + sync `render(&vello::Scene, peniko::
// Color)` + sync `resize(u32, u32)`. R46.3.3 emit template uses
// fully-qualified `::vello::*` paths so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods (emitted by
// pinion-forge) into the `pinion_shell::VelloRenderer` trait so the
// generic `AppShell<V>` can construct + render + resize it without
// hardcoding the concrete struct name. Keeps the codegen template
// pinion-shell-free (renderer consumers without the shell still work).
vello_renderer_impl!(HelloButtonRenderer, HelloButtonRendererError);

const WIN_W: u32 = 320;
const WIN_H: u32 = 200;
// R46.5 opaque RGB triples — `Color::from_argb(0x00...)` would decode
// to alpha=0 (fully transparent) on the Vello path; explicit `rgb`
// guarantees opacity.
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40);
const BTN_W: u32 = 160;
const BTN_H: u32 = 80;

/// view-fn (§6.3): pure sync mapping `ButtonState` → `Scene`. The
/// `&Frame` slot is the §6.3 ZST hedge — zero-cost today, ready for
/// `dt`/`frame_index` without a `SemVer` major. Purity is the §2
/// `dry_run` invariant: same `(state, frame)` always yields the same
/// `Scene`. The shell calls `compute_layout` on the result before
/// paint, so the view fn need not (and should not) resolve pixel rects.
//
// `&Frame` intentional per §6.3 signature contract even though
// `Frame` is presently a ZST: once real per-frame fields land,
// passing by value would force a `SemVer` major on every view-fn.
// Allow the lint at the view-fn boundary.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let btn_fill: Color = match state {
        ButtonState::Idle => Color::rgb(0xff, 0xff, 0xff),
        ButtonState::Hover => Color::rgb(0xd0, 0xd0, 0xd0),
        ButtonState::Pressed => Color::rgb(0x50, 0x50, 0x50),
        ButtonState::Disabled => Color::rgb(0xb0, 0x20, 0x20),
    };
    let label = match state {
        ButtonState::Disabled => "Disabled",
        _ => "Click me!",
    };
    let label_text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(Color::rgb(0, 0, 0)),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![label_text])
            // R48 §5.35: framework dispatch identifier. The shell's
            // InputRouter hit-tests the paint scene for this tag and
            // routes pointer events to the state scene's
            // ExternalNode("main_btn") — application code never sees
            // the cursor coordinates.
            .with_tag("main_btn")
            .with_style(BoxStyle::filled(btn_fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(BTN_W, BTN_H)),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![button])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// `WidgetView` binding for the Button widget. Carries no runtime
/// state — every method is associated (`fn`, not `&self`) so the
/// shell instantiates `AppShell<ButtonView>` without holding a value
/// of this type. The unit struct exists solely as the impl carrier.
struct ButtonView;

impl WidgetView for ButtonView {
    type State = ButtonState;
    type Event = ButtonEvent;
    type Renderer = HelloButtonRenderer;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "main_btn"
    }

    fn read_state(scene: &Scene) -> ButtonState {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    return parse_button_state(&name);
                }
            }
        }
        ButtonState::Idle
    }

    fn view(state: ButtonState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: ButtonEvent) -> &'static str {
        match event {
            ButtonEvent::PointerEnter => "PointerEnter",
            ButtonEvent::PointerLeave => "PointerLeave",
            ButtonEvent::PointerDown => "PointerDown",
            ButtonEvent::PointerUp => "PointerUp",
            ButtonEvent::Disable => "Disable",
            ButtonEvent::Enable => "Enable",
            // Internal SCXML variants the winit handler never produces
            // — route through a sentinel name `parse_button_event`
            // rejects.
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-button (R51.30 §5.16 pinion-shell)"
    }

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }

    fn keybinding(key: &str) -> Option<ButtonEvent> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    /// R51.54 §5.39 — ARIA Button keyboard activation. Space / Enter
    /// on the focused button fires a `KeyboardActivate` event, which
    /// the SCXML template (`standard_button.sce-template.xml`)
    /// processes as an internal transition from `Idle` or `Hover` —
    /// no visual state change, but the `Button::detect` substrate
    /// emits a `"click"` intent (parity with the `Pressed → Hover`
    /// pointer path). `Disabled` ignores activation; the SCXML
    /// transition is absent from that state per the ARIA spec.
    ///
    /// `focused` must match `Self::tag()` — the `FocusManager`
    /// dispatches the same key to every `WidgetView::apply_key`
    /// implementation but each widget gates on its own tag so
    /// activation never leaks to the wrong widget when multiple
    /// focusable controls share a screen.
    /// R51.63 §5.40 — AccessKit semantic tree contribution. Emits a
    /// single `AriaRole::Button` node whose `state_flags` mirror the
    /// four `ButtonState` variants 1:1 (`Idle` = no flags, `Hover` =
    /// `hovered`, `Pressed` = `pressed`, `Disabled` = `disabled`).
    /// `focused` is set when `focused == Some(Self::tag())`; bounds
    /// are filled by `AppShell::render` via `rect_for_tag` after
    /// layout.
    ///
    /// R51.69 §5.40 — the accessible name is no longer hard-coded
    /// here. `AppShell` calls `enrich_names_from_scene` with the
    /// paint scene after `view`, and the WAI-ARIA name-from-contents
    /// rule lifts the button's label text (`"Click me!"` or
    /// `"Disabled"`) directly out of the scene's `TextNode`. The
    /// duplicate match block this impl used to carry is now a single
    /// match in the `view` function — DRY restored. AT clients
    /// (Narrator / `VoiceOver` / Orca / `TalkBack`) see the same
    /// label the visible button shows.
    fn access_node(state: &ButtonState, focused: Option<&str>) -> Vec<AccessNode> {
        let access_state = AccessState {
            focused: focused == Some(Self::tag()),
            disabled: matches!(state, ButtonState::Disabled),
            hovered: matches!(state, ButtonState::Hover),
            pressed: matches!(state, ButtonState::Pressed),
            checked: None,
        };
        vec![AccessNode::new(Self::tag(), AriaRole::Button).with_state(access_state)]
    }

    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        if !matches!(key, "Space" | "Enter") {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke("send", IntrospectValue::Text("KeyboardActivate".to_string()))
            .is_ok()
    }
}

fn parse_button_state(name: &str) -> ButtonState {
    match name {
        "Hover" => ButtonState::Hover,
        "Pressed" => ButtonState::Pressed,
        "Disabled" => ButtonState::Disabled,
        // "Idle" + anything unexpected — defensive default.
        _ => ButtonState::Idle,
    }
}

fn main() {
    pinion_shell::run::<ButtonView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    /// R51.69 §5.40 — convenience that mirrors the production
    /// `AppShell::render` pipeline: `view` + `access_node` +
    /// `enrich_names_from_scene`. Tests verifying the AT-exposed
    /// name use this so the assertion reads the same name an AT
    /// client would see, not just the bare `access_node` output.
    fn enriched(state: ButtonState, focused: Option<&str>) -> Vec<AccessNode> {
        let scene = view(state, &Frame::new());
        let mut nodes = ButtonView::access_node(&state, focused);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        nodes
    }

    #[test]
    fn idle_emits_button_role_with_label() {
        let nodes = enriched(ButtonState::Idle, None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert_eq!(nodes[0].name.as_deref(), Some("Click me!"));
        assert_eq!(nodes[0].tag, "main_btn");
    }

    #[test]
    fn disabled_state_sets_disabled_flag_and_label() {
        let nodes = enriched(ButtonState::Disabled, None);
        assert!(nodes[0].state.disabled);
        assert_eq!(nodes[0].name.as_deref(), Some("Disabled"));
    }

    #[test]
    fn hover_state_sets_hovered_flag() {
        let nodes = ButtonView::access_node(&ButtonState::Hover, None);
        assert!(nodes[0].state.hovered);
        assert!(!nodes[0].state.pressed);
        assert!(!nodes[0].state.disabled);
    }

    #[test]
    fn pressed_state_sets_pressed_flag() {
        let nodes = ButtonView::access_node(&ButtonState::Pressed, None);
        assert!(nodes[0].state.pressed);
        assert!(!nodes[0].state.hovered);
    }

    #[test]
    fn focused_tag_sets_focused_flag() {
        let nodes = ButtonView::access_node(&ButtonState::Idle, Some("main_btn"));
        assert!(nodes[0].state.focused);
    }

    #[test]
    fn other_focused_tag_does_not_set_focused() {
        let nodes = ButtonView::access_node(&ButtonState::Idle, Some("other_widget"));
        assert!(!nodes[0].state.focused);
    }

    #[test]
    fn checked_is_none_for_button() {
        let nodes = ButtonView::access_node(&ButtonState::Idle, None);
        assert_eq!(nodes[0].state.checked, None);
    }

    #[test]
    fn bare_access_node_leaves_name_none() {
        let nodes = ButtonView::access_node(&ButtonState::Idle, None);
        assert!(
            nodes[0].name.is_none(),
            "name comes from enrich_names_from_scene, not from access_node"
        );
    }
}
