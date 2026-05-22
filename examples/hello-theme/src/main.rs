//! `hello-theme` — R57.0 §5.50 visible substrate demo.
//!
//! Wires the new [`pinion_core::theme`] substrate
//! ([`ThemeProvider`] / [`ColorRole`] / [`use_theme`]) end-to-end:
//!
//! - [`WidgetCore::view`] calls [`use_theme`] to resolve the active
//!   [`ThemeProvider`] from the root owner's typed cache slot and
//!   reads [`ThemeProvider::theme`] inside the view-fn so the
//!   reactive subscription captures palette swaps automatically.
//! - [`WidgetCore::update`] listens for the Toggle widget's
//!   `"toggle"` intent and calls [`ThemeProvider::set_theme`] with
//!   [`Theme::dark`] when the toggle flips on, [`Theme::light`] when
//!   it flips off. The signal write notifies the view's subscriber,
//!   the shell schedules a repaint, and the panel re-renders against
//!   the freshly-swapped palette.
//!
//! Every visible surface in the panel is sourced through a
//! [`ColorRole`] token, never from a hard-coded RGB literal — that
//! is the substrate exit criterion: the entire panel re-themes from
//! one `set_theme` call without any view-fn-internal branching on
//! the mode.
//!
//! Same shell entrypoint (`pinion_shell::run::<HelloThemeView>()`)
//! as every other hello-* example. The widget that drives the
//! palette swap is the existing Tier-1 `Toggle` (R51.29 paint-side
//! N=2); R57.0 is strictly the substrate slice and does not retro-
//! fit Toggle's own primitive paint colors — those land in a later
//! slice once every widget binding has been audited for theme-
//! awareness.

use pinion_core::external::{External, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{
    Command, ColorRole, Frame, Scene, Theme, ThemeMode, WidgetCore, use_theme,
};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};

// R46.5 codegen output — `HelloThemeRenderer` + matching error +
// async `new` + sync `render` / `resize`. Same pattern as every
// hello-* binary.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloThemeRenderer, HelloThemeRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 240;

const TRACK_W: u32 = 64;
const TRACK_H: u32 = 32;
const TRACK_RADIUS: u32 = 16;
const TRACK_PAD: u32 = 4;
const KNOB_SIZE: u32 = 24;
const KNOB_RADIUS: u32 = 12;
const ROW_GAP: u32 = 14;

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;
const ACCENT_LABEL_FONT_PX: u32 = 13;

const ACCENT_W: u32 = 160;
const ACCENT_H: u32 = 32;
const ACCENT_RADIUS: u32 = 8;
const ACCENT_PAD: u32 = 8;

const OUTLINE_W: u32 = 220;
const OUTLINE_H: u32 = 1;

/// Root owner cache key for this app's [`ThemeProvider`]. Exposed as
/// a `&'static str` so the `view` + `update` halves agree on the
/// same typed [`use_theme`] slot without repeating the literal.
const THEME_TAG: &str = "app";

/// Toggle widget's canonical paint-root tag (matches
/// [`HelloThemeView::tag`] below). Reused by the view fn for the
/// `.with_tag` attachment and by [`WidgetCore::read_state`] for the
/// `Scene::External` lookup.
const TOGGLE_TAG: &str = "theme_toggle";

