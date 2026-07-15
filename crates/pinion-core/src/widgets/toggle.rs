//! R51.2 §5.38 — Toggle widget: Button R12 interaction state machine
//! re-used with a `value: bool` (Off / On) sidecar that flips on each
//! `pressed → hover` activate transition.
//!
//! The §5.38 ratify (Round 484) frames Tier-1 widget catalog as
//! "shared interaction state machine, divergent value semantics" —
//! Toggle is the first concrete instance after Button. The SCXML
//! shape (idle / hover / pressed / disabled) is byte-for-byte the
//! same as `button.scxml`; only the `raise event` name diverges
//! (`toggle.activate` vs `button.activate`) so listeners can
//! discriminate at the intent layer.
//!
//! Figma fidelity & AI introspect (user frame, 2026-05-18):
//!
//! * **Visual rendering**: Toggle is a pure state + value primitive;
//!   labels and styling live in the surrounding `Scene::Text` /
//!   `Scene::Box` nodes, which already carry the full
//!   §5.36 R47.5/6 `TextStyle` (`font_weight` × 11 const, `font_style`
//!   italic / oblique, `line_height`, `letter_spacing`, `text_align`,
//!   decoration underline / strikethrough, overflow clip) Figma-
//!   fidelity field set. Toggle imposes no rendering constraint;
//!   applications compose the visual the same way they would for any
//!   Scene primitive.
//! * **AI introspect**: `ToggleExternal` exposes the §5.15 8-item
//!   contract (state read / value read / send action). Schema slots
//!   are `("state", "string")`, `("value", "bool")`, and
//!   `("send", "string")`, so the §5.12 `scene/query` and
//!   `scene/invoke` paths cover every observable + every action
//!   without screenshot inspection (Scene-as-data invariant §2 #7).

// `unsafe_code` intentionally absent — the workspace `forbid` policy
// (Cargo.toml) rejects per-site overrides, and the sce-build codegen
// output does not use `unsafe`. The remaining allows silence the
// stylistic / dead-code lints that generated code routinely trips.
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
    include!(concat!(env!("OUT_DIR"), "/toggle_sm.rs"));
}

pub use sm::{ToggleEvent, ToggleState};

// R698 §5.16 — route ToggleState <-> SCXML-id mapping through the R643
// `WidgetStateName` SSOT primitive, replacing the hand-written
// `toggle_state_name` fn (mirrors the R696.A Disclosure adoption).
crate::widget_state_name!(
    ToggleState,
    default = Idle,
    [Idle, Hover, Pressed, Disabled,]
);
// R699 §5.16 — ToggleEvent <-> SCXML-name mapping through the
// `WidgetEventName` SSOT primitive, replacing `parse_toggle_event`.
crate::widget_event_name!(
    ToggleEvent,
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
    internal = [ToggleActivate, Null],
);
use sm::TogglePolicy;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// Toggle widget state machine + Off/On value sidecar. R51.4 §5.38
/// refactor: the engine wrapping moves into the shared
/// [`Widget<P>`] facade; this newtype now adds only the `value: bool`
/// sidecar that flips on each `pressed → hover` activate transition.
pub struct Toggle {
    inner: Widget<TogglePolicy>,
    value: bool,
}

impl Toggle {
    /// Construct a Toggle in the `Idle` state with `value = false`.
    /// Use [`Toggle::set_on`] to seed a different initial value
    /// before the first activate.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Widget::new(),
            value: false,
        }
    }

    /// Drive a [`ToggleEvent`] through the SCXML. The `value` field
    /// flips Off ↔ On on either activation path:
    ///
    /// * `Pressed → Hover` — pointer click (release on widget).
    /// * `KeyboardActivate` from `Idle`/`Hover` — R51.55 §5.39 ARIA
    ///   keyboard activation; the SCXML internal transition leaves
    ///   state unchanged so the sidecar mutation lands here.
    ///
    /// `Disabled` ignores both paths (the SCXML template has no
    /// activation transitions from the disabled state).
    pub fn send(&mut self, event: ToggleEvent) {
        let before = self.state();
        let is_keyboard_activate = matches!(event, ToggleEvent::KeyboardActivate);
        self.inner.send(event);
        let after = self.state();
        let pointer_activate =
            matches!(before, ToggleState::Pressed) && matches!(after, ToggleState::Hover);
        let keyboard_activate = is_keyboard_activate && !matches!(before, ToggleState::Disabled);
        if pointer_activate || keyboard_activate {
            self.value = !self.value;
        }
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> ToggleState {
        self.inner.state()
    }

    /// Current Off / On value. Application code reads this to drive
    /// the visual (e.g. knob position, accent colour) and to expose
    /// the bound model state.
    #[must_use]
    pub fn is_on(&self) -> bool {
        self.value
    }

    /// Seed or restore the Off / On value without an activate
    /// transition. Useful when binding to an external store
    /// (settings panel checkbox, persisted user preference); the
    /// interaction state machine is untouched.
    pub fn set_on(&mut self, on: bool) {
        self.value = on;
    }
}

