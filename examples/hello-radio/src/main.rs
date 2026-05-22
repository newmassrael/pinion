//! `hello-radio` — R51.33 §5.38 paint-side N=4 amortization evidence
//! on the pinion-shell substrate (R51.30). Radio is the first Tier-1
//! widget whose interaction shape is *button-like* but whose value
//! semantics are *set-not-flip*: a `false → true` activate sticks
//! (re-activation is idempotent and silent on the §5.20 channel).
//! `pinion_shell::run::<RadioView>()` covers the shell exactly as it
//! did for checkbox — substrate amortization continues without a
//! refactor.
//!
//! Visual contract: a 24×24 ring (outer Container, transparent fill,
//! 2-px border, `corner_radius = 12` to make a true circle) with the
//! `(state, selected)` cross-product encoded as border colour. When
//! `selected = true` a centred 12×12 filled dot child (Material /
//! `SwiftUI` / Qt convention) marks the chosen radio. The dot is a
//! `Scene::Box` with `corner_radius = 6` — same flex-centred Container
//! technique [`crate::widgets::Toggle`]'s knob and
//! [`crate::widgets::Checkbox`]'s glyph already use. A right-of label
//! ("Premium tier") makes the picker semantic without `Group` /
//! sibling Radio context (hello-radio demonstrates one Radio in
//! isolation; `RadioGroup` multi-tag routing lands in a later round).
//!
//! Keybindings exposed to the shell (`d` / `e`) drive the Disable /
//! Enable SCXML transitions. The activate path (pointer enter +
//! down + up on the ring) is the canonical mouse-driven select; no
//! drag UX applies. AI clients reach the same activate via
//! `scene/invoke /external/send {"PointerDown" | "PointerUp" | ...}`
//! exactly as winit does — §2 invariant #2 holds (RPC headless ==
//! human cursor for the activation path).

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::radio::{RadioEvent, RadioExternal, RadioState};
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRadioRenderer, HelloRadioRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 180;
/// (R57.X.radio §5.50) [`ThemeProvider`] cache key. Matches the
/// `"app"` convention shared with `hello-toggle` / `hello-theme` /
/// `hello-listbox` / `hello-textfield` / `hello-button` so the
/// example gallery shares one provider when a host binds them
/// together.
const THEME_TAG: &str = "app";
// Outer ring is a 24×24 Container with corner_radius = half-extent so
// taffy's box clipping inscribes a perfect circle (same trick the
// hello-toggle knob uses with KNOB_SIZE/2 = KNOB_RADIUS).
const RING_SIZE: u32 = 24;
const RING_RADIUS: u32 = 12;
// Inner dot is a 12×12 filled Box centred inside the ring's flex row.
// Material / SwiftUI convention is dot ≈ ring/2 — leaves a 4-px gap
// inside the 2-px border (24 - 2*2 = 20 inner; 20 - 12 dot = 4 each
// side) which reads as a clean concentric circle pair.
const DOT_SIZE: u32 = 12;
const DOT_RADIUS: u32 = 6;
const ROW_GAP: u32 = 10;

/// (R57.X.radio §5.50) Material 3 Radio border colour. The selected
/// axis (`Accent`) and unselected axis (`Outline`) get the canonical
/// M3 state-layer treatment (hover = 8 %, pressed = 12 %, disabled
/// = 38 % fade toward `Surface`).
fn radio_border_color(theme: &Theme, state: RadioState, selected: bool) -> Color {
    let base = if selected {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::Outline)
    };
    match state {
        RadioState::Idle => base,
        RadioState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        RadioState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
        RadioState::Disabled => base.lerp(theme.resolve(ColorRole::Surface), 0.38),
    }
}

