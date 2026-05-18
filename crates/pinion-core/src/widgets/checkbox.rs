//! R51.5 §5.38 — Checkbox widget: Toggle R51.2 1:1 pattern, divergent
//! intent name (`"checked"`) and accessibility role (`checkbox` vs.
//! `switch`). The four-state interaction body is shared via the
//! R51.3 SCE `sce:template` (`standard_button.sce-template.xml`),
//! and the engine facade + intent buffer use the R51.4
//! [`Widget<P>`](crate::widgets::Widget) /
//! [`IntentEmitter<W>`](crate::widgets::IntentEmitter) generics.
//!
//! Per-widget surface here: `value: bool` sidecar (flip on activate,
//! mirror of Toggle) and the per-widget intent name. Everything else
//! is inherited.

#[allow(
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
    include!(concat!(env!("OUT_DIR"), "/checkbox_sm.rs"));
}

pub use sm::{CheckboxEvent, CheckboxState};
use sm::CheckboxPolicy;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};

/// Checkbox widget state machine + Off/On value sidecar. Statechart
/// identical to [`crate::widgets::Toggle`]; divergence is the
/// per-widget intent name (`"checked"`) emitted by
/// [`CheckboxExternal`] and the surrounding accessibility role
/// applications attach (`checkbox`, not `switch`).
pub struct Checkbox {
    inner: Widget<CheckboxPolicy>,
    value: bool,
}

impl Checkbox {
    /// Construct an unchecked Checkbox in the `Idle` state.
    #[must_use]
    pub fn new() -> Self {
        Self { inner: Widget::new(), value: false }
    }

    /// Drive a [`CheckboxEvent`] through the SCXML. `Pressed → Hover`
    /// flips the `value` field (mirror of Toggle).
    pub fn send(&mut self, event: CheckboxEvent) {
        let before = self.state();
        self.inner.send(event);
        let after = self.state();
        if matches!(before, CheckboxState::Pressed) && matches!(after, CheckboxState::Hover) {
            self.value = !self.value;
        }
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> CheckboxState {
        self.inner.state()
    }

    /// `true` if checked.
    #[must_use]
    pub fn is_checked(&self) -> bool {
        self.value
    }

    /// Seed or restore the checked value without an activate
    /// transition. Useful when binding to a form model or persisted
    /// preference.
    pub fn set_checked(&mut self, checked: bool) {
        self.value = checked;
    }
}

impl Default for Checkbox {
    fn default() -> Self {
        Self::new()
    }
}

/// R51.12 §5.38 — Checkbox transition contract. Mirror of Toggle's
/// impl with the intent name swapped (`"checked"` vs `"toggle"`) —
/// statechart + value semantics are identical, the kind discriminates
/// at the listener layer so form-bound code can subscribe to
/// checkboxes independently from settings switches.
impl WidgetTransition for Checkbox {
    type Event = CheckboxEvent;
    type Snapshot = (CheckboxState, bool);

    fn snapshot(&self) -> Self::Snapshot {
        (self.state(), self.is_checked())
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(before: Self::Snapshot, after: Self::Snapshot) -> Option<Intent> {
        let (before_state, before_value) = before;
        let (after_state, after_value) = after;
        if matches!(before_state, CheckboxState::Pressed)
            && matches!(after_state, CheckboxState::Hover)
            && before_value != after_value
        {
            Some(Intent::new_static(
                "checked",
                IntrospectValue::Bool(after_value),
            ))
        } else {
            None
        }
    }
}

/// `External` adapter wrapping a [`Checkbox`]. Mirrors
/// [`crate::widgets::ToggleExternal`] one-to-one with the intent name
/// (`"checked"`) and schema label (`"checked"` instead of `"value"`)
/// adjusted so form-bound listeners can subscribe to checkbox state
/// independently from settings toggles.
pub struct CheckboxExternal {
    em: IntentEmitter<Checkbox>,
}

impl CheckboxExternal {
    #[must_use]
    pub fn new() -> Self {
        Self { em: IntentEmitter::default() }
    }

    /// Drive a [`CheckboxEvent`] and queue any `"checked"` intent the
    /// transition produces.
    ///
    /// R51.12 §5.38 refactor: pipeline on [`IntentEmitter::dispatch`],
    /// detection rule on [`WidgetTransition`] impl for [`Checkbox`].
    /// `Pressed → Hover` with value flip pushes a `"checked"` intent
    /// carrying the new boolean as [`IntrospectValue::Bool`].
    pub fn send(&mut self, event: CheckboxEvent) {
        self.em.dispatch(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> CheckboxState {
        self.em.inner.state()
    }

    /// Current checked value.
    #[must_use]
    pub fn is_checked(&self) -> bool {
        self.em.inner.is_checked()
    }
}

impl Default for CheckboxExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for CheckboxExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CheckboxExternal")
            .field("state", &self.state())
            .field("checked", &self.is_checked())
            .finish()
    }
}

impl External for CheckboxExternal {
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
        self.em.drain(sink);
    }

    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }
}

