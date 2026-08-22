//! R696 §5.38 — Disclosure widget: Checkbox R51.5 1:1 statechart with
//! a divergent intent name (`"expanded"`) and accessibility surface
//! (`button` + WAI-ARIA `aria-expanded` governing a separate `region`
//! panel, vs. `checkbox` + `aria-checked` on its own value). The
//! four-state interaction body is shared via the R51.3 SCE
//! `sce:template` (`standard_button.sce-template.xml`); the engine
//! facade + intent buffer use the R51.4
//! [`Widget<P>`](crate::widgets::Widget) /
//! [`IntentEmitter<W>`](crate::widgets::IntentEmitter) generics.
//!
//! Per-widget surface here: `expanded: bool` sidecar (flip on
//! activate, mirror of Checkbox's `value`) and the per-widget intent
//! name. Everything else is inherited.
//!
//! A disclosure is the atomic building block of an **accordion** (a
//! stack of disclosures): activating the header button reveals or
//! hides the associated content region. The WAI-ARIA APG disclosure
//! pattern is a `button` with `aria-expanded` reflecting whether the
//! controlled region is shown — exactly the `expanded` sidecar this
//! widget tracks.

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
    clippy::all
)]
mod sm {
    include!("../../generated/disclosure_sm.rs");
}

use sm::DisclosurePolicy;
pub use sm::{DisclosureEvent, DisclosureState};

// SCE-002 §5.16 — the `WidgetStateName` / `WidgetEventName` impls for the
// sce-generated `DisclosureState` / `DisclosureEvent` enums are injected
// as `#[derive]`s by `build.rs` (`compile_scxml_with_derives`),
// reconstructed from the codegen's `#[default]` state +
// `EXTERNALLY_DRIVABLE_EVENTS` const (see `pinion-derive`); the per-widget
// `widget_{state,event}_name!` macros are retired. The External introspect
// below calls `self.state().as_name()`; the hello-disclosure binding's
// `read_state` calls `from_name_or_default`.

use crate::WidgetStateName;
use crate::external::{
    ArgForm, Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner,
    SchemaArg, SchemaField, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};

/// Disclosure widget state machine + collapsed/expanded sidecar.
/// Statechart identical to [`Checkbox`](crate::widgets::checkbox::Checkbox); divergence is
/// the per-widget intent name (`"expanded"`) emitted by
/// [`DisclosureExternal`] and the surrounding accessibility role
/// applications attach (`button` + `aria-expanded`, not `checkbox` +
/// `aria-checked`).
pub struct Disclosure {
    inner: Widget<DisclosurePolicy>,
    expanded: bool,
}

impl Disclosure {
    /// Construct a collapsed Disclosure in the `Idle` state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Widget::new(),
            expanded: false,
        }
    }

    /// Drive a [`DisclosureEvent`] through the SCXML. The `expanded`
    /// field flips collapsed ↔ expanded on either activation path:
    ///
    /// * `Pressed → Hover` — pointer click (release on the header).
    /// * `KeyboardActivate` from `Idle`/`Hover` — §5.39 ARIA Space /
    ///   Enter keyboard activation; the SCXML internal transition
    ///   leaves state unchanged so the sidecar mutation lands here.
    ///
    /// `Disabled` ignores both paths (the SCXML template has no
    /// activation transitions from the disabled state).
    pub fn send(&mut self, event: DisclosureEvent) {
        let before = self.state();
        let is_keyboard_activate = matches!(event, DisclosureEvent::KeyboardActivate);
        self.inner.send(event);
        let after = self.state();
        let pointer_activate =
            matches!(before, DisclosureState::Pressed) && matches!(after, DisclosureState::Hover);
        let keyboard_activate =
            is_keyboard_activate && !matches!(before, DisclosureState::Disabled);
        if pointer_activate || keyboard_activate {
            self.expanded = !self.expanded;
        }
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> DisclosureState {
        self.inner.state()
    }

    /// `true` if the controlled region is currently expanded.
    #[must_use]
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Seed or restore the expanded value without an activate
    /// transition. Useful when binding to a persisted "section open"
    /// preference or restoring an accordion's last layout.
    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }
}

impl Default for Disclosure {
    fn default() -> Self {
        Self::new()
    }
}

/// R696 §5.38 — Disclosure transition contract. Mirror of Checkbox's
/// impl with the intent name swapped (`"expanded"` vs `"checked"`) —
/// statechart + value semantics are identical, the kind discriminates
/// at the listener layer so show/hide controls can be subscribed to
/// independently from form checkboxes.
impl WidgetTransition for Disclosure {
    type Event = DisclosureEvent;
    type Snapshot = (DisclosureState, bool);

