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

use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::button::ButtonState;

/// (R686.B §5.16) Material 3 hover state-layer opacity — the cursor-
/// over overlay fraction lerped from the resting fill toward the
/// [`ButtonColors::state_layer`] colour. M3 canonical 8 %.
pub const HOVER_STATE_LAYER: f32 = 0.08;

/// (R686.B §5.16) Material 3 pressed state-layer opacity. M3
/// canonical 12 %.
pub const PRESSED_STATE_LAYER: f32 = 0.12;

/// (R686.B §5.16) Material 3 disabled state-layer opacity — the
/// fade fraction the [`ButtonColors::filled_tonal`] /
/// [`ButtonColors::new`] disabled-fill helpers apply when a consumer
/// wants the "fade toward a background" disabled appearance (the
/// editor instead switches to a distinct surface tier, expressed via
/// an explicit [`ButtonColors::fill_disabled`]). M3 canonical 38 %.
pub const DISABLED_STATE_LAYER: f32 = 0.38;

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
    ) -> Self {
        Self {
            base,
            state_layer,
            fill_disabled,
            label,
            label_disabled,
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
#[must_use]
pub fn view_button(
    label: &str,
    state: ButtonState,
    hover_progress: f32,
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
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            label.to_string(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.label_font_size_px)
                .with_fg(fg),
        ))])
        .with_tag(style.tag.clone())
        .with_style(BoxStyle::filled(fill).with_corner_radius(style.corner_radius))
        .with_layout(layout),
    )
}

#[cfg(test)]
mod tests {
    //! R686.B §5.16 — button paint substrate tests. Pin the M3
    //! state-layer fill matrix + the colour value-object constructors
    //! + the composition shape the 3 migrated consumers rely on.

    use super::{
        m3_button_fill, view_button, ButtonColors, ButtonStyle, DISABLED_STATE_LAYER,
        HOVER_STATE_LAYER, PRESSED_STATE_LAYER,
    };
    use pinion_core::scene::{Rect, Scene};
    use pinion_core::style::{Color, Size};
    use pinion_core::theme::{ColorRole, Theme};
    use pinion_core::widgets::button::ButtonState;

    fn explicit_colors() -> ButtonColors {
        ButtonColors::new(
            Color::rgb(103, 80, 164),
            Color::rgb(255, 255, 255),
            Color::rgb(40, 40, 40),
            Color::rgb(255, 255, 255),
            Color::rgb(180, 180, 180),
        )
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
        let scene = view_button("Go", ButtonState::Idle, 0.0, &c, &style);
        let Scene::Container(outer) = &scene else {
            panic!("button must be a Container");
        };
        assert_eq!(outer.tag.as_deref(), Some("test_btn"));
        assert_eq!(outer.style.fill, c.base, "Idle fill is the base colour");
        assert_eq!(outer.style.corner_radius, 20);
        assert_eq!(outer.children.len(), 1, "exactly the label child");
    }

    #[test]
    fn r686_b_view_button_disabled_uses_disabled_label_colour() {
        let c = explicit_colors();
        let style = ButtonStyle::m3_default("test_btn");
        let scene = view_button("Off", ButtonState::Disabled, 0.0, &c, &style);
        let Scene::Container(outer) = &scene else {
            panic!("button must be a Container");
        };
        assert_eq!(outer.style.fill, c.fill_disabled);
        let Scene::Text(label) = &outer.children[0] else {
            panic!("child must be the label Text");
        };
        assert_eq!(label.style.fg_color, c.label_disabled);
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