impl ExternalIntrospect for CheckboxExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("state", "string"),
            ("checked", "bool"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(
                checkbox_state_name(self.state()).to_string(),
            )),
            "checked" => Some(IntrospectValue::Bool(self.is_checked())),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            "state" => Err(InterveneError::ReadOnly),
            "checked" => match value {
                IntrospectValue::Bool(b) => {
                    self.em.inner.set_checked(b);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(ref name) => {
                    let ev = parse_checkbox_event(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(
                        checkbox_state_name(self.state()).to_string(),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn checkbox_state_name(state: CheckboxState) -> &'static str {
    match state {
        CheckboxState::Idle => "Idle",
        CheckboxState::Hover => "Hover",
        CheckboxState::Pressed => "Pressed",
        CheckboxState::Disabled => "Disabled",
    }
}

fn parse_checkbox_event(name: &str) -> Option<CheckboxEvent> {
    match name {
        "PointerEnter" => Some(CheckboxEvent::PointerEnter),
        "PointerLeave" => Some(CheckboxEvent::PointerLeave),
        "PointerDown" => Some(CheckboxEvent::PointerDown),
        "PointerUp" => Some(CheckboxEvent::PointerUp),
        "Disable" => Some(CheckboxEvent::Disable),
        "Enable" => Some(CheckboxEvent::Enable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle_unchecked() {
        let cb = Checkbox::new();
        assert_eq!(cb.state(), CheckboxState::Idle);
        assert!(!cb.is_checked());
    }

    #[test]
    fn full_click_cycle_flips_checked() {
        let mut cb = Checkbox::new();
        cb.send(CheckboxEvent::PointerEnter);
        cb.send(CheckboxEvent::PointerDown);
        cb.send(CheckboxEvent::PointerUp);
        assert!(cb.is_checked(), "first activate checks");
        cb.send(CheckboxEvent::PointerDown);
        cb.send(CheckboxEvent::PointerUp);
        assert!(!cb.is_checked(), "second activate unchecks");
    }

    #[test]
    fn cancel_does_not_flip() {
        let mut cb = Checkbox::new();
        cb.send(CheckboxEvent::PointerEnter);
        cb.send(CheckboxEvent::PointerDown);
        cb.send(CheckboxEvent::PointerLeave);
        assert!(!cb.is_checked(), "cancel must not flip");
    }

    #[test]
    fn set_checked_does_not_drive_state() {
        let mut cb = Checkbox::new();
        cb.set_checked(true);
        assert!(cb.is_checked());
        assert_eq!(cb.state(), CheckboxState::Idle);
    }

    #[test]
    fn external_emits_checked_intent_on_activate() {
        let mut cx = CheckboxExternal::new();
        cx.send(CheckboxEvent::PointerEnter);
        cx.send(CheckboxEvent::PointerDown);
        assert!(!cx.is_dirty());
        cx.send(CheckboxEvent::PointerUp);
        assert!(cx.is_dirty());
        let mut harvested = Vec::new();
        cx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "checked");
        assert_eq!(harvested[0].payload, IntrospectValue::Bool(true));
    }

    #[test]
    fn external_cancel_emits_no_intent() {
        let mut cx = CheckboxExternal::new();
        cx.send(CheckboxEvent::PointerEnter);
        cx.send(CheckboxEvent::PointerDown);
        cx.send(CheckboxEvent::PointerLeave);
        let mut harvested = Vec::new();
        cx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn external_query_state_and_checked() {
        let mut cx = CheckboxExternal::new();
        let s = cx.query("state").unwrap();
        assert_eq!(s, IntrospectValue::Text("Idle".to_string()));
        let v = cx.query("checked").unwrap();
        assert_eq!(v, IntrospectValue::Bool(false));
        cx.send(CheckboxEvent::PointerEnter);
        cx.send(CheckboxEvent::PointerDown);
        cx.send(CheckboxEvent::PointerUp);
        let v = cx.query("checked").unwrap();
        assert_eq!(v, IntrospectValue::Bool(true));
    }

    #[test]
    fn external_intervene_checked_writes_value_no_intent() {
        let mut cx = CheckboxExternal::new();
        let r = cx.intervene("checked", IntrospectValue::Bool(true));
        assert!(r.is_ok());
        assert!(cx.is_checked());
        assert!(!cx.is_dirty(), "intervene must not fire intent");
    }

    #[test]
    fn external_intervene_state_is_read_only() {
        let mut cx = CheckboxExternal::new();
        let r = cx.intervene("state", IntrospectValue::Text("Pressed".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
    }

    #[test]
    fn external_invoke_send_drives_transition() {
        let mut cx = CheckboxExternal::new();
        let out = cx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Hover".to_string()));
    }

    #[test]
    fn external_schema_declares_three_slots() {
        let cx = CheckboxExternal::new();
        let schema = cx.schema();
        assert_eq!(
            schema.fields,
            &[("state", "string"), ("checked", "bool"), ("send", "string")]
        );
    }
}