/// Fully-prefixed wire tag for the `Toggle` widget's `"toggle"`
/// intent, built via `pinion_core::intent_tag!`. See that macro's
/// doc-comment for the §5.20 R22 wire-form contract — here we just
/// bind the compile-time concatenation result for `V::update` to
/// compare against.
const TOGGLE_INTENT_TAG_FULL: &str = pinion_core::intent_tag!("theme_toggle", "toggle");

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool, Frame) ->
/// Scene`. Reads the active palette via [`use_theme`] so the same
/// view-fn renders correctly under light + dark by virtue of the
/// reactive subscription, not by branching on `on`.
///
/// Visible affordances (top-to-bottom, theme-driven):
///
/// 1. **Title** — "Theme demo", 18 px, [`ColorRole::OnSurface`].
/// 2. **Mode toggle** — the Tier-1 `Toggle` widget. Tag matches
///    [`TOGGLE_TAG`] so the input router resolves middle-click and
///    pointer events to this node and forwards `"toggle"` intents
///    to `update` for the [`ThemeProvider::set_theme`] swap.
/// 3. **Status caption** — "Light mode" / "Dark mode", 12 px,
///    [`ColorRole::OnSurfaceMuted`].
/// 4. **Accent banner** — 160×32 rounded rect filled with
///    [`ColorRole::Accent`], inner label rendered with
///    [`ColorRole::OnAccent`] so the paired contrast remains
///    legible across both palettes (Material 3 paired-role
///    convention).
/// 5. **Outline divider** — 220×1 hairline filled with
///    [`ColorRole::Outline`]. Pure structural affordance — present
///    so the demo verifies the outline role swaps with the
///    palette.
///
/// The outer container fills with [`ColorRole::Surface`] so the
/// window background re-themes too.
///
/// (R57.X.theme-cleanup §5.50) Track Off uses
/// [`ColorRole::SurfaceContainerHighest`] (M3 canonical "filled
/// inactive container" surface) — the same role R57.X.toggle
/// introduced. Pre-cleanup the Off branch resolved to
/// [`ColorRole::Outline`], which is a stroke / hairline role; using
/// it as a fill made the Off track read as a thick border instead of
/// the M3 chip surface. Knob fills mirror the
/// [`hello-toggle`](../hello_toggle/index.html) Switch mapping
/// ([`ColorRole::Outline`] Off / [`ColorRole::OnAccent`] On) so the
/// two demo bindings share the same M3 Switch role pairing.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    // Reactive read — every paint subscribes the root owner to the
    // ThemeProvider's `palette` signal. The next `set_theme` flips
    // the surface for free, no view-fn branch on `on` required.
    let provider = use_theme(THEME_TAG);
    let theme = provider.theme();

    let title = Scene::Text(TextNode::styled(
        "Theme demo",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let knob_fill = match state {
        ToggleState::Disabled => theme.resolve(ColorRole::OnSurfaceMuted),
        _ if on => theme.resolve(ColorRole::OnAccent),
        _ => theme.resolve(ColorRole::Outline),
    };
    let track_fill = match (state, on) {
        (ToggleState::Idle | ToggleState::Hover | ToggleState::Pressed, false) => {
            theme.resolve(ColorRole::SurfaceContainerHighest)
        }
        (_, true) => theme.resolve(ColorRole::Accent),
        (ToggleState::Disabled, false) => theme.resolve(ColorRole::SurfaceContainerHighest),
    };
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
    let toggle = Scene::Container(
        ContainerNode::new(vec![knob])
            .with_tag(TOGGLE_TAG)
            .with_aria_label("Theme mode")
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

    let status_str = if on { "Dark mode" } else { "Light mode" };
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    let accent_banner = accent_banner(theme);
    let outline_divider = outline_divider(theme);

    Scene::Container(
        ContainerNode::new(vec![
            title,
            toggle,
            status,
            outline_divider,
            accent_banner,
        ])
        .with_style(
            BoxStyle::filled(theme.resolve(ColorRole::Surface))
                .with_border(Border::new(theme.resolve(ColorRole::Outline), 1)),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_gap(ROW_GAP),
        ),
    )
}

/// 160×32 rounded accent rect with a centered label, sourced through
/// the paired [`ColorRole::Accent`] / [`ColorRole::OnAccent`] roles
/// so the foreground / background contrast survives every palette
/// swap (Material 3 paired-role contract).
fn accent_banner(theme: Theme) -> Scene {
    let label = Scene::Text(TextNode::styled(
        "Accent role swatch",
        Rect::default(),
        TextStyle::new()
            .with_size_px(ACCENT_LABEL_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnAccent)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::Accent))
                    .with_corner_radius(ACCENT_RADIUS),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(ACCENT_W, ACCENT_H))
                    .with_padding(Rect::new(ACCENT_PAD, ACCENT_PAD, ACCENT_PAD, ACCENT_PAD)),
            ),
    )
}

