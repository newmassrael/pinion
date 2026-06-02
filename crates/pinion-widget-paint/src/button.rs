//! R686.B §5.16 — backend-agnostic M3 filled-button paint substrate.
//!
//! ## Role
//!
//! The button **state machine** (`{Idle, Hover, Pressed, Disabled}`
//! SCXML + activate wire) already lives in
//! [`pinion_core::widgets::button`] (`ButtonState` / `ButtonExternal`
//! / `ButtonEvent`). What this module adds is the **paint side**: the
//! Material 3 state-layer fill matrix + the
//! [`Scene`](pinion_core::Scene) composition every button consumer
//! re-implemented inline before R686.B.
//!
//! Pre-R686.B `hello-button`, `figma-button-m3`, and
//! `hello-dock-panels-editor` each carried a near-identical
//! `button_fill_*` helper + button Container builder. The
//! [[abstraction-needs-second-consumer]] Rule-of-Three was satisfied
//! (3 paint consumers), and a canonical M3 button is a core Phase-B
//! widget-catalog entry (Qt / Flutter / Compose / React all ship
//! one), so the paint lifts here.
//!
//! ## The genuine shared kernel vs the variance
//!
//! A close read of the three consumers showed they share **only** the
//! M3 state-layer overlay matrix (the 0.08 / 0.12 / 0.38 coefficients
//! plus the [`Color::lerp`] composition) and the centered
//! text-in-a-box shape. They diverge on:
//!
//! * **Colour source** — `hello-button` + the editor resolve from
//!   [`Theme`] roles; `figma-button-m3` uses hard-coded design tokens
//!   (a deliberate Figma-parity demo).
//! * **Disabled appearance** — some fade toward a background colour;
//!   the editor switches to a different surface tier.
//! * **Hover animation** — `hello-button` + figma drive a spring;
//!   the editor is discrete.
//! * **Geometry** — corner radius, padding, size, label font all
//!   differ per consumer.
//!
//! Following the [`crate::splitter`] / [`crate::dock`] precedent, all
//! variance becomes **data**: [`ButtonColors`] (a value object the
//! caller fills from theme roles *or* explicit tokens), [`ButtonStyle`]
//! (the geometry sidecar), and a `hover_progress: f32` argument the
//! caller drives (mirror of [`view_splitter`](crate::splitter::view_splitter)'s
//! `dragging: bool`). A discrete consumer passes `1.0` on hover /
//! `0.0` otherwise and gets the same output a spring would settle to.
//! Structure is shared; nothing is forced.
//!
//! ## Dep graph
//!
//! Pure [`Scene`](pinion_core::Scene) composition over
//! [`pinion_core::widgets::button::ButtonState`]; no `pinion-text`,
//! no Vello / winit coupling. Sits beside [`crate::splitter`] /
//! [`crate::checkbox`].

use std::borrow::Cow;
use std::rc::Rc;

use pinion_core::animation::{Animation, SpringConfig};
use pinion_core::reactive::Owner;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::button::ButtonState;
use pinion_core::external::IntrospectValue;
use pinion_core::WidgetStateName;
use pinion_a11y::AccessState;

// (R686.B → R752 §5.50) The M3 state-layer opacity tokens now live once in
// the cross-cutting [`crate::state_layer`] SSOT; re-exported here under the
// historical `*_STATE_LAYER` names so `button`'s role-specific overlay (it
// tints toward [`ButtonColors::state_layer`], not `OnSurface`, so it keeps
// its own arms) and existing `button::HOVER_STATE_LAYER` paths still resolve
// against the single definition.
pub use crate::state_layer::{
    DISABLED as DISABLED_STATE_LAYER, HOVER as HOVER_STATE_LAYER, PRESSED as PRESSED_STATE_LAYER,
};

