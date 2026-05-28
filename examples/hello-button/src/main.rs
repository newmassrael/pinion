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

use pinion_core::scene::ContainerNode;
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::theme::{use_theme, ColorRole};
#[cfg(test)]
use pinion_core::theme::ThemeMode;
use pinion_core::{Frame, Scene, WidgetCore};
#[cfg(test)]
use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_widget_paint::button::{use_hover_progress, view_button, ButtonColors, ButtonStyle};
use pinion_shell::vello_renderer_impl;

// R650 §5.16 — single-tag binding uses the `"main_btn"` literal
// directly per [[abstraction-needs-second-consumer]]. The R644
// `enum Tags { MainBtn }` + `#[derive(WidgetTag)]` adoption was an
// over-engineered single-consumer rehearsal of a substrate that pays
// off only at composite-widget scale (multiple coordinated tags in
// one binary). The substrate itself stays land — the
// [`pinion_derive::WidgetTag`] derive macro and the
// `tag = SomeEnum::Variant` path form of `#[widget]` are both
// preserved for the future composite consumer; the binding-side
// boilerplate was walked back here.

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
/// (R57.X.button §5.50) [`ThemeProvider`] cache key. Matches the
/// `"app"` convention shared with `hello-toggle` / `hello-theme` /
/// `hello-listbox` / `hello-textfield` so the example gallery shares
/// one provider when a host binds them together.
const THEME_TAG: &str = "app";
const BTN_W: u32 = 160;
const BTN_H: u32 = 80;

/// (R686.C §5.22) Per-binding cache key for the hover-progress spring
/// driven by `pinion_widget_paint::button::use_hover_progress`. The
/// unique `&'static str` keeps the spring's owner-scoped cache slot
/// from colliding with any other cached value in the same scope.
const HOVER_ANIM_KEY: &str = "hello_button::hover_progress";


/// view-fn (§6.3): pure sync mapping `ButtonState` → `Scene`. The
/// `&Frame` slot is the §6.3 ZST hedge — zero-cost today, ready for
/// `dt`/`frame_index` without a `SemVer` major. Purity is the §2
/// `dry_run` invariant: same `(state, frame)` always yields the same
/// `Scene` AT A GIVEN SETTLED ANIMATION STATE — the R51.147 hover
/// transition layers a spring-driven progress value on top, which the
/// shell's repaint loop drives to steady state once the user input
/// stops. Re-paints during the transition observe the same monotonic
/// time series as the spring's tick history, so `dry_run` still gets
/// a deterministic snapshot when paired with the
/// [`Owner::snapshot`](pinion_core::Owner::snapshot) +
/// [`Owner::restore`](pinion_core::Owner::restore) primitives. The
/// shell calls `compute_layout` on the result before paint, so the
/// view fn need not (and should not) resolve pixel rects.
//
// `&Frame` intentional per §6.3 signature contract even though
// `Frame` is presently a ZST: once real per-frame fields land,
// passing by value would force a `SemVer` major on every view-fn.
// Allow the lint at the view-fn boundary.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    // (R57.X.button §5.50) Active palette — `use_theme` auto-
    // subscribes this view-fn so a `ThemeProvider::set_theme` from
    // anywhere in the application re-runs the view + repaints with
    // the new tones.
    let theme = use_theme(THEME_TAG).theme_animated();
    // R686.C §5.28 — animated hover progress (0.0 = Idle baseline,
    // 1.0 = full Hover) via the shared substrate hook. The spring
    // smooths the Idle ↔ Hover lerp over ~200ms; `view_button` threads
    // it into the M3 state-layer matrix.
    let hover_progress = use_hover_progress(matches!(state, ButtonState::Hover), HOVER_ANIM_KEY);
    let label = match state {
        ButtonState::Disabled => "Disabled",
        _ => "Click me!",
    };
    // R686.B §5.16 — M3 filled-tonal button via the
    // `pinion_widget_paint::button` substrate. `ButtonColors::filled_tonal`
    // resolves the same SurfaceContainerHighest / OnSurface roles the
    // pre-R686.B inline `button_fill_endpoints` did; the tag
    // ("main_btn") routes the InputRouter hit-test to the wrapped
    // ButtonExternal.
    let button = view_button(
        label,
        state,
        hover_progress,
        &ButtonColors::filled_tonal(&theme),
        &ButtonStyle::m3_default("main_btn")
            .with_size(Size::px(BTN_W, BTN_H))
            .with_label_font_size_px(18),
    );
    Scene::Container(
        ContainerNode::new(vec![button])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// `WidgetView` binding for the Button widget. R641 §5.16 lifted the
/// mechanical [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`] trait
/// wiring into the [`#[widget]`](pinion_derive::widget) attribute;
/// R642 §5.16 added the declarative `role = Button` +
/// `state_flags(...)` derive that emits the single-node
/// [`WidgetA11y::access_node`] body the binding used to spell out by
/// hand. The widget-specific methods that still need bespoke logic
/// ([`read_state`] / [`event_name`] / [`view`] / [`apply_key`] /
/// [`keybinding`]) remain as inherent `fn` items the macro forwards to.
///
/// The unit struct itself still exists solely as the impl carrier —
/// every emitted trait method is associated (`fn`, not `&self`) so the
/// shell instantiates `AppShell<ButtonView>` without holding a value
/// of this type.
///
/// [`create_external`]: pinion_core::WidgetCore::create_external
/// [`initial_size`]: pinion_shell::WidgetView::initial_size
/// [`view`]: pinion_core::WidgetCore::view
/// [`read_state`]: pinion_core::WidgetCore::read_state
/// [`event_name`]: pinion_core::WidgetCore::event_name
/// [`WidgetA11y::access_node`]: pinion_a11y::WidgetA11y::access_node
/// [`apply_key`]: pinion_core::WidgetCore::apply_key
/// [`keybinding`]: pinion_core::WidgetCore::keybinding
#[widget(
    tag = "main_btn",
    state = ButtonState,
    event = ButtonEvent,
    title = "pinion hello-button (R643 §5.16 #[widget])",
    renderer = HelloButtonRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ButtonExternal::new,
    role = Button,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
    ),
    apply_key,
    keybinding,
    state_name_derive,
)]
struct ButtonView;