impl Default for Toggle {
    fn default() -> Self {
        Self::new()
    }
}

/// R51.12 §5.38 — Toggle transition contract. Snapshot tuples the
/// interaction state with the Off/On value sidecar; detect emits a
/// `"toggle"` intent only on the `Pressed → Hover` activate path
/// when the value actually flipped, carrying the new value as
/// [`IntrospectValue::Bool`]. Re-arming on the cancel path or any
/// non-activate transition stays silent on §5.20.
impl WidgetTransition for Toggle {
    type Event = ToggleEvent;
    type Snapshot = (ToggleState, bool);

    fn snapshot(&self) -> Self::Snapshot {
        (self.state(), self.is_on())
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(before: Self::Snapshot, event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        let (before_state, before_value) = before;
        let (after_state, after_value) = after;
        let pointer_toggle = matches!(before_state, ToggleState::Pressed)
            && matches!(after_state, ToggleState::Hover)
            && before_value != after_value;
        // R51.55 §5.39 — keyboard activation 은 state-stable internal
        // transition. before_value != after_value 검증으로 disabled
        // (mutation skipped in send) 무시 자동 보장.
        let keyboard_toggle =
            matches!(event, ToggleEvent::KeyboardActivate) && before_value != after_value;
        if pointer_toggle || keyboard_toggle {
            vec![Intent::new_static(
                "toggle",
                IntrospectValue::Bool(after_value),
            )]
        } else {
            Vec::new()
        }
    }
}

/// `External` adapter wrapping a [`Toggle`] SCXML widget. Surfaces
/// the toggle's [`ToggleState`] and [`Toggle::is_on`] value to the
/// §5.12 `scene/query` RPC path, and emits a `"toggle"` [`Intent`]
/// every time an activate transition flips the value.
///
/// Mirrors `ButtonExternal` (R12 first-class §5.15 reference impl)
/// with one structural difference: the intent payload is
/// [`IntrospectValue::Bool`] (the new value), not
/// [`IntrospectValue::Null`]. AI listeners can subscribe to a
/// single intent kind and read the post-flip value from the
/// payload without a follow-up `query` round-trip.
pub struct ToggleExternal {
    /// R51.5 §5.38 refactor: §5.20 intent buffer + wrapped widget
    /// share the [`IntentEmitter`] helper; the adapter only owns the
    /// transition-detection logic in `send` and the `value` intervene
    /// override.
    em: IntentEmitter<Toggle>,
}

impl ToggleExternal {
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
        }
    }

    /// Drive a [`ToggleEvent`] through the wrapped SCXML and queue
    /// any §5.20 intent the transition produces.
    ///
    /// R51.12 §5.38 refactor: the snapshot → drive → detect → push
    /// pipeline lives on [`IntentEmitter::dispatch`]; the detection
    /// rule (`Pressed → Hover` with value flip ⇒ `"toggle"` carrying
    /// the new value as [`IntrospectValue::Bool`]) lives on the
    /// [`WidgetTransition`] impl for [`Toggle`]. The §5.22 R22
    /// runtime walk prefixes the scene-side `ExternalNode.tag` so
    /// the wire form becomes e.g. `"dark_mode.toggle"` without the
    /// widget needing to know its parent identifier.
    pub fn send(&mut self, event: ToggleEvent) {
        self.em.dispatch(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> ToggleState {
        self.em.inner.state()
    }

    /// Current Off / On value.
    #[must_use]
    pub fn is_on(&self) -> bool {
        self.em.inner.is_on()
    }

    /// Capture the current `(state, value)` as an owned, `Send`-
    /// friendly, read-only RPC view. See [`ToggleStateSnapshot`].
    #[must_use]
    pub fn snapshot(&self) -> ToggleStateSnapshot {
        ToggleStateSnapshot::new(self.state(), self.is_on())
    }
}

impl Default for ToggleExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for ToggleExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ToggleExternal")
            .field("state", &self.state())
            .field("value", &self.is_on())
            .finish()
    }
}