/// (R694 §5.16 §5.39) Keyboard-focus indicator ring width in logical
/// pixels. Material 3's focus indicator is a 3-dp outline; pinion paints
/// it as a [`Border`] on the button's outer container (drawn only while
/// the widget holds the shell [`FocusManager`](pinion_runtime::FocusManager)
/// focus). M3's 2-dp *offset gap* between the component edge and the ring
/// needs a border-offset primitive pinion does not have yet, so the ring
/// sits on the edge — a visible keyboard-focus affordance, with the
/// offset gap deferred as an additive axis.
pub const FOCUS_RING_WIDTH: u32 = 3;

/// (R686.B §5.16) Per-state colour value-object for a filled button.
///
/// Absorbs the colour-source variance between consumers: theme-role
/// callers use [`Self::filled_tonal`] / [`Self::accent`]; hard-coded
/// design-token callers (Figma parity) use [`Self::new`]. The
/// [`m3_button_fill`] matrix reads `base` + `state_layer` for the
/// Idle/Hover/Pressed overlay lerp and `fill_disabled` verbatim for
/// the disabled state; [`view_button`] reads `label` / `label_disabled`
/// for the text colour.
///
/// `#[non_exhaustive]` so future axes (an outline / elevated button's
/// border + shadow tokens) land via additional constructors without
/// breaking the struct surface.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonColors {
    /// Resting fill (the `Idle` colour + the lerp origin for the
    /// Hover / Pressed state layers).
    pub base: Color,
    /// State-layer overlay colour — the Hover / Pressed overlays lerp
    /// from `base` toward this (M3 convention: the `on*` role of the
    /// base surface, e.g. `OnSurface` over `SurfaceContainerHighest`).
    pub state_layer: Color,
    /// Disabled fill, used verbatim. Callers that want the M3
    /// "fade toward a background" look pass
    /// `base.lerp(bg, DISABLED_STATE_LAYER)`; callers that switch to a
    /// distinct surface tier pass that tier's resolved colour.
    pub fill_disabled: Color,
    /// Label text colour for the enabled states (Idle / Hover /
    /// Pressed).
    pub label: Color,
    /// Label text colour when `Disabled`.
    pub label_disabled: Color,
    /// (R694 §5.16 §5.39) Keyboard-focus ring colour — the [`Border`]
    /// [`view_button`] paints while the button holds shell focus. M3
    /// uses `primary` ([`ColorRole::Accent`], documented as the
    /// focus-ring role) for neutral / tonal buttons; an accent-filled
    /// button rings in [`ColorRole::OnAccent`] so the indicator
    /// contrasts against its own fill.
    pub focus_ring: Color,
}

impl ButtonColors {
    /// (R686.B §5.16) Explicit colours — the hard-coded-token path
    /// (`figma-button-m3` design-parity demo). Theme-role callers
    /// prefer [`Self::filled_tonal`] / [`Self::accent`].
    #[must_use]
    pub const fn new(
        base: Color,
        state_layer: Color,
        fill_disabled: Color,
        label: Color,
        label_disabled: Color,
        focus_ring: Color,
    ) -> Self {
        Self {
            base,
            state_layer,
            fill_disabled,
            label,
            label_disabled,
            focus_ring,
        }
    }

    /// (R686.B §5.16) M3 filled-tonal button colours resolved from a
    /// [`Theme`]: `base = SurfaceContainerHighest`,
    /// `state_layer = OnSurface`, disabled = `base` faded toward
    /// `Surface` by [`DISABLED_STATE_LAYER`], label `OnSurface`,
    /// disabled label `OnSurfaceMuted`. Matches `hello-button`.
    #[must_use]
    pub fn filled_tonal(theme: &Theme) -> Self {
        let base = theme.resolve(ColorRole::SurfaceContainerHighest);
        Self {
            base,
            state_layer: theme.resolve(ColorRole::OnSurface),
            fill_disabled: base.lerp(theme.resolve(ColorRole::Surface), DISABLED_STATE_LAYER),
            label: theme.resolve(ColorRole::OnSurface),
            label_disabled: theme.resolve(ColorRole::OnSurfaceMuted),
            focus_ring: theme.resolve(ColorRole::Accent),
        }
    }

