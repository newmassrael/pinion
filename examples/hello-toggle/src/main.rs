//! `hello-toggle` — §5.38 paint-side N=2 (R51.29) + R51.30 pinion-shell
//! migration.
//!
//! Same architecture as hello-button (R17 bidirectional RPC live
//! dogfood), differing only in:
//!
//! * the cached state shape is `(ToggleState, bool)` — interaction
//!   state plus the Off/On value sidecar [`Toggle::is_on`];
//! * the widget External is [`ToggleExternal`], introspect-exposing
//!   both `state` (string) and `value` (bool);
//! * the view fn draws a 64×32 rounded-pill track with the inner
//!   24×24 white knob justified Start (Off) / End (On) — the
//!   animation-free "snap" form (spring transitions are a §5.x carry).
//!
//! Every other framework primitive (App lifecycle, `RenderState`,
//! `dispatch_rpc`, stdin RPC reader, `InputRouter` wiring, intent
//! draining, paint loop) lives in [`pinion_shell`] — see that
//! crate's docs for the substrate-incompleteness-signal lesson that
//! produced this refactor (R51.30 immediate response to R51.29
//! N=2 evidence).

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{Color, Frame, Scene};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole};
use pinion_shell::{vello_renderer_impl, WidgetView};

// pinion-forge codegen output. Defines `pub struct HelloToggleRenderer`
// + `pub enum HelloToggleRendererError` + async `new<W: Into<wgpu::
// SurfaceTarget<'static>>>` + sync `render(&vello::Scene, peniko::
// Color)` + sync `resize(u32, u32)`. R46.3.3 emit template uses
// fully-qualified `::vello::*` paths so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so the generic `AppShell<V>`
// can construct + render + resize it. Identical pattern to
// hello-button — the only diff is the concrete renderer / error type
// name.
vello_renderer_impl!(HelloToggleRenderer, HelloToggleRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 220;
// Window background — same dark navy hello-button uses, for visual
// consistency across the example gallery.
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40);
// Track is a 64×32 rounded pill (radius 16 = half height = full
// pill). Padding 4 around the inner area gives a 24-px-tall inner
// strip that exactly matches the 24×24 knob, so the knob is
// vertically centered by AlignItems::Center without manual offset
// math.
const TRACK_W: u32 = 64;
const TRACK_H: u32 = 32;
const TRACK_RADIUS: u32 = 16;
const TRACK_PAD: u32 = 4;
const KNOB_SIZE: u32 = 24;
const KNOB_RADIUS: u32 = 12;
// Gap between "Dark mode" label, track, and status line in the root
// flex column — matches the macOS / iOS system-settings vertical
// rhythm (~16 px between related controls).
const ROW_GAP: u32 = 16;

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `&Frame` slot is the §6.3 ZST hedge — zero-cost today, ready for
/// `dt` / `frame_index` without a `SemVer` major. Purity is the §2
/// `dry_run` invariant: same `(state, value, frame)` always yields
/// the same `Scene`. The shell calls `compute_layout` on the result
/// before paint, so the view fn need not (and should not) resolve
/// pixel rects.
///
/// Layout (top-to-bottom, centered):
/// 1. "Dark mode" label (18 px white) — descriptive caption.
/// 2. Toggle track (64×32 rounded pill, tag = `main_toggle`): fill
///    encodes the joint `(state, value)` cross product; the inner
///    24×24 knob justifies Start when Off / End when On.
/// 3. Status line ("`<State>` | `<Value>`", 12 px grey) — text-only
///    state mirror so the AI side can verify by reading the Scene
///    tree even when the screenshot path is unavailable.
///
/// R48 §5.35: the `main_toggle` tag on the track container is the
/// shell's `InputRouter` hit-test handle — pointer events resolve to
/// that node and route to the matching `Scene::External("main_toggle")`
/// in the state scene. The knob and the labels carry no tag.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    // Track fill — encodes the (state, value) cross product. The Off
    // column stays in greyscale; the On column shifts to a green
    // accent (system "active" affordance). Pressed darkens both
    // columns for haptic feedback. Disabled is a distinct muted
    // brown-grey so users can visually distinguish it from Hover-off
    // (matches the macOS / iOS convention that disabled controls are
    // chromatically muted, not just dimmer).
    let track_fill: Color = match (state, on) {
        (ToggleState::Idle, false) => Color::rgb(0x40, 0x40, 0x40),
        (ToggleState::Hover, false) => Color::rgb(0x55, 0x55, 0x55),
        (ToggleState::Pressed, false) => Color::rgb(0x30, 0x30, 0x30),
        (ToggleState::Idle, true) => Color::rgb(0x30, 0xa0, 0x50),
        (ToggleState::Hover, true) => Color::rgb(0x40, 0xb0, 0x60),
        (ToggleState::Pressed, true) => Color::rgb(0x20, 0x70, 0x40),
        (ToggleState::Disabled, _) => Color::rgb(0x4a, 0x42, 0x38),
    };
    // Knob stays pure white in interactive states (canonical iOS /
    // Material affordance for the thumb), drops to a muted grey when
    // the widget is Disabled so it visually reads as inactive.
    let knob_fill: Color = match state {
        ToggleState::Disabled => Color::rgb(0xa0, 0xa0, 0xa0),
        _ => Color::rgb(0xff, 0xff, 0xff),
    };
    // The animation-free "snap" form: Off positions the knob via
    // JustifyContent::Start, On via JustifyContent::End. Tween /
    // spring transitions are a §5.x carry — the framework needs a
    // time source on the view-fn before that can land.
    let knob_justify = if on {
        JustifyContent::End
    } else {
        JustifyContent::Start
    };
    let knob = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(knob_fill).with_corner_radius(KNOB_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(KNOB_SIZE, KNOB_SIZE))),
    );
    let track = Scene::Container(
        ContainerNode::new(vec![knob])
            .with_tag("main_toggle")
            // R51.69 §5.40 — explicit accessible-name (WAI-ARIA
            // `aria-label`). The visible "Dark mode" caption sits as
            // a sibling of the track for layout reasons, so the
            // scene-walk name derivation cannot reach it; the
            // override pins the AT-exposed name without a duplicate
            // literal in `access_node`.
            .with_aria_label("Dark mode")
            .with_style(BoxStyle::filled(track_fill).with_corner_radius(TRACK_RADIUS))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(knob_justify)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(TRACK_W, TRACK_H))
                    .with_padding(Rect::new(TRACK_PAD, TRACK_PAD, TRACK_PAD, TRACK_PAD)),
            ),
    );
    let label = Scene::Text(TextNode::styled(
        "Dark mode",
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(Color::rgb(0xe0, 0xe0, 0xe0)),
    ));
    let status_str = format!(
        "{} | {}",
        toggle_state_name(state),
        if on { "On" } else { "Off" },
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(Color::rgb(0x90, 0x90, 0x90)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label, track, status])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// `WidgetView` binding for the Toggle widget. The state shape is
/// the joint `(ToggleState, bool)` pair — interaction state + the
/// Off/On value sidecar [`Toggle::is_on`].
struct ToggleView;

