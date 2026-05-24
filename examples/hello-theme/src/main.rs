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

use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{
    Command, ColorRole, Frame, Scene, Theme, ThemeMode, WidgetCore, use_theme,
};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

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
// R596 §5.50 — Tag for the "Press R" affordance hint so the AT-side
// + RPC snapshot can locate the label without scanning text bodies.
const HINT_TAG: &str = "palette_cycle_hint";
const HINT_FONT_PX: u32 = 11;

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

// ────────────────────────────────────────────────────────────────────
// R594 §5.50 — Material 3 dynamic-color palette pair cycle
// ────────────────────────────────────────────────────────────────────
//
// First consumer of the R593 `ThemeProvider::set_palettes` atomic
// batch primitive. Cycles the active light + dark palette pair
// through three M3 accent tonal seeds (Blue / Green / Magenta) on
// each "r" / "R" key activation. Both signals mutate inside a single
// `batch`, so every downstream subscriber (this binding's view fn +
// the at-rest snap path in `theme_animated`) re-runs at most once
// per cycle.

// Three accent-seed positions the demo cycles through. Mirrors the
// Material 3 "Generate from key color" gallery — Blue is the
// baseline (`Theme::light()` / `Theme::dark()` already ship with
// it), Green + Magenta override only the `accent` field of each
// palette while leaving every other role on the canonical Material 3
// baseline.
thread_local! {
    static PALETTE_SEED: std::cell::Cell<u8> =
        const { std::cell::Cell::new(0) };
}

/// Compose the light + dark palette pair for the given seed index.
/// Seed `0` is the canonical Material 3 Blue baseline (unmodified).
/// Seed `1` is Material Green 800 / 200, seed `2` is Material Pink
/// 700 / 300 — both tonal pairs paired so the dark accent is lifted
/// for legibility against `#121212`, mirroring the Blue baseline.
fn palette_pair_for_seed(seed: u8) -> (Theme, Theme) {
    let mut light = Theme::light();
    let mut dark = Theme::dark();
    match seed % 3 {
        0 => {
            // Blue baseline — no tweak; both palettes ship M3 Blue.
        }
        1 => {
            light.accent = pinion_core::style::Color::rgb(0x2e, 0x7d, 0x32);
            dark.accent = pinion_core::style::Color::rgb(0x81, 0xc7, 0x84);
        }
        _ => {
            light.accent = pinion_core::style::Color::rgb(0xc2, 0x18, 0x5b);
            dark.accent = pinion_core::style::Color::rgb(0xec, 0x40, 0x7a);
        }
    }
    (light, dark)
}

