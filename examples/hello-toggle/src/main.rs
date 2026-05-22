//! `hello-toggle` — §5.38 paint-side N=2 (R51.29) + R51.30 pinion-shell
//! migration, R57.X.toggle theme retrofit.
//!
//! Same architecture as hello-button (R17 bidirectional RPC live
//! dogfood), differing only in:
//!
//! * the cached state shape is `(ToggleState, bool)` — interaction
//!   state plus the Off/On value sidecar [`Toggle::is_on`];
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
//! Off/On sidecar drives a [`ThemeProvider::set_theme`] swap so the
//! demo's "Dark mode" label becomes semantically accurate — On really
//! is dark mode. The Material 3 Switch role mapping is the canonical
//! source:
//!
//! - Track Off — [`ColorRole::SurfaceContainerHighest`] (M3 chip surface).
//! - Track On — [`ColorRole::Accent`] (M3 primary).
//! - Thumb Off — [`ColorRole::Outline`] (M3 outline).
//! - Thumb On — [`ColorRole::OnAccent`] (M3 onPrimary).
//! - Hover / Pressed — [`Color::lerp`] toward
//!   [`ColorRole::OnSurface`] at the M3 state-layer overlay weights
//!   (0.08 hover, 0.12 pressed), in linear sRGB space per
//!   [[color-lerp-linear-space]] (R51.151).
//!
//! Every other framework primitive (App lifecycle, `RenderState`,
//! `dispatch_rpc`, stdin RPC reader, `InputRouter` wiring, intent
//! draining, paint loop) lives in [`pinion_shell`] — see that
//! crate's docs for the substrate-incompleteness-signal lesson that
//! produced this refactor (R51.30 immediate response to R51.29
//! N=2 evidence).

use pinion_core::external::{External, IntrospectValue};
// Borrowed in the V::update reducer to read the post-flip authority
// out of the Toggle intent's payload — see the body comment for why
// the intent payload (not V::read_state) is the canonical source.
use pinion_core::intent::Intent;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{Color, ColorRole, Command, Frame, Scene, ThemeMode, WidgetCore, use_theme};
#[cfg(test)]
use pinion_core::Theme;
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
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

const LABEL_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

/// Material 3 state-layer overlay weights (linear-sRGB lerp toward
/// [`ColorRole::OnSurface`]). M3 specifies 8 % for hover and 12 % for
/// pressed; pinion's [`Color::lerp`] is the §5.27 linear-space path
/// so the result matches the M3 perceptual intent without a separate
/// alpha-compositing pass.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
/// Disabled treatment — fade the role color toward [`ColorRole::Surface`]
/// by half. M3 specifies a 12 % opacity disabled overlay; without an
/// alpha pipeline yet, a midpoint lerp toward the surface produces a
/// comparable "washed out" tone in both palettes.
const DISABLED_OVERLAY_T: f32 = 0.50;

/// Root-owner cache key for this binary's [`ThemeProvider`]. Shared
/// between the [`view`] fn (reactive read) and
/// [`ToggleView::update`] (palette swap on `"toggle"` intent) so both
/// halves resolve the same typed [`use_theme`] slot without repeating
/// the literal.
const THEME_TAG: &str = "app";

