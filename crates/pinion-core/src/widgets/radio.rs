//! R51.6 §5.38 — Radio widget: shared interaction statechart
//! (button-like via `standard_button.sce-template.xml`) with one
//! semantic divergence at the Rust binding layer: activate *sets*
//! the value to `true` (selected) instead of flipping. A selected
//! Radio stays selected until the application deselects it via
//! [`Radio::set_selected`] (typical pattern: a `RadioGroup`
//! deselects siblings when one is selected).
//!
//! This contrasts with [`crate::widgets::Toggle`] and
//! [`crate::widgets::Checkbox`], whose activate path *flips* a
//! boolean. Statechart is identical; only the value-mutation
//! callback differs.

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
    include!(concat!(env!("OUT_DIR"), "/radio_sm.rs"));
}

pub use sm::{RadioEvent, RadioState};

// R698 §5.16 — route RadioState <-> SCXML-id mapping through the R643
// `WidgetStateName` SSOT primitive, replacing the hand-written
// `radio_state_name` fn (mirrors the R696.A Disclosure adoption).
// radio_group.rs calls `self.state(idx).as_name()` via the trait too.
crate::widget_state_name!(RadioState, default = Idle, [
    Idle, Hover, Pressed, Disabled,
]);
// R699 §5.16 — RadioEvent <-> SCXML-name mapping through the
// `WidgetEventName` SSOT primitive, replacing `parse_radio_event`.
// radio_group.rs drives selection through `RadioEvent::from_name` too.
crate::widget_event_name!(RadioEvent,
    external = [
        PointerEnter,
        PointerLeave,
        PointerDown,
        PointerUp,
        PointerCancel,
        KeyboardActivate,
        Disable,
        Enable,
    ],
    internal = [RadioActivate, Null],
);
use sm::RadioPolicy;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// Radio widget state machine + selection value sidecar. Activate
/// (`Pressed → Hover`) sets the value to `true` unconditionally;
/// the value only returns to `false` when application code calls
/// [`Radio::set_selected`] (typically driven by a sibling Radio
/// being activated within the same group).
pub struct Radio {
    inner: Widget<RadioPolicy>,
    selected: bool,
}

impl Radio {
    /// Construct an unselected Radio in the `Idle` state.
    #[must_use]
    pub fn new() -> Self {
        Self { inner: Widget::new(), selected: false }
    }

    /// Drive a [`RadioEvent`] through the SCXML. `selected` is set
    /// to `true` (set-not-flip, idempotent) on either activation
    /// path:
    ///
    /// * `Pressed → Hover` — pointer click (release on widget).
    /// * `KeyboardActivate` from `Idle`/`Hover` — R51.55 §5.39 ARIA
    ///   Space keyboard activation; the SCXML internal transition
    ///   leaves state unchanged so the sidecar mutation lands here.
    ///
    /// `Disabled` ignores both paths. Sibling deselection is the
    /// group's responsibility (`RadioGroup::send` calls
    /// `set_selected(false)` on the previously-selected child after
    /// any new selection lands).
    pub fn send(&mut self, event: RadioEvent) {
        let before = self.state();
        let is_keyboard_activate = matches!(event, RadioEvent::KeyboardActivate);
        self.inner.send(event);
        let after = self.state();
        let pointer_activate =
            matches!(before, RadioState::Pressed) && matches!(after, RadioState::Hover);
        let keyboard_activate =
            is_keyboard_activate && !matches!(before, RadioState::Disabled);
        if pointer_activate || keyboard_activate {
            self.selected = true;
        }
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> RadioState {
        self.inner.state()
    }

    /// `true` if selected.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.selected
    }

    /// Set the selection value directly. Group code calls
    /// `set_selected(false)` on sibling radios when one is
    /// activated. Persisted-preference restore also uses this path.
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }
}

impl Default for Radio {
    fn default() -> Self {
        Self::new()
    }
}

/// R51.12 §5.38 — Radio transition contract. Same snapshot shape as
/// Toggle / Checkbox (`(State, bool)`), but the detect rule is
/// set-not-flip: emit `"selected"` only when the value transitions
/// `false → true` (not on every activate). Re-activating an
/// already-selected Radio is idempotent and silent — matches user
/// expectation that "select the already-selected option" is a no-op.
/// Payload is [`Null`]; the selection is identity-only, the
/// scene-side `ExternalNode.tag` carries which option was picked.
///
/// [`Null`]: IntrospectValue::Null
impl WidgetTransition for Radio {
    type Event = RadioEvent;
    type Snapshot = (RadioState, bool);