    /// (R686.B §5.16) M3 accent-tinted button colours resolved from a
    /// [`Theme`]: `base = Accent`, `state_layer = OnAccent`, disabled
    /// switches to `SurfaceContainerHigh` (a distinct surface tier,
    /// not a fade), label `OnAccent`, disabled label `OnSurfaceMuted`.
    /// Matches `hello-dock-panels-editor`'s viewport button.
    #[must_use]
    pub fn accent(theme: &Theme) -> Self {
        Self {
            base: theme.resolve(ColorRole::Accent),
            state_layer: theme.resolve(ColorRole::OnAccent),
            fill_disabled: theme.resolve(ColorRole::SurfaceContainerHigh),
            label: theme.resolve(ColorRole::OnAccent),
            label_disabled: theme.resolve(ColorRole::OnSurfaceMuted),
            focus_ring: theme.resolve(ColorRole::OnAccent),
        }
    }
}

/// (R686.B §5.16) Resolve the filled-button fill for `state` via the
/// M3 state-layer overlay matrix.
///
/// * `Idle` / `Hover` — `base` lerped toward the Hover endpoint
///   (`base` → `state_layer` at [`HOVER_STATE_LAYER`]) by
///   `hover_progress`. A spring-driven consumer threads its animated
///   `0.0..=1.0` value; a discrete consumer passes `1.0` on Hover /
///   `0.0` otherwise and lands on the same endpoints.
/// * `Pressed` — `base` lerped toward `state_layer` at
///   [`PRESSED_STATE_LAYER`].
/// * `Disabled` — [`ButtonColors::fill_disabled`] verbatim.
///
/// All lerps run in linear-light space via [`Color::lerp`]
/// ([[color-lerp-linear-space]]), matching the §5.28 spring solver's
/// colour-space convention.
#[must_use]
pub fn m3_button_fill(colors: &ButtonColors, state: ButtonState, hover_progress: f32) -> Color {
    match state {
        ButtonState::Idle | ButtonState::Hover => {
            let hover_endpoint = colors.base.lerp(colors.state_layer, HOVER_STATE_LAYER);
            colors.base.lerp(hover_endpoint, hover_progress)
        }
        ButtonState::Pressed => colors.base.lerp(colors.state_layer, PRESSED_STATE_LAYER),
        ButtonState::Disabled => colors.fill_disabled,
    }
}

/// (R686.B §5.16) Geometry sidecar for [`view_button`].
///
/// `#[non_exhaustive]` so future axes (icon slot, elevation, ripple
/// origin) land via builders. [`Self::m3_default`] is a neutral
/// baseline (sharp corners, no padding, intrinsic size, 14-px label);
/// the *fill* is the M3-canonical part (via [`ButtonColors`] +
/// [`m3_button_fill`]), and each consumer tunes geometry through the
/// `with_*` builders.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ButtonStyle {
    /// Paint-side tag on the button's outer [`Scene::Container`] so
    /// the [`InputRouter`](pinion_runtime::InputRouter) deepest-tagged
    /// hit-test routes pointer events to the paired
    /// [`ButtonExternal`](pinion_core::widgets::button::ButtonExternal).
    pub tag: Cow<'static, str>,
    /// `BoxStyle` corner radius in logical pixels (`0` = sharp). A
    /// large value (≥ half the height) yields the M3 pill shape.
    pub corner_radius: u32,
    /// Inner padding (logical pixels) around the label.
    pub padding: Rect,
    /// Fixed button size, or `None` to size to the label's intrinsic
    /// extent plus padding.
    pub size: Option<Size>,
    /// Label font size (logical pixels).
    pub label_font_size_px: u32,
}

impl ButtonStyle {
    /// (R686.B §5.16) Neutral baseline: sharp corners, no padding,
    /// intrinsic size, 14-px label. Tune via the `with_*` builders.
    #[must_use]
    pub fn m3_default(tag: impl Into<Cow<'static, str>>) -> Self {
        Self {
            tag: tag.into(),
            corner_radius: 0,
            padding: Rect::default(),
            size: None,
            label_font_size_px: 14,
        }
    }

