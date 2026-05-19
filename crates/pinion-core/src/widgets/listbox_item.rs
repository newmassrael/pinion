//! R51.95 §5.38 — `ListBoxItem` widget: shared interaction statechart
//! (button-like via `standard_button.sce-template.xml`) with the same
//! semantic shape as [`Radio`](crate::widgets::Radio) at the binding
//! layer — activate *sets* `selected = true` unconditionally (never
//! flips). Group exclusivity (single-select `Listbox`) is the
//! composite's responsibility.
//!
//! Semantic axis vs `Radio`:
//!
//! * **Wire-level activate event** — `ListBoxItem` raises
//!   `listbox_item.activate`; `Radio` raises `radio.activate`. The
//!   SCXML-level distinction lets future SCXML observers route the
//!   two without snooping the surrounding composite. The Rust
//!   composite uses indexed dispatch and does not depend on this
//!   distinction.
//! * **ARIA role at the composite** — `ListBox` exposes
//!   `AriaRole::Listbox` + `AriaRole::Option` per item;
//!   [`crate::widgets::RadioGroup`] exposes `AriaRole::RadioGroup` +
//!   `AriaRole::RadioButton`. The application's `access_node` impl
//!   chooses which role per the composite's identity.
//! * **Composite keyboard model** — `ListBox` (W3C ARIA Listbox
//!   pattern): Arrow keys move focus only; `Space` / `Enter`
//!   commits the selection. `RadioGroup` (W3C ARIA Radio Group
//!   pattern): Arrow keys move focus AND select. `ListBoxItem`'s
//!   `KeyboardActivate` arm (inherited from the template) is reached
//!   only from `Space` / `Enter` in the `Listbox` model.
//!
//! The statechart body is widget-agnostic (press / release / cancel /
//! keyboard primitives), so the per-widget binding only differs in
//! the activate-event name parameter and the surrounding composite
//! semantics — the canonical pinion §5.38 "shared template +
//! per-widget activate channel" pattern.

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
    include!(concat!(env!("OUT_DIR"), "/listbox_item_sm.rs"));
}

pub use sm::{ListboxItemEvent, ListboxItemState};
use sm::ListboxItemPolicy;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};

/// `ListBoxItem` widget state machine + selection value sidecar.
/// Activate (`Pressed → Hover` from pointer release, or
/// `KeyboardActivate` from `Idle`/`Hover` while focused) sets the
/// value to `true` unconditionally — re-activating an already-
/// selected item is idempotent. The value only returns to `false`
/// when application or composite code calls [`Self::set_selected`]
/// (the canonical pattern: `ListBox` composite deselects siblings
/// on the new selection's activation edge).
pub struct ListBoxItem {
    inner: Widget<ListboxItemPolicy>,
    selected: bool,
}

impl ListBoxItem {
    /// Construct an unselected `ListBoxItem` in the `Idle` state.
    #[must_use]
    pub fn new() -> Self {
        Self { inner: Widget::new(), selected: false }
    }

    /// Drive a [`ListboxItemEvent`] through the SCXML. `selected` is
    /// set to `true` (set-not-flip, idempotent) on either activation
    /// path:
    ///
    /// * `Pressed → Hover` — pointer release on the item.
    /// * `KeyboardActivate` from `Idle`/`Hover` — R51.55 §5.39 ARIA
    ///   Space / Enter keyboard activation (the `Listbox` composite
    ///   only routes `Space`/`Enter` to `KeyboardActivate`; Arrow
    ///   keys are focus-only in the ARIA `Listbox` model and do not
    ///   reach this method).
    ///
    /// `Disabled` ignores both activate paths. R51.93 §5.35
    /// `PointerCancel` from `Pressed` drops back to `Idle` without
    /// firing — touch cancellation does not commit the selection.
    /// Sibling deselection is the composite's responsibility (the
    /// future `ListBox::send` will call `set_selected(false)` on
    /// the previously-selected child after any new selection lands,
    /// mirroring [`crate::widgets::RadioGroup::send`]).
    pub fn send(&mut self, event: ListboxItemEvent) {
        let before = self.state();
        let is_keyboard_activate = matches!(event, ListboxItemEvent::KeyboardActivate);
        self.inner.send(event);
        let after = self.state();
        let pointer_activate =
            matches!(before, ListboxItemState::Pressed)
                && matches!(after, ListboxItemState::Hover);
        let keyboard_activate =
            is_keyboard_activate && !matches!(before, ListboxItemState::Disabled);
        if pointer_activate || keyboard_activate {
            self.selected = true;
        }
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> ListboxItemState {
        self.inner.state()
    }

    /// `true` if selected.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.selected
    }

    /// Set the selection value directly. Group code calls
    /// `set_selected(false)` on sibling items when one is activated.
    /// Persisted-preference restore also uses this path.
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }
}

impl Default for ListBoxItem {
    fn default() -> Self {
        Self::new()
    }
}