/// Advance the seed by one, then drive the active provider through a
/// single `set_palettes` call so both light + dark signals mutate in
/// the same reactive batch.
///
/// **Owner contract**: must be called inside a scope where
/// [`pinion_core::Owner::current()`] is `Some` — `apply_key` already
/// runs inside the shell's `root_owner.run` wrap per
/// [[callback-root-owner-wrap]], so the existing call site is safe.
/// Tests exercise the helper directly inside an explicit
/// `owner.run(|| {...})` to honor the same contract.
fn cycle_palette_pair() {
    let next = PALETTE_SEED.with(|c| {
        let n = c.get().wrapping_add(1) % 3;
        c.set(n);
        n
    });
    let (light, dark) = palette_pair_for_seed(next);
    use_theme(THEME_TAG).set_palettes(light, dark);
}

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
    // (R586 §5.50) Animated palette — opts in to R57.X.theme-fade
    // cross-fade so the OS dark-mode toggle visually transitions over
    // ~200 ms instead of snapping; at-rest snap returns the exact
    // palette so widget-cascade tests stay exact-equality friendly.
    let theme = provider.theme_animated();

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
    // R596 §5.50 — discoverability affordance for the R594 'r' / 'R'
    // shortcut. Without the hint a user has no way to discover the
    // palette-cycle action; the label sits below the accent banner so
    // the visual flow reads top → bottom as "label, toggle, mode,
    // divider, accent sample, secondary affordance". `HINT_TAG`
    // exposes the label through scene/snapshot for the AT-side and
    // for regression tests that pin discoverability.
    let hint = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            "Press R to cycle palette",
            Rect::default(),
            TextStyle::new()
                .with_size_px(HINT_FONT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(HINT_TAG),
    );

    Scene::Container(
        ContainerNode::new(vec![
            title,
            toggle,
            status,
            outline_divider,
            accent_banner,
            hint,
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

/// R654 §5.16 Cat A cascade retrofit. Same `(ToggleState, bool)`
/// shape + AriaRole::Switch + `update` (theme palette swap via
/// intent.payload authority per [[intent-payload-post-flip-authority]])
/// as hello-toggle (R653 first consumer). hello-theme adds the
/// R594 `r`/`R` palette-pair cycle escape that the macro cannot
/// derive, so the inherent `apply_key` carries the extra branch
/// before forwarding to `apply_aria_activate`.
#[widget(
    // Literal must match `const TOGGLE_TAG` above and the
    // `.with_tag(TOGGLE_TAG)` call in `view`. The `#[widget]` macro
    // accepts either `tag = "literal"` (R641) or `tag = Path` to a
    // `WidgetTag` derive variant (R644); a bare `const` path is not
    // either form per [[r650-widget-tag-walk-back]], so the literal
    // lives here and the `const` keeps the duplicate-prevention
    // pin elsewhere.
    tag = "theme_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-theme (R654 §5.16 #[widget] retrofit)",
    renderer = HelloThemeRenderer,
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
    apply_key,
    keybinding,
    update,
    fmt_state_log,
)]
struct HelloThemeView;

impl HelloThemeView {
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

    fn view(state: (ToggleState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
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
    ///
    /// (R594 §5.50) Extra shortcut `"r"` — first consumer of the
    /// R593 [`ThemeProvider::set_palettes`] atomic batch primitive.
    /// Cycles the light + dark palette pair through three M3 dynamic-
    /// color seeds (Blue / Green / Magenta) so the demo can show how
    /// a single user action atomically retones both sides without
    /// driving the mode toggle. The active mode stays where the
    /// Toggle put it; only the *palette pair* swaps. Pinned by
    /// `r594_apply_key_r_cycles_palette_pair_through_set_palettes`.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if matches!(key, "r" | "R") && focused == Some(Self::tag()) {
            cycle_palette_pair();
            return true;
        }
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    /// R57.0 §5.50 — reducer side-effect: on the [`Toggle`]'s
    /// `"toggle"` intent, swap the active [`ThemeProvider`] palette
    /// to mirror the post-flip on/off value.
    ///
    /// **Authority source = `intent.payload`, not `state`.** The
    /// `"toggle"` intent fires from inside the Toggle SCXML's
    /// `Pressed -> Hover` transition; `read_state` lags by one
    /// reducer tick (per [[intent-payload-post-flip-authority]]).
    /// `update` runs inside the shell's `root_owner.run` wrap
    /// ([[callback-root-owner-wrap]]) so [`use_theme`] resolves the
    /// same `Rc<ThemeProvider>` the view-fn already resolved.
    /// Signal equality-skip (per [[reducer-incoming-vs-drain-symmetry]])
    /// makes the second call a no-op.
    fn update(_state: (ToggleState, bool), intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == TOGGLE_INTENT_TAG_FULL {
            if let IntrospectValue::Bool(on) = intent.payload {
                let provider = use_theme(THEME_TAG);
                provider.set_mode(if on { ThemeMode::Dark } else { ThemeMode::Light });
            }
        }
        Vec::new()
    }

    fn fmt_state_log(state: (ToggleState, bool)) -> String {
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
        // colors; the swap rides the reactive subscription. R586
        // §5.50: the view-fn reads through `theme_animated`, so the
        // dark scene assertion is gated on the R57.X.theme-fade
        // spring settling past the M3 short4 window via
        // `settle_owner_animations`.
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
            // Trigger the spring re-target; the in-flight value is
            // intentionally discarded.
            let _ = view(ToggleState::Idle, true, &Frame::new());
        });
        pinion_core::test_fixtures::settle_owner_animations(&owner);
        owner.run(|| {
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
    /// (R596 §5.50) The "Press R to cycle palette" affordance hint
    /// must land in the painted scene so users can discover the R594
    /// keyboard shortcut. Pinned by tag lookup — a future refactor
    /// that drops the hint label silently regresses discoverability.
    #[test]
    fn r596_view_emits_palette_cycle_hint_tagged_for_discoverability() {
        let owner = Owner::new();
        owner.run(|| {
            let scene = view(ToggleState::Idle, false, &Frame::new());
            assert!(scene.contains_tag(HINT_TAG),
                "scene must carry the palette_cycle_hint tag for AT + RPC discovery");
        });
    }

    /// (R594 §5.50) `cycle_palette_pair` is the demo-side consumer of
    /// the R593 atomic batch primitive — it must advance the
    /// `PALETTE_SEED` *and* publish a fresh `(light, dark)` pair through
    /// `ThemeProvider::set_palettes`. Pinning the cycle order here
    /// guards against a future refactor that accidentally drops one
    /// of the two signal writes (which would make `set_palettes` look
    /// equivalent to `set_light_palette` to the test surface).
    #[test]
    fn r594_cycle_palette_pair_advances_seed_and_publishes_both_signals() {
        // Reset the thread-local so a sibling test that ran earlier
        // cannot smear the seed forward.
        PALETTE_SEED.with(|c| c.set(0));
        let owner = Owner::new();
        owner.run(|| {
            let provider = use_theme(THEME_TAG);
            // Seed 0 -> seed 1: Green tonal pair.
            cycle_palette_pair();
            assert_eq!(provider.light_palette().accent,
                pinion_core::style::Color::rgb(0x2e, 0x7d, 0x32),
                "light accent advanced to Green");
            assert_eq!(provider.dark_palette().accent,
                pinion_core::style::Color::rgb(0x81, 0xc7, 0x84),
                "dark accent advanced to Green (lifted tone)");
            // Seed 1 -> seed 2: Magenta tonal pair.
            cycle_palette_pair();
            assert_eq!(provider.light_palette().accent,
                pinion_core::style::Color::rgb(0xc2, 0x18, 0x5b));
            assert_eq!(provider.dark_palette().accent,
                pinion_core::style::Color::rgb(0xec, 0x40, 0x7a));
            // Seed 2 -> seed 0: baseline Blue (Theme::light/dark defaults).
            cycle_palette_pair();
            assert_eq!(provider.light_palette().accent, Theme::light().accent);
            assert_eq!(provider.dark_palette().accent, Theme::dark().accent);
        });
    }

    /// (R594 §5.50) `apply_key` routes the demo-only `"r"` shortcut
    /// to [`cycle_palette_pair`] when the toggle tag holds focus.
    /// Together with the cycle-helper regression above, this pins the
    /// end-to-end key->batch wiring without requiring the visual
    /// shell loop to be running.
    #[test]
    fn r594_apply_key_r_cycles_palette_pair_when_toggle_focused() {
        PALETTE_SEED.with(|c| c.set(0));
        let owner = Owner::new();
        owner.run(|| {
            let provider = use_theme(THEME_TAG);
            let mut scene = view(ToggleState::Idle, false, &Frame::new());
            let consumed = <HelloThemeView as WidgetCore>::apply_key(
                &mut scene,
                Some(<HelloThemeView as WidgetCore>::tag()),
                "r",
                pinion_core::Modifiers::default(),
            );
            assert!(consumed, "apply_key returns true for the 'r' shortcut");
            // Cycle landed: light accent moved off Blue.
            assert_ne!(provider.light_palette().accent, Theme::light().accent);
            // Unfocused 'r' must NOT consume — guards against
            // background-tab key leaks. Reset seed before re-firing
            // so the un-focus check exercises the gate, not state.
            PALETTE_SEED.with(|c| c.set(0));
            provider.set_palettes(Theme::light(), Theme::dark());
            let consumed_blurred = <HelloThemeView as WidgetCore>::apply_key(
                &mut scene,
                None,
                "r",
                pinion_core::Modifiers::default(),
            );
            assert!(!consumed_blurred, "blurred 'r' must not consume");
            assert_eq!(provider.light_palette().accent, Theme::light().accent,
                "blurred 'r' must not mutate the palette");
        });
    }

    #[test]
    fn r57_x_theme_cleanup_track_off_uses_surface_container_highest() {
        // R586 §5.50: same settle structure as the surface-swap test
        // above — the view-fn reads through `theme_animated` so the
        // dark assertion needs the R57.X.theme-fade spring to settle
        // before the at-rest snap returns the new palette exactly.
        let owner = Owner::new();
        owner.run(|| {
            // Light mode: track Off fill == light.surface_container_highest.
            use_theme(THEME_TAG).set_mode(ThemeMode::Light);
            let scene_light = view(ToggleState::Idle, false, &Frame::new());
            assert!(scene_contains_surface(
                &scene_light,
                Theme::light().surface_container_highest,
            ));
            // Flip to Dark + trigger the spring re-target.
            use_theme(THEME_TAG).set_mode(ThemeMode::Dark);
            let _ = view(ToggleState::Idle, false, &Frame::new());
        });
        pinion_core::test_fixtures::settle_owner_animations(&owner);
        owner.run(|| {
            let scene_dark = view(ToggleState::Idle, false, &Frame::new());
            assert!(scene_contains_surface(
                &scene_dark,
                Theme::dark().surface_container_highest,
            ));
        });
    }
}