impl WidgetView for ToggleView {
    type State = (ToggleState, bool);
    type Event = ToggleEvent;
    type Renderer = HelloToggleRenderer;

    fn create_external() -> Box<dyn External> {
        Box::new(ToggleExternal::new())
    }

    fn tag() -> &'static str {
        "main_toggle"
    }

    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    parse_toggle_state(&name)
                } else {
                    ToggleState::Idle
                };
                let value = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
                return (state, value);
            }
        }
        (ToggleState::Idle, false)
    }

    fn view(state: (ToggleState, bool), frame: &Frame) -> Scene {
        view(state.0, state.1, frame)
    }

    fn event_name(event: ToggleEvent) -> &'static str {
        match event {
            ToggleEvent::PointerEnter => "PointerEnter",
            ToggleEvent::PointerLeave => "PointerLeave",
            ToggleEvent::PointerDown => "PointerDown",
            ToggleEvent::PointerUp => "PointerUp",
            ToggleEvent::Disable => "Disable",
            ToggleEvent::Enable => "Enable",
            // Internal SCXML variants — route through a sentinel.
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-toggle (R51.30 §5.38 pinion-shell)"
    }

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }

    fn keybinding(key: &str) -> Option<ToggleEvent> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }

    /// R51.55 §5.39 — ARIA Toggle Button keyboard activation. Space
    /// and Enter on the focused toggle fire `KeyboardActivate`,
    /// which flips the Off ↔ On sidecar and emits the `"toggle"`
    /// intent in parity with a pointer click. ARIA toggle buttons
    /// accept both keys; pure ARIA checkboxes accept only Space —
    /// `hello-toggle` is a toggle button so both land here.
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    /// R51.64 §5.40 — AccessKit semantic tree contribution. Emits a
    /// single `AriaRole::Switch` node (toggle button per WAI-ARIA;
    /// distinct from `AriaRole::CheckBox` because Switch carries
    /// On/Off semantics rather than tri-state Checked/Unchecked/Mixed).
    /// `value` is `AccessValue::Bool(on)`; `state.checked` mirrors
    /// the same boolean so AT clients reading either field see a
    /// consistent on/off state.
    ///
    /// R51.69 §5.40 — the accessible name is sourced from the
    /// track container's `aria_label` override (set in `view`) so
    /// the literal `"Dark mode"` lives in exactly one place. The
    /// shell's `enrich_names_from_scene` lifts it onto this node.
    fn access_node(state: &(ToggleState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, on) = (state.0, state.1);
        let access_state = AccessState {
            focused: focused == Some(Self::tag()),
            disabled: matches!(interaction, ToggleState::Disabled),
            hovered: matches!(interaction, ToggleState::Hover),
            pressed: matches!(interaction, ToggleState::Pressed),
            checked: Some(on),
        };
        vec![AccessNode::new(Self::tag(), AriaRole::Switch)
            .with_value(AccessValue::Bool(on))
            .with_state(access_state)]
    }

    fn fmt_state_log(state: &(ToggleState, bool)) -> String {
        format!(
            "{} / {}",
            toggle_state_name(state.0),
            if state.1 { "On" } else { "Off" },
        )
    }
}