    fn snapshot(&self) -> Self::Snapshot {
        (self.state(), self.is_selected())
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(
        before: Self::Snapshot,
        event: Self::Event,
        after: Self::Snapshot,
    ) -> Vec<Intent> {
        let (before_state, before_value) = before;
        let (after_state, after_value) = after;
        let pointer_select = matches!(before_state, RadioState::Pressed)
            && matches!(after_state, RadioState::Hover)
            && !before_value
            && after_value;
        // R51.55 §5.39 — keyboard activation is a state-stable
        // internal transition. !before_value && after_value covers
        // disabled (mutation skipped in send) and already-selected
        // (idempotent set-not-flip) both silently.
        let keyboard_select = matches!(event, RadioEvent::KeyboardActivate)
            && !before_value
            && after_value;
        if pointer_select || keyboard_select {
            vec![Intent::new_static("selected", IntrospectValue::Null)]
        } else {
            Vec::new()
        }
    }
}

/// `External` adapter wrapping a [`Radio`]. Emits a `"selected"`
/// intent on the activate path only when the value actually
/// transitions `false → true` (so re-activating an already-selected
/// Radio is silent on the §5.20 channel — matches user expectation
/// that "select the already-selected option" is a no-op).
pub struct RadioExternal {
    em: IntentEmitter<Radio>,
}

impl RadioExternal {
    #[must_use]
    pub fn new() -> Self {
        Self { em: IntentEmitter::default() }
    }

    /// Drive a [`RadioEvent`] and queue a `"selected"` intent only on
    /// `false → true` value transition; idempotent re-activation is
    /// silent.
    ///
    /// R51.12 §5.38 refactor: pipeline on [`IntentEmitter::dispatch`],
    /// detection rule on [`WidgetTransition`] impl for [`Radio`] (the
    /// set-not-flip variant — `false → true` activation only).
    pub fn send(&mut self, event: RadioEvent) {
        self.em.dispatch(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> RadioState {
        self.em.inner.state()
    }

    /// `true` if selected.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.em.inner.is_selected()
    }
}

impl Default for RadioExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for RadioExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RadioExternal")
            .field("state", &self.state())
            .field("selected", &self.is_selected())
            .finish()
    }
}