/// R51.12 §5.38 — `ListBoxItem` transition contract. Same snapshot
/// shape as [`Radio`](crate::widgets::Radio) (`(State, bool)`) — the
/// detect rule is set-not-flip: emit `"selected"` only when the
/// value transitions `false → true` (not on every activate).
/// Re-activating an already-selected item is idempotent and silent
/// — matches user expectation that "select the already-selected
/// option" is a no-op. Payload is [`Null`]; the selection is
/// identity-only, and the scene-side `ExternalNode.tag` carries
/// which option was picked.
///
/// [`Null`]: IntrospectValue::Null
impl WidgetTransition for ListBoxItem {
    type Event = ListboxItemEvent;
    type Snapshot = (ListboxItemState, bool);

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
    ) -> Option<Intent> {
        let (before_state, before_value) = before;
        let (after_state, after_value) = after;
        let pointer_select = matches!(before_state, ListboxItemState::Pressed)
            && matches!(after_state, ListboxItemState::Hover)
            && !before_value
            && after_value;
        // R51.55 §5.39 — keyboard activation is a state-stable
        // internal transition. !before_value && after_value covers
        // disabled (mutation skipped in send) and already-selected
        // (idempotent set-not-flip) both silently.
        let keyboard_select = matches!(event, ListboxItemEvent::KeyboardActivate)
            && !before_value
            && after_value;
        if pointer_select || keyboard_select {
            Some(Intent::new_static("selected", IntrospectValue::Null))
        } else {
            None
        }
    }
}

/// `External` adapter wrapping a [`ListBoxItem`]. Emits a `"selected"`
/// intent on the activate path only when the value actually
/// transitions `false → true` (so re-activating an already-selected
/// item is silent on the §5.20 channel — matches user expectation
/// that "select the already-selected option" is a no-op).
pub struct ListBoxItemExternal {
    em: IntentEmitter<ListBoxItem>,
}

impl ListBoxItemExternal {
    #[must_use]
    pub fn new() -> Self {
        Self { em: IntentEmitter::default() }
    }

    /// Drive a [`ListboxItemEvent`] and queue a `"selected"` intent
    /// only on `false → true` value transition; idempotent
    /// re-activation is silent.
    pub fn send(&mut self, event: ListboxItemEvent) {
        self.em.dispatch(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> ListboxItemState {
        self.em.inner.state()
    }

    /// `true` if selected.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.em.inner.is_selected()
    }
}

impl Default for ListBoxItemExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for ListBoxItemExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ListBoxItemExternal")
            .field("state", &self.state())
            .field("selected", &self.is_selected())
            .finish()
    }
}