impl ButtonView {
    /// R641 inherent forward for [`WidgetCore::view`]. The macro
    /// emits the trait method as `<ButtonView>::view(state, *frame)`
    /// (deref'd from the trait's `&Frame` to keep the inherent
    /// signature clippy-clean for the `Copy` 4-byte `Frame` ZST). The
    /// free-fn implementation below carries the actual view body.
    fn view(state: ButtonState, frame: Frame) -> Scene {
        // The free `view(...)` fn above takes `&Frame` (R51.147
        // hover animation predates the macro's by-value bridging);
        // re-borrow here to reuse it without touching the inner body.
        view(state, &frame)
    }

    /// R641 inherent forward for [`WidgetCore::keybinding`]. Maps
    /// single-character keys to typed [`ButtonEvent`] variants the
    /// shell dispatches through the §5.15 invoke channel.
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
    /// R641 §5.16 — moved to inherent method; the
    /// [`#[widget(..., apply_key)]`](pinion_derive::widget) flag
    /// instructs the macro to emit the [`WidgetCore::apply_key`]
    /// forwarding stub.
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

fn main() {
    pinion_shell::run::<ButtonView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;
    use pinion_core::theme::Theme;
    use pinion_core::Color;

    /// R51.69 §5.40 — convenience that mirrors the production
    /// `AppShell::render` pipeline: `view` + `access_node` +
    /// `enrich_names_from_scene`. Tests verifying the AT-exposed
    /// name use this so the assertion reads the same name an AT
    /// client would see, not just the bare `access_node` output.
    ///
    /// R51.147 §5.28 — `view()` now creates an `Animation<f32>` via
    /// `Owner::current()` on its first call, so the production
    /// `compute_paint_scene` `root_owner().run(...)` wrap (R51.146)
    /// must be mirrored here. The test owner stays alive only for the
    /// `run` body; the `HOVER_ANIM` thread-local cell caches the
    /// animation past the owner drop, but value reads still resolve
    /// (`Animation` holds its `AnimationInner` `Rc` independently of
    /// the registry; the registry tie only governs the tick walk).
    fn enriched(state: ButtonState, focused: Option<&str>) -> Vec<AccessNode> {
        let owner = pinion_core::Owner::new();
        let scene = owner.run(|| view(state, &Frame::new()));
        let mut nodes = <ButtonView as WidgetA11y>::access_node(&state, focused);
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
        let nodes = <ButtonView as WidgetA11y>::access_node(&ButtonState::Hover, None);
        assert!(nodes[0].state.hovered);
        assert!(!nodes[0].state.pressed);
        assert!(!nodes[0].state.disabled);
    }

    #[test]
    fn pressed_state_sets_pressed_flag() {
        let nodes = <ButtonView as WidgetA11y>::access_node(&ButtonState::Pressed, None);
        assert!(nodes[0].state.pressed);
        assert!(!nodes[0].state.hovered);
    }

    #[test]
    fn focused_tag_sets_focused_flag() {
        let nodes = <ButtonView as WidgetA11y>::access_node(&ButtonState::Idle, Some("main_btn"));
        assert!(nodes[0].state.focused);
    }

    #[test]
    fn other_focused_tag_does_not_set_focused() {
        let nodes = <ButtonView as WidgetA11y>::access_node(&ButtonState::Idle, Some("other_widget"));
        assert!(!nodes[0].state.focused);
    }

    #[test]
    fn checked_is_none_for_button() {
        let nodes = <ButtonView as WidgetA11y>::access_node(&ButtonState::Idle, None);
        assert_eq!(nodes[0].state.checked, None);
    }

    #[test]
    fn bare_access_node_leaves_name_none() {
        let nodes = <ButtonView as WidgetA11y>::access_node(&ButtonState::Idle, None);
        assert!(
            nodes[0].name.is_none(),
            "name comes from enrich_names_from_scene, not from access_node"
        );
    }

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        // R55.G.20 §5.49 — paint scene must carry the composite
        // `WidgetCore::tag()` so AI-side `{path: "main_btn"}` input
        // routing and `rect_for_tag` AT bounds attach resolve. The
        // hover animation observes `Owner::current()` on first call
        // (R51.147 §5.28).
        //
        // R55.G.22 §5.49 — pinned via the framework helper which
        // wraps `V::view` in an `Owner::new()` scope (subsumes the
        // R51.147 hover-animation requirement) and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ButtonView>(
            ButtonState::Idle,
            &Frame::new(),
        );
    }

    #[test]
    fn r650_widget_tag_literal_pin() {
        // R650 §5.16 — pin the binding-side tag literal directly.
        // R644 routed this through `Tags::MainBtn.as_tag()` which was
        // walked back per [[abstraction-needs-second-consumer]]; the
        // substrate round-trip lives at
        // `crates/pinion-derive/tests/widget_tag_derive.rs`. This pin
        // guards the binary-local contract: `WidgetCore::tag()` must
        // resolve to the same `"main_btn"` literal the paint scene's
        // `.with_tag(...)` emits, otherwise hit-tested events would
        // never reach the `External(ButtonExternal)`.
        assert_eq!(<ButtonView as WidgetCore>::tag(), "main_btn");
    }

    // ─────────────────────────────────────────────────────────────
    // R57.X.button §5.50 — theme retrofit regression. Pins (1) idle
    // endpoint resolves to `SurfaceContainerHighest`, (2) hover
    // endpoint = M3 8 % state-layer overlay, (3) panel bg = `Surface`
    // role and palette swap propagates through the auto-subscribed
    // view-fn.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_x_button_idle_endpoint_uses_surface_container_highest() {
        // R686.B — the fill colours moved to the
        // `pinion_widget_paint::button` substrate; this binding test
        // now verifies hello-button picks the M3 filled-tonal palette
        // (base = SurfaceContainerHighest) via `ButtonColors::filled_tonal`.
        let light = Theme::light();
        assert_eq!(
            ButtonColors::filled_tonal(&light).base,
            light.surface_container_highest,
        );
        let dark = Theme::dark();
        assert_eq!(
            ButtonColors::filled_tonal(&dark).base,
            dark.surface_container_highest,
        );
    }

    #[test]
    fn r57_x_button_hover_endpoint_is_m3_state_layer() {
        // Hover endpoint = `lerp(base, OnSurface, 0.08)` — M3 hover
        // state-layer canonical overlay weight, resolved by the
        // substrate's `m3_button_fill` at full hover progress.
        let theme = Theme::light();
        let colors = ButtonColors::filled_tonal(&theme);
        let expected = colors.base.lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(
            pinion_widget_paint::button::m3_button_fill(&colors, ButtonState::Hover, 1.0),
            expected,
        );
    }

    #[test]
    fn r57_x_button_panel_uses_surface_role_light_dark() {
        // End-to-end: flipping the ThemeMode between two `view`
        // invocations surfaces the Surface role somewhere in the
        // rendered scene tree. The view-fn reads through
        // `theme_animated` (R586 §5.50), so the dark assertion needs
        // the R57.X.theme-fade spring to settle past the M3 short4
        // window after the mode flip — `settle_owner_animations` then
        // the at-rest snap returns the exact dark palette.
        let owner = pinion_core::Owner::new();
        owner.run(|| {
            use_theme(THEME_TAG).set_mode(ThemeMode::Light);
            let light_scene = view(ButtonState::Idle, &Frame::new());
            assert!(scene_contains_fill(&light_scene, Theme::light().surface));
            use_theme(THEME_TAG).set_mode(ThemeMode::Dark);
            // Trigger the spring re-target; the value here is the
            // in-flight light anchor and is intentionally discarded.
            let _ = view(ButtonState::Idle, &Frame::new());
        });
        pinion_core::test_fixtures::settle_owner_animations(&owner);
        owner.run(|| {
            let dark_scene = view(ButtonState::Idle, &Frame::new());
            assert!(scene_contains_fill(&dark_scene, Theme::dark().surface));
        });
    }

    fn scene_contains_fill(scene: &Scene, target: Color) -> bool {
        match scene {
            Scene::Container(n) => {
                n.style.fill == target
                    || n.children.iter().any(|c| scene_contains_fill(c, target))
            }
            Scene::Box(n) => n.style.fill == target,
            _ => false,
        }
    }
}
