//! R752 §5.50 — the Material 3 **state-layer** SSOT: the single home for
//! the interaction-overlay opacity tokens and the common-case overlay
//! function shared across the whole widget gallery.
//!
//! ## Why this module exists (the cross-cutting SSOT)
//!
//! Material 3 defines the state-layer opacities as **global tokens** (hover
//! 8 %, pressed 12 %, disabled 38 %), not per-component values. Before R752
//! those three numbers were hand-copied as raw `0.08` / `0.12` / `0.38`
//! literals across ~16 files and five widget-state enums
//! (`RadioState` / `ButtonState` / `CheckboxState` / `ToggleState` /
//! `SliderState`), each re-deriving the same `base.lerp(OnSurface, …)` /
//! `base.lerp(Surface, …)` arms. (R750 lifted only a `RadioState`-shaped
//! slice into `radio_composite::state_layer` and over-claimed it as "SSOT"
//! while identical copies remained in this very crate's `datepicker` /
//! `table` modules — the partial-lift smell R752 clears.)
//!
//! This module owns the tokens **once** ([`HOVER`] / [`PRESSED`] /
//! [`DISABLED`]) and a single generic [`state_layer`] overlay function. The
//! [`InteractionState`] trait it tints over was **lifted to
//! `pinion_core::widgets::interaction` in R755** so the a11y layer (which
//! depends only on `pinion-core`) can share the same posture mapping via
//! `AccessState::from_interaction`; this module **re-exports** the trait so
//! the overlay function and every existing caller path are unchanged.
//! Every common-case consumer tints through this one definition. Consumers
//! whose tint **diverges** from the common case —
//! the segmented button / todomvc fold `Disabled` into the resting fill
//! (their base is the transparent track, nothing to tint); `button` tints
//! toward a role-specific `ButtonColors::state_layer` rather than
//! `OnSurface` — keep their own arms but reference these shared token
//! constants, so the magic numbers still live in exactly one place.

use pinion_core::Color;
use pinion_core::theme::{ColorRole, Theme};

// R755 — the interaction-posture trait lifted down to `pinion-core` so the
// a11y layer can share it (see `pinion_core::widgets::interaction`). Re-
// exported here so the generic `state_layer` overlay and every existing
// `use pinion_widget_paint::state_layer::InteractionState` caller keep
// compiling against the same path.
pub use pinion_core::widgets::interaction::InteractionState;

/// Material 3 hover state-layer opacity — the cursor-over overlay fraction
/// lerped from the resting fill toward `OnSurface`. M3 canonical 8 %.
pub const HOVER: f32 = 0.08;

/// Material 3 pressed state-layer opacity. M3 canonical 12 %.
pub const PRESSED: f32 = 0.12;

/// Material 3 disabled state-layer opacity — the fade fraction toward
/// `Surface`. M3 canonical 38 %.
pub const DISABLED: f32 = 0.38;

/// The common-case M3 state-layer overlay: tint `base` toward `OnSurface`
/// by [`HOVER`] / [`PRESSED`], or toward `Surface` by [`DISABLED`]; an
/// idle control is `base` untinted (`Color::lerp` in linear space, see
/// [`color-lerp-linear-space`]). `Disabled` takes precedence over
/// `Pressed` over `Hover`.
///
/// This is the single definition every common-case consumer (radio cells,
/// breadcrumb / stepper / nav-rail links, checkbox boxes, slider tracks,
/// datepicker / table rows) shares. Divergent consumers (transparent-base
/// fold, role-specific tint target) keep their own arms but reference the
/// [`HOVER`] / [`PRESSED`] / [`DISABLED`] token constants above.
#[must_use]
pub fn state_layer<S: InteractionState + Copy>(base: Color, state: S, theme: &Theme) -> Color {
    if state.is_disabled() {
        base.lerp(theme.resolve(ColorRole::Surface), DISABLED)
    } else if state.is_pressed() {
        base.lerp(theme.resolve(ColorRole::OnSurface), PRESSED)
    } else if state.is_hovered() {
        base.lerp(theme.resolve(ColorRole::OnSurface), HOVER)
    } else {
        base
    }
}

