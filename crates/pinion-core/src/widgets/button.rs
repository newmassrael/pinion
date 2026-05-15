#[allow(
    unsafe_code,
    non_snake_case,
    unused_imports,
    dead_code,
    unused_variables,
    unused_mut,
    unused_labels,
    unreachable_patterns,
    unreachable_code,
    unused_assignments,
    clippy::style,
    clippy::complexity,
    clippy::pedantic,
    clippy::all,
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/button_sm.rs"));
}

use sce_rust_runtime::Engine;

pub use sm::{ButtonEvent, ButtonState};
use sm::ButtonPolicy;

pub struct Button {
    engine: Engine<ButtonPolicy>,
}

impl Button {
    pub fn new() -> Self {
        let mut engine = Engine::new(ButtonPolicy::new());
        engine.initialize();
        Self { engine }
    }

    pub fn send(&mut self, event: ButtonEvent) {
        self.engine.process_event(event);
    }

    pub fn state(&self) -> ButtonState {
        self.engine.get_current_state()
    }
}

impl Default for Button {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle() {
        let button = Button::new();
        assert_eq!(button.state(), ButtonState::Idle);
    }

    #[test]
    fn pointer_enter_transitions_to_hover() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        assert_eq!(button.state(), ButtonState::Hover);
    }

    #[test]
    fn full_click_cycle_idle_hover_pressed_hover() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        assert_eq!(button.state(), ButtonState::Hover);
        button.send(ButtonEvent::PointerDown);
        assert_eq!(button.state(), ButtonState::Pressed);
        button.send(ButtonEvent::PointerUp);
        assert_eq!(button.state(), ButtonState::Hover);
    }

    #[test]
    fn pointer_leave_during_press_cancels_to_idle() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        button.send(ButtonEvent::PointerDown);
        button.send(ButtonEvent::PointerLeave);
        assert_eq!(button.state(), ButtonState::Idle);
    }

    #[test]
    fn disable_absorbs_pointer_events() {
        let mut button = Button::new();
        button.send(ButtonEvent::Disable);
        assert_eq!(button.state(), ButtonState::Disabled);
        button.send(ButtonEvent::PointerEnter);
        assert_eq!(button.state(), ButtonState::Disabled);
        button.send(ButtonEvent::PointerDown);
        assert_eq!(button.state(), ButtonState::Disabled);
    }

    #[test]
    fn enable_returns_to_idle() {
        let mut button = Button::new();
        button.send(ButtonEvent::Disable);
        button.send(ButtonEvent::Enable);
        assert_eq!(button.state(), ButtonState::Idle);
    }

    #[test]
    fn disable_from_hover_to_disabled() {
        let mut button = Button::new();
        button.send(ButtonEvent::PointerEnter);
        button.send(ButtonEvent::Disable);
        assert_eq!(button.state(), ButtonState::Disabled);
    }
}
