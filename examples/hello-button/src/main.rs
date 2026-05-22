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

use pinion_core::animation::SpringConfig;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::theme::{use_theme, ColorRole, Theme};
#[cfg(test)]
use pinion_core::theme::ThemeMode;
use pinion_core::{Animation, Color, Frame, Owner, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
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
/// (R57.X.button §5.50) [`ThemeProvider`] cache key. Matches the
/// `"app"` convention shared with `hello-toggle` / `hello-theme` /
/// `hello-listbox` / `hello-textfield` so the example gallery shares
/// one provider when a host binds them together.
const THEME_TAG: &str = "app";
const BTN_W: u32 = 160;
const BTN_H: u32 = 80;

/// R51.150 §5.22 — string key identifying the hover-progress
/// [`Animation`] in the binding's owner-scoped cache. A `&'static str`
/// per [`Owner::cache`]'s contract; the unique name guarantees no
/// collision with future cached values inside the same scope.
const HOVER_ANIM_KEY: &str = "hello_button::hover_progress";

/// R51.147 §5.28 + R51.150 §5.22 — drive the hover progress animation
/// off the current [`ButtonState`] and return the displayed value in
/// `[0.0, 1.0]`.
///
/// Materialises (on first call) the [`Animation<f32>`] inside the
/// binding's root [`Owner`] via [`Owner::cache`] (R51.150). The
/// animation lives for as long as the shell — owner drop releases
/// the cache map, dropping the animation and unregistering it from
/// the tick list. Idle / Pressed / Disabled target `0.0`; Hover
/// targets `1.0`. The spring carries velocity through re-targets so
/// transitioning Idle → Hover → Idle without waiting for settle looks
/// natural.
///
/// Pre-R51.150 used a thread-local `OnceCell<Animation<f32>>` —
/// `[[textbook-long-term-correct]]` violation since the cell survived
/// shell drops and aliased across multiple shells in the same thread.
/// The [`Owner::cache`] primitive replaces that workaround with the
/// canonical `SolidJS` / Leptos `useMemo` / React `useRef` shape:
/// per-owner instance, dropped with the owner.
fn drive_hover_progress(state: ButtonState) -> f32 {
    // R51.146 — `Owner::current()` resolves to the shell's
    // `root_owner` because `compute_paint_scene` wrapped this call in
    // `root_owner().run(...)`. Panicking here means the view fn is
    // running outside the framework wrap (a broken integration); we
    // choose loud failure over a silently-broken animation.
    let owner = Owner::current()
        .expect("hello-button view fn must run inside ShellCore::root_owner().run(...)");
    let anim: std::rc::Rc<Animation<f32>> = owner.cache(HOVER_ANIM_KEY, || {
        Animation::new(&owner, 0.0_f32, SpringConfig::default())
    });
    let target = if matches!(state, ButtonState::Hover) { 1.0 } else { 0.0 };
    anim.set_target(target);
    anim.value()
}

/// R51.151 §5.28 — Idle and Hover endpoints for the spring-driven
/// fade. [`Color::lerp`] interpolates between them in linear-light
/// space, matching the §5.28 spring solver's own colour-space
/// convention (R51.134).
///
/// (R57.X.button §5.50) Both endpoints are theme-resolved through
/// [`button_fill_endpoints`]: Idle = [`ColorRole::SurfaceContainerHighest`]
/// (Material 3 filled-tonal button surface), Hover =
/// `lerp(SurfaceContainerHighest, OnSurface, 0.08)` (M3 hover state
/// layer). Pre-cleanup the endpoints were hard-coded white → gray
/// `Color::rgb(...)` constants.
fn button_fill_endpoints(theme: &Theme) -> (Color, Color) {
    let idle = theme.resolve(ColorRole::SurfaceContainerHighest);
    let hover = idle.lerp(theme.resolve(ColorRole::OnSurface), 0.08);
    (idle, hover)
}

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
    // R51.147 §5.28 — animated hover progress (0.0 = Idle baseline,
    // 1.0 = full Hover). Drives the lightness lerp below.
    let hover_progress = drive_hover_progress(state);
    let (idle_fill, hover_fill) = button_fill_endpoints(&theme);
    let btn_fill: Color = match state {
        // Idle ↔ Hover lerp in linear-light space via
        // [`Color::lerp`] (R51.151). The spring smooths the
        // transition over ~200ms at the default config — no discrete
        // snap on cursor enter / leave.
        ButtonState::Idle | ButtonState::Hover => idle_fill.lerp(hover_fill, hover_progress),
        // (R57.X.button §5.50) Pressed = M3 pressed state layer
        // (12 % `OnSurface` overlay on the idle surface tier).
        ButtonState::Pressed => idle_fill.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
        // (R57.X.button §5.50) Disabled = M3 disabled overlay
        // (38 % fade toward `Surface`).
        ButtonState::Disabled => idle_fill.lerp(theme.resolve(ColorRole::Surface), 0.38),
    };
    let label = match state {
        ButtonState::Disabled => "Disabled",
        _ => "Click me!",
    };
    let label_fg = if matches!(state, ButtonState::Disabled) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let label_text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new().with_size_px(18).with_fg(label_fg),
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
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
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

impl WidgetCore for ButtonView {
    type State = ButtonState;
    type Event = ButtonEvent;

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
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

impl WidgetA11y for ButtonView {
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
    /// `"Disabled"`) directly out of the scene's `TextNode`. AT
    /// clients (Narrator / `VoiceOver` / Orca / `TalkBack`) see the
    /// same label the visible button shows.
    fn access_node(state: &ButtonState, focused: Option<&str>) -> Vec<AccessNode> {
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(state, ButtonState::Disabled),
            hovered: matches!(state, ButtonState::Hover),
            pressed: matches!(state, ButtonState::Pressed),
            checked: None,
        };
        vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Button)
                .with_state(access_state),
        ]
    }
}

impl WidgetView for ButtonView {
    type Renderer = HelloButtonRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
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

    // ─────────────────────────────────────────────────────────────
    // R57.X.button §5.50 — theme retrofit regression. Pins (1) idle
    // endpoint resolves to `SurfaceContainerHighest`, (2) hover
    // endpoint = M3 8 % state-layer overlay, (3) panel bg = `Surface`
    // role and palette swap propagates through the auto-subscribed
    // view-fn.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_x_button_idle_endpoint_uses_surface_container_highest() {
        let light = Theme::light();
        let (idle, _hover) = button_fill_endpoints(&light);
        assert_eq!(idle, light.surface_container_highest);
        let dark = Theme::dark();
        let (idle_d, _hover_d) = button_fill_endpoints(&dark);
        assert_eq!(idle_d, dark.surface_container_highest);
    }

    #[test]
    fn r57_x_button_hover_endpoint_is_m3_state_layer() {
        // Hover endpoint = `lerp(idle, OnSurface, 0.08)` — M3 hover
        // state-layer canonical overlay weight.
        let theme = Theme::light();
        let (idle, hover) = button_fill_endpoints(&theme);
        let expected = idle.lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(hover, expected);
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
