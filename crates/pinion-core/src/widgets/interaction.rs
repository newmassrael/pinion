//! R755 §5.50 — the **interaction-posture SSOT**: the single trait that maps
//! a widget-state enum's resting/hover/pressed/disabled variants onto the
//! three booleans every cross-cutting consumer reads.
//!
//! ## Why this lives in `pinion-core` (not `pinion-widget-paint`)
//!
//! R752 introduced [`InteractionState`] inside `pinion-widget-paint`'s
//! `state_layer` module so the Material 3 overlay (`pinion-widget-paint`)
//! could tint any state enum through one definition. But the **same**
//! posture mapping — `hovered: matches!(state, X::Hover)` /
//! `pressed: matches!(state, X::Pressed)` / `disabled: matches!(state,
//! X::Disabled)` — was also hand-copied into every widget's `access_node`
//! when populating [`AccessState`](../../../pinion_a11y/struct.AccessState.html),
//! 24 sites across the gallery (R755 audit: `grep -rln "hovered: matches!"`).
//! `pinion-a11y` could not reach the trait because it depends only on
//! `pinion-core`, not on `pinion-widget-paint`.
//!
//! Lifting the trait (and its per-enum impls) down to `pinion-core` lets
//! **both** the paint layer (state-layer overlay) and the a11y layer
//! (`AccessState::from_interaction`) share one posture definition.
//! `pinion-widget-paint::state_layer` re-exports it so existing callers and
//! the generic [`state_layer`](../../../pinion_widget_paint/state_layer/fn.state_layer.html)
//! overlay keep compiling unchanged.

use crate::widgets::button::ButtonState;
use crate::widgets::checkbox::CheckboxState;
use crate::widgets::color_area::ColorAreaState;
use crate::widgets::disclosure::DisclosureState;
use crate::widgets::listbox_item::ListboxItemState;
use crate::widgets::radio::RadioState;
use crate::widgets::slider::SliderState;
use crate::widgets::toggle::ToggleState;

/// The interaction posture a widget-state enum exposes to its cross-cutting
/// consumers — the Material 3 state-layer overlay
/// (`pinion-widget-paint::state_layer`) and the ARIA accessibility state
/// (`pinion-a11y::AccessState::from_interaction`).
///
/// Implemented for every gallery state enum; the drag-driven enums
/// (`SliderState` / `ColorAreaState`) map their `Dragging` posture onto
/// [`Self::is_pressed`] so the pressed overlay/`aria-pressed` apply while a
/// thumb is held, exactly as M3 specifies.
pub trait InteractionState {
    /// The pointer is hovering (but not pressed/dragging or disabled).
    fn is_hovered(&self) -> bool;
    /// The control is being actuated (pressed, or a thumb is being dragged).
    fn is_pressed(&self) -> bool;
    /// The control is disabled.
    fn is_disabled(&self) -> bool;
}

/// Blanket forward so a shared reference is accepted wherever an owned
/// state is — `access_node(state: &ButtonState, …)` bindings pass `&T`
/// while most callers pass an owned `Copy` enum; both reach the same
/// posture without a deref at the call site (mirrors std's
/// `impl Read for &File` / `impl Iterator for &mut I`).
impl<T: InteractionState + ?Sized> InteractionState for &T {
    fn is_hovered(&self) -> bool {
        (**self).is_hovered()
    }
    fn is_pressed(&self) -> bool {
        (**self).is_pressed()
    }
    fn is_disabled(&self) -> bool {
        (**self).is_disabled()
    }
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

impl InteractionState for ColorAreaState {
    fn is_hovered(&self) -> bool {
        matches!(self, Self::Hover)
    }
    /// A held saturation/brightness thumb (`Dragging`) is the color area's
    /// pressed posture, mirroring [`SliderState`].
    fn is_pressed(&self) -> bool {
        matches!(self, Self::Dragging)
    }
    fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_postures_are_mutually_exclusive() {
        let s = RadioState::Hover;
        assert!(s.is_hovered() && !s.is_pressed() && !s.is_disabled());
        let s = RadioState::Pressed;
        assert!(s.is_pressed() && !s.is_hovered() && !s.is_disabled());
        let s = RadioState::Disabled;
        assert!(s.is_disabled() && !s.is_hovered() && !s.is_pressed());
        assert!(
            !RadioState::Idle.is_hovered()
                && !RadioState::Idle.is_pressed()
                && !RadioState::Idle.is_disabled()
        );
    }

    #[test]
    fn drag_driven_enums_map_dragging_to_pressed() {
        assert!(SliderState::Dragging.is_pressed());
        assert!(!SliderState::Dragging.is_hovered());
        assert!(ColorAreaState::Dragging.is_pressed());
        assert!(!ColorAreaState::Dragging.is_hovered());
    }
}