/// Fully-prefixed wire tag for the [`Toggle`] widget's `"toggle"`
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
/// next [`ThemeProvider::set_theme`] re-runs the view without any
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
    // ThemeProvider's `palette` signal. The next `set_theme` flips
    // the surface for free.
    let theme = use_theme(THEME_TAG).theme();
    // Cache the OnSurface role once — it both renders the title /
    // status-line foregrounds and serves as the M3 state-layer
    // overlay direction for hover / pressed modulation.
    let on_surface = theme.resolve(ColorRole::OnSurface);

    // Track fill — encodes the (state, value) cross product through
    // the M3 Switch role mapping. Off sits on the inactive container
    // surface; On sits on the accent. Hover / Pressed lerp toward the
    // on-surface overlay color at the M3 state-layer weights, in
    // linear sRGB space per [[color-lerp-linear-space]] (R51.151).
    // Disabled fades the base role half-way toward Surface so the
    // chip reads as washed-out in both palettes.
    let track_base = if on {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    };
    let track_fill: Color = match state {
        ToggleState::Idle => track_base,
        ToggleState::Hover => track_base.lerp(on_surface, HOVER_OVERLAY_T),
        ToggleState::Pressed => track_base.lerp(on_surface, PRESSED_OVERLAY_T),
        ToggleState::Disabled => {
            track_base.lerp(theme.resolve(ColorRole::Surface), DISABLED_OVERLAY_T)
        }
    };
    // Knob fill — M3 Switch thumb mapping: outline (Off) / on-accent
    // (On); disabled fades the base toward the muted on-surface so
    // the thumb reads as inactive without disappearing.
    let knob_base = if on {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::Outline)
    };
    let knob_fill: Color = if matches!(state, ToggleState::Disabled) {
        knob_base.lerp(theme.resolve(ColorRole::OnSurfaceMuted), DISABLED_OVERLAY_T)
    } else {
        knob_base
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
            .with_size_px(LABEL_FONT_PX)
            .with_fg(on_surface),
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

/// `WidgetView` binding for the Toggle widget. The state shape is
/// the joint `(ToggleState, bool)` pair — interaction state + the
/// Off/On value sidecar [`Toggle::is_on`].
struct ToggleView;

impl WidgetCore for ToggleView {
    type State = (ToggleState, bool);
    type Event = ToggleEvent;

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
        "pinion hello-toggle (R57.X.toggle §5.50 theme retrofit)"
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
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    /// R57.X.toggle — reducer side-effect: on the [`Toggle`]'s
    /// `"toggle"` intent, swap the active [`ThemeProvider`] palette to
    /// mirror the post-flip On/Off value so the demo's "Dark mode"
    /// label tracks the actual palette.
    ///
    /// **Authority source = `intent.payload`, not `state`.** The
    /// `"toggle"` intent fires from inside the Toggle SCXML's
    /// `Pressed -> Hover` transition; that transition is in flight
    /// while [`Self::update`] runs, so [`Self::read_state`] still
    /// observes the *pre*-flip `Off` value. The intent payload
    /// (`IntrospectValue::Bool(new_value)`) carries the canonical
    /// post-flip on/off bit, mirroring the W3C event-driven contract
    /// (event detail is authoritative for the action that produced
    /// the event). Same fix lives in `hello-theme` for the parity
    /// substrate demo.
    ///
    /// Owner-current discipline: the shell wraps this call in
    /// `root_owner.run` ([[callback-root-owner-wrap]]) so
    /// [`use_theme`] resolves the same [`ThemeProvider`] Rc the
    /// view-fn subscribes to — the typed [`Owner::cache`] slot keyed
    /// by [`THEME_TAG`] is shared across both halves.
    ///
    /// Signal equality-skip covers the
    /// [[reducer-incoming-vs-drain-symmetry]] double-fire: a second
    /// call against the now-active palette is a no-op rather than a
    /// double-paint.
    fn update(_state: (ToggleState, bool), intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == TOGGLE_INTENT_TAG_FULL {
            if let IntrospectValue::Bool(on) = intent.payload {
                let provider = use_theme(THEME_TAG);
                provider.set_mode(if on { ThemeMode::Dark } else { ThemeMode::Light });
            }
        }
        Vec::new()
    }

    fn fmt_state_log(state: &(ToggleState, bool)) -> String {
        format!(
            "{} / {}",
            toggle_state_name(state.0),
            if state.1 { "On" } else { "Off" },
        )
    }
}

impl WidgetA11y for ToggleView {
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
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, ToggleState::Disabled),
            hovered: matches!(interaction, ToggleState::Hover),
            pressed: matches!(interaction, ToggleState::Pressed),
            checked: Some(on),
        };
        vec![AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Switch)
            .with_value(AccessValue::Bool(on))
            .with_state(access_state)]
    }
}

impl WidgetView for ToggleView {
    type Renderer = HelloToggleRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
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
        // scene).
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
    fn scene_contains_fill(scene: &Scene, target: Color) -> bool {
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