impl External for ToggleExternal {
    /// R741 §5.35 — capture the pointer on press so a real-mouse click is
    /// robust to sub-pixel jitter between press and release.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R741 §5.35 — a deliberate release off the widget cancels the press.
    fn cancel_on_release_off_target(&self) -> bool {
        true
    }

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

impl ExternalIntrospect for ToggleExternal {
    fn schema(&self) -> IntrospectSchema {
        // `state` — interaction state slot (query).
        // `value` — Off / On boolean slot (query + intervene).
        // `send`  — action channel accepting a `ToggleEvent` variant
        //           name (invoke). `value` is the only slot that is
        //           mutable via intervene; `state` mutates only
        //           through `send` to keep the SCXML the single
        //           source of truth for interaction transitions.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("state", "string"),
                    SchemaField::new("value", "bool"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(self.state().as_name().to_string())),
            "value" => Some(IntrospectValue::Bool(self.is_on())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Read-only — interaction state mutates only via the SCXML
            // (drive through `send`).
            "state" => Err(InterveneError::ReadOnly),
            // Direct value override (settings restore, preference
            // seed). Does not fire a `"toggle"` intent — only an
            // activate transition does, so external bindings cannot
            // accidentally trigger the intent stream.
            "value" => match value {
                IntrospectValue::Bool(b) => {
                    self.em.inner.set_on(b);
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
            // Drive a `ToggleEvent` symbolically by name. Returns the
            // resulting `(state, value)` pair as a JSON-style object
            // wrapped in `IntrospectValue::Text` (canonical
            // `state=<S>, value=<V>` formatting) so the caller sees
            // both transition outcomes in one round-trip without a
            // follow-up query.
            "send" => match args {
                IntrospectValue::Text(ref name) => {
                    let ev = ToggleEvent::from_name(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(format!(
                        "state={}, value={}",
                        self.state().as_name(),
                        self.is_on(),
                    )))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Read-only snapshot of a single `Toggle`'s `(state, value)` at a
/// point in time. Same role as `ButtonStateSnapshot`: lets a live
/// app feed its current Toggle state into the §5.12 query path
/// without surrendering ownership of the wrapped SCXML engine.
///
/// `intervene` and `invoke` both reject; this is observation-only.
/// Live-mutating RPC requires the `ToggleExternal` itself.
#[derive(Debug, Clone, Copy)]
pub struct ToggleStateSnapshot {
    state: ToggleState,
    value: bool,
}

impl ToggleStateSnapshot {
    #[must_use]
    pub const fn new(state: ToggleState, value: bool) -> Self {
        Self { state, value }
    }

    #[must_use]
    pub const fn state(&self) -> ToggleState {
        self.state
    }

    #[must_use]
    pub const fn is_on(&self) -> bool {
        self.value
    }
}

impl External for ToggleStateSnapshot {
    fn backends(&self) -> BackendSupport {
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

impl ExternalIntrospect for ToggleStateSnapshot {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("state", "string"),
                    SchemaField::new("value", "bool"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(self.state.as_name().to_string())),
            "value" => Some(IntrospectValue::Bool(self.value)),
            _ => None,
        }
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::ReadOnly)
    }

    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        Err(InvokeError::Rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Toggle (bare state machine + value) ------------------------

    #[test]
    fn initial_state_is_idle_and_value_is_off() {
        let t = Toggle::new();
        assert_eq!(t.state(), ToggleState::Idle);
        assert!(!t.is_on());
    }

    // R51.93 §5.35 — touch-cancel must NOT flip the value.

    #[test]
    fn r51_93_pointer_cancel_during_press_returns_to_idle_without_flip() {
        let mut tx = ToggleExternal::new();
        tx.send(ToggleEvent::PointerEnter);
        tx.send(ToggleEvent::PointerDown);
        assert!(matches!(tx.state(), ToggleState::Pressed));
        let before = tx.is_on();
        tx.send(ToggleEvent::PointerCancel);
        assert!(matches!(tx.state(), ToggleState::Idle));
        assert_eq!(
            tx.is_on(),
            before,
            "PointerCancel must not flip the value sidecar"
        );
        assert!(
            !tx.is_dirty(),
            "PointerCancel from Pressed must not fire `toggle` intent"
        );
    }

    #[test]
    fn r51_93_parse_pointer_cancel_event_name() {
        assert_eq!(
            ToggleEvent::from_name("PointerCancel"),
            Some(ToggleEvent::PointerCancel)
        );
        // R699 §5.16 — internal raise + Null + unknown all reject.
        assert_eq!(ToggleEvent::from_name("ToggleActivate"), None);
        assert_eq!(ToggleEvent::from_name("Null"), None);
        assert_eq!(ToggleEvent::from_name("Bogus"), None);
        assert_eq!(ToggleEvent::ToggleActivate.as_name(), "ToggleActivate");
    }

    #[test]
    fn pointer_enter_transitions_to_hover_without_flipping_value() {
        let mut t = Toggle::new();
        t.send(ToggleEvent::PointerEnter);
        assert_eq!(t.state(), ToggleState::Hover);
        assert!(!t.is_on(), "hover entry must not flip value");
    }

    #[test]
    fn activate_cycle_flips_value() {
        // Idle → Hover → Pressed → Hover (activate). Value Off → On.
        let mut t = Toggle::new();
        t.send(ToggleEvent::PointerEnter);
        t.send(ToggleEvent::PointerDown);
        t.send(ToggleEvent::PointerUp);
        assert_eq!(t.state(), ToggleState::Hover);
        assert!(t.is_on(), "activate must flip Off → On");
    }

    #[test]
    fn two_activates_return_to_off() {
        let mut t = Toggle::new();
        for _ in 0..2 {
            t.send(ToggleEvent::PointerEnter);
            t.send(ToggleEvent::PointerDown);
            t.send(ToggleEvent::PointerUp);
            t.send(ToggleEvent::PointerLeave);
        }
        // Two activates → Off → On → Off.
        assert!(!t.is_on(), "two activates must return to Off");
    }

    #[test]
    fn cancel_path_does_not_flip_value() {
        // Pressed → Idle (via PointerLeave) is the cancel path —
        // value must stay Off; no activate fired.
        let mut t = Toggle::new();
        t.send(ToggleEvent::PointerEnter);
        t.send(ToggleEvent::PointerDown);
        t.send(ToggleEvent::PointerLeave);
        assert_eq!(t.state(), ToggleState::Idle);
        assert!(!t.is_on(), "cancel must not flip value");
    }

    #[test]
    fn disable_absorbs_pointer_events_and_preserves_value() {
        let mut t = Toggle::new();
        t.set_on(true);
        t.send(ToggleEvent::Disable);
        assert_eq!(t.state(), ToggleState::Disabled);
        t.send(ToggleEvent::PointerEnter);
        t.send(ToggleEvent::PointerDown);
        t.send(ToggleEvent::PointerUp);
        assert_eq!(t.state(), ToggleState::Disabled);
        assert!(t.is_on(), "disabled must preserve On value");
    }

    #[test]
    fn enable_returns_to_idle_without_flipping_value() {
        let mut t = Toggle::new();
        t.set_on(true);
        t.send(ToggleEvent::Disable);
        t.send(ToggleEvent::Enable);
        assert_eq!(t.state(), ToggleState::Idle);
        assert!(t.is_on(), "enable must preserve value");
    }

    #[test]
    fn set_on_seeds_value_without_state_transition() {
        let mut t = Toggle::new();
        assert_eq!(t.state(), ToggleState::Idle);
        t.set_on(true);
        assert_eq!(t.state(), ToggleState::Idle, "set_on must not transition");
        assert!(t.is_on());
    }

    // ---- ToggleExternal (intent + introspect) -----------------------

    #[test]
    fn external_initial_query_state_is_idle_and_value_is_false() {
        let tx = ToggleExternal::new();
        assert_eq!(
            tx.query("state"),
            Some(IntrospectValue::Text("Idle".to_string()))
        );
        assert_eq!(tx.query("value"), Some(IntrospectValue::Bool(false)));
    }

    #[test]
    fn external_unknown_query_path_returns_none() {
        let tx = ToggleExternal::new();
        assert!(tx.query("nope").is_none());
    }

    #[test]
    fn external_intervene_value_writes_through() {
        let mut tx = ToggleExternal::new();
        tx.intervene("value", IntrospectValue::Bool(true)).unwrap();
        assert!(tx.is_on());
        // Intent stream stays empty — intervene does not synthesize an
        // activate transition.
        let mut drained: Vec<Intent> = Vec::new();
        tx.drain_intents(&mut |i| drained.push(i));
        assert!(drained.is_empty());
    }

    #[test]
    fn external_intervene_state_is_read_only() {
        let mut tx = ToggleExternal::new();
        assert_eq!(
            tx.intervene("state", IntrospectValue::Text("Pressed".to_string())),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn external_intervene_value_type_mismatch_rejected() {
        let mut tx = ToggleExternal::new();
        assert_eq!(
            tx.intervene("value", IntrospectValue::Text("on".to_string())),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn external_invoke_send_drives_transition_and_returns_pair() {
        let mut tx = ToggleExternal::new();
        let out = tx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .expect("PointerEnter is a known ToggleEvent variant");
        assert_eq!(
            out,
            IntrospectValue::Text("state=Hover, value=false".to_string())
        );
    }

    #[test]
    fn external_emits_toggle_intent_on_activate() {
        let mut tx = ToggleExternal::new();
        tx.send(ToggleEvent::PointerEnter);
        tx.send(ToggleEvent::PointerDown);
        assert!(!tx.is_dirty(), "PointerDown alone is not an activate");
        tx.send(ToggleEvent::PointerUp);
        assert!(tx.is_dirty(), "PointerUp from Pressed must arm an intent");
        let mut harvested: Vec<Intent> = Vec::new();
        tx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "toggle");
        assert_eq!(harvested[0].payload, IntrospectValue::Bool(true));
        assert!(!tx.is_dirty(), "drain leaves the buffer empty");
    }

    #[test]
    fn external_cancel_path_emits_no_intent() {
        let mut tx = ToggleExternal::new();
        tx.send(ToggleEvent::PointerEnter);
        tx.send(ToggleEvent::PointerDown);
        tx.send(ToggleEvent::PointerLeave);
        let mut harvested: Vec<Intent> = Vec::new();
        tx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
        assert!(!tx.is_on(), "cancel must not flip value");
    }

    #[test]
    fn external_schema_declares_all_three_slots() {
        let tx = ToggleExternal::new();
        assert_eq!(
            tx.schema().fields,
            &[
                SchemaField::new("state", "string"),
                SchemaField::new("value", "bool"),
                SchemaField::new("send", "string")
            ]
        );
    }

    // ---- Snapshot ----------------------------------------------------

    #[test]
    fn snapshot_captures_state_and_value() {
        let mut tx = ToggleExternal::new();
        tx.send(ToggleEvent::PointerEnter);
        tx.send(ToggleEvent::PointerDown);
        tx.send(ToggleEvent::PointerUp);
        let snap = tx.snapshot();
        assert_eq!(snap.state(), ToggleState::Hover);
        assert!(snap.is_on());
        assert_eq!(
            snap.query("state"),
            Some(IntrospectValue::Text("Hover".to_string()))
        );
        assert_eq!(snap.query("value"), Some(IntrospectValue::Bool(true)));
    }

    #[test]
    fn snapshot_intervene_is_always_read_only() {
        let mut snap = ToggleStateSnapshot::new(ToggleState::Idle, false);
        assert_eq!(
            snap.intervene("value", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn snapshot_invoke_always_rejects() {
        let mut snap = ToggleStateSnapshot::new(ToggleState::Idle, false);
        assert_eq!(
            snap.invoke("send", IntrospectValue::Text("PointerEnter".to_string())),
            Err(InvokeError::Rejected)
        );
    }

    // ----- R51.55 §5.39 keyboard activation -----

    #[test]
    fn keyboard_activate_from_idle_flips_value_and_emits_toggle_intent() {
        let mut bx = ToggleExternal::new();
        assert_eq!(bx.state(), ToggleState::Idle);
        assert!(!bx.is_on());
        bx.send(ToggleEvent::KeyboardActivate);
        assert_eq!(bx.state(), ToggleState::Idle, "state-stable internal");
        assert!(bx.is_on(), "value must flip Off → On");
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "toggle");
        assert_eq!(harvested[0].payload, IntrospectValue::Bool(true));
    }

    #[test]
    fn keyboard_activate_from_hover_flips_value_and_emits_toggle_intent() {
        let mut bx = ToggleExternal::new();
        bx.send(ToggleEvent::PointerEnter);
        assert_eq!(bx.state(), ToggleState::Hover);
        bx.send(ToggleEvent::KeyboardActivate);
        assert_eq!(bx.state(), ToggleState::Hover);
        assert!(bx.is_on());
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "toggle");
    }

    #[test]
    fn keyboard_activate_from_disabled_emits_no_intent() {
        let mut bx = ToggleExternal::new();
        bx.send(ToggleEvent::Disable);
        bx.send(ToggleEvent::KeyboardActivate);
        assert!(!bx.is_on(), "disabled toggle must not flip");
        let mut harvested: Vec<Intent> = Vec::new();
        bx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.is_empty());
    }

    #[test]
    fn keyboard_activate_via_invoke_send_flips() {
        let mut bx = ToggleExternal::new();
        let _ = bx
            .invoke(
                "send",
                IntrospectValue::Text("KeyboardActivate".to_string()),
            )
            .unwrap();
        assert!(bx.is_on());
    }
}
