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
//! [`DISABLED`]) and a single generic [`state_layer`] over the
//! [`InteractionState`] trait, so every common-case consumer tints through
//! one definition. Consumers whose tint **diverges** from the common case —
//! the segmented button / todomvc fold `Disabled` into the resting fill
//! (their base is the transparent track, nothing to tint); `button` tints
//! toward a role-specific [`ButtonColors::state_layer`] rather than
//! `OnSurface` — keep their own arms but reference these shared token
//! constants, so the magic numbers still live in exactly one place.

use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::button::ButtonState;
use pinion_core::widgets::checkbox::CheckboxState;
use pinion_core::widgets::disclosure::DisclosureState;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::slider::SliderState;
use pinion_core::widgets::toggle::ToggleState;
use pinion_core::Color;

/// Material 3 hover state-layer opacity — the cursor-over overlay fraction
/// lerped from the resting fill toward `OnSurface`. M3 canonical 8 %.
pub const HOVER: f32 = 0.08;

/// Material 3 pressed state-layer opacity. M3 canonical 12 %.
pub const PRESSED: f32 = 0.12;

/// Material 3 disabled state-layer opacity — the fade fraction toward
/// `Surface`. M3 canonical 38 %.
pub const DISABLED: f32 = 0.38;

/// The interaction posture a widget-state enum exposes to the shared
/// [`state_layer`] overlay. Implemented for every gallery state enum;
/// `SliderState` (and the other drag-driven enums) map their `Dragging`
/// posture onto [`Self::is_pressed`] so the pressed overlay applies while a
/// thumb is held, exactly as M3 specifies.
pub trait InteractionState {
    /// The pointer is hovering (but not pressed/dragging or disabled).
    fn is_hovered(&self) -> bool;
    /// The control is being actuated (pressed, or a thumb is being dragged).
    fn is_pressed(&self) -> bool;
    /// The control is disabled.
    fn is_disabled(&self) -> bool;
}

impl InteractionState for RadioState {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Pressed)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

impl InteractionState for ButtonState {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Pressed)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

impl InteractionState for CheckboxState {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Pressed)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

impl InteractionState for ToggleState {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Pressed)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

impl InteractionState for SliderState {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    /// A held thumb (`Dragging`) is the slider's pressed posture.
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Dragging)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

impl InteractionState for DisclosureState {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Pressed)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

impl InteractionState for ListboxItemState {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Pressed)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
