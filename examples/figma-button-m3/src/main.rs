//! `figma-button-m3` — R640 §5.7 reactive Material 3 Filled Button.
//!
//! R635 landed the binding as a static one-frame snapshot: `read_state`
//! always returned [`ButtonState::Idle`] and the view fn ignored its
//! `state` argument, so the binary painted the same Figma Filled /
//! Enabled rendering regardless of pointer events. R640 lifts the
//! binding onto the [[abstraction-needs-second-consumer]] +
//! [[ai-first-rpc-introspection-obligation]] reactive substrate
//! `hello-button` (R51.30 §5.16) has shipped for over a hundred rounds:
//!
//! - [`read_state`] queries the wrapped `ButtonExternal`'s SCXML state
//!   via `External::introspect().query("state")`. The shell's
//!   `InputRouter` already routes the four pointer events (`PointerEnter`
//!   / `PointerLeave` / `PointerDown` / `PointerUp`) plus
//!   `PointerCancel` for touch-revoke parity, so wiring `event_name`
//!   1:1 with the SCXML transition names is the only widget-side step.
//! - The [`view`] fn lerps the Figma `#675AA4` fill toward the Material
//!   3 hover / pressed / disabled state-layer overlays. The hover
//!   transition is spring-driven via the same [`Owner::cache`] +
//!   [`Animation`] shape `hello-button` carries (R51.150 §5.22 +
//!   R51.147 §5.28) — same per-binding owner-scoped cache, same
//!   `SpringConfig::default()` timing, no thread-local global.
//!
//! ## Figma spec — verbatim from R635
//!
//! Re-creates the Material 3 "Filled / Enabled" Button variant from
//! the Material 3 Design Kit Community Figma file (node `51553:5180`):
//!
//! - 109 × 40 px button rect
//! - fill: `#675AA4` (M3 Primary; Figma `r=0.4039 g=0.3137 b=0.6431 a=1.0`)
//! - `cornerRadius: 100` (fully rounded — M3 default for filled
//!   buttons; clamped to `min(w, h) / 2 = 20` inside `paint_adapter`,
//!   which is why the rendering is a pill, not a square; the R639
//!   wire-up pinned this contract end-to-end)
//! - HORIZONTAL auto-layout, padding L=16, R=16, T=10, B=10
//! - label: `"Button"` Roboto Medium 14px white (leading + trailing
//!   18×18 icon slots elided to placeholders — Figma `INSTANCE`
//!   nodes are external-component references that need a glyph /
//!   svg substrate; tracked in the R660+ axis queue)
//!
//! ## Material 3 state-layer overlays — R640
//!
//! Material 3 specifies state layers as a tinted overlay on top of the
//! component fill, mixed in the component's foreground colour
//! (`onPrimary` for Filled buttons = `#FFFFFF` at the baseline). The
//! weights are spec-canonical:
//!
//! | State     | Overlay                                |
//! | --------- | -------------------------------------- |
//! | Idle      | none (raw `#675AA4`)                   |
//! | Hover     | `lerp(fill, onPrimary, 0.08)`          |
//! | Pressed   | `lerp(fill, onPrimary, 0.12)`          |
//! | Disabled  | `lerp(fill, canvas, 0.38)` (fade)      |
//!
//! The Idle ↔ Hover lerp is spring-driven by a `[0.0, 1.0]` progress
//! animation [`drive_hover_progress`] owns; Pressed and Disabled are
//! direct (snap) transitions — Material 3's hover-only spring is
//! consistent with `hello-button`'s pattern and with the Material
//! Components Android reference.
//!
//! Endpoints stay raw `Color::rgb(...)` constants, not theme-resolved
//! through [`pinion_core::theme`] — the binding is a Figma → pinion
//! design-parity reproduction, not a themed widget. Themed variants
//! are the `hello-button` arc; the Figma binding holds the line on
//! spec fidelity.
//!
//! ## R640 scope vs deferred
//!
//! In scope:
//! - Pointer-driven Idle / Hover / Pressed / Disabled state transitions
//! - Spring-driven Idle ↔ Hover fade
//! - SCXML state observable via `scene/query /external/state` (RPC
//!   introspection per [[ai-first-rpc-introspection-obligation]])
//! - `click` intent emitted on the `Pressed → Hover` activate path
//!
//! Deferred to a future round (currently `R660+ candidate`):
//! - Keyboard focus + ARIA Space / Enter activation: the `paint_focus_ring`
//!   substrate ignores `BoxStyle::corner_radius` and would render a
//!   square ring around the rounded pill (R639 watch-out). Lifting
//!   that gap is its own sub-task, intentionally not bundled with
//!   the M3 reactive lift to keep the diff focused.
//! - Ripple animation overlay (M3 pressed touch ripple): needs the
//!   §5.27 path-animation substrate; queued behind R660+ Figma axis.
//!
//! ## R640 verification — [[center-only-pixel-sample-anti-pattern]]
//!
//! Unit tests cover the colour math at each state plus the scene
//! structure (`corner_radius`, tag, layout). The companion
//! `tools/demos/figma_button_m3_r640.py` script drives the pointer
//! transition arc via JSON-RPC (`scene/click` + `scene/query`) and
//! asserts the observable state machine cycle, satisfying the
//! AI-first introspection obligation without asking the user to
//! describe what they see on screen.
//!
//! ```sh
//! PINION_SCREENSHOT=/tmp/pinion-btn.png cargo run -p figma-button-m3
//! python3 tools/demos/figma_button_m3_r640.py
//! ```

