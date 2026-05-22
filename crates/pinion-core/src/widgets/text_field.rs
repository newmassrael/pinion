//! R56.1 §5.38 — `TextField` widget catalogue entry.
//!
//! First slice of the R56 axis: §5.38 SCXML statechart + Rust binding
//! ([`TextField`] / [`TextFieldEvent`] / [`TextFieldState`] /
//! [`TextFieldExternal`]). Subsequent sub-rounds layer on top:
//! caret rendering (R56.1.b), blink animation (R56.1.c), key input
//! (R56.1.d), clipboard (R56.1.e), selection (R56.1.f), and IME
//! composition (R56.1.g). R55.D.1 → R55.D.2 cascade mirror — land the
//! interaction substrate first so the following sub-rounds compose
//! without re-engineering it.
//!
//! ## State model
//!
//! Four-state interaction machine (file `widgets/text_field.scxml`):
//! Idle / Focused / Editing / Disabled.
//!
//! | State | Meaning |
//! |---|---|
//! | `Idle` | not focused; caret hidden; no key dispatch. |
//! | `Focused` | focused; caret visible (R56.1.b); key dispatch active (R56.1.d). |
//! | `Editing` | focused + IME composition active; preedit text visible (R56.1.g). |
//! | `Disabled` | absorbing class until [`TextFieldEvent::Enable`] arrives. |
//!
//! Unlike Slider's typed f32 sidecar (R51.39 §5.38), `TextField`
//! carries **no per-state value sidecar on this first slice** — the
//! text content (`String` + caret position) and the IME preedit
//! buffer arrive with R56.1.b / R56.1.g respectively, mirroring the
//! R55.D.2 `ScrollBar` staging (statechart first, content sidecar in
//! a later sub-round). The introspect schema therefore declares only
//! `state` (read) and `send` (invoke) on R56.1.a.
//!
//! ## Intent contract
//!
//! Emits the §5.20 `"text_committed"` intent on the two SCXML
//! commit-raise transitions:
//!
//! - `Editing → Focused` via [`TextFieldEvent::CommitEdit`] — IME
//!   commit (e.g. Enter during composition; Wayland
//!   `text-input-v3` `commit_string`).
//! - `Editing → Idle` via [`TextFieldEvent::Blur`] — focus loss
//!   during composition. IME canonical commit-on-blur (matches macOS
//!   `NSTextInputContext` / GTK `IBus` / Wayland `text-input-v3` /
//!   Windows TSF). Applications that want the cancel-on-blur variant
//!   intercept `Blur` before forwarding.
//!
//! The cancel paths stay silent:
//!
//! - `Editing → Focused` via [`TextFieldEvent::CancelEdit`] — IME
//!   cancel preedit (e.g. Escape during composition; preedit
//!   discarded without committing).
//! - `Editing → Disabled` via [`TextFieldEvent::Disable`] — widget
//!   disabled during composition; preedit dropped on the floor
//!   (matches Wayland IME disable contract).
//!
//! Intent payload is [`IntrospectValue::Null`] on R56.1.a — the
//! committed-text payload (`IntrospectValue::Text`) arrives with
//! R56.1.g once the IME preedit buffer substrate lands.

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
    include!(concat!(env!("OUT_DIR"), "/text_field_sm.rs"));
}

pub use sm::{TextFieldEvent, TextFieldState};
use sm::TextFieldPolicy;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};

/// `TextField` widget state machine.
///
/// Four-state interaction model (Idle / Focused / Editing / Disabled)
/// generated from `widgets/text_field.scxml`. Carries no value
/// sidecar on R56.1.a — the text content and IME preedit buffer
/// arrive with later sub-rounds. Mirrors the R55.D.2 `ScrollBar`
/// staging where the visible peer's statechart lands before the
/// reactive content container.
pub struct TextField {
    inner: Widget<TextFieldPolicy>,
}

impl TextField {
    /// Construct a `TextField` in the [`TextFieldState::Idle`] state.
    /// W3C ARIA `textbox` role canonical (un-focused) default.
    #[must_use]
    pub fn new() -> Self {
        Self { inner: Widget::new() }
    }