    /// Override the corner radius (≥ half-height ⇒ M3 pill).
    #[must_use]
    pub const fn with_corner_radius(mut self, radius: u32) -> Self {
        self.corner_radius = radius;
        self
    }

    /// Override the inner padding around the label.
    #[must_use]
    pub const fn with_padding(mut self, padding: Rect) -> Self {
        self.padding = padding;
        self
    }

    /// Pin a fixed button size (else intrinsic + padding).
    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    /// Override the label font size in logical pixels.
    #[must_use]
    pub const fn with_label_font_size_px(mut self, size: u32) -> Self {
        self.label_font_size_px = size;
        self
    }
}

/// (R686.B §5.16) Compose a Material 3 filled button [`Scene`].
///
/// A single centered [`TextNode`] label inside a tagged
/// [`Scene::Container`] whose fill is resolved by [`m3_button_fill`]
/// and whose label colour swaps to [`ButtonColors::label_disabled`]
/// on the `Disabled` state. Geometry (corner / padding / size / font)
/// comes from `style`.
///
/// The caller wraps this in its own backdrop container (matching the
/// [`view_splitter`](crate::splitter::view_splitter) contract — the
/// substrate paints the widget, the binding paints the canvas) and
/// registers a
/// [`ButtonExternal`](pinion_core::widgets::button::ButtonExternal)
/// against `style.tag` for the click wire.
///
/// (R694 §5.16 §5.39) `focused` paints the keyboard-focus indicator: a
/// [`FOCUS_RING_WIDTH`]-pixel [`Border`] in [`ButtonColors::focus_ring`]
/// while the button holds the shell
/// [`FocusManager`](pinion_runtime::FocusManager) focus (sourced by the
/// binding from the external's focus posture, the same channel hover
/// flows through). `false` leaves the container border-free — the
/// pre-R694 fill-only appearance.
#[must_use]
pub fn view_button(
    label: &str,
    state: ButtonState,
    hover_progress: f32,
    focused: bool,
    colors: &ButtonColors,
    style: &ButtonStyle,
) -> Scene {
    let fill = m3_button_fill(colors, state, hover_progress);
    let fg = match state {
        ButtonState::Disabled => colors.label_disabled,
        _ => colors.label,
    };
    let mut layout = LayoutStyle::new()
        .flex(FlexDirection::Row)
        .with_justify(JustifyContent::Center)
        .with_align_items(AlignItems::Center)
        .with_padding(style.padding);
    if let Some(size) = style.size {
        layout = layout.with_size(size);
    }
    let mut box_style = BoxStyle::filled(fill).with_corner_radius(style.corner_radius);
    if focused {
        box_style = box_style.with_border(Border::new(colors.focus_ring, FOCUS_RING_WIDTH));
    }
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            label.to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.label_font_size_px)
                .with_fg(fg),
        ))])
        .with_tag(style.tag.clone())
        .with_style(box_style)
        .with_layout(layout),
    )
}

/// (R686.C §5.16 §5.22 §5.28) Drive a hover-progress spring and read
/// its current `0.0..=1.0` value.
///
/// Materialises a per-key [`Animation<f32>`] in the current
/// [`Owner`]'s [`cache`](Owner::cache) (so it persists across paint
/// cycles + is dropped with the owner), retargets it to `1.0` when
/// `is_hover` else `0.0`, and returns the spring's current value. The
/// [`SpringConfig::default`] timing smooths the Idle ↔ Hover fade over
/// ~200 ms; the shell's repaint loop drives it to steady state once
/// input stops (R51.147).
///
/// Threads straight into [`view_button`]'s `hover_progress` argument.
/// Lifted from the verbatim `drive_hover_progress` helper that
/// `hello-button`, `figma-button-m3`, and `hello-button-tui` each
/// carried (R686.C, [[abstraction-needs-second-consumer]] Rule of
/// Three). The `anim_key` stays per-binding so independent buttons
/// own independent springs (cache slots keyed on
/// `(TypeId::of::<Animation<f32>>(), anim_key)`).
///
/// # Panics
///
/// Panics if called outside an [`Owner`] scope (i.e. not inside the
/// shell's `root_owner().run(...)` view-fn wrap) — loud failure is
/// chosen over a silently-broken animation, matching the pre-R686.C
/// binding-local helpers.
#[must_use]
pub fn use_hover_progress(is_hover: bool, anim_key: &'static str) -> f32 {
    let owner = Owner::current()
        .expect("use_hover_progress: view fn must run inside ShellCore::root_owner().run(...)");
    let anim: Rc<Animation<f32>> =
        owner.cache(anim_key, || Animation::new(&owner, 0.0_f32, SpringConfig::default()));
    anim.set_target(if is_hover { 1.0 } else { 0.0 });
    anim.value()
}