use pinion_core::animation::SpringConfig;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Animation, Color, Frame, Owner, Scene};
#[cfg(test)]
use pinion_core::WidgetCore;
#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// R650 §5.16 — single-tag binding uses the `"figma_button_m3"`
// literal directly per [[abstraction-needs-second-consumer]]. The
// R644 `enum Tags { FigmaButtonM3 }` + `#[derive(WidgetTag)]`
// adoption was a single-consumer rehearsal — the derive's value is
// at composite-widget scale (multiple coordinated tags in one
// binary), not for a binding with one tag. The substrate
// ([`pinion_derive::WidgetTag`] derive + `tag = Path` form of
// `#[widget]`) stays land for the future composite consumer;
// substrate-only coverage moved to
// `crates/pinion-derive/tests/widget_tag_derive.rs`.

// pinion-forge codegen output. Defines `pub struct FigmaButtonM3Renderer`
// + `pub enum FigmaButtonM3RendererError` plus the Vello-backed
// async `new` / sync `render` / sync `resize` methods.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so the generic `AppShell<V>`
// can construct + render + resize.
vello_renderer_impl!(FigmaButtonM3Renderer, FigmaButtonM3RendererError);

// Window slightly larger than the button itself so the surrounding
// canvas has breathing room for the design-parity inspection.
const WIN_W: u32 = 320;
const WIN_H: u32 = 160;

/// Figma Filled / Enabled Button spec — exact transcription from the
/// Material 3 Design Kit Community file (node `51553:5180`). Each
/// `const` mirrors a single Figma field so a future spec drift
/// surfaces as a single-line diff here.
const BTN_W: u32 = 109;
const BTN_H: u32 = 40;
const BTN_RADIUS: u32 = 100;
// Figma color: r=0.4039 g=0.3137 b=0.6431 → (103, 80, 164) sRGB byte
// per the standard 255× quantization R628 `quantize_unit_byte` uses
// internally; the trio matches the M3 token `Primary` (#675AA4).
const BTN_FILL: Color = Color::rgb(103, 80, 164);
const LABEL_FG: Color = Color::rgb(255, 255, 255);
const LABEL_PX: u32 = 14;
// Background canvas tone behind the button — neutral dark so the
// `#675AA4` button contrasts visually during the inspection. Not a
// Figma spec value; pinion-side framing only.
const CANVAS_BG: Color = Color::rgb(0x1F, 0x1F, 0x1F);

/// Material 3 hover state-layer weight — 8 % `onPrimary` overlay on
/// top of the component fill. Mirrors the Material Design 3 spec
/// table at <https://m3.material.io/foundations/interaction/states/state-layers>.
const HOVER_OVERLAY: f32 = 0.08;
/// Material 3 pressed state-layer weight — 12 % `onPrimary` overlay.
const PRESSED_OVERLAY: f32 = 0.12;
/// Material 3 disabled overlay weight — 38 % fade toward the canvas.
/// Material's actual spec is "12 % onSurface overlay at 38 % alpha";
/// the binding collapses both factors into a single linear-space
/// lerp toward `CANVAS_BG`, which is visually indistinguishable for
/// the Filled / Primary endpoint pair the Figma spec carries.
const DISABLED_OVERLAY: f32 = 0.38;