/// 220×1 hairline filled with [`ColorRole::Outline`] — a pure
/// structural affordance that verifies the outline role swaps with
/// the palette across light / dark.
fn outline_divider(theme: Theme) -> Scene {
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(theme.resolve(ColorRole::Outline)),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(OUTLINE_W, OUTLINE_H))),
    )
}

/// `WidgetView` binding for the theme demo. State shape is the same
/// `(ToggleState, bool)` pair the Tier-1 `Toggle` widget exposes —
/// reuse keeps the focus / pointer / a11y pipelines unchanged.
struct HelloThemeView;

impl WidgetCore for HelloThemeView {
    type State = (ToggleState, bool);
    type Event = ToggleEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ToggleExternal::new())
    }

    fn tag() -> &'static str {
        TOGGLE_TAG
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
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-theme (R57.0 §5.50 ThemeProvider substrate)"
    }

    fn keybinding(key: &str) -> Option<ToggleEvent> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }

    /// R51.55 §5.39 — ARIA Toggle Button keyboard activation. Same
    /// Space / Enter handler the `Toggle` widget standardizes via
    /// `apply_aria_activate`; an activation here flips the value
    /// sidecar, fires the `"toggle"` intent, and `update` then
    /// swaps the palette.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    /// R57.0 §5.50 — reducer side-effect: on the [`Toggle`]'s
    /// `"toggle"` intent, swap the active [`ThemeProvider`] palette
    /// to mirror the post-flip on/off value.
    ///
    /// **Authority source = `intent.payload`, not `state`.** The
    /// `"toggle"` intent fires from inside the Toggle SCXML's
    /// `Pressed -> Hover` transition; that transition is in flight
    /// while [`Self::update`] runs, so [`Self::read_state`] still
    /// observes the *pre*-flip Off value. The intent payload
    /// (`IntrospectValue::Bool(new_value)`) carries the canonical
    /// post-flip on/off bit, mirroring the W3C event-driven contract
    /// (event detail is authoritative for the action that produced
    /// the event). The original R57.0 land routed through `state.1`,
    /// which gated `set_theme(Theme::dark())` on a value the scene
    /// did not yet hold — the visible defect surfaced by RPC
    /// snapshotting after a click; this rework is the substrate fix.
    ///
    /// `update` runs inside the shell's `root_owner.run` wrap
    /// ([[callback-root-owner-wrap]] +
    /// [[callback-root-owner-wrap-create-access]]) so [`use_theme`]
    /// resolves the same `Rc<ThemeProvider>` the view-fn already
    /// resolved (typed cache slot keyed by [`THEME_TAG`]).
    ///
    /// Signal equality-skip (`Signal::set` short-circuits when the
    /// new value equals the current) covers the
    /// [[reducer-incoming-vs-drain-symmetry]] double-fire: incoming
    /// dispatch and drain both hit this arm, and the second call
    /// against the now-active palette is a no-op rather than a
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

impl WidgetA11y for HelloThemeView {
    /// R51.69 §5.40 — single `AriaRole::Switch` node (toggle button
    /// per WAI-ARIA), exposing the `(ToggleState, bool)` joint
    /// projection. `value` carries the on/off bool so AT clients
    /// reading the value see the same boolean `state.checked`
    /// mirrors. The accessible name "Theme mode" is set on the
    /// track container's `aria_label` override in `view` so the
    /// literal lives in exactly one place.
    fn access_node(state: &(ToggleState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, on) = (state.0, state.1);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, ToggleState::Disabled),
            hovered: matches!(interaction, ToggleState::Hover),
            pressed: matches!(interaction, ToggleState::Pressed),
            checked: Some(on),
        };
        vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Switch)
                .with_value(AccessValue::Bool(on))
                .with_state(access_state),
        ]
    }
}

impl WidgetView for HelloThemeView {
    type Renderer = HelloThemeRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }
}

