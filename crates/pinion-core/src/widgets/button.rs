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

impl ButtonExternal {
    /// Capture the current state as an owned, `Send`-friendly,
    /// read-only RPC view (see [`ButtonStateSnapshot`]). Lets a live
    /// app feed its current `ButtonState` to `dispatch` without
    /// surrendering ownership of the wrapped SCXML engine.
    #[must_use]
    pub fn snapshot(&self) -> ButtonStateSnapshot {
        ButtonStateSnapshot::new(self.state())
    }
}

/// Read-only RPC view of a single `Button`'s state at a point in
/// time. Implements [`External`] + [`ExternalIntrospect`] so it can
/// be embedded in `Scene::External` and queried via the §5.12
/// `scene/query` method, while remaining cheap (single enum field)
/// and `Send` — the live `Button` itself stays on the UI thread.
///
/// `intervene` always errors with [`InterveneError::ReadOnly`]: this
/// type is a *snapshot*, not a control surface. Live-mutating RPC
/// (e.g. RPC-driven `ButtonEvent::PointerDown`) requires a `Box<dyn
/// External>` downcast story that is carry-forward to a later spec
/// round.
#[derive(Debug, Clone, Copy)]
pub struct ButtonStateSnapshot {
    state: ButtonState,
}

impl ButtonStateSnapshot {
    #[must_use]
    pub const fn new(state: ButtonState) -> Self {
        Self { state }
    }

    #[must_use]
    pub const fn state(&self) -> ButtonState {
        self.state
    }
}

impl External for ButtonStateSnapshot {
    fn backends(&self) -> BackendSupport {
        // RPC-only: snapshot does not paint or take input, it only
        // surfaces state to the §5.12 query path.
        BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
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

impl ExternalIntrospect for ButtonStateSnapshot {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[("state", "string")])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(
                button_state_name(self.state).to_string(),
            )),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        _path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        // Snapshot is observation-only by design — see type doc.
        Err(InterveneError::ReadOnly)
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

    #[test]
    fn button_external_snapshot_captures_current_state() {
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        let snap = bx.snapshot();
        assert_eq!(snap.state(), ButtonState::Hover);
        let v = snap.query("state").unwrap();
        assert_eq!(v, IntrospectValue::Text("Hover".to_string()));
    }

    #[test]
    fn button_state_snapshot_intervene_is_always_read_only() {
        let mut snap = ButtonStateSnapshot::new(ButtonState::Idle);
        let r = snap.intervene("state", IntrospectValue::Text("Pressed".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
        let r = snap.intervene("nope", IntrospectValue::Null);
        assert_eq!(r, Err(InterveneError::ReadOnly));
    }

    #[test]
    fn button_state_snapshot_clone_is_independent() {
        let snap = ButtonStateSnapshot::new(ButtonState::Pressed);
        let copy = snap;
        assert_eq!(snap.state(), copy.state());
    }
}