/// R947 §5.38 §5.40 §5.50 — the Material 3 **focus-highlight fill** for a DCC
/// cell / row: the [`HOVER`] state-layer tinted from `Surface` toward
/// `OnSurface` when `focused`, else [`Color::TRANSPARENT`] (the underlying
/// surface shows through an unfocused cell). The editable data-grid
/// (`hello-data-grid`) and the property-grid / inspector
/// (`hello-property-grid`) share this one focused background, so the two DCC
/// surfaces' focus highlight cannot drift apart — distinct from the tree's
/// [`row_focus_bg`](crate::tree_view::row_focus_bg), which uses an opaque
/// `SurfaceContainerHighest` (a deliberately different focus visual).
///
/// A divergent-from-[`state_layer`] consumer per this module's R752 doctrine:
/// the focus → tint, else → transparent shape is not the resting-base overlay
/// `state_layer` produces, so it keeps its own arm — but references the
/// [`HOVER`] token, so the 8 % magic number still lives in exactly one place.
#[must_use]
pub fn focus_fill(theme: &Theme, focused: bool) -> Color {
    if focused {
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::OnSurface), HOVER)
    } else {
        Color::TRANSPARENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::button::ButtonState;
    use pinion_core::widgets::radio::RadioState;
    use pinion_core::widgets::slider::SliderState;

    #[test]
    fn idle_is_untinted() {
        let theme = Theme::light();
        let base = Color::rgb(0x12, 0x34, 0x56);
        assert_eq!(state_layer(base, RadioState::Idle, &theme), base);
        assert_eq!(state_layer(base, ButtonState::Idle, &theme), base);
        assert_eq!(state_layer(base, SliderState::Idle, &theme), base);
    }

    #[test]
    fn hover_pressed_disabled_match_the_tokens() {
        let theme = Theme::light();
        let base = Color::rgb(0x12, 0x34, 0x56);
        let on_surface = theme.resolve(ColorRole::OnSurface);
        let surface = theme.resolve(ColorRole::Surface);
        assert_eq!(
            state_layer(base, RadioState::Hover, &theme),
            base.lerp(on_surface, HOVER)
        );
        assert_eq!(
            state_layer(base, RadioState::Pressed, &theme),
            base.lerp(on_surface, PRESSED)
        );
        assert_eq!(
            state_layer(base, RadioState::Disabled, &theme),
            base.lerp(surface, DISABLED)
        );
    }

    #[test]
    fn slider_dragging_is_the_pressed_layer() {
        let theme = Theme::light();
        let base = Color::rgb(0x12, 0x34, 0x56);
        assert_eq!(
            state_layer(base, SliderState::Dragging, &theme),
            base.lerp(theme.resolve(ColorRole::OnSurface), PRESSED),
            "a held slider thumb wears the pressed state layer"
        );
    }

    #[test]
    fn r947_focus_fill_is_the_hover_layer_over_surface_or_transparent() {
        // The DCC focus-highlight fill: focused == Surface tinted toward
        // OnSurface by the HOVER token; unfocused == transparent (the cell
        // shows the underlying surface). The data-grid + property-grid share
        // this one decision, so the two cannot drift.
        let theme = Theme::light();
        let surface = theme.resolve(ColorRole::Surface);
        let on_surface = theme.resolve(ColorRole::OnSurface);
        assert_eq!(
            focus_fill(&theme, true),
            surface.lerp(on_surface, HOVER),
            "a focused cell wears the OnSurface hover state layer over Surface"
        );
        assert_eq!(
            focus_fill(&theme, false),
            Color::TRANSPARENT,
            "an unfocused cell is transparent"
        );
    }
}