fn parse_toggle_state(name: &str) -> ToggleState {
    match name {
        "Hover" => ToggleState::Hover,
        "Pressed" => ToggleState::Pressed,
        "Disabled" => ToggleState::Disabled,
        // "Idle" + anything unexpected — defensive default.
        _ => ToggleState::Idle,
    }
}

fn toggle_state_name(state: ToggleState) -> &'static str {
    match state {
        ToggleState::Idle => "Idle",
        ToggleState::Hover => "Hover",
        ToggleState::Pressed => "Pressed",
        ToggleState::Disabled => "Disabled",
    }
}

fn main() {
    pinion_shell::run::<ToggleView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    /// R51.69 §5.40 — pipeline mirror (`view` + `access_node` +
    /// `enrich_names_from_scene`) so name assertions read what the
    /// AT client sees, not the pre-enrichment intermediate.
    fn enriched(state: (ToggleState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (s, on) = state;
        let scene = view(s, on, &Frame::new());
        let mut nodes = ToggleView::access_node(&state, focused);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        nodes
    }

    #[test]
    fn off_idle_emits_switch_role_unchecked() {
        let nodes = enriched((ToggleState::Idle, false), None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].name.as_deref(), Some("Dark mode"));
        assert_eq!(nodes[0].value, Some(AccessValue::Bool(false)));
        assert_eq!(nodes[0].state.checked, Some(false));
    }

    #[test]
    fn on_idle_emits_checked_state() {
        let nodes = ToggleView::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].value, Some(AccessValue::Bool(true)));
        assert_eq!(nodes[0].state.checked, Some(true));
    }

    #[test]
    fn disabled_sets_disabled_flag() {
        let nodes = ToggleView::access_node(&(ToggleState::Disabled, false), None);
        assert!(nodes[0].state.disabled);
    }

    #[test]
    fn focused_tag_sets_focused_flag() {
        let nodes = ToggleView::access_node(&(ToggleState::Idle, false), Some("main_toggle"));
        assert!(nodes[0].state.focused);
    }

    #[test]
    fn aria_label_override_persists_when_on() {
        let nodes = enriched((ToggleState::Idle, true), None);
        assert_eq!(nodes[0].name.as_deref(), Some("Dark mode"));
    }
}