impl External for RadioExternal {
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

impl ExternalIntrospect for RadioExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("state", "string"),
            ("selected", "bool"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(
                self.state().as_name().to_string(),
            )),
            "selected" => Some(IntrospectValue::Bool(self.is_selected())),
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
            "selected" => match value {
                IntrospectValue::Bool(b) => {
                    self.em.inner.set_selected(b);
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
                    let ev =
                        RadioEvent::from_name(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(
                        self.state().as_name().to_string(),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle_unselected() {
        let r = Radio::new();
        assert_eq!(r.state(), RadioState::Idle);
        assert!(!r.is_selected());
    }

    // R51.93 §5.35 — touch-cancel must NOT set selected.

    #[test]
    fn r51_93_pointer_cancel_during_press_returns_to_idle_without_select() {
        let mut rx = RadioExternal::new();
        rx.send(RadioEvent::PointerEnter);
        rx.send(RadioEvent::PointerDown);
        assert!(matches!(rx.state(), RadioState::Pressed));
        let before = rx.is_selected();
        rx.send(RadioEvent::PointerCancel);
        assert!(matches!(rx.state(), RadioState::Idle));
        assert_eq!(
            rx.is_selected(),
            before,
            "PointerCancel must not set selected"
        );
        assert!(
            !rx.is_dirty(),
            "PointerCancel from Pressed must not fire `selected` intent"
        );
    }

    #[test]
    fn r51_93_parse_pointer_cancel_event_name() {
        assert_eq!(
            RadioEvent::from_name("PointerCancel"),
            Some(RadioEvent::PointerCancel)
        );
        // R699 §5.16 — internal raise + Null + unknown all reject.
        assert_eq!(RadioEvent::from_name("RadioActivate"), None);
        assert_eq!(RadioEvent::from_name("Null"), None);
        assert_eq!(RadioEvent::from_name("Bogus"), None);
        assert_eq!(RadioEvent::RadioActivate.as_name(), "RadioActivate");
    }

    #[test]
    fn activate_selects_unconditionally() {
        let mut r = Radio::new();
        r.send(RadioEvent::PointerEnter);
        r.send(RadioEvent::PointerDown);
        r.send(RadioEvent::PointerUp);
        assert!(r.is_selected(), "first activate selects");
        // Re-activate keeps selected (Radio does not flip).
        r.send(RadioEvent::PointerDown);
        r.send(RadioEvent::PointerUp);
        assert!(r.is_selected(), "re-activate stays selected");
    }

    #[test]
    fn cancel_does_not_select() {
        let mut r = Radio::new();
        r.send(RadioEvent::PointerEnter);
        r.send(RadioEvent::PointerDown);
        r.send(RadioEvent::PointerLeave);
        assert!(!r.is_selected(), "cancel must not select");
    }

    #[test]
    fn set_selected_false_simulates_group_deselect() {
        let mut r = Radio::new();
        r.send(RadioEvent::PointerEnter);
        r.send(RadioEvent::PointerDown);
        r.send(RadioEvent::PointerUp);
        assert!(r.is_selected());
        // Group code (sibling Radio selected) deselects this one.
        r.set_selected(false);
        assert!(!r.is_selected());
    }

    #[test]
    fn external_first_activate_emits_selected_intent() {
        let mut rx = RadioExternal::new();
        rx.send(RadioEvent::PointerEnter);
        rx.send(RadioEvent::PointerDown);
        assert!(!rx.is_dirty());
        rx.send(RadioEvent::PointerUp);
        assert!(rx.is_dirty());
        let mut harvested = Vec::new();
        rx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "selected");
        assert_eq!(harvested[0].payload, IntrospectValue::Null);
    }

    #[test]
    fn external_reactivate_emits_no_intent() {
        // Re-activating an already-selected Radio is idempotent and
        // must be silent on the §5.20 channel.
        let mut rx = RadioExternal::new();
        for _ in 0..2 {
            rx.send(RadioEvent::PointerEnter);
            rx.send(RadioEvent::PointerDown);
            rx.send(RadioEvent::PointerUp);
            rx.send(RadioEvent::PointerLeave);
        }
        let mut harvested = Vec::new();
        rx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "only first activate emits");
    }

    #[test]
    fn external_query_state_and_selected() {
        let mut rx = RadioExternal::new();
        assert_eq!(
            rx.query("selected").unwrap(),
            IntrospectValue::Bool(false)
        );
        rx.send(RadioEvent::PointerEnter);
        rx.send(RadioEvent::PointerDown);
        rx.send(RadioEvent::PointerUp);
        assert_eq!(
            rx.query("selected").unwrap(),
            IntrospectValue::Bool(true)
        );
    }

    #[test]
    fn external_intervene_selected_writes_value_no_intent() {
        let mut rx = RadioExternal::new();
        let r = rx.intervene("selected", IntrospectValue::Bool(true));
        assert!(r.is_ok());
        assert!(rx.is_selected());
        assert!(!rx.is_dirty(), "intervene must not fire intent");
    }

    #[test]
    fn external_intervene_state_is_read_only() {
        let mut rx = RadioExternal::new();
        let r = rx.intervene("state", IntrospectValue::Text("Pressed".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
    }

    #[test]
    fn external_invoke_send_drives_transition() {
        let mut rx = RadioExternal::new();
        let out = rx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Hover".to_string()));
    }

    #[test]
    fn external_schema_declares_three_slots() {
        let rx = RadioExternal::new();
        let schema = rx.schema();
        assert_eq!(
            schema.fields,
            &[("state", "string"), ("selected", "bool"), ("send", "string")]
        );
    }

    // ----- R51.55 §5.39 keyboard activation -----

    #[test]
    fn keyboard_activate_from_idle_selects_and_emits_selected_intent() {
        let mut bx = RadioExternal::new();
        assert!(!bx.is_selected());
        bx.send(RadioEvent::KeyboardActivate);
        assert_eq!(bx.state(), RadioState::Idle, "state-stable internal");
        assert!(bx.is_selected());
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "selected");
    }

    #[test]
    fn keyboard_activate_when_already_selected_is_idempotent_silent() {
        let mut bx = RadioExternal::new();
        bx.send(RadioEvent::KeyboardActivate);
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        // second activation while already selected — set-not-flip,
        // no new intent.
        bx.send(RadioEvent::KeyboardActivate);
        assert!(bx.is_selected());
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "second activation must stay silent");
    }

    #[test]
    fn keyboard_activate_from_disabled_emits_no_intent() {
        let mut bx = RadioExternal::new();
        bx.send(RadioEvent::Disable);
        bx.send(RadioEvent::KeyboardActivate);
        assert!(!bx.is_selected());
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }
}