fn parse_toggle_state(name: &str) -> ToggleState {
    match name {
        "Hover" => ToggleState::Hover,
        "Pressed" => ToggleState::Pressed,
        "Disabled" => ToggleState::Disabled,
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
    pinion_shell::run::<HelloThemeView>();
}

#[cfg(test)]
mod tests {
    //! R57.0 §5.50 — hello-theme visible-substrate convention test.
    //! Mirrors the `r55_g20_view_contains_composite_paint_root_tag`
    //! family: pins that the paint scene returned by `view` carries
    //! the [`HelloThemeView::tag`] somewhere, so the input router
    //! `scene/click` / `scene/key` `{path: V::tag()}` routing
    //! resolves to this binding.

    use super::*;
    use pinion_core::test_fixtures::assert_widget_view_carries_tag;
    use pinion_core::Owner;

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        // R55.G.20 §5.49 carry — the paint scene must carry the
        // composite WidgetCore::tag() somewhere so AI-side
        // {path: V::tag()} input routing resolves to this binding.
        // The framework helper wraps the call in a fresh Owner so
        // the inner use_theme resolves under the canonical
        // root_owner.run discipline.
        assert_widget_view_carries_tag::<HelloThemeView>(
            (ToggleState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn r57_0_view_swaps_surface_color_when_on_flips_via_set_mode() {
        // The substrate exit criterion: a `set_mode` call against
        // the use_theme provider must result in a view scene whose
        // root container fills with the swapped palette's surface
        // color. The view-fn itself does no branching on `on` for
        // colors; the swap rides the reactive subscription.
        let owner = Owner::new();
        owner.run(|| {
            // Force Light mode first — protects against an earlier
            // test on this thread leaving SystemColorScheme=Dark in
            // the thread-local global (R57.1 isolation contract).
            use_theme(THEME_TAG).set_mode(ThemeMode::Light);
            let scene_light = view(ToggleState::Idle, false, &Frame::new());
            assert!(scene_contains_surface(&scene_light, Theme::light().surface));
            // Simulate the `update` reducer's side-effect: the
            // toggle just flipped to `on = true`, so mode swaps to
            // Dark.
            use_theme(THEME_TAG).set_mode(ThemeMode::Dark);
            let scene_dark = view(ToggleState::Idle, true, &Frame::new());
            assert!(scene_contains_surface(&scene_dark, Theme::dark().surface));
        });
    }

    /// Walks the scene tree looking for a node whose fill style
    /// matches `target`. Returns `true` if any node matches — the
    /// test does not pin the exact tree shape so a future layout
    /// refactor of `view` does not produce a flaky failure.
    fn scene_contains_surface(scene: &Scene, target: pinion_core::Color) -> bool {
        match scene {
            Scene::Container(node) => {
                node.style.fill == target
                    || node.children.iter().any(|c| scene_contains_surface(c, target))
            }
            Scene::Box(node) => node.style.fill == target,
            _ => false,
        }
    }

    /// (R57.X.theme-cleanup §5.50) Track Off must resolve to
    /// `ColorRole::SurfaceContainerHighest` — the M3 "filled inactive
    /// container" role — not `ColorRole::Outline` (a stroke role).
    /// Pin both light and dark palettes so a regression that swaps
    /// the role back is caught at test time rather than visible
    /// inspection.
    #[test]
    fn r57_x_theme_cleanup_track_off_uses_surface_container_highest() {
        let owner = Owner::new();
        owner.run(|| {
            // Light mode: track Off fill == light.surface_container_highest.
            use_theme(THEME_TAG).set_mode(ThemeMode::Light);
            let scene_light = view(ToggleState::Idle, false, &Frame::new());
            assert!(scene_contains_surface(
                &scene_light,
                Theme::light().surface_container_highest,
            ));
            // Dark mode: track Off fill swaps with palette.
            use_theme(THEME_TAG).set_mode(ThemeMode::Dark);
            let scene_dark = view(ToggleState::Idle, false, &Frame::new());
            assert!(scene_contains_surface(
                &scene_dark,
                Theme::dark().surface_container_highest,
            ));
        });
    }
}