/// R51.150 §5.22 — string key identifying the hover-progress
/// [`Animation`] in the binding's owner-scoped cache. A `&'static str`
/// per [`Owner::cache`]'s contract; the unique name guarantees no
/// collision with future cached values inside the same scope.
const HOVER_ANIM_KEY: &str = "figma_button_m3::hover_progress";

/// R51.147 §5.28 + R51.150 §5.22 — drive the hover progress animation
/// off the current [`ButtonState`] and return the displayed value in
/// `[0.0, 1.0]`.
///
/// Materialises (on first call) the [`Animation<f32>`] inside the
/// binding's root [`Owner`] via [`Owner::cache`] (R51.150). The
/// animation lives for as long as the shell — owner drop releases the
/// cache map, dropping the animation and unregistering it from the
/// tick list. Idle / Pressed / Disabled target `0.0`; Hover targets
/// `1.0`. The spring carries velocity through re-targets so
/// transitioning Idle → Hover → Idle without waiting for settle looks
/// natural — same pattern `hello-button` (R51.147 §5.28) carries.
fn drive_hover_progress(state: ButtonState) -> f32 {
    // R51.146 — `Owner::current()` resolves to the shell's
    // `root_owner` because `compute_paint_scene` wrapped this call in
    // `root_owner().run(...)`. Panicking here means the view fn is
    // running outside the framework wrap (a broken integration); we
    // choose loud failure over a silently-broken animation.
    let owner = Owner::current()
        .expect("figma-button-m3 view fn must run inside ShellCore::root_owner().run(...)");
    let anim: std::rc::Rc<Animation<f32>> = owner.cache(HOVER_ANIM_KEY, || {
        Animation::new(&owner, 0.0_f32, SpringConfig::default())
    });
    let target = if matches!(state, ButtonState::Hover) { 1.0 } else { 0.0 };
    anim.set_target(target);
    anim.value()
}

/// Resolve the current button fill colour for `state`, applying the
/// Material 3 state-layer overlay matrix. Pure linear-space lerps via
/// [`Color::lerp`] (R51.151 §5.28) — matches the [[color-lerp-linear-space]]
/// canon `hello-button` already enforces.
///
/// Pulled into a free fn (rather than inlined into [`view`]) so the
/// unit tests can pin the colour math without re-running the whole
/// view pipeline.
fn button_fill_for(state: ButtonState) -> Color {
    let hover_progress = drive_hover_progress(state);
    // Hover endpoint = `lerp(fill, onPrimary, 0.08)` — the M3 hover
    // state-layer canonical 8 % white overlay. The Idle endpoint is
    // raw `BTN_FILL`; the spring carries the visual through the
    // intermediate values.
    let hover_endpoint = BTN_FILL.lerp(LABEL_FG, HOVER_OVERLAY);
    match state {
        ButtonState::Idle | ButtonState::Hover => BTN_FILL.lerp(hover_endpoint, hover_progress),
        ButtonState::Pressed => BTN_FILL.lerp(LABEL_FG, PRESSED_OVERLAY),
        ButtonState::Disabled => BTN_FILL.lerp(CANVAS_BG, DISABLED_OVERLAY),
    }
}

/// R640 §5.7 — paint the Material 3 Filled Button at the supplied
/// state. Pure sync `(state, frame) -> Scene` per §6.3.
///
/// `_frame` is unused at R640 — the view fn does not advance simulation
/// time on its own; the [`Animation`] cached inside [`drive_hover_progress`]
/// owns the spring tick. [`Frame`] is `Copy`, so the free fn takes it
/// by value per the workspace `clippy::pedantic` `trivially_copy_pass_by_ref`
/// rule; the `WidgetCore::view` trait shim below dereferences the
/// `&Frame` argument when forwarding.
fn view(state: ButtonState, _frame: Frame) -> Scene {
    let btn_fill = button_fill_for(state);
    let label = Scene::Text(TextNode::styled(
        "Button",
        Rect::default(),
        TextStyle::new().with_size_px(LABEL_PX).with_fg(LABEL_FG),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![label])
            // Framework dispatch identifier — matches `WidgetCore::tag()`
            // so the shell's `InputRouter` can hit-test pointer events
            // and dispatch them through the wrapped `ButtonExternal`.
            .with_tag("figma_button_m3")
            .with_style(BoxStyle::filled(btn_fill).with_corner_radius(BTN_RADIUS))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(BTN_W, BTN_H))
                    .with_padding(Rect::new(16, 10, 16, 10)),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![button])
            .with_style(BoxStyle::filled(CANVAS_BG))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// Unit struct carrying the `WidgetCore` / `WidgetA11y` / `WidgetView`