/// view-fn (§6.3): pure sync mapping `(RadioState, bool) -> Scene`.
///
/// Layout: a horizontal row `[ring] [label]` centred in the window.
/// The ring is a `Scene::Container` (not a `Scene::Box`) so the
/// optional inner dot can render as a centred child when `selected`
/// is true — exactly the technique hello-checkbox uses for its
/// `\u{2713}` glyph. The container carries the dispatch tag
/// `main_radio` so the shell's `InputRouter` routes pointer events
/// to the matching `Scene::External("main_radio")`.
///
/// (R57.X.radio §5.50) Border + dot share the [`radio_border_color`]
/// resolution so the M3 Radio role mapping stays canonical:
/// unselected = `Outline`, selected = `Accent`, all states layer
/// through `Color::lerp` for hover / pressed / disabled overlays.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RadioState, selected: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme();
    let border_color = radio_border_color(&theme, state, selected);
    // Dot fill mirrors the active-side border colour (the Material
    // convention: selected dot uses the same accent as the ring) and
    // is only rendered when `selected` is true.
    let mut ring_children: Vec<Scene> = Vec::new();
    if selected {
        ring_children.push(Scene::Box(
            BoxNode::new(
                Rect::default(),
                BoxStyle::filled(border_color).with_corner_radius(DOT_RADIUS),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(DOT_SIZE, DOT_SIZE))),
        ));
    }
    let ring = Scene::Container(
        ContainerNode::new(ring_children)
            .with_style(
                BoxStyle::filled(Color::rgba(0, 0, 0, 0))
                    .with_corner_radius(RING_RADIUS)
                    .with_border(Border::new(border_color, 2)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(RING_SIZE, RING_SIZE)),
            ),
    );
    let label_color = match state {
        RadioState::Disabled => theme.resolve(ColorRole::OnSurfaceMuted),
        _ => theme.resolve(ColorRole::OnSurface),
    };
    let label = Scene::Text(TextNode::styled(
        "Premium tier",
        Rect::default(),
        TextStyle::new().with_size_px(16).with_fg(label_color),
    ));
    let row = Scene::Container(
        ContainerNode::new(vec![ring, label])
            .with_tag("main_radio")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![row])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

struct RadioView;

impl WidgetCore for RadioView {
    type State = (RadioState, bool);
    type Event = RadioEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(RadioExternal::new())
    }

    fn tag() -> &'static str {
        "main_radio"
    }

    fn read_state(scene: &Scene) -> (RadioState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    parse_radio_state(&name)
                } else {
                    RadioState::Idle
                };
                let selected =
                    matches!(intro.query("selected"), Some(IntrospectValue::Bool(true)));
                return (state, selected);
            }
        }
        (RadioState::Idle, false)
    }

    fn view(state: (RadioState, bool), frame: &Frame) -> Scene {
        view(state.0, state.1, frame)
    }

    fn event_name(event: RadioEvent) -> &'static str {
        match event {
            RadioEvent::PointerEnter => "PointerEnter",
            RadioEvent::PointerLeave => "PointerLeave",
            RadioEvent::PointerDown => "PointerDown",
            RadioEvent::PointerUp => "PointerUp",
            RadioEvent::Disable => "Disable",
            RadioEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-radio (R51.33 §5.38 pinion-shell)"
    }

    fn keybinding(key: &str) -> Option<RadioEvent> {
        match key {
            "d" => Some(RadioEvent::Disable),
            "e" => Some(RadioEvent::Enable),
            _ => None,
        }
    }

    /// R51.55 §5.39 — ARIA Radio keyboard activation. Space on the
    /// focused radio fires `KeyboardActivate`, which sets selected
    /// to `true` (set-not-flip) and emits the `"selected"` intent
    /// in parity with a pointer click. Already-selected radios stay
    /// silent (idempotent). The group-context arrow navigation that
    /// also activates the new radio lives in `hello-radio-group`
    /// (composite widget, R51.57 roving tabindex).
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        if key != "Space" {
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

    fn fmt_state_log(state: &(RadioState, bool)) -> String {
        format!(
            "{} / {}",
            radio_state_name(state.0),
            if state.1 { "selected" } else { "unselected" },
        )
    }
}