/// R727 §5.16 — compose a button paint node: derive the hover-progress
/// from `state` (the R686.C spring via [`use_hover_progress`]) and hand
/// it to [`view_button`]. This is the SSOT for the *mechanical*
/// `use_hover_progress(matches!(Hover)) + view_button(...)` pairing that
/// the composite multi-`ButtonExternal` bindings (`hello-dialog`,
/// `hello-drawer`, `hello-snackbar`) each repeated verbatim — a
/// 3-consumer mechanical lift (the [`use_hover_progress`] R686.C
/// precedent).
///
/// The *opinionated* per-example defaults (which [`ButtonColors`] tone,
/// which label font) stay at the call site: callers pass the resolved
/// [`ButtonColors`] (filled-tonal / accent / a custom inverse-surface
/// action tone) and a [`ButtonStyle`] carrying the tag / size / font. So
/// this lifts only the un-opinionated wiring, leaving styling decisions
/// with each binding (R703 keeps opinionated paint-compose per-binding
/// until a 3rd *identical* consumer).
#[must_use]
pub fn button_scene(
    label: &str,
    state: ButtonState,
    focused: bool,
    hover_key: &'static str,
    colors: &ButtonColors,
    style: &ButtonStyle,
) -> Scene {
    let hover = use_hover_progress(matches!(state, ButtonState::Hover), hover_key);
    view_button(label, state, hover, focused, colors, style)
}

/// R703 §5.16 §5.40 — read a
/// [`ButtonExternal`](pinion_core::widgets::button::ButtonExternal)'s
/// [`ButtonState`] out of the state scene by tag (via the §5.15
/// introspect `"state"` slot). Defaults to [`ButtonState::Idle`] when
/// the tag is absent or carries no introspect handle.
///
/// Lifted from the verbatim helper `hello-dialog` + `hello-drawer` each
/// carried for their composite multi-`ButtonExternal` bindings (R703
/// SSOT lift, 2nd consumer). Composite bindings (real `ButtonExternal`s
/// addressed by tag) cannot use the single-node `#[widget]` derive, so
/// they read posture through these helpers instead.
#[must_use]
pub fn read_button_state(scene: &Scene, tag: &str) -> ButtonState {
    scene
        .find_external_with_tag(tag)
        .and_then(|node| node.handle.introspect())
        .and_then(|intro| intro.query("state"))
        .map_or(ButtonState::Idle, |v| match v {
            IntrospectValue::Text(s) => ButtonState::from_name_or_default(&s),
            _ => ButtonState::Idle,
        })
}

/// R703 §5.16 §5.39 — read a
/// [`ButtonExternal`](pinion_core::widgets::button::ButtonExternal)'s
/// keyboard-focus posture (the `"focused"` introspect slot the shell
/// mirrors via [`External::on_focus_change`](pinion_core::external::External)),
/// so a composite binding paints the focus ring on whichever button
/// holds shell focus. `false` when the tag is absent. R703 SSOT lift —
/// see [`read_button_state`].
#[must_use]
pub fn read_button_focused(scene: &Scene, tag: &str) -> bool {
    scene
        .find_external_with_tag(tag)
        .and_then(|node| node.handle.introspect())
        .and_then(|intro| intro.query("focused"))
        .is_some_and(|v| matches!(v, IntrospectValue::Bool(true)))
}

