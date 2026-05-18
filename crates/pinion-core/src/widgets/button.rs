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
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;

pub struct Button {
    engine: Engine<ButtonPolicy>,
}

impl Button {
    #[must_use]
    pub fn new() -> Self {
        let mut engine = Engine::new(ButtonPolicy::new());
        engine.initialize();
        Self { engine }
    }

    pub fn send(&mut self, event: ButtonEvent) {
        self.engine.process_event(event);
    }

    #[must_use]
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
    /// §5.20 intent buffer. Filled when the wrapped SCXML transitions
    /// in a way the framework should report (e.g. Pressed → Hover
    /// signals a completed click); drained by the runtime walk or the
    /// `scene/intents` RPC method.
    pending_intents: Vec<Intent>,
}

impl ButtonExternal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Button::new(),
            pending_intents: Vec::new(),
        }
    }

    /// Drive a [`ButtonEvent`] through the wrapped SCXML and enqueue
    /// any §5.20 intent the transition produces.
    ///
    /// `Pressed` → `Hover` (via `PointerUp`) emits a `"click"` intent
    /// with `IntrospectValue::Null` payload. The widget emits only
    /// the *kind* — the §5.20 R22 runtime walk prefixes
    /// `ExternalNode.tag` (e.g. `"save_btn"` → `"save_btn.click"`),
    /// so widget-internal identity stays decoupled from the
    /// user-chosen scene-side identifier.
    pub fn send(&mut self, event: ButtonEvent) {
        let before = self.inner.state();
        self.inner.send(event);
        let after = self.inner.state();
        if matches!(before, ButtonState::Pressed) && matches!(after, ButtonState::Hover) {
            self.pending_intents
                .push(Intent::new_static("click", IntrospectValue::Null));
        }
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

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        for intent in self.pending_intents.drain(..) {
            sink(intent);
        }
    }

    fn is_dirty(&self) -> bool {
        !self.pending_intents.is_empty()
    }
}