impl WidgetA11y for RadioView {
    /// R51.64 §5.40 — AccessKit semantic tree contribution. Emits a
    /// single `AriaRole::RadioButton` node; `value` + `state.checked`
    /// mirror the boolean selected state. A grouped Radio (R51.66
    /// composite) attaches under an `AriaRole::RadioGroup` parent;
    /// the single-radio dogfood case here remains AT-discoverable on
    /// its own.
    ///
    /// R51.69 §5.40 — the accessible name is lifted from the row's
    /// `"Premium tier"` `TextNode` by `enrich_names_from_scene`
    /// (WAI-ARIA "name from contents"). No duplicate literal here.
    fn access_node(state: &(RadioState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, selected) = (state.0, state.1);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, RadioState::Disabled),
            hovered: matches!(interaction, RadioState::Hover),
            pressed: matches!(interaction, RadioState::Pressed),
            checked: Some(selected),
        };
        vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::RadioButton)
                .with_value(AccessValue::Bool(selected))
                .with_state(access_state),
        ]
    }
}

impl WidgetView for RadioView {
    type Renderer = HelloRadioRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }
}

fn parse_radio_state(name: &str) -> RadioState {
    match name {
        "Hover" => RadioState::Hover,
        "Pressed" => RadioState::Pressed,
        "Disabled" => RadioState::Disabled,
        _ => RadioState::Idle,
    }
}

fn radio_state_name(state: RadioState) -> &'static str {
    match state {
        RadioState::Idle => "Idle",
        RadioState::Hover => "Hover",
        RadioState::Pressed => "Pressed",
        RadioState::Disabled => "Disabled",
    }
}

fn main() {
    pinion_shell::run::<RadioView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn enriched(state: (RadioState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (s, sel) = state;
        // (R57.X.radio §5.50) `view` now calls [`use_theme`] so the
        // call must run inside an `Owner` scope mirroring the
        // framework's `root_owner.run(...)` wrap (callback-root-owner-
        // wrap discipline).
        let owner = pinion_core::Owner::new();
        let scene = owner.run(|| view(s, sel, &Frame::new()));
        let mut nodes = RadioView::access_node(&state, focused);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        nodes
    }

    #[test]
    fn unselected_idle_emits_radiobutton_role() {
        let nodes = enriched((RadioState::Idle, false), None);
        assert_eq!(nodes[0].role, AriaRole::RadioButton);
        assert_eq!(nodes[0].name.as_deref(), Some("Premium tier"));
        assert_eq!(nodes[0].state.checked, Some(false));
    }

    #[test]
    fn selected_idle_value_and_state_align() {
        let nodes = RadioView::access_node(&(RadioState::Idle, true), None);
        assert_eq!(nodes[0].value, Some(AccessValue::Bool(true)));
        assert_eq!(nodes[0].state.checked, Some(true));
    }

    #[test]
    fn disabled_sets_disabled_flag() {
        let nodes = RadioView::access_node(&(RadioState::Disabled, false), None);
        assert!(nodes[0].state.disabled);
    }

    #[test]
    fn focused_tag_sets_focused_flag() {
        let nodes = RadioView::access_node(&(RadioState::Idle, false), Some("main_radio"));
        assert!(nodes[0].state.focused);
    }

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        // R55.G.20 §5.49 — paint scene must carry the composite
        // `WidgetCore::tag()` so AI-side `{path: "main_radio"}`
        // input routing and `rect_for_tag` AT bounds attach resolve.
        //
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<RadioView>(
            (RadioState::Idle, false),
            &Frame::new(),
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R57.X.radio §5.50 — theme retrofit regression. Pins the M3
    // Radio role mapping: unselected = Outline, selected = Accent,
    // with state-layer overlays for hover / pressed / disabled.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_x_radio_unselected_idle_uses_outline_role() {
        let light = Theme::light();
        assert_eq!(
            radio_border_color(&light, RadioState::Idle, false),
            light.outline,
        );
    }

    #[test]
    fn r57_x_radio_selected_idle_uses_accent_role() {
        let light = Theme::light();
        assert_eq!(
            radio_border_color(&light, RadioState::Idle, true),
            light.accent,
        );
        let dark = Theme::dark();
        assert_eq!(
            radio_border_color(&dark, RadioState::Idle, true),
            dark.accent,
        );
    }

    #[test]
    fn r57_x_radio_hover_overlay_lerps_toward_on_surface() {
        // M3 hover state-layer = 8 % `OnSurface` overlay on the role base.
        let theme = Theme::light();
        let expected = theme
            .resolve(ColorRole::Outline)
            .lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(
            radio_border_color(&theme, RadioState::Hover, false),
            expected,
        );
    }
}
