//! `hello-toggle` — §5.38 paint-side N=2 (R51.29) + R51.30 pinion-shell
//! migration, R57.X.toggle theme retrofit.
//!
//! Same architecture as hello-button (R17 bidirectional RPC live
//! dogfood), differing only in:
//!
//! * the cached state shape is `(ToggleState, bool)` — interaction
//!   state plus the Off/On value sidecar `Toggle::is_on`;
//! * the widget External is [`ToggleExternal`], introspect-exposing
//!   both `state` (string) and `value` (bool);
//! * the view fn draws a 64×32 rounded-pill track with the inner
//!   24×24 knob justified Start (Off) / End (On) — the
//!   animation-free "snap" form (spring transitions are a §5.x carry).
//!
//! R57.X.toggle — first real consumer retrofit of the
//! [`pinion_core::theme`] substrate (R57.0 §5.50). Every visible color
//! now resolves through a [`ColorRole`] against the active
//! [`use_theme("app")`](pinion_core::use_theme) palette; flipping the
//! Off/On sidecar drives a [`ThemeProvider::set_mode`](pinion_core::theme::ThemeProvider::set_mode) swap so the
//! demo's "Dark mode" label becomes semantically accurate — On really
//! is dark mode.
//!
//! R1574 — the Material 3 Switch role mapping (track Off / On, thumb
//! Off / On, and the hover / pressed state-layer overlay) used to be
//! spelled out here and computed inline below. It is
//! [`pinion_widget_paint::switch`]'s now, and this binding was one of
//! exactly **two** in the tree that genuinely hand-rolled it — see that
//! module's doc for the census that corrected the debt note from twelve
//! consumers to two.
//!
//! Every other framework primitive (App lifecycle, `RenderState`,
//! `dispatch_rpc`, stdin RPC reader, `InputRouter` wiring, intent
//! draining, paint loop) lives in [`pinion_shell`] — see that
//! crate's docs for the substrate-incompleteness-signal lesson that
//! produced this refactor (R51.30 immediate response to R51.29
//! N=2 evidence).

use pinion_core::external::IntrospectValue;
// Borrowed in the V::update reducer to read the post-flip authority
// out of the Toggle intent's payload — see the body comment for why
// the intent payload (not V::read_state) is the canonical source.
#[cfg(test)]
use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
#[cfg(test)]
use pinion_core::Theme;
use pinion_core::intent::Intent;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Command, Frame, Scene, ThemeMode, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::switch::{SwitchStyle, view_switch};

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
// Gap between "Dark mode" label, track, and status line in the root
// flex column — matches the macOS / iOS system-settings vertical
// rhythm (~16 px between related controls).
const ROW_GAP: u32 = 16;

const LABEL_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// Root-owner cache key for this binary's [`ThemeProvider`](pinion_core::theme::ThemeProvider). Shared
/// between the [`view`] fn (reactive read) and
/// [`ToggleView::update`] (palette swap on `"toggle"` intent) so both
/// halves resolve the same typed [`use_theme`] slot without repeating
/// the literal.
const THEME_TAG: &str = "app";