/// R703 §5.40 — canonical [`ButtonState`] + shell-focus flag ⇒
/// [`AccessState`] interaction-flag mapping for composite
/// `ButtonExternal` bindings.
///
/// The single source of truth that mirrors what the
/// `#[widget(role = Button, state_flags(...))]` derive emits for
/// single-node button bindings — composite bindings (`hello-dialog`,
/// `hello-drawer`) that hand-build their `access_node` route through
/// here so the `ButtonState → AccessState` mapping never diverges
/// between them (R703 SSOT lift: an a11y mapping with two copies is an
/// a11y bug waiting to happen, so it gets one home regardless of the
/// paint Rule-of-Three count).
#[must_use]
pub fn button_a11y_state(state: ButtonState, focused: bool) -> AccessState {
    AccessState {
        focused,
        ..AccessState::from_interaction(state, None)
    }
}

#[cfg(test)]
mod tests {
    //! R686.B §5.16 — button paint substrate tests. Pin the M3
    //! state-layer fill matrix + the colour value-object constructors
    //! + the composition shape the 3 migrated consumers rely on.

    use super::{
        button_a11y_state, m3_button_fill, read_button_focused, read_button_state,
        use_hover_progress, view_button, ButtonColors, ButtonStyle, DISABLED_STATE_LAYER,
        HOVER_STATE_LAYER, PRESSED_STATE_LAYER,
    };
    use pinion_core::animation::Animation;
    use pinion_core::reactive::Owner;
    use pinion_core::scene::{ExternalNode, Rect, Scene};
    use pinion_core::style::{Color, Size};
    use pinion_core::theme::{ColorRole, Theme};
    use pinion_core::widgets::button::{ButtonExternal, ButtonState};

    fn explicit_colors() -> ButtonColors {
        ButtonColors::new(
            Color::rgb(103, 80, 164),
            Color::rgb(255, 255, 255),
            Color::rgb(40, 40, 40),
            Color::rgb(255, 255, 255),
            Color::rgb(180, 180, 180),
            Color::rgb(103, 80, 164),
        )
    }

    // ── R703 §5.16 §5.40 — lifted composite-button binding helpers ──

    #[test]
    fn r703_button_a11y_state_maps_interaction_flags() {
        let hover = button_a11y_state(ButtonState::Hover, false);
        assert!(hover.hovered && !hover.pressed && !hover.disabled && !hover.focused);
        assert_eq!(hover.checked, None, "a button carries no aria-checked");
        let pressed = button_a11y_state(ButtonState::Pressed, true);
        assert!(pressed.pressed && pressed.focused);
        let disabled = button_a11y_state(ButtonState::Disabled, false);
        assert!(disabled.disabled);
        let idle = button_a11y_state(ButtonState::Idle, false);
        assert!(!idle.hovered && !idle.pressed && !idle.disabled && !idle.focused);
    }

