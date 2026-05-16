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

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, RepaintOwner, ThreadOwnership,
};

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

/// `External` adapter wrapping a [`Button`] SCXML widget. Surfaces
/// the button's [`ButtonState`] to the §5.12 `scene/query` RPC
/// method via the §5.15 item 8 introspect path `state` (read-only,
/// returns [`IntrospectValue::Text`] carrying the variant name).
///
/// First concrete §5.15 reference impl bridging an R12 widget into
/// the RPC plane — `CountedExternal` covers trait-surface mechanics,
/// but `ButtonExternal` is the first time a real widget's state
/// machine round-trips through `dispatch`.
pub struct ButtonExternal {
    inner: Button,
}

impl ButtonExternal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Button::new(),
        }
    }

    pub fn send(&mut self, event: ButtonEvent) {
        self.inner.send(event);
    }

    #[must_use]
    pub fn state(&self) -> ButtonState {
        self.inner.state()
    }
}

impl Default for ButtonExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for ButtonExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ButtonExternal")
            .field("state", &self.state())
            .finish()
    }
}

impl External for ButtonExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for ButtonExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[("state", "string")])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(
                button_state_name(self.state()).to_string(),
            )),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        // v0: state is observed-only via introspect. Drive transitions
        // through `send(ButtonEvent)`, not direct slot intervention.
        match path {
            "state" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }
}

fn button_state_name(state: ButtonState) -> &'static str {
    match state {
        ButtonState::Idle => "Idle",
        ButtonState::Hover => "Hover",
        ButtonState::Pressed => "Pressed",
        ButtonState::Disabled => "Disabled",
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

    #[test]
    fn button_external_initial_query_state_is_idle() {
        let bx = ButtonExternal::new();
        let v = bx.query("state").expect("schema declares `state`");
        assert_eq!(v, IntrospectValue::Text("Idle".to_string()));
    }

    #[test]
    fn button_external_query_tracks_send_transitions() {
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        let v = bx.query("state").unwrap();
        assert_eq!(v, IntrospectValue::Text("Hover".to_string()));
        bx.send(ButtonEvent::PointerDown);
        let v = bx.query("state").unwrap();
        assert_eq!(v, IntrospectValue::Text("Pressed".to_string()));
    }

    #[test]
    fn button_external_unknown_query_path_returns_none() {
        let bx = ButtonExternal::new();
        assert!(bx.query("nope").is_none());
    }

    #[test]
    fn button_external_intervene_state_is_read_only() {
        let mut bx = ButtonExternal::new();
        let r = bx.intervene("state", IntrospectValue::Text("Pressed".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
        let r = bx.intervene("nope", IntrospectValue::Null);
        assert_eq!(r, Err(InterveneError::UnknownPath));
    }

    #[test]
    fn button_external_schema_declares_state() {
        let bx = ButtonExternal::new();
        let schema = bx.schema();
        assert_eq!(schema.fields, &[("state", "string")]);
    }
}