/// Fully-prefixed wire tag for the `Toggle` widget's `"toggle"`
/// intent, built via `pinion_core::intent_tag!`. See that macro's
/// doc-comment for the §5.20 R22 wire-form contract — here we just
/// bind the compile-time concatenation result for `V::update` to
/// compare against.
const TOGGLE_INTENT_TAG_FULL: &str = pinion_core::intent_tag!("main_toggle", "toggle");

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `&Frame` slot is the §6.3 ZST hedge — zero-cost today, ready for
/// `dt` / `frame_index` without a `SemVer` major. Purity is the §2
/// `dry_run` invariant: same `(state, value, frame, theme)` always
/// yields the same `Scene`. The shell calls `compute_layout` on the
/// result before paint, so the view fn need not (and should not)
/// resolve pixel rects.
///
/// Reads the active palette via [`use_theme(THEME_TAG)`] so the
/// reactive subscription captures palette swaps automatically — the
/// next [`ThemeProvider::set_mode`](pinion_core::theme::ThemeProvider::set_mode) re-runs the view without any
/// view-fn branch on the mode.
///
/// Layout (top-to-bottom, centered):
/// 1. "Dark mode" label (18 px, [`ColorRole::OnSurface`]) —
///    descriptive caption.
/// 2. Toggle track (64×32 rounded pill, tag = `main_toggle`): fill
///    encodes the joint `(state, value)` cross product through
///    [`ColorRole::SurfaceContainerHighest`] (Off) /
///    [`ColorRole::Accent`] (On), modulated by the M3 state-layer
///    weight for Hover / Pressed; the inner 24×24 knob justifies
///    Start when Off / End when On with
///    [`ColorRole::Outline`] (Off) / [`ColorRole::OnAccent`] (On).
/// 3. Status line ("`<State>` | `<Value>`", 12 px,
///    [`ColorRole::OnSurfaceMuted`]) — text-only state mirror so
///    the AI side can verify by reading the Scene tree even when the
///    screenshot path is unavailable.
///
/// R48 §5.35: the `main_toggle` tag on the track container is the
/// shell's `InputRouter` hit-test handle — pointer events resolve to
/// that node and route to the matching `Scene::External("main_toggle")`
/// in the state scene. The knob and the labels carry no tag.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    // Reactive read — every paint subscribes the root owner to the
    // ThemeProvider's `palette` signal. The next `set_mode` flips
    // the surface for free.
    let theme = use_theme(THEME_TAG).theme_animated();
    // Cache the OnSurface role once — it both renders the title /
    // status-line foregrounds and serves as the M3 state-layer
    // overlay direction for hover / pressed modulation.
    let on_surface = theme.resolve(ColorRole::OnSurface);

    // R1574 — the track, the knob, their M3 role mapping, the state-layer
    // overlay, the knob's justification, the tag, the focus stop and the
    // accessible name are all `pinion_widget_paint::switch`'s now. This binding
    // had every one of them inline, and so did eleven others; R1570.1 measured
    // the cost when it had to repeat `.with_focusable(true)` in ten of them.
    let track = view_switch(
        "main_toggle",
        state,
        on,
        &theme,
        &SwitchStyle::m3(),
        // The visible "Dark mode" caption is a SIBLING of the track (the row
        // puts the label beside the control), so the scene-walk name derivation
        // cannot reach it and the name has to be stated here.
        "Dark mode",
    );
    let label = Scene::Text(TextNode::styled(
        "Dark mode",
        Rect::default(),
        TextStyle::new()
            .with_size_px(LABEL_FONT_PX)
            .with_fg(on_surface),
    ));
    let status_str = format!("{} | {}", state.as_name(), if on { "On" } else { "Off" },);
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label, track, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// `WidgetView` binding for the Toggle widget. R653 §5.16 retrofit:
/// the [`#[widget]`](pinion_derive::widget) attribute below derives
/// the mechanical [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`]
/// shim using three R653-new substrate axes — `state_flags(checked =
/// bool_field(1))` extracts the on/off bit from the tuple's second
/// elem ([[r653-state-flags-bool-field]]), `access_value =
/// bool_field(1)` adds the `.with_value(AccessValue::Bool(state.1))`
/// chain the Switch role needs, and the `update` flag forwards the
/// R27 reducer that swaps the [`ThemeProvider`](pinion_core::theme::ThemeProvider) palette on the
/// `"toggle"` intent. The binding still owns the methods the macro
/// cannot derive: `view` (calls the free [`view`] fn), `read_state`
/// (tuple state reads two introspect fields per [[r645-tuple-state-state-flags]]
/// `WidgetStateName` covers only the enum half), `event_name`
/// (`ToggleEvent` has no `WidgetEventName` impl yet — SCE-002 carry),
/// `keybinding` / `apply_key` (custom ARIA dispatch), `update`
/// (theme palette swap), and `fmt_state_log` (custom format).
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
/// [`ThemeProvider`]: pinion_core::theme::ThemeProvider
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-toggle (R653 §5.16 #[widget] retrofit)",
    renderer = HelloToggleRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ToggleExternal::new,
    role = Switch,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
        checked = bool_field(1),
    ),
    access_value = bool_field(1),
    apply_key = aria_activate,
    keybinding,
    update,
    fmt_state_log,
)]
struct ToggleView;

impl ToggleView {
    /// Tuple-state introspect: pulls both the SCXML state name (via
    /// `query("state")`) and the Off/On sidecar (via `query("value")`).
    /// Defaults to `(Idle, false)` when either field is missing so a
    /// fresh External (zero introspect output) reads as Off-Idle.
    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    ToggleState::from_name_or_default(&name)
                } else {
                    ToggleState::Idle
                };
                let value = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
                return (state, value);
            }
        }
        (ToggleState::Idle, false)
    }

    /// R641 §5.16 inherent view shim. The macro emits the trait method
    /// as `<ToggleView>::view(state, *frame)` (deref'd from the
    /// trait's `&Frame` per [[widget-macro-by-value-bridge]]). This
    /// fn unpacks the tuple state and forwards to the free [`view`].
    fn view(state: (ToggleState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    fn event_name(event: ToggleEvent) -> &'static str {
        // R699 §5.16 — route the forward Event->name mapping through the
        // WidgetEventName SSOT (`as_name`), retiring the hand-written
        // match table. `as_name` is total over internal variants too;
        // only external events reach this path via `ShellCore::forward`.
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn keybinding(key: &str) -> Option<ToggleEvent> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }

    /// R57.X.toggle — reducer side-effect: on the [`Toggle`]'s
    /// `"toggle"` intent, swap the active [`ThemeProvider`](pinion_core::theme::ThemeProvider) palette to
    /// mirror the post-flip On/Off value so the demo's "Dark mode"
    /// label tracks the actual palette.
    ///
    /// **Authority source = `intent.payload`, not `state`.** The
    /// `"toggle"` intent fires from inside the Toggle SCXML's
    /// `Pressed -> Hover` transition; that transition is in flight
    /// while `update` runs, so [`read_state`] still observes the
    /// *pre*-flip `Off` value. The intent payload
    /// (`IntrospectValue::Bool(new_value)`) carries the canonical
    /// post-flip on/off bit, mirroring the W3C event-driven contract
    /// (event detail is authoritative for the action that produced
    /// the event). Same fix lives in `hello-theme` for the parity
    /// substrate demo.
    ///
    /// [`read_state`]: ToggleView::read_state
    /// [`ThemeProvider`]: pinion_core::theme::ThemeProvider
    /// [`Toggle`]: pinion_core::widgets::toggle::Toggle
    fn update(_state: (ToggleState, bool), intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == TOGGLE_INTENT_TAG_FULL {
            if let IntrospectValue::Bool(on) = intent.payload {
                let provider = use_theme(THEME_TAG);
                provider.set_mode(if on {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                });
            }
        }
        Vec::new()
    }

    fn fmt_state_log(state: (ToggleState, bool)) -> String {
        format!(
            "{} / {}",
            state.0.as_name(),
            if state.1 { "On" } else { "Off" },
        )
    }
}