impl External for ListBoxItemExternal {
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

impl ExternalIntrospect for ListBoxItemExternal {
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
                listbox_item_state_name(self.state()).to_string(),
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
                    let ev = parse_listbox_item_event(name)
                        .ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(
                        listbox_item_state_name(self.state()).to_string(),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

pub(crate) fn listbox_item_state_name(state: ListboxItemState) -> &'static str {
    match state {
        ListboxItemState::Idle => "Idle",
        ListboxItemState::Hover => "Hover",
        ListboxItemState::Pressed => "Pressed",
        ListboxItemState::Disabled => "Disabled",
    }
}

pub(crate) fn parse_listbox_item_event(name: &str) -> Option<ListboxItemEvent> {
    match name {
        "PointerEnter" => Some(ListboxItemEvent::PointerEnter),
        "PointerLeave" => Some(ListboxItemEvent::PointerLeave),
        "PointerDown" => Some(ListboxItemEvent::PointerDown),
        "PointerUp" => Some(ListboxItemEvent::PointerUp),
        // R51.93 §5.35 — touch-cancel sibling of PointerUp; does
        // not set selected = true or fire the `"selected"` intent.
        "PointerCancel" => Some(ListboxItemEvent::PointerCancel),
        // R51.55 §5.39 — ARIA Space / Enter keyboard activation.
        "KeyboardActivate" => Some(ListboxItemEvent::KeyboardActivate),
        "Disable" => Some(ListboxItemEvent::Disable),
        "Enable" => Some(ListboxItemEvent::Enable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activate(item: &mut ListBoxItem) {
        item.send(ListboxItemEvent::PointerEnter);
        item.send(ListboxItemEvent::PointerDown);
        item.send(ListboxItemEvent::PointerUp);
    }

    #[test]
    fn initial_state_is_idle_unselected() {
        let i = ListBoxItem::new();
        assert_eq!(i.state(), ListboxItemState::Idle);
        assert!(!i.is_selected());
    }

    #[test]
    fn activate_sets_selected_unconditionally() {
        let mut i = ListBoxItem::new();
        activate(&mut i);
        assert!(i.is_selected(), "first activate selects");
        // Re-activate keeps selected (set-not-flip).
        activate(&mut i);
        assert!(i.is_selected(), "re-activate stays selected");
    }

    #[test]
    fn cancel_does_not_select() {
        let mut i = ListBoxItem::new();
        i.send(ListboxItemEvent::PointerEnter);
        i.send(ListboxItemEvent::PointerDown);
        i.send(ListboxItemEvent::PointerLeave);
        assert!(!i.is_selected(), "leave cancel must not select");
    }

    #[test]
    fn r51_93_pointer_cancel_does_not_select() {
        // Touch cancellation (OS-revoked gesture) — Pressed → Idle
        // via PointerCancel must not commit selection.
        let mut i = ListBoxItem::new();
        i.send(ListboxItemEvent::PointerEnter);
        i.send(ListboxItemEvent::PointerDown);
        assert_eq!(i.state(), ListboxItemState::Pressed);
        i.send(ListboxItemEvent::PointerCancel);
        assert_eq!(i.state(), ListboxItemState::Idle);
        assert!(!i.is_selected(), "PointerCancel must not select");
    }

    #[test]
    fn set_selected_false_simulates_group_deselect() {
        let mut i = ListBoxItem::new();
        activate(&mut i);
        assert!(i.is_selected());
        i.set_selected(false);
        assert!(!i.is_selected(), "composite deselect path");
    }

    #[test]
    fn keyboard_activate_selects_from_idle() {
        let mut i = ListBoxItem::new();
        i.send(ListboxItemEvent::KeyboardActivate);
        assert!(i.is_selected(), "Space/Enter on focused item selects");
    }

    #[test]
    fn keyboard_activate_when_disabled_is_silent() {
        let mut i = ListBoxItem::new();
        i.send(ListboxItemEvent::Disable);
        i.send(ListboxItemEvent::KeyboardActivate);
        assert!(!i.is_selected(), "disabled ignores keyboard activation");
        assert_eq!(i.state(), ListboxItemState::Disabled);
    }

    #[test]
    fn external_emits_selected_intent_only_on_false_to_true() {
        let mut ix = ListBoxItemExternal::new();
        // First activation: selected emit.
        ix.send(ListboxItemEvent::PointerEnter);
        ix.send(ListboxItemEvent::PointerDown);
        ix.send(ListboxItemEvent::PointerUp);
        let mut harvested = Vec::new();
        ix.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "selected");
        // Re-activation: silent.
        ix.send(ListboxItemEvent::PointerEnter);
        ix.send(ListboxItemEvent::PointerDown);
        ix.send(ListboxItemEvent::PointerUp);
        let mut second = Vec::new();
        ix.drain_intents(&mut |i| second.push(i));
        assert!(
            second.is_empty(),
            "re-activate already-selected must be silent"
        );
    }

    #[test]
    fn r51_93_external_pointer_cancel_silent() {
        let mut ix = ListBoxItemExternal::new();
        ix.send(ListboxItemEvent::PointerEnter);
        ix.send(ListboxItemEvent::PointerDown);
        ix.send(ListboxItemEvent::PointerCancel);
        let mut harvested = Vec::new();
        ix.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty(), "cancel must not fire selected intent");
        assert!(!ix.is_selected());
    }

    #[test]
    fn external_query_returns_state_and_selected() {
        let mut ix = ListBoxItemExternal::new();
        assert_eq!(
            ix.query("state"),
            Some(IntrospectValue::Text("Idle".to_string()))
        );
        assert_eq!(ix.query("selected"), Some(IntrospectValue::Bool(false)));
        ix.send(ListboxItemEvent::PointerEnter);
        ix.send(ListboxItemEvent::PointerDown);
        ix.send(ListboxItemEvent::PointerUp);
        assert_eq!(ix.query("selected"), Some(IntrospectValue::Bool(true)));
        assert_eq!(
            ix.query("state"),
            Some(IntrospectValue::Text("Hover".to_string()))
        );
    }

    #[test]
    fn external_intervene_selected_writes_value() {
        let mut ix = ListBoxItemExternal::new();
        ix.intervene("selected", IntrospectValue::Bool(true)).unwrap();
        assert!(ix.is_selected());
        ix.intervene("selected", IntrospectValue::Bool(false))
            .unwrap();
        assert!(!ix.is_selected());
    }

    #[test]
    fn external_intervene_state_is_read_only() {
        let mut ix = ListBoxItemExternal::new();
        assert_eq!(
            ix.intervene("state", IntrospectValue::Text("Hover".to_string())),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn external_invoke_send_drives_transition() {
        let mut ix = ListBoxItemExternal::new();
        let out = ix
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Hover".to_string()));
        assert_eq!(ix.state(), ListboxItemState::Hover);
    }

    #[test]
    fn external_invoke_unknown_event_rejected() {
        let mut ix = ListBoxItemExternal::new();
        let r = ix.invoke("send", IntrospectValue::Text("BogusEvent".to_string()));
        assert!(matches!(r, Err(InvokeError::Rejected)));
    }

    #[test]
    fn parse_event_names_round_trip() {
        for name in [
            "PointerEnter",
            "PointerLeave",
            "PointerDown",
            "PointerUp",
            "PointerCancel",
            "KeyboardActivate",
            "Disable",
            "Enable",
        ] {
            assert!(
                parse_listbox_item_event(name).is_some(),
                "round-trip {name:?}"
            );
        }
        assert!(parse_listbox_item_event("Bogus").is_none());
    }
}