/// impls. R641 §5.16 + R642 §5.16 collapsed the three manual impl
/// blocks into the [`#[widget(...)]`](pinion_derive::widget) attribute
/// — `tag` / `title` / associated types / [`create_external`] factory
/// / [`initial_size`] / `role` / `state_flags` are now declarative;
/// the widget-specific methods ([`read_state`] / [`event_name`] /
/// [`view`]) remain as inherent `fn` items the macro forwards into.
///
/// Per the R51.30 shell contract every method is associated (`fn`,
/// not `&self`) so the shell instantiates `AppShell<FigmaButtonView>`
/// without holding a value of this type.
///
/// [`create_external`]: pinion_core::WidgetCore::create_external
/// [`initial_size`]: pinion_shell::WidgetView::initial_size
/// [`view`]: pinion_core::WidgetCore::view
/// [`read_state`]: pinion_core::WidgetCore::read_state
/// [`event_name`]: pinion_core::WidgetCore::event_name
#[widget(
    tag = "figma_button_m3",
    state = ButtonState,
    event = ButtonEvent,
    title = "Figma Material 3 Filled Button (R643 §5.16 #[widget])",
    renderer = FigmaButtonM3Renderer,
    initial_size = (WIN_W, WIN_H),
    external = ButtonExternal::new,
    role = Button,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
    ),
    state_name_derive,
)]
struct FigmaButtonView;

impl FigmaButtonView {
    /// R642 inherent forward for [`WidgetCore::view`]. The macro emits
    /// the trait method as `<FigmaButtonView>::view(state, *frame)` —
    /// the free `view(...)` fn below already takes `Frame` by value, so
    /// this stub is a 1:1 passthrough.
    fn view(state: ButtonState, frame: Frame) -> Scene {
        view(state, frame)
    }
}