    #[test]
    fn r703_read_button_state_and_focused_from_scene() {
        // A single ButtonExternal tagged "b"; default posture is Idle,
        // unfocused. Absent tags fall back to Idle / false.
        let scene =
            Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())).with_tag("b"));
        assert_eq!(read_button_state(&scene, "b"), ButtonState::Idle);
        assert!(!read_button_focused(&scene, "b"));
        assert_eq!(read_button_state(&scene, "absent"), ButtonState::Idle);
        assert!(!read_button_focused(&scene, "absent"));
    }

    #[test]
    fn r686_b_idle_fill_is_base_at_zero_hover_progress() {
        let c = explicit_colors();
        assert_eq!(m3_button_fill(&c, ButtonState::Idle, 0.0), c.base);
    }

    #[test]
    fn r686_b_hover_full_progress_is_hover_endpoint() {
        // hover_progress = 1.0 must land exactly on the M3 hover
        // endpoint (base lerped toward state_layer at 8 %).
        let c = explicit_colors();
        let endpoint = c.base.lerp(c.state_layer, HOVER_STATE_LAYER);
        assert_eq!(m3_button_fill(&c, ButtonState::Hover, 1.0), endpoint);
    }

    #[test]
    fn r686_b_idle_and_hover_share_the_progress_lerp() {
        // The Idle/Hover arm is identical — the only difference a
        // consumer sees is the hover_progress value it threads. Idle
        // at progress 0.5 equals Hover at progress 0.5.
        let c = explicit_colors();
        assert_eq!(
            m3_button_fill(&c, ButtonState::Idle, 0.5),
            m3_button_fill(&c, ButtonState::Hover, 0.5),
        );
    }

    #[test]
    fn r686_b_pressed_fill_is_12pct_state_layer() {
        let c = explicit_colors();
        assert_eq!(
            m3_button_fill(&c, ButtonState::Pressed, 0.0),
            c.base.lerp(c.state_layer, PRESSED_STATE_LAYER),
        );
    }

    #[test]
    fn r686_b_disabled_fill_is_verbatim() {
        // Disabled ignores hover_progress + the overlay matrix; it
        // uses the explicit fill_disabled (absorbs fade-vs-switch).
        let c = explicit_colors();
        assert_eq!(m3_button_fill(&c, ButtonState::Disabled, 1.0), c.fill_disabled);
    }

    #[test]
    fn r686_b_state_layer_coefficients_are_m3_canonical() {
        assert!((HOVER_STATE_LAYER - 0.08).abs() < f32::EPSILON);
        assert!((PRESSED_STATE_LAYER - 0.12).abs() < f32::EPSILON);
        assert!((DISABLED_STATE_LAYER - 0.38).abs() < f32::EPSILON);
    }

    #[test]
    fn r686_b_filled_tonal_resolves_from_theme_roles() {
        let theme = Theme::light();
        let c = ButtonColors::filled_tonal(&theme);
        assert_eq!(c.base, theme.resolve(ColorRole::SurfaceContainerHighest));
        assert_eq!(c.state_layer, theme.resolve(ColorRole::OnSurface));
        assert_eq!(c.label, theme.resolve(ColorRole::OnSurface));
        assert_eq!(c.label_disabled, theme.resolve(ColorRole::OnSurfaceMuted));
        assert_eq!(
            c.fill_disabled,
            theme
                .resolve(ColorRole::SurfaceContainerHighest)
                .lerp(theme.resolve(ColorRole::Surface), DISABLED_STATE_LAYER),
        );
    }

    #[test]
    fn r686_b_accent_disabled_switches_surface_tier_not_fade() {
        let theme = Theme::light();
        let c = ButtonColors::accent(&theme);
        assert_eq!(c.base, theme.resolve(ColorRole::Accent));
        assert_eq!(c.state_layer, theme.resolve(ColorRole::OnAccent));
        // Disabled is a distinct tier, not a fade of the base.
        assert_eq!(c.fill_disabled, theme.resolve(ColorRole::SurfaceContainerHigh));
    }

    #[test]
    fn r686_b_view_button_carries_tag_fill_and_single_label() {
        let c = explicit_colors();
        let style = ButtonStyle::m3_default("test_btn")
            .with_size(Size::px(120, 40))
            .with_corner_radius(20)
            .with_label_font_size_px(16);
        let scene = view_button("Go", ButtonState::Idle, 0.0, false, &c, &style);
        let Scene::Container(outer) = &scene else {
            panic!("button must be a Container");
        };
        assert_eq!(outer.tag.as_deref(), Some("test_btn"));
        assert_eq!(outer.style.fill, c.base, "Idle fill is the base colour");
        assert_eq!(outer.style.corner_radius, 20);
        assert_eq!(outer.children.len(), 1, "exactly the label child");
        assert!(outer.style.border.is_none(), "unfocused button has no ring");
    }

    #[test]
    fn r686_b_view_button_disabled_uses_disabled_label_colour() {
        let c = explicit_colors();
        let style = ButtonStyle::m3_default("test_btn");
        let scene = view_button("Off", ButtonState::Disabled, 0.0, false, &c, &style);
        let Scene::Container(outer) = &scene else {
            panic!("button must be a Container");
        };
        assert_eq!(outer.style.fill, c.fill_disabled);
        let Scene::Text(label) = &outer.children[0] else {
            panic!("child must be the label Text");
        };
        assert_eq!(label.style.fg_color, c.label_disabled);
    }

    // R694 §5.16 §5.39 — keyboard-focus ring.

    #[test]
    fn r694_filled_tonal_focus_ring_is_accent() {
        // M3 documents Accent (`primary`) as the focus-ring role.
        let theme = Theme::light();
        let c = ButtonColors::filled_tonal(&theme);
        assert_eq!(c.focus_ring, theme.resolve(ColorRole::Accent));
    }

    #[test]
    fn r694_accent_button_focus_ring_contrasts_via_on_accent() {
        let theme = Theme::light();
        let c = ButtonColors::accent(&theme);
        assert_eq!(c.focus_ring, theme.resolve(ColorRole::OnAccent));
    }

    #[test]
    fn r694_focused_button_paints_focus_ring_border() {
        let c = explicit_colors();
        let style = ButtonStyle::m3_default("test_btn");
        let scene = view_button("Go", ButtonState::Idle, 0.0, true, &c, &style);
        let Scene::Container(outer) = &scene else {
            panic!("button must be a Container");
        };
        let border = outer.style.border.expect("focused button carries a ring");
        assert_eq!(border.color, c.focus_ring, "ring uses the focus_ring colour");
        assert_eq!(border.width, super::FOCUS_RING_WIDTH);
    }

    #[test]
    fn r694_focus_ring_orthogonal_to_state() {
        // The ring is a focus indicator, independent of the fill state:
        // a focused Hover button keeps its Hover fill *and* gains a ring.
        let c = explicit_colors();
        let style = ButtonStyle::m3_default("b");
        let scene = view_button("Go", ButtonState::Hover, 1.0, true, &c, &style);
        let Scene::Container(outer) = &scene else {
            panic!("button must be a Container");
        };
        assert_eq!(
            outer.style.fill,
            m3_button_fill(&c, ButtonState::Hover, 1.0),
            "focus does not alter the hover fill",
        );
        assert!(outer.style.border.is_some(), "focused button still rings");
    }

    #[test]
    fn r686_c_use_hover_progress_idle_zero_and_caches_spring() {
        // Fresh idle spring sits at the 0.0 initial; the hook caches
        // an Animation<f32> per key so the spring persists across
        // paint cycles (spring dynamics themselves are pinned in
        // pinion-core + the bindings' settle tests).
        let owner = Owner::new();
        let idle = owner.run(|| use_hover_progress(false, "test::hp"));
        assert!(
            idle.abs() < f32::EPSILON,
            "fresh idle hover spring sits at 0.0, got {idle}",
        );
        assert!(
            owner.cache_contains::<Animation<f32>>("test::hp"),
            "use_hover_progress must cache the spring under its key",
        );
    }

    #[test]
    fn r686_c_use_hover_progress_independent_per_key() {
        // Distinct keys own distinct springs (two buttons on screen
        // animate independently).
        let owner = Owner::new();
        owner.run(|| {
            let _ = use_hover_progress(true, "btn_a::hp");
            let _ = use_hover_progress(false, "btn_b::hp");
        });
        assert!(owner.cache_contains::<Animation<f32>>("btn_a::hp"));
        assert!(owner.cache_contains::<Animation<f32>>("btn_b::hp"));
    }

    #[test]
    fn r686_b_m3_default_is_neutral_baseline() {
        let style = ButtonStyle::m3_default("b");
        assert_eq!(style.corner_radius, 0);
        assert_eq!(style.padding, Rect::default());
        assert!(style.size.is_none());
        assert_eq!(style.label_font_size_px, 14);
    }
}