    fn snapshot(&self) -> Self::Snapshot {
        (self.state(), self.is_expanded())
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(before: Self::Snapshot, event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        let (before_state, before_value) = before;
        let (after_state, after_value) = after;
        let pointer_toggle = matches!(before_state, DisclosureState::Pressed)
            && matches!(after_state, DisclosureState::Hover)
            && before_value != after_value;
        // §5.39 — keyboard activation is a state-stable internal
        // transition. `before_value != after_value` automatically
        // ignores the disabled case (mutation skipped in `send`).
        let keyboard_toggle =
            matches!(event, DisclosureEvent::KeyboardActivate) && before_value != after_value;
        if pointer_toggle || keyboard_toggle {
            vec![Intent::new_static(
                "expanded",
                IntrospectValue::Bool(after_value),
            )]
        } else {
            Vec::new()
        }
    }
}

/// `External` adapter wrapping a [`Disclosure`]. Mirrors
/// [`CheckboxExternal`](crate::widgets::checkbox::CheckboxExternal) one-to-one with the intent
/// name (`"expanded"`) and schema label (`"expanded"`) adjusted so
/// AI listeners can subscribe to show/hide state independently from
/// form checkboxes / settings switches.
pub struct DisclosureExternal {
    em: IntentEmitter<Disclosure>,
}

impl DisclosureExternal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
        }
    }

    /// Drive a [`DisclosureEvent`] and queue any `"expanded"` intent
    /// the transition produces. Pipeline on
    /// [`IntentEmitter::dispatch`]; detection rule on the
    /// [`WidgetTransition`] impl for [`Disclosure`].
    pub fn send(&mut self, event: DisclosureEvent) {
        self.em.dispatch(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> DisclosureState {
        self.em.inner.state()
    }

    /// Current expanded value.
    #[must_use]
    pub fn is_expanded(&self) -> bool {
        self.em.inner.is_expanded()
    }
}

impl Default for DisclosureExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for DisclosureExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DisclosureExternal")
            .field("state", &self.state())
            .field("expanded", &self.is_expanded())
            .finish()
    }
}

