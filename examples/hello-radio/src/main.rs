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
//! technique `crate::widgets::Toggle`'s knob and
//! `crate::widgets::Checkbox`'s glyph already use. A right-of label
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

#[cfg(test)]
use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::radio::{RadioEvent, RadioExternal, RadioState};
use pinion_core::{Color, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::state_layer::state_layer;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRadioRenderer, HelloRadioRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 180;
/// (R57.X.radio §5.50) [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key. Matches the
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
    state_layer(base, state, theme)
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
    let theme = use_theme(THEME_TAG).theme_animated();
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

/// R654 §5.16 Cat A cascade retrofit. AriaRole::RadioButton; tuple
/// state `(RadioState, bool)` with the `selected` bool in the
/// second elem. Substrate-derived a11y body via R653-extended
/// `state_flags(checked = bool_field(1))` + `access_value =
/// bool_field(1)`. Mirrors hello-checkbox / hello-toggle shape.
#[widget(
    tag = "main_radio",
    state = (RadioState, bool),
    event = RadioEvent,
    title = "pinion hello-radio (R654 §5.16 #[widget] retrofit)",
    renderer = HelloRadioRenderer,
    initial_size = (WIN_W, WIN_H),
    external = RadioExternal::new,
    role = RadioButton,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
        checked = bool_field(1),
    ),
    access_value = bool_field(1),
    apply_key,
    keybinding,
    fmt_state_log,
)]
struct RadioView;

impl RadioView {
    fn read_state(scene: &Scene) -> (RadioState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    RadioState::from_name_or_default(&name)
                } else {
                    RadioState::Idle
                };
                let selected = matches!(intro.query("selected"), Some(IntrospectValue::Bool(true)));
                return (state, selected);
            }
        }
        (RadioState::Idle, false)
    }

    fn view(state: (RadioState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    fn event_name(event: RadioEvent) -> &'static str {
        // R699 §5.16 — route the forward Event->name mapping through the
        // WidgetEventName SSOT (`as_name`), retiring the hand-written
        // match table. `as_name` is total over internal variants too;
        // only external events reach this path via `ShellCore::forward`.
        pinion_core::WidgetEventName::as_name(&event)
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
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
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
            .invoke(
                "send",
                IntrospectValue::Text("KeyboardActivate".to_string()),
            )
            .is_ok()
    }

    fn fmt_state_log(state: (RadioState, bool)) -> String {
        format!(
            "{} / {}",
            state.0.as_name(),
            if state.1 { "selected" } else { "unselected" },
        )
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