fn main() {
    pinion_shell::run::<FigmaButtonView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `body` inside a transient [`Owner`] scope so the view-fn
    /// can resolve `Owner::current()` (R51.146). Mirrors the
    /// `enriched` helper in `hello-button` — the production
    /// `compute_paint_scene` wraps view in `root_owner().run(...)`,
    /// so tests must do the same.
    fn with_owner<F: FnOnce() -> R, R>(body: F) -> R {
        let owner = pinion_core::Owner::new();
        owner.run(body)
    }

    // ─────────────────────────────────────────────────────────────
    // Colour math — pins each Material 3 state-layer overlay weight
    // against the spec table, so a future drift in `HOVER_OVERLAY`
    // / `PRESSED_OVERLAY` / `DISABLED_OVERLAY` (or a regression in
    // `Color::lerp`'s linear-space semantics) shows as a typed test
    // failure rather than a pixel-level diff in a downstream demo.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r640_idle_fill_is_raw_figma_primary() {
        // Hover progress = 0 at Idle steady state — the lerp endpoint
        // collapses to BTN_FILL exactly.
        let fill = with_owner(|| button_fill_for(ButtonState::Idle));
        assert_eq!(fill, BTN_FILL);
    }

    #[test]
    fn r640_pressed_fill_is_m3_pressed_state_layer() {
        let fill = with_owner(|| button_fill_for(ButtonState::Pressed));
        let expected = BTN_FILL.lerp(LABEL_FG, PRESSED_OVERLAY);
        assert_eq!(fill, expected);
    }

    #[test]
    fn r640_disabled_fill_fades_toward_canvas() {
        let fill = with_owner(|| button_fill_for(ButtonState::Disabled));
        let expected = BTN_FILL.lerp(CANVAS_BG, DISABLED_OVERLAY);
        assert_eq!(fill, expected);
    }

    #[test]
    fn r640_hover_endpoint_is_m3_hover_state_layer() {
        // The hover endpoint a settled spring at progress = 1 would
        // converge on. The test settles via `set_target(1.0)` + spring
        // settle so the read value matches the lerp endpoint.
        let owner = pinion_core::Owner::new();
        owner.run(|| {
            // First call materialises the spring at progress 0 and
            // sets the target to 1.0 (matches!(Hover, Hover) == true).
            let _ = button_fill_for(ButtonState::Hover);
        });
        pinion_core::test_fixtures::settle_owner_animations(&owner);
        let fill = owner.run(|| button_fill_for(ButtonState::Hover));
        let expected = BTN_FILL.lerp(LABEL_FG, HOVER_OVERLAY);
        assert_eq!(fill, expected);
    }

    // ─────────────────────────────────────────────────────────────
    // Scene structure — pins the Figma spec verbatim. R639 lesson
    // ([[center-only-pixel-sample-anti-pattern]]) shows that
    // mutation of any of these fields can silently regress without
    // a typed test catching it; the scene-tree assertions are the
    // pre-pixel verification layer.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r640_view_carries_widget_tag() {
        // R55.G.22 §5.49 — paint scene must carry `WidgetCore::tag()`
        // so AI-side `{path: "figma_button_m3"}` input routing
        // resolves. Shared helper wraps `view` in an `Owner::new()`
        // scope and asserts `Scene::contains_tag`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<FigmaButtonView>(
            ButtonState::Idle,
            &Frame::new(),
        );
    }

    #[test]
    fn r640_view_carries_figma_corner_radius() {
        // [[center-only-pixel-sample-anti-pattern]] (R639) — the
        // `corner_radius = 100` field MUST reach the painted scene.
        // R635 set it on the style, R639 wired it through
        // paint_adapter; this test pins the upstream half so a future
        // refactor cannot silently drop the spec value.
        let scene = with_owner(|| view(ButtonState::Idle, Frame::new()));
        let button = find_button(&scene).expect("view must contain figma_button_m3 tag");
        assert_eq!(button.style.corner_radius, BTN_RADIUS);
    }

    #[test]
    fn r640_view_carries_figma_size() {
        let scene = with_owner(|| view(ButtonState::Idle, Frame::new()));
        let button = find_button(&scene).expect("view must contain figma_button_m3 tag");
        let size = button.layout.size;
        assert_eq!(size, Size::px(BTN_W, BTN_H));
    }

    #[test]
    fn r640_view_fill_tracks_state() {
        // Cross-state regression: each state's fill in the actual
        // scene matches `button_fill_for(state)`. Pins the
        // dispatch from view-fn to the colour helper.
        for state in [
            ButtonState::Idle,
            ButtonState::Pressed,
            ButtonState::Disabled,
        ] {
            let owner = pinion_core::Owner::new();
            let scene = owner.run(|| view(state, Frame::new()));
            let button = find_button(&scene)
                .unwrap_or_else(|| panic!("view({state:?}) must contain tag"));
            let expected = owner.run(|| button_fill_for(state));
            assert_eq!(
                button.style.fill, expected,
                "fill for {state:?} must match button_fill_for"
            );
        }
    }

    /// Depth-first walk for the `figma_button_m3` container.
    fn find_button(scene: &Scene) -> Option<&ContainerNode> {
        match scene {
            Scene::Container(node) => {
                if node.tag.as_deref() == Some("figma_button_m3") {
                    return Some(node);
                }
                node.children.iter().find_map(find_button)
            }
            _ => None,
        }
    }

    // ─────────────────────────────────────────────────────────────
    // A11y — surfaces ARIA Button role + lifts hover / pressed /
    // disabled state into the AccessNode flags. R635 clamped these
    // to `false`; R640 mirrors the live state.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r640_access_idle_has_no_state_flags() {
        let nodes = FigmaButtonView::access_node(&ButtonState::Idle, None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert!(!nodes[0].state.focused);
        assert!(!nodes[0].state.hovered);
        assert!(!nodes[0].state.pressed);
        assert!(!nodes[0].state.disabled);
    }

    #[test]
    fn r640_access_hover_sets_hovered_flag() {
        let nodes = FigmaButtonView::access_node(&ButtonState::Hover, None);
        assert!(nodes[0].state.hovered);
        assert!(!nodes[0].state.pressed);
    }

    #[test]
    fn r640_access_pressed_sets_pressed_flag() {
        let nodes = FigmaButtonView::access_node(&ButtonState::Pressed, None);
        assert!(nodes[0].state.pressed);
        assert!(!nodes[0].state.hovered);
    }

    #[test]
    fn r640_access_disabled_sets_disabled_flag() {
        let nodes = FigmaButtonView::access_node(&ButtonState::Disabled, None);
        assert!(nodes[0].state.disabled);
    }

    #[test]
    fn r640_access_focused_tag_sets_focused_flag() {
        let nodes = FigmaButtonView::access_node(
            &ButtonState::Idle,
            Some(<FigmaButtonView as WidgetCore>::tag()),
        );
        assert!(nodes[0].state.focused);
    }

    // ─────────────────────────────────────────────────────────────
    // event_name — every variant the shell's `InputRouter` produces
    // must map to a recognised SCXML transition. Pins the
    // bidirectional wire so a future `parse_button_event` rename
    // surfaces here, not as a silent input-routing dead-end.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r640_event_name_pointer_arc_round_trips() {
        for (event, name) in [
            (ButtonEvent::PointerEnter, "PointerEnter"),
            (ButtonEvent::PointerLeave, "PointerLeave"),
            (ButtonEvent::PointerDown, "PointerDown"),
            (ButtonEvent::PointerUp, "PointerUp"),
            (ButtonEvent::PointerCancel, "PointerCancel"),
            (ButtonEvent::Disable, "Disable"),
            (ButtonEvent::Enable, "Enable"),
        ] {
            assert_eq!(
                <FigmaButtonView as WidgetCore>::event_name(event),
                name,
                "event_name({event:?}) drift"
            );
        }
    }

    // ─────────────────────────────────────────────────────────────
    // read_state — wire test against a real `ButtonExternal`. Same
    // shape `hello-button` exercises; here we pin the parse arc for
    // the four documented variants.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r643_state_name_derive_known_variants() {
        // R643 §5.16 — the parse arc moved from a hand-written
        // `parse_button_state` helper to the derived
        // `WidgetStateName::from_name_or_default` impl (wired in
        // `pinion-core/src/widgets/button.rs` via the
        // `widget_state_name!` declarative macro). Same defensive-
        // default semantics; the test pins the four documented
        // SCXML state ids against the derived parse.
        use pinion_core::WidgetStateName;
        assert_eq!(ButtonState::from_name_or_default("Idle"), ButtonState::Idle);
        assert_eq!(ButtonState::from_name_or_default("Hover"), ButtonState::Hover);
        assert_eq!(ButtonState::from_name_or_default("Pressed"), ButtonState::Pressed);
        assert_eq!(ButtonState::from_name_or_default("Disabled"), ButtonState::Disabled);
    }

    #[test]
    fn r643_state_name_derive_unknown_falls_back_to_idle() {
        use pinion_core::WidgetStateName;
        assert_eq!(ButtonState::from_name_or_default(""), ButtonState::Idle);
        assert_eq!(ButtonState::from_name_or_default("Unknown"), ButtonState::Idle);
    }

    #[test]
    fn r650_widget_tag_literal_pin() {
        // R650 §5.16 — pin the binding-side tag literal directly.
        // R644 routed this through `Tags::FigmaButtonM3.as_tag()`
        // which was walked back per
        // [[abstraction-needs-second-consumer]]; the substrate
        // round-trip (including the `M3` → `m3` no-underscore
        // digit-as-lowercase-letter convention) now lives at
        // `crates/pinion-derive/tests/widget_tag_derive.rs`. This pin
        // guards the binary-local contract: `WidgetCore::tag()` must
        // resolve to the same `"figma_button_m3"` literal the paint
        // scene's `.with_tag(...)` emits, otherwise hit-tested events
        // would never reach the wrapped `ButtonExternal`.
        assert_eq!(<FigmaButtonView as WidgetCore>::tag(), "figma_button_m3");
    }

    #[test]
    fn r640_read_state_default_external_is_idle() {
        let scene = Scene::External(pinion_core::scene::ExternalNode::new(Box::new(
            ButtonExternal::new(),
        )));
        assert_eq!(
            <FigmaButtonView as WidgetCore>::read_state(&scene),
            ButtonState::Idle,
        );
    }
}