impl ExternalIntrospect for ButtonExternal {
    fn schema(&self) -> IntrospectSchema {
        // `state` — read-only slot (query). `send` — action channel
        // accepting a `ButtonEvent` variant name (invoke); §5.15
        // schema does not yet distinguish state slots from actions
        // syntactically, that classification lands with §5.3 DSL.
        IntrospectSchema::new(&[("state", "string"), ("send", "string")])
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
        // State is observed-only via intervene; mutation flows through
        // the `send` action on the invoke channel (R17 bidirectional
        // RPC spec round) — see `invoke` below.
        match path {
            "state" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Action: drive a `ButtonEvent` symbolically by name.
            // Returns the resulting `ButtonState` as `Text`, so the
            // caller (winit handler or RPC client) sees the transition
            // outcome in a single round-trip.
            "send" => match args {
                IntrospectValue::Text(ref name) => {
                    let ev = parse_button_event(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(
                        button_state_name(self.state()).to_string(),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
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

    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // Snapshot has no action channel — the live `ButtonExternal`
        // is where transitions land.
        Err(InvokeError::Rejected)
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

/// Parse a `ButtonEvent` variant from its name (e.g. `"PointerEnter"`).
/// `None` if the name is not a known variant — bidirectional RPC
/// callers see this as `InvokeError::Rejected`.
fn parse_button_event(name: &str) -> Option<ButtonEvent> {
    match name {
        "PointerEnter" => Some(ButtonEvent::PointerEnter),
        "PointerLeave" => Some(ButtonEvent::PointerLeave),
        "PointerDown" => Some(ButtonEvent::PointerDown),
        "PointerUp" => Some(ButtonEvent::PointerUp),
        "Disable" => Some(ButtonEvent::Disable),
        "Enable" => Some(ButtonEvent::Enable),
        _ => None,
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
    fn button_external_schema_declares_state_slot() {
        let bx = ButtonExternal::new();
        let schema = bx.schema();
        assert!(schema.fields.iter().any(|f| f == &("state", "string")));
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

    #[test]
    fn button_external_invoke_send_drives_transition_and_returns_new_state() {
        let mut bx = ButtonExternal::new();
        let out = bx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .expect("PointerEnter is a known ButtonEvent variant");
        assert_eq!(out, IntrospectValue::Text("Hover".to_string()));
        assert_eq!(bx.state(), ButtonState::Hover);
    }

    #[test]
    fn button_external_invoke_unknown_event_name_is_rejected() {
        let mut bx = ButtonExternal::new();
        let r = bx.invoke("send", IntrospectValue::Text("Teleport".to_string()));
        assert_eq!(r, Err(InvokeError::Rejected));
        // State unchanged because the action did not fire.
        assert_eq!(bx.state(), ButtonState::Idle);
    }

    #[test]
    fn button_external_invoke_wrong_arg_type_is_type_mismatch() {
        let mut bx = ButtonExternal::new();
        let r = bx.invoke("send", IntrospectValue::Int(42));
        assert_eq!(r, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn button_external_invoke_unknown_path_is_unknown_path() {
        let mut bx = ButtonExternal::new();
        let r = bx.invoke("nope", IntrospectValue::Null);
        assert_eq!(r, Err(InvokeError::UnknownPath));
    }

    #[test]
    fn button_state_snapshot_invoke_always_rejects() {
        let mut snap = ButtonStateSnapshot::new(ButtonState::Idle);
        let r = snap.invoke("send", IntrospectValue::Text("PointerEnter".to_string()));
        assert_eq!(r, Err(InvokeError::Rejected));
    }

    #[test]
    fn button_external_schema_includes_send_action() {
        let bx = ButtonExternal::new();
        let schema = bx.schema();
        assert_eq!(
            schema.fields,
            &[("state", "string"), ("send", "string")]
        );
    }

    #[test]
    fn button_external_emits_click_intent_on_pressed_to_hover() {
        // §5.20 R22: widget emits only the kind ("click"); the
        // scene-side ExternalNode.tag supplies the widget prefix at
        // walk time. This isolates widget identity from UI naming.
        let mut bx = ButtonExternal::new();
        assert!(!bx.is_dirty());
        bx.send(ButtonEvent::PointerEnter);
        bx.send(ButtonEvent::PointerDown);
        assert!(!bx.is_dirty(), "PointerDown alone is not a click");
        bx.send(ButtonEvent::PointerUp);
        assert!(bx.is_dirty(), "PointerUp from Pressed should arm click");
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "click");
        assert_eq!(harvested[0].payload, IntrospectValue::Null);
        assert!(!bx.is_dirty(), "drain should leave the buffer empty");
    }

    #[test]
    fn button_external_pointer_down_alone_emits_no_intent() {
        // Mid-press without release should not signal a click —
        // guards against premature emission.
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        bx.send(ButtonEvent::PointerDown);
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn button_external_press_then_leave_cancels_click() {
        // Pressed → Idle (via PointerLeave) is a *cancel*, not a
        // click; nothing should land on the intent channel.
        let mut bx = ButtonExternal::new();
        bx.send(ButtonEvent::PointerEnter);
        bx.send(ButtonEvent::PointerDown);
        bx.send(ButtonEvent::PointerLeave);
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn button_external_invoke_send_pointer_up_emits_click_intent() {
        // §5.20 cross-channel: a click driven through the §5.15
        // invoke action also queues the intent — same emission path
        // whether the transition comes from winit or RPC.
        let mut bx = ButtonExternal::new();
        let _ = bx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .unwrap();
        let _ = bx
            .invoke("send", IntrospectValue::Text("PointerDown".to_string()))
            .unwrap();
        let _ = bx
            .invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .unwrap();
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "click");
    }

    #[test]
    fn button_state_snapshot_emits_no_intents() {
        // Snapshots are observation-only; the §5.20 channel must stay
        // silent on them.
        let mut snap = ButtonStateSnapshot::new(ButtonState::Hover);
        assert!(!snap.is_dirty());
        let mut harvested: Vec<Intent> = Vec::new();
        snap.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }
}