fn main() {
    pinion_shell::run::<ToggleView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;
    use pinion_core::Owner;

    /// R51.69 §5.40 — pipeline mirror (`view` + `access_node` +
    /// `enrich_names_from_scene`) so name assertions read what the
    /// AT client sees, not the pre-enrichment intermediate.
    ///
    /// R57.X.toggle — wraps the `view` call in a fresh [`Owner::run`]
    /// scope so the inner [`use_theme`] resolves under the canonical
    /// root-owner discipline ([[callback-root-owner-wrap]]).
    fn enriched(state: (ToggleState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (s, on) = state;
        let owner = Owner::new();
        let scene = owner.run(|| view(s, on, &Frame::new()));
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

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        // R55.G.20 §5.49 — paint scene must carry the composite
        // `WidgetCore::tag()` so `scene/click` / `scene/key` /
        // `scene/wheel` `{path: "main_toggle"}` AI-side input routing
        // and `rect_for_tag` AT bounds attach resolve to the toggle.
        // hello-toggle places the tag on the inner track container;
        // this test pins the convention regardless of where it lives.
        //
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ToggleView>(
            (ToggleState::Idle, false),
            &Frame::default(),
        );
    }

    #[test]
    fn r57_x_toggle_track_fill_swaps_with_palette() {
        // R57.X.toggle exit criterion — the track fill in the Off
        // posture must equal the active palette's
        // `surface_container_highest` color. Calling
        // `set_mode(ThemeMode::Dark)` and re-rendering must produce a
        // scene whose track fill flipped to the dark palette's tone,
        // pinning the substrate cascade end-to-end (use_theme
        // subscription -> mode signal -> view reactive read -> paint
        // scene). R586 §5.50: the view reads through `theme_animated`
        // so the dark assertion needs the R57.X.theme-fade spring to
        // settle (`settle_owner_animations`) before the at-rest snap
        // returns the new palette exactly.
        let owner = Owner::new();
        owner.run(|| {
            // Force Light mode up front so the first scene resolves
            // to the light palette regardless of whichever
            // SystemColorScheme an earlier test on this thread left
            // behind (R57.1 thread-local signal isolation contract).
            use_theme(THEME_TAG).set_mode(ThemeMode::Light);
            let scene_light = view(ToggleState::Idle, false, &Frame::new());
            assert!(
                scene_contains_fill(&scene_light, Theme::light().surface_container_highest),
                "light Off track must fill with light surface_container_highest",
            );
            use_theme(THEME_TAG).set_mode(ThemeMode::Dark);
            // Trigger the spring re-target; in-flight value discarded.
            let _ = view(ToggleState::Idle, false, &Frame::new());
        });
        pinion_core::test_fixtures::settle_owner_animations(&owner);
        owner.run(|| {
            let scene_dark = view(ToggleState::Idle, false, &Frame::new());
            assert!(
                scene_contains_fill(&scene_dark, Theme::dark().surface_container_highest),
                "dark Off track must fill with dark surface_container_highest",
            );
        });
    }

    #[test]
    fn r57_x_toggle_on_state_fills_with_accent() {
        // The On posture must source its track fill from the active
        // palette's `accent` — pinning the (state, on) -> role
        // mapping decision so a refactor cannot silently revert to
        // an RGB literal.
        let owner = Owner::new();
        owner.run(|| {
            let scene = view(ToggleState::Idle, true, &Frame::new());
            assert!(
                scene_contains_fill(&scene, Theme::light().accent),
                "On track must fill with the active palette's accent",
            );
        });
    }

    /// Walks the scene tree looking for any node whose fill style
    /// matches `target`. Used by the R57.X.toggle theme cascade
    /// tests so they do not pin the exact tree shape — a future
    /// layout refactor of `view` does not produce a flaky failure.
    fn scene_contains_fill(scene: &Scene, target: pinion_core::Color) -> bool {
        match scene {
            Scene::Container(node) => {
                node.style.fill == target
                    || node.children.iter().any(|c| scene_contains_fill(c, target))
            }
            Scene::Box(node) => node.style.fill == target,
            _ => false,
        }
    }
}