    /// Drive a [`TextFieldEvent`] through the SCXML. Pure state
    /// transition — no value sidecar mutation on R56.1.a.
    pub fn send(&mut self, event: TextFieldEvent) {
        self.inner.send(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> TextFieldState {
        self.inner.state()
    }
}

impl Default for TextField {
    fn default() -> Self {
        Self::new()
    }
}

/// R56.1.a §5.38 — `TextField` transition contract. The snapshot is
/// just the SCXML state (no value sidecar on this slice). Detection
/// emits a single `"text_committed"` intent on the two commit-raise
/// transitions out of [`TextFieldState::Editing`]
/// ([`TextFieldEvent::CommitEdit`] → `Focused`,
/// [`TextFieldEvent::Blur`] → `Idle`). The
/// [`TextFieldEvent::CancelEdit`] / [`TextFieldEvent::Disable`] exit
/// paths stay silent — matches the IME canonical
/// cancel-discards-preedit + disable-drops-preedit behaviour.
///
/// State-pair-only detection (the `ScrollBar` R55.D.2 / Slider
/// R51.14 idiom) does not work here — `Editing → Focused` is
/// reachable via
/// **both** [`TextFieldEvent::CommitEdit`] (commit) and
/// [`TextFieldEvent::CancelEdit`] (silent). The `event` argument is
/// the R51.54 §5.39 surface added exactly for this case: distinguish
/// commit from cancel when the state-pair signature is ambiguous.
impl WidgetTransition for TextField {
    type Event = TextFieldEvent;
    type Snapshot = TextFieldState;

    fn snapshot(&self) -> Self::Snapshot {
        self.state()
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(
        before: Self::Snapshot,
        event: Self::Event,
        after: Self::Snapshot,
    ) -> Vec<Intent> {
        let commit_via_event = matches!(
            event,
            TextFieldEvent::CommitEdit | TextFieldEvent::Blur,
        );
        let exited_editing = matches!(before, TextFieldState::Editing)
            && !matches!(after, TextFieldState::Editing);
        if commit_via_event && exited_editing {
            vec![Intent::new_static("text_committed", IntrospectValue::Null)]
        } else {
            Vec::new()
        }
    }
}

/// `External` adapter wrapping a [`TextField`]. Emits a single
/// intent kind on R56.1.a:
///
/// * `"text_committed"` ([`IntrospectValue::Null`] payload) on
///   `Editing → Focused` ([`TextFieldEvent::CommitEdit`]) or
///   `Editing → Idle` ([`TextFieldEvent::Blur`]).
///
/// Future sub-rounds extend the surface — R56.1.g attaches the
/// preedit-buffer payload, R56.1.d adds the key-dispatch surface,
/// R56.1.f wires the ARIA `textbox` accessible name/value.
pub struct TextFieldExternal {
    em: IntentEmitter<TextField>,
}

impl TextFieldExternal {
    /// Construct a `TextFieldExternal` wrapping a fresh
    /// [`TextField`] in [`TextFieldState::Idle`].
    #[must_use]
    pub fn new() -> Self {
        Self { em: IntentEmitter::default() }
    }

    /// Drive a [`TextFieldEvent`] and queue a `"text_committed"`
    /// intent on the two commit-raise transitions. Pipeline lives on
    /// [`IntentEmitter::dispatch`]; the detection rule lives on the
    /// [`WidgetTransition`] impl for [`TextField`].
    pub fn send(&mut self, event: TextFieldEvent) {
        self.em.dispatch(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> TextFieldState {
        self.em.inner.state()
    }
}

impl Default for TextFieldExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for TextFieldExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextFieldExternal")
            .field("state", &self.state())
            .finish()
    }
}

impl External for TextFieldExternal {
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

impl ExternalIntrospect for TextFieldExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[("state", "string"), ("send", "string")])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(
                text_field_state_name(self.state()).to_string(),
            )),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            // §5.38 — `state` is SCXML-owned (driven via `send`).
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
            "send" => match args {
                IntrospectValue::Text(ref name) => {
                    let ev =
                        parse_text_field_event(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(
                        text_field_state_name(self.state()).to_string(),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn text_field_state_name(state: TextFieldState) -> &'static str {
    match state {
        TextFieldState::Idle => "Idle",
        TextFieldState::Focused => "Focused",
        TextFieldState::Editing => "Editing",
        TextFieldState::Disabled => "Disabled",
    }
}

/// External-introspect [`InvokeError::Rejected`] guard. Lowercase /
/// snake-case event aliases are not accepted — the AI client passes
/// the `PascalCase` variant name verbatim, mirroring the
/// `parse_scroll_bar_event` / `parse_slider_event` convention.
fn parse_text_field_event(name: &str) -> Option<TextFieldEvent> {
    match name {
        "Focus" => Some(TextFieldEvent::Focus),
        "Blur" => Some(TextFieldEvent::Blur),
        "BeginEdit" => Some(TextFieldEvent::BeginEdit),
        "CommitEdit" => Some(TextFieldEvent::CommitEdit),
        "CancelEdit" => Some(TextFieldEvent::CancelEdit),
        "Disable" => Some(TextFieldEvent::Disable),
        "Enable" => Some(TextFieldEvent::Enable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.a §5.38 — `TextField` widget binding regression battery.
    //! Mirror of the R55.D.2 `ScrollBar` test layout: initial state,
    //! four-state transition graph, commit/cancel detection, ARIA
    //! commit-on-blur path, introspect surface.

    use super::{
        parse_text_field_event, text_field_state_name, TextField, TextFieldEvent,
        TextFieldExternal, TextFieldState,
    };
    use crate::external::{
        Backend, External, ExternalIntrospect, InterveneError, IntrospectValue,
        InvokeError, RepaintOwner, ThreadOwnership,
    };

    // ─────────────────────────────────────────────────────────────
    // SCXML state machine — transition graph
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn initial_state_is_idle() {
        assert_eq!(TextField::new().state(), TextFieldState::Idle);
    }

    #[test]
    fn focus_transitions_idle_to_focused() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        assert_eq!(tf.state(), TextFieldState::Focused);
    }

    #[test]
    fn blur_returns_focused_to_idle() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::Blur);
        assert_eq!(tf.state(), TextFieldState::Idle);
    }

    #[test]
    fn begin_edit_from_focused_enters_editing() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        assert_eq!(tf.state(), TextFieldState::Editing);
    }

    #[test]
    fn commit_edit_returns_editing_to_focused() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::CommitEdit);
        assert_eq!(tf.state(), TextFieldState::Focused);
    }

    #[test]
    fn cancel_edit_returns_editing_to_focused() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::CancelEdit);
        assert_eq!(tf.state(), TextFieldState::Focused);
    }

    #[test]
    fn blur_from_editing_drops_to_idle() {
        // IME canonical: focus loss during composition exits all the
        // way to Idle (and commits — see the *External* test below).
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::Blur);
        assert_eq!(tf.state(), TextFieldState::Idle);
    }

    #[test]
    fn disable_from_any_state_enters_disabled() {
        // SCXML lands the `disable` transition on idle / focused /
        // editing — every non-disabled state is reachable.
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Disable);
        assert_eq!(tf.state(), TextFieldState::Disabled);

        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::Disable);
        assert_eq!(tf.state(), TextFieldState::Disabled);

        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Focus);
        tf.send(TextFieldEvent::BeginEdit);
        tf.send(TextFieldEvent::Disable);
        assert_eq!(tf.state(), TextFieldState::Disabled);
    }

    #[test]
    fn enable_from_disabled_returns_to_idle() {
        let mut tf = TextField::new();
        tf.send(TextFieldEvent::Disable);
        tf.send(TextFieldEvent::Enable);
        assert_eq!(tf.state(), TextFieldState::Idle);
    }

    // ─────────────────────────────────────────────────────────────
    // Intent emission — commit / cancel / disable
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn commit_edit_emits_text_committed_intent() {
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::CommitEdit);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "exactly one commit per CommitEdit");
        assert_eq!(harvested[0].tag_str(), "text_committed");
        assert!(matches!(harvested[0].payload, IntrospectValue::Null));
    }

    #[test]
    fn blur_during_editing_emits_text_committed_intent() {
        // IME canonical commit-on-blur — Wayland text-input-v3, GTK
        // IBus, macOS NSTextInputContext, Windows TSF.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::Blur);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "commit-on-blur is canonical IME");
        assert_eq!(harvested[0].tag_str(), "text_committed");
    }

    #[test]
    fn cancel_edit_does_not_emit_commit() {
        // Escape-during-composition discards preedit silently.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::CancelEdit);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "CancelEdit must discard preedit silently",
        );
    }

    #[test]
    fn disable_during_editing_does_not_emit_commit() {
        // Wayland IME disable contract — drop preedit on the floor.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::BeginEdit);
        tfx.send(TextFieldEvent::Disable);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "Disable during editing must drop preedit silently",
        );
    }

    #[test]
    fn blur_from_focused_does_not_emit_commit() {
        // Focus loss without composition is not a commit event —
        // only `Editing → *` raises `textfield.commit` in the SCXML.
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        tfx.send(TextFieldEvent::Blur);
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "blur without composition must not commit",
        );
    }

    // ─────────────────────────────────────────────────────────────
    // External adapter — introspect surface
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn external_schema_declares_two_slots() {
        // R56.1.a surface: state + send. No `value` slot yet — text
        // content sidecar arrives with R56.1.b.
        let tfx = TextFieldExternal::new();
        let schema = tfx.schema();
        assert_eq!(
            schema.fields,
            &[("state", "string"), ("send", "string")],
        );
    }

    #[test]
    fn external_query_state_returns_pascal_case() {
        let tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.query("state").unwrap(),
            IntrospectValue::Text("Idle".to_string()),
        );
        assert_eq!(tfx.query("value"), None);
    }

    #[test]
    fn external_query_state_reflects_transitions() {
        let mut tfx = TextFieldExternal::new();
        tfx.send(TextFieldEvent::Focus);
        assert_eq!(
            tfx.query("state").unwrap(),
            IntrospectValue::Text("Focused".to_string()),
        );
        tfx.send(TextFieldEvent::BeginEdit);
        assert_eq!(
            tfx.query("state").unwrap(),
            IntrospectValue::Text("Editing".to_string()),
        );
    }

    #[test]
    fn external_intervene_state_read_only() {
        let mut tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.intervene(
                "state",
                IntrospectValue::Text("Focused".to_string()),
            ),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(tfx.state(), TextFieldState::Idle);
    }

    #[test]
    fn external_intervene_unknown_path_rejects() {
        let mut tfx = TextFieldExternal::new();
        assert_eq!(
            tfx.intervene("value", IntrospectValue::Text(String::new())),
            Err(InterveneError::UnknownPath),
        );
    }

    #[test]
    fn external_invoke_send_drives_transition() {
        let mut tfx = TextFieldExternal::new();
        let out = tfx
            .invoke("send", IntrospectValue::Text("Focus".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Focused".to_string()));
        assert_eq!(tfx.state(), TextFieldState::Focused);
    }

    #[test]
    fn external_invoke_send_rejects_unknown_event() {
        let mut tfx = TextFieldExternal::new();
        let r = tfx.invoke("send", IntrospectValue::Text("Click".to_string()));
        assert_eq!(r, Err(InvokeError::Rejected));
    }

    #[test]
    fn external_invoke_send_rejects_non_text_args() {
        let mut tfx = TextFieldExternal::new();
        let r = tfx.invoke("send", IntrospectValue::Int(0));
        assert_eq!(r, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn external_invoke_unknown_path_rejects() {
        let mut tfx = TextFieldExternal::new();
        let r = tfx.invoke("set_text", IntrospectValue::Text(String::new()));
        assert_eq!(r, Err(InvokeError::UnknownPath));
    }

    #[test]
    fn external_invoke_full_commit_cycle_returns_focused_state_name() {
        // End-to-end: Focus → BeginEdit → CommitEdit. The final
        // `send` returns the post-transition state name as proof of
        // the dispatch round-trip through introspect.
        let mut tfx = TextFieldExternal::new();
        tfx.invoke("send", IntrospectValue::Text("Focus".to_string())).unwrap();
        tfx.invoke("send", IntrospectValue::Text("BeginEdit".to_string()))
            .unwrap();
        let out = tfx
            .invoke("send", IntrospectValue::Text("CommitEdit".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Focused".to_string()));
        // And a `text_committed` intent was queued.
        let mut harvested = Vec::new();
        tfx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "text_committed");
    }

    // ─────────────────────────────────────────────────────────────
    // External adapter — backend + ownership contract
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn external_backend_support_declares_gui_and_rpc() {
        // R56.1.a — TextField is GUI + RPC visible; the TUI backend
        // gates on R49+ key dispatch + R47.5 text shaping carry, so
        // it is not declared on this slice. `Skip` fallback matches
        // the ScrollBar R55.D.2 convention.
        let tfx = TextFieldExternal::new();
        let bs = tfx.backends();
        assert!(bs.supported.contains(&Backend::Gui));
        assert!(bs.supported.contains(&Backend::Rpc));
    }

    #[test]
    fn external_repaint_ownership_is_framework() {
        // Framework drives repaint — caret blink (R56.1.c) will be
        // an animation-tick driver under framework ownership, not a
        // self-repainting widget. Matches Slider / ScrollBar.
        let tfx = TextFieldExternal::new();
        assert!(matches!(tfx.repaint_ownership(), RepaintOwner::Framework));
    }

    #[test]
    fn external_thread_ownership_is_ui_thread_sync() {
        // No background work on R56.1.a — IME composition (R56.1.g)
        // may need an OS-side preedit channel later, but the
        // statechart itself stays UI-thread synchronous.
        let tfx = TextFieldExternal::new();
        assert!(matches!(
            tfx.thread_ownership(),
            ThreadOwnership::UiThreadSync,
        ));
    }

    #[test]
    fn external_does_not_want_pointer_capture() {
        // No drag interaction on R56.1.a — the default `External`
        // returns `false` for `wants_pointer_capture`. R56.1.f
        // selection (mouse-drag selection) may revisit this, but
        // the canonical text-input contract is *cursor* + *keyboard*,
        // not drag-style pointer capture.
        let tfx = TextFieldExternal::new();
        assert!(!tfx.wants_pointer_capture());
    }

    // ─────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn state_name_helper_covers_every_variant() {
        // Guard the parse_text_field_event ↔ text_field_state_name
        // string round-trip — every state's name must be stable so
        // RPC consumers can build assertions against it.
        assert_eq!(text_field_state_name(TextFieldState::Idle), "Idle");
        assert_eq!(text_field_state_name(TextFieldState::Focused), "Focused");
        assert_eq!(text_field_state_name(TextFieldState::Editing), "Editing");
        assert_eq!(text_field_state_name(TextFieldState::Disabled), "Disabled");
    }

    #[test]
    fn event_parser_covers_every_input_variant() {
        // Every externally-dispatchable event resolves. The internal
        // `TextfieldCommit` raise event is NOT in the parser table
        // (consumers do not drive raised events directly).
        assert!(matches!(
            parse_text_field_event("Focus"),
            Some(TextFieldEvent::Focus),
        ));
        assert!(matches!(
            parse_text_field_event("Blur"),
            Some(TextFieldEvent::Blur),
        ));
        assert!(matches!(
            parse_text_field_event("BeginEdit"),
            Some(TextFieldEvent::BeginEdit),
        ));
        assert!(matches!(
            parse_text_field_event("CommitEdit"),
            Some(TextFieldEvent::CommitEdit),
        ));
        assert!(matches!(
            parse_text_field_event("CancelEdit"),
            Some(TextFieldEvent::CancelEdit),
        ));
        assert!(matches!(
            parse_text_field_event("Disable"),
            Some(TextFieldEvent::Disable),
        ));
        assert!(matches!(
            parse_text_field_event("Enable"),
            Some(TextFieldEvent::Enable),
        ));
        assert_eq!(parse_text_field_event("textfield_commit"), None);
        assert_eq!(parse_text_field_event(""), None);
    }
}