impl External for DisclosureExternal {
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

impl ExternalIntrospect for DisclosureExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("state", "string"),
                    SchemaField::new("expanded", "bool"),
                    // R1769 — the lossless read of the statechart, and the
                    // action that takes it back. ⚠ It restores the MACHINE and
                    // not `expanded`, which is this widget's own sidecar.
                    SchemaField::new("configuration", "json"),
                    SchemaField::action_with(
                        "send",
                        "string",
                        ArgForm::Scalar,
                        const { &[SchemaArg::event(&DisclosureEvent::DRIVABLE_NAMES)] },
                    ),
                    SchemaField::action_with(
                        "resume",
                        "json",
                        ArgForm::Scalar,
                        const { &[SchemaArg::open("configuration", "json")] },
                    ),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "state" => Ok(IntrospectValue::Text(self.state().as_name().to_string())),
            "expanded" => Ok(IntrospectValue::Bool(self.is_expanded())),
            "configuration" => {
                crate::widget_core::widget_configuration("disclosure", &self.em.inner.inner)
            }
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "state" => Err(InterveneError::ReadOnly),
            "expanded" => match value {
                IntrospectValue::Bool(b) => {
                    self.em.inner.set_expanded(b);
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
                        crate::widget_core::require_event::<DisclosureEvent>("disclosure", name)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R1769 — enter a configuration this widget was in, running no
            // `<onentry>`; a different verb from `send` on the same channel.
            "resume" => {
                crate::widget_core::resume_widget("disclosure", &mut self.em.inner.inner, args)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WidgetEventName;

    #[test]
    fn initial_state_is_idle_collapsed() {
        let d = Disclosure::new();
        assert_eq!(d.state(), DisclosureState::Idle);
        assert!(!d.is_expanded());
    }

    #[test]
    fn r699_event_name_round_trip() {
        // External-drivable variants round-trip; internal raise + Null
        // + unknown reject; `as_name` is total over internal variants.
        assert_eq!(
            DisclosureEvent::from_name("KeyboardActivate"),
            Some(DisclosureEvent::KeyboardActivate)
        );
        assert_eq!(DisclosureEvent::from_name("DisclosureActivate"), None);
        assert_eq!(DisclosureEvent::from_name("Null"), None);
        assert_eq!(DisclosureEvent::from_name("Bogus"), None);
        assert_eq!(
            DisclosureEvent::DisclosureActivate.as_name(),
            "DisclosureActivate"
        );
    }

    #[test]
    fn full_click_cycle_toggles_expanded() {
        let mut d = Disclosure::new();
        d.send(DisclosureEvent::PointerEnter);
        d.send(DisclosureEvent::PointerDown);
        d.send(DisclosureEvent::PointerUp);
        assert!(d.is_expanded(), "first activate expands");
        d.send(DisclosureEvent::PointerDown);
        d.send(DisclosureEvent::PointerUp);
        assert!(!d.is_expanded(), "second activate collapses");
    }

    #[test]
    fn cancel_does_not_toggle() {
        let mut d = Disclosure::new();
        d.send(DisclosureEvent::PointerEnter);
        d.send(DisclosureEvent::PointerDown);
        d.send(DisclosureEvent::PointerLeave);
        assert!(!d.is_expanded(), "cancel must not toggle");
    }

    #[test]
    fn pointer_cancel_during_press_returns_to_idle_without_toggle() {
        let mut dx = DisclosureExternal::new();
        dx.send(DisclosureEvent::PointerEnter);
        dx.send(DisclosureEvent::PointerDown);
        assert!(matches!(dx.state(), DisclosureState::Pressed));
        let before = dx.is_expanded();
        dx.send(DisclosureEvent::PointerCancel);
        assert!(matches!(dx.state(), DisclosureState::Idle));
        assert_eq!(
            dx.is_expanded(),
            before,
            "PointerCancel must not toggle the expanded bit"
        );
        assert!(
            !dx.is_dirty(),
            "PointerCancel from Pressed must not fire `expanded` intent"
        );
    }

    #[test]
    fn set_expanded_does_not_drive_state() {
        let mut d = Disclosure::new();
        d.set_expanded(true);
        assert!(d.is_expanded());
        assert_eq!(d.state(), DisclosureState::Idle);
    }

    #[test]
    fn external_emits_expanded_intent_on_activate() {
        let mut dx = DisclosureExternal::new();
        dx.send(DisclosureEvent::PointerEnter);
        dx.send(DisclosureEvent::PointerDown);
        assert!(!dx.is_dirty());
        dx.send(DisclosureEvent::PointerUp);
        assert!(dx.is_dirty());
        let mut harvested = Vec::new();
        dx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "expanded");
        assert_eq!(harvested[0].payload, IntrospectValue::Bool(true));
    }

    #[test]
    fn external_cancel_emits_no_intent() {
        let mut dx = DisclosureExternal::new();
        dx.send(DisclosureEvent::PointerEnter);
        dx.send(DisclosureEvent::PointerDown);
        dx.send(DisclosureEvent::PointerLeave);
        let mut harvested = Vec::new();
        dx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn external_query_state_and_expanded() {
        let mut dx = DisclosureExternal::new();
        assert_eq!(
            dx.query("state").unwrap(),
            IntrospectValue::Text("Idle".to_string())
        );
        assert_eq!(dx.query("expanded").unwrap(), IntrospectValue::Bool(false));
        dx.send(DisclosureEvent::PointerEnter);
        dx.send(DisclosureEvent::PointerDown);
        dx.send(DisclosureEvent::PointerUp);
        assert_eq!(dx.query("expanded").unwrap(), IntrospectValue::Bool(true));
    }

    #[test]
    fn external_intervene_expanded_writes_value_no_intent() {
        let mut dx = DisclosureExternal::new();
        let r = dx.intervene("expanded", IntrospectValue::Bool(true));
        assert!(r.is_ok());
        assert!(dx.is_expanded());
        assert!(!dx.is_dirty(), "intervene must not fire intent");
    }

    #[test]
    fn external_intervene_state_is_read_only() {
        let mut dx = DisclosureExternal::new();
        let r = dx.intervene("state", IntrospectValue::Text("Pressed".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
    }

    #[test]
    fn external_invoke_send_drives_transition() {
        let mut dx = DisclosureExternal::new();
        let out = dx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Hover".to_string()));
    }

    #[test]
    fn external_schema_declares_its_five_slots() {
        let dx = DisclosureExternal::new();
        assert_eq!(
            dx.schema().fields,
            // R1769 — `configuration` + `resume` joined as a pair; the count in
            // this test's NAME moved with them rather than being left to lie.
            &[
                SchemaField::new("state", "string"),
                SchemaField::new("expanded", "bool"),
                SchemaField::new("configuration", "json"),
                SchemaField::action_with(
                    "send",
                    "string",
                    ArgForm::Scalar,
                    const { &[SchemaArg::event(&DisclosureEvent::DRIVABLE_NAMES)] },
                ),
                SchemaField::action_with(
                    "resume",
                    "json",
                    ArgForm::Scalar,
                    const { &[SchemaArg::open("configuration", "json")] },
                )
            ]
        );
    }

    // §5.39 keyboard activation

    #[test]
    fn keyboard_activate_from_idle_toggles_and_emits_expanded_intent() {
        let mut dx = DisclosureExternal::new();
        assert!(!dx.is_expanded());
        dx.send(DisclosureEvent::KeyboardActivate);
        assert_eq!(dx.state(), DisclosureState::Idle, "state-stable internal");
        assert!(dx.is_expanded());
        let mut harvested: Vec<Intent> = Vec::new();
        dx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "expanded");
        assert_eq!(harvested[0].payload, IntrospectValue::Bool(true));
    }

    #[test]
    fn keyboard_activate_from_disabled_emits_no_intent() {
        let mut dx = DisclosureExternal::new();
        dx.send(DisclosureEvent::Disable);
        dx.send(DisclosureEvent::KeyboardActivate);
        assert!(!dx.is_expanded());
        let mut harvested: Vec<Intent> = Vec::new();
        dx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }
}
