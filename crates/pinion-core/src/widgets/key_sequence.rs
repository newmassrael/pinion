//! R1569 §5.38 §5.39 §5.20 — `KeySequenceEdit`: the field a keyboard shortcut
//! is *recorded* into (Qt [`QKeySequenceEdit`]).
//!
//! This is the widget every keymap-preferences pane is built from, and the one
//! widget whose entire job is to **not** let the window's shortcuts fire: a
//! chord that already means something must still be recordable as itself. It
//! is therefore the extreme consumer of the R1569 accelerator-shadow axis —
//! while recording it claims every chord, where a
//! [`TextField`](crate::widgets::text_field::TextFieldExternal) claims only the
//! chords that are its own text.
//!
//! ## Where this is more than the toolkit 6.11
//!
//! 1. **Recording is a state the user chose.** the toolkit starts on focus-in and stops
//!    only on focus-out or a 1-second release timer, so a toolkit keymap editor
//!    cannot stop grabbing while it still has focus, and a user who tabs into
//!    the field and then presses the application's Save shortcut silently
//!    overwrites the binding they were reading. `record` / `cancel` are events
//!    here (`widgets/key_sequence.scxml`), which is also what makes
//!    [`crate::external::External::shadows_accelerator`]
//!    a function of something observable rather than of a private timer.
//! 2. **A modifier-only press is kept, not dropped.** the toolkit's `keyPressEvent`
//!    returns early for <kbd>Ctrl</kbd> / <kbd>Shift</kbd> / <kbd>Alt</kbd> /
//!    <kbd>Meta</kbd>, so a held modifier is invisible: the field shows nothing
//!    until a real key lands. [`KeySequenceEdit::pending`] publishes the prefix,
//!    so `Ctrl+…` can be shown while the user is still deciding.
//! 3. **Overflow is a named refusal.** key-sequence editor's documented
//!    behaviour is that a sequence longer than `maximumSequenceLength()` "is
//!    truncated" — silently, with no return value — so a caller cannot tell a
//!    sequence that fit from one that was cut down.
//!    [`crate::accelerator::SequenceFull`] carries the chord that
//!    did not fit.
//! 4. **A recorded chord reports what it would COLLIDE with.** the toolkit has nothing
//!    here: key-sequence editor will happily record a chord that is already a
//!    shortcut, and the collision surfaces only later, at dispatch, as
//!    `isAmbiguous()`. The conflict is derived from the two
//!    accelerator layers that actually exist
//!    ([`AcceleratorLayer`](crate::accelerator::AcceleratorLayer)) and answered
//!    by `scene/accelerators` — a fact about the WINDOW rather than about this
//!    widget, which is why it is not a slot here.
//!
//! ## What is deliberately the toolkit's shape
//!
//! The default maximum is the toolkit's four
//! ([`crate::accelerator::QT_MAX_SEQUENCE_LENGTH`]),
//! and focus loss **accepts** the in-flight sequence because `focusOutEvent`
//! calls `finishEditing()` in the toolkit. Neither is a capability, so neither is worth
//! diverging on ([[the toolkit-is-the-floor-not-the-target]] leaves the *shape* a fresh
//! choice, and here the toolkit's shape is already right).
//!
//! [`QKeySequenceEdit`]: https://doc.qt.io/qt-6/qkeysequenceedit.html

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
    include!(concat!(env!("OUT_DIR"), "/key_sequence_sm.rs"));
}

use sm::KeySequencePolicy;
pub use sm::{KeySequenceEvent, KeySequenceState};

use crate::WidgetStateName;
use crate::accelerator::{Chord, KeySequence, QT_MAX_SEQUENCE_LENGTH, SequenceFull};
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::input::Modifiers;
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};
use std::cell::RefCell;
use std::rc::Rc;

/// R1569 §5.20 — the §5.20 intent name an accepted recording emits.
///
/// The undotted half; the framework's intent walk prefixes the widget's tag,
/// exactly as [`TEXT_COMMITTED_EVENT`](crate::widgets::commit::TEXT_COMMITTED_EVENT)
/// is prefixed.
pub const KEY_SEQUENCE_CAPTURED_EVENT: &str = "key_sequence_captured";

/// R1569 §5.38 — what happened to a chord offered to a recording editor.
///
/// Every arm is an outcome key-sequence editor also produces and does not
/// report: it records, ignores a modifier, or truncates, and the caller learns
/// which only by re-reading `keySequence()` and inferring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordOutcome {
    /// The chord was appended at this index.
    Recorded(usize),
    /// The chord was a bare modifier and is held as a **prefix** rather than
    /// recorded. The toolkit drops it entirely.
    Pending,
    /// The widget was not in [`KeySequenceState::Recording`], so the chord was
    /// not offered to the sequence at all.
    NotRecording,
}

/// R1569 §5.38 §5.22 — the editor's spelling, shared with the view fn.
///
/// A [`WidgetCore::State`](crate::WidgetCore::State) is `Copy`, and a chord
/// sequence is `String`-shaped, so the paint cannot ride the cached state
/// projection the way a slider's `f32` does. That is not a gap: it is the
/// case [`use_text_edit_state`](crate::widgets::text_edit::use_text_edit_state)
/// and [`use_caret_blink`](crate::widgets::caret_blink::use_caret_blink)
/// already exist for — an `Owner::cache` handle both the External and the view
/// fn resolve by tag, so there is one value rather than two that must agree
/// ([[use-substrate-not-hand-rolled-equivalent]]).
#[derive(Debug, Default)]
pub struct KeySequenceDisplay {
    accepted: RefCell<String>,
    in_flight: RefCell<String>,
    pending: RefCell<String>,
}

impl KeySequenceDisplay {
    /// The accepted spelling — what the field shows at rest.
    #[must_use]
    pub fn accepted(&self) -> String {
        self.accepted.borrow().clone()
    }

    /// The run being built right now (empty when not recording).
    #[must_use]
    pub fn in_flight(&self) -> String {
        self.in_flight.borrow().clone()
    }

    /// The held-modifier prefix, `"Ctrl+Shift+"`. Empty when none is held.
    #[must_use]
    pub fn pending(&self) -> String {
        self.pending.borrow().clone()
    }
}

/// R1569 §5.38 §5.22 — resolve the tag-keyed [`KeySequenceDisplay`].
///
/// The `Owner::cache` key is `(TypeId, tag)`, so this composes with
/// `use_text_edit_state(tag)` and `use_caret_blink(tag)` on the same tag
/// without collision.
///
/// # Panics
///
/// Panics if no current [`Owner`](crate::reactive::Owner) is set — i.e. when
/// invoked outside a `root_owner.run(...)` wrap, exactly as its two siblings do.
#[must_use]
pub fn use_key_sequence_display(key: &'static str) -> Rc<KeySequenceDisplay> {
    crate::reactive::Owner::current()
        .expect("use_key_sequence_display requires an active Owner scope")
        .cache(key, KeySequenceDisplay::default)
}

/// R1569 §5.38 — the recording state machine plus the sequence it builds.
pub struct KeySequenceEdit {
    inner: Widget<KeySequencePolicy>,
    /// The accepted sequence — what the widget displays when not recording.
    sequence: KeySequence,
    /// The sequence being built while [`KeySequenceState::Recording`].
    in_flight: KeySequence,
    /// Modifiers held with no key yet. The toolkit discards this fact.
    pending: Option<Modifiers>,
    /// The toolkit's `maximumSequenceLength`.
    max_len: usize,
}

impl KeySequenceEdit {
    /// An empty editor in [`KeySequenceState::Idle`], bounded to the toolkit's four.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Widget::new(),
            sequence: KeySequence::new(),
            in_flight: KeySequence::new(),
            pending: None,
            max_len: QT_MAX_SEQUENCE_LENGTH,
        }
    }

    /// Declare the maximum chord count (the toolkit `setMaximumSequenceLength`).
    ///
    /// A `max` of zero is raised to one: a sequence that can hold nothing is a
    /// widget that cannot do its job, and the toolkit clamps the same way.
    #[must_use]
    pub fn with_max_len(mut self, max: usize) -> Self {
        self.max_len = max.max(1);
        self
    }

    /// Seed the displayed sequence without recording it (the toolkit `setKeySequence`).
    ///
    /// # Errors
    ///
    /// [`SequenceFull`] naming the first chord past `max_len`, where the toolkit truncates and
    /// says nothing. Nothing is stored when the call refuses, so a refused
    /// seed cannot leave a half-applied value behind.
    pub fn set_sequence(&mut self, sequence: &KeySequence) -> Result<(), SequenceFull> {
        let mut next = KeySequence::new();
        for chord in sequence.chords() {
            next.push(chord.clone(), self.max_len)?;
        }
        self.sequence = next;
        Ok(())
    }

    /// Drive a [`KeySequenceEvent`] through the SCXML.
    ///
    /// Entering [`KeySequenceState::Recording`] clears the in-flight buffer;
    /// the two accepting exits (`Commit` / `Blur`) publish it, and `Cancel`
    /// discards it leaving the previous sequence in place.
    pub fn send(&mut self, event: KeySequenceEvent) {
        let before = self.inner.state();
        self.inner.send(event);
        let after = self.inner.state();
        if before == after {
            return;
        }
        match after {
            KeySequenceState::Recording => {
                self.in_flight = KeySequence::new();
                self.pending = None;
            }
            KeySequenceState::Idle | KeySequenceState::Disabled => {
                if before == KeySequenceState::Recording {
                    // `Cancel` is the one exit that does not accept, and the
                    // SCXML is the authority on which those are: only the
                    // accepting transitions raise `keysequence.captured`.
                    if matches!(event, KeySequenceEvent::Commit | KeySequenceEvent::Blur) {
                        self.sequence = std::mem::take(&mut self.in_flight);
                    }
                    self.in_flight = KeySequence::new();
                    self.pending = None;
                }
            }
        }
    }

    /// Current statechart state.
    #[must_use]
    pub fn state(&self) -> KeySequenceState {
        self.inner.state()
    }

    /// Whether this editor claims chords right now.
    #[must_use]
    pub fn is_recording(&self) -> bool {
        matches!(self.inner.state(), KeySequenceState::Recording)
    }

    /// The accepted sequence.
    #[must_use]
    pub fn sequence(&self) -> &KeySequence {
        &self.sequence
    }

    /// The sequence being built right now (empty when not recording).
    #[must_use]
    pub fn in_flight(&self) -> &KeySequence {
        &self.in_flight
    }

    /// Modifiers held with no key yet — the prefix the toolkit discards.
    #[must_use]
    pub const fn pending(&self) -> Option<Modifiers> {
        self.pending
    }

    /// The toolkit's `maximumSequenceLength`.
    #[must_use]
    pub const fn max_len(&self) -> usize {
        self.max_len
    }

    /// Offer `chord` to the in-flight sequence.
    ///
    /// Appends and reports; it does **not** commit. Reaching `max_len` finishes the
    /// recording (the toolkit's key-sequence editor does the same), but that
    /// is a statechart transition carrying a §5.20 intent, so it is driven by
    /// [`KeySequenceEditExternal::record`] through the emitter rather than raised behind it — a commit that
    /// skipped the emitter would be a transition whose intent silently never
    /// fired.
    pub(crate) fn record(&mut self, chord: &Chord) -> RecordOutcome {
        if !self.is_recording() {
            return RecordOutcome::NotRecording;
        }
        if chord.is_modifier_only() {
            self.pending = Some(chord.modifiers());
            return RecordOutcome::Pending;
        }
        self.pending = None;
        // The bound cannot be hit here: `KeySequenceEditExternal::record`
        // commits the moment the run fills, so a recording editor always has
        // room. `expect` states that rather than inventing an outcome for it.
        let index = self
            .in_flight
            .push(chord.clone(), self.max_len)
            .expect("a recording editor is never at its bound — a full run commits");
        RecordOutcome::Recorded(index)
    }

    /// Whether the in-flight sequence has reached its declared maximum.
    ///
    /// Crate-private with [`Self::record`], for the same reason: the pair is
    /// how the External keeps the bound, not a fact a consumer needs.
    #[must_use]
    pub(crate) fn is_in_flight_full(&self) -> bool {
        self.in_flight.len() >= self.max_len
    }
}

impl Default for KeySequenceEdit {
    fn default() -> Self {
        Self::new()
    }
}

/// R1569 §5.20 — the transition contract. The single accepting signature is
/// `Recording → Idle`, which both `Commit` and `Blur` reach and `Cancel` also
/// reaches — the `TextField` R51.54 case exactly, so detection reads the
/// EVENT rather than the state pair.
impl WidgetTransition for KeySequenceEdit {
    type Event = KeySequenceEvent;
    type Snapshot = KeySequenceState;

    fn snapshot(&self) -> Self::Snapshot {
        self.state()
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(before: Self::Snapshot, event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        let accepted = matches!(event, KeySequenceEvent::Commit | KeySequenceEvent::Blur);
        if before == KeySequenceState::Recording && after == KeySequenceState::Idle && accepted {
            vec![Intent::new_static(
                KEY_SEQUENCE_CAPTURED_EVENT,
                IntrospectValue::Null,
            )]
        } else {
            Vec::new()
        }
    }
}

/// R1569 §5.15 §5.38 — the `External` face of [`KeySequenceEdit`].
pub struct KeySequenceEditExternal {
    em: IntentEmitter<KeySequenceEdit>,
    /// The view fn's read of this widget's spelling. Written on every change
    /// rather than pulled, so the paint never has to ask an External it does
    /// not own; `None` for a bare `new()` (unit tests), matching the
    /// `TextFieldExternal::text_state` optionality.
    display: Option<Rc<KeySequenceDisplay>>,
    /// Bumped on every mutation, so a binding's `Copy`
    /// [`WidgetCore::State`](crate::WidgetCore::State) can carry a value that
    /// CHANGES when the spelling does.
    ///
    /// Without it the shell's change detection compares two equal statechart
    /// states and skips the repaint, so a chord recorded into an
    /// already-`Recording` editor would never reach the pixels. `TextField`
    /// carries its caret offset in the same slot for the same reason.
    revision: u32,
}

impl KeySequenceEditExternal {
    /// An empty editor in [`KeySequenceState::Idle`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
            display: None,
            revision: 0,
        }
    }

    /// Declare the maximum chord count (the toolkit `setMaximumSequenceLength`).
    #[must_use]
    pub fn with_max_len(mut self, max: usize) -> Self {
        let inner = std::mem::take(&mut self.em.inner);
        self.em.inner = inner.with_max_len(max);
        self
    }

    /// Attach the tag-keyed [`KeySequenceDisplay`] the view fn reads
    /// (`TextFieldExternal::attach_state`'s shape).
    #[must_use]
    pub fn attach_display(mut self, display: Rc<KeySequenceDisplay>) -> Self {
        self.display = Some(display);
        self.publish();
        self
    }

    /// Drive a [`KeySequenceEvent`] through the statechart + intent channel.
    pub fn send(&mut self, event: KeySequenceEvent) {
        self.em.dispatch(event);
        self.publish();
    }

    /// Mirror the widget's spelling into the shared handle.
    ///
    /// Called after every mutation rather than at read time: a `&self` query
    /// path cannot write, and a paint that re-derived the spelling would be a
    /// second implementation of `KeySequence::portable` free to disagree with
    /// the one the wire publishes.
    fn publish(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        let Some(display) = &self.display else { return };
        let edit = &self.em.inner;
        display.accepted.replace(edit.sequence().portable());
        display.in_flight.replace(edit.in_flight().portable());
        display
            .pending
            .replace(edit.pending().map_or_else(String::new, |m| {
                let mut probe = Chord::new(String::new(), m).portable();
                if probe.is_empty() {
                    probe.push('+');
                }
                probe
            }));
    }

    /// Offer a chord to the recording sequence.
    ///
    /// A chord that FILLS the sequence commits it, and a commit is an
    /// intent-bearing transition, so it is driven through the emitter — a
    /// fill-commit therefore emits exactly the intent an explicit commit does,
    /// rather than being a second path that silently emits nothing.
    pub fn record(&mut self, chord: &Chord) -> RecordOutcome {
        let outcome = self.em.inner.record(chord);
        if matches!(outcome, RecordOutcome::Recorded(_)) && self.em.inner.is_in_flight_full() {
            self.send(KeySequenceEvent::Commit);
        }
        self.publish();
        outcome
    }

    /// Read-only view of the statechart + sequence.
    #[must_use]
    pub fn edit(&self) -> &KeySequenceEdit {
        &self.em.inner
    }

    /// Statechart state.
    #[must_use]
    pub fn state(&self) -> KeySequenceState {
        self.em.inner.state()
    }
}

impl Default for KeySequenceEditExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for KeySequenceEditExternal {
    /// The `External` bound's `Debug`. Prints the facts a failing test needs —
    /// the state, what is recorded, and what is in flight — rather than the
    /// emitter's private buffer.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let edit = &self.em.inner;
        f.debug_struct("KeySequenceEditExternal")
            .field("state", &edit.state())
            .field("sequence", &edit.sequence().portable())
            .field("in_flight", &edit.in_flight().portable())
            .field("pending", &edit.pending())
            .field("max_len", &edit.max_len())
            .field("revision", &self.revision)
            // Whether the shared handle is attached, not its contents: the
            // contents are the four fields above, and printing them twice
            // would let one copy read stale in a failure message.
            .field("display_attached", &self.display.is_some())
            .finish()
    }
}

impl External for KeySequenceEditExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
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

    /// R1569 §5.39 — while recording, EVERY chord is this widget's.
    ///
    /// The one widget for which the blanket answer is the right one: a keymap
    /// editor that let the window's accelerators win could not record the very
    /// chords a user most wants to rebind. The bound is the state, not the
    /// chord — which is why the axis asks a `&self` question rather than
    /// reading a static property.
    fn shadows_accelerator(&self, _chord: &Chord) -> bool {
        self.em.inner.is_recording()
    }

    /// Focus loss ACCEPTS the in-flight sequence — the toolkit's `focusOutEvent` calls
    /// `finishEditing()`.
    fn on_focus_change(&mut self, focused: bool) {
        if !focused && self.em.inner.is_recording() {
            self.send(KeySequenceEvent::Blur);
        }
    }
}

impl ExternalIntrospect for KeySequenceEditExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("state", "string"),
                    SchemaField::new("recording", "bool"),
                    SchemaField::new("sequence", "string"),
                    SchemaField::new("in_flight", "string"),
                    SchemaField::new("pending", "string"),
                    SchemaField::new("max_len", "number"),
                    SchemaField::new("revision", "number"),
                    SchemaField::action("send", "string"),
                    SchemaField::action("record", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let edit = &self.em.inner;
        match path {
            "state" => Some(IntrospectValue::Text(edit.state().as_name().to_string())),
            "recording" => Some(IntrospectValue::Bool(edit.is_recording())),
            "sequence" => Some(IntrospectValue::Text(edit.sequence().portable())),
            "in_flight" => Some(IntrospectValue::Text(edit.in_flight().portable())),
            // The prefix the toolkit drops. A held modifier with no key spells
            // as the chord it would become, trailing separator and all,
            // because that is what the field displays.
            "pending" => Some(IntrospectValue::Text(edit.pending().map_or_else(
                String::new,
                |m| {
                    let mut probe = Chord::new(String::new(), m).portable();
                    if probe.is_empty() {
                        probe.push('+');
                    }
                    probe
                },
            ))),
            "revision" => Some(IntrospectValue::Int(i64::from(self.revision))),
            "max_len" => Some(IntrospectValue::Int(
                i64::try_from(edit.max_len()).unwrap_or(i64::MAX),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "sequence" => match value {
                IntrospectValue::Text(spelling) => {
                    let parsed = KeySequence::parse(&spelling)
                        .map_err(|err| InterveneError::out_of_range(err.to_string()))?;
                    self.em
                        .inner
                        .set_sequence(&parsed)
                        .map_err(|full| InterveneError::out_of_range(full.to_string()))
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R1566 §5.15 — a name this impl did not recognise is judged
            // against the DECLARED schema, so a readable slot is refused as
            // read-only and only a genuinely undeclared one is `UnknownPath`.
            _ => Err(crate::external::read_only_or_unknown(&self.schema(), path)),
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
                    let ev = crate::widget_core::require_event::<KeySequenceEvent>(
                        "key_sequence",
                        name,
                    )?;
                    self.send(ev);
                    Ok(IntrospectValue::Bool(true))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "record" => match args {
                IntrospectValue::Text(ref spelling) => {
                    let chord = Chord::parse(spelling).map_err(|err| {
                        InvokeError::rejected(format!("unreadable chord {spelling:?}: {err}"))
                    })?;
                    Ok(IntrospectValue::Text(match self.record(&chord) {
                        RecordOutcome::Recorded(i) => format!("recorded:{i}"),
                        RecordOutcome::Pending => "pending".to_string(),
                        RecordOutcome::NotRecording => "not_recording".to_string(),
                    }))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        KEY_SEQUENCE_CAPTURED_EVENT, KeySequenceEdit, KeySequenceEditExternal, KeySequenceEvent,
        KeySequenceState, RecordOutcome,
    };
    use crate::accelerator::{Chord, KeySequence, QT_MAX_SEQUENCE_LENGTH};
    use crate::external::{External, ExternalIntrospect, IntrospectValue};
    use crate::input::Modifiers;
    use crate::intent::Intent;

    fn ctrl(key: &str) -> Chord {
        Chord::new(
            key,
            Modifiers {
                ctrl: true,
                ..Modifiers::empty()
            },
        )
    }

    fn recording() -> KeySequenceEditExternal {
        let mut w = KeySequenceEditExternal::new();
        w.send(KeySequenceEvent::Record);
        assert_eq!(w.state(), KeySequenceState::Recording);
        w
    }

    fn drained(w: &mut KeySequenceEditExternal) -> Vec<String> {
        let mut out = Vec::new();
        w.drain_intents(&mut |i: Intent| out.push(i.tag_str().to_owned()));
        out
    }

    #[test]
    fn a_recording_editor_claims_every_chord_and_an_idle_one_claims_none() {
        // The extreme case the axis exists for: an idle editor must let the
        // File menu's Alt+F through, and a recording one must not.
        let idle = KeySequenceEditExternal::new();
        let rec = recording();
        for chord in [
            ctrl("s"),
            Chord::new(
                "f",
                Modifiers {
                    alt: true,
                    ..Modifiers::empty()
                },
            ),
            Chord::new("a", Modifiers::empty()),
        ] {
            assert!(!idle.shadows_accelerator(&chord), "{chord} while idle");
            assert!(rec.shadows_accelerator(&chord), "{chord} while recording");
        }
    }

    #[test]
    fn a_disabled_editor_claims_nothing() {
        // A widget that will not act on the key must not stop the accelerator
        // that would have — the toolkit gates `ShortcutOverride` on `isReadOnly()` for the same reason.
        let mut w = recording();
        w.send(KeySequenceEvent::Disable);
        assert_eq!(w.state(), KeySequenceState::Disabled);
        assert!(!w.shadows_accelerator(&ctrl("s")));
    }

    #[test]
    fn a_modifier_only_press_is_a_published_prefix_not_a_drop() {
        // The toolkit's `keyPressEvent` returns early here and the fact is lost.
        let mut w = recording();
        let held = Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::empty()
        };
        assert_eq!(
            w.record(&Chord::new("Control", held)),
            RecordOutcome::Pending,
        );
        assert_eq!(w.edit().pending(), Some(held));
        assert!(w.edit().in_flight().is_empty(), "a prefix is not a chord");
        assert_eq!(
            w.query("pending"),
            Some(IntrospectValue::Text("Ctrl+Shift+".to_string())),
            "the prefix spells as the chord it would become",
        );
        // A real key lands: the prefix resolves and stops being pending.
        assert_eq!(w.record(&ctrl("s")), RecordOutcome::Recorded(0));
        assert_eq!(w.edit().pending(), None);
    }

    #[test]
    fn filling_the_sequence_commits_through_the_intent_channel() {
        // The fill-commit is a second path to the same transition; routing it
        // around the emitter would make it silently emit nothing.
        let mut w = KeySequenceEditExternal::new().with_max_len(2);
        w.send(KeySequenceEvent::Record);
        assert_eq!(w.record(&ctrl("k")), RecordOutcome::Recorded(0));
        assert_eq!(w.state(), KeySequenceState::Recording, "not full yet");
        assert_eq!(w.record(&ctrl("s")), RecordOutcome::Recorded(1));
        assert_eq!(w.state(), KeySequenceState::Idle, "full commits");
        assert_eq!(w.edit().sequence().portable(), "Ctrl+k, Ctrl+s");
        assert_eq!(
            drained(&mut w),
            vec![KEY_SEQUENCE_CAPTURED_EVENT.to_string()]
        );
    }

    #[test]
    fn an_explicit_commit_and_a_fill_commit_emit_the_same_intent() {
        let mut explicit = KeySequenceEditExternal::new().with_max_len(2);
        explicit.send(KeySequenceEvent::Record);
        explicit.record(&ctrl("k"));
        explicit.send(KeySequenceEvent::Commit);
        let mut filled = KeySequenceEditExternal::new().with_max_len(1);
        filled.send(KeySequenceEvent::Record);
        filled.record(&ctrl("k"));
        assert_eq!(drained(&mut explicit), drained(&mut filled));
    }

    #[test]
    fn cancel_discards_the_in_flight_run_and_keeps_the_previous_one() {
        // The toolkit cannot do this at all: the release timer commits
        // whatever arrived.
        let mut w = KeySequenceEditExternal::new();
        w.send(KeySequenceEvent::Record);
        w.record(&ctrl("k"));
        w.send(KeySequenceEvent::Commit);
        assert_eq!(w.edit().sequence().portable(), "Ctrl+k");
        let _ = drained(&mut w);

        w.send(KeySequenceEvent::Record);
        w.record(&ctrl("z"));
        w.send(KeySequenceEvent::Cancel);
        assert_eq!(w.state(), KeySequenceState::Idle);
        assert_eq!(
            w.edit().sequence().portable(),
            "Ctrl+k",
            "the abandoned run did not overwrite the accepted one",
        );
        assert!(w.edit().in_flight().is_empty());
        assert!(drained(&mut w).is_empty(), "cancel raises nothing");
    }

    #[test]
    fn focus_loss_accepts_what_was_typed() {
        // The toolkit's `focusOutEvent` calls `finishEditing()`; kept deliberately.
        let mut w = recording();
        w.record(&ctrl("p"));
        w.on_focus_change(false);
        assert_eq!(w.state(), KeySequenceState::Idle);
        assert_eq!(w.edit().sequence().portable(), "Ctrl+p");
        assert_eq!(
            drained(&mut w),
            vec![KEY_SEQUENCE_CAPTURED_EVENT.to_string()]
        );
    }

    #[test]
    fn a_chord_offered_while_idle_is_reported_not_silently_dropped() {
        let mut w = KeySequenceEditExternal::new();
        assert_eq!(w.record(&ctrl("s")), RecordOutcome::NotRecording);
        assert!(w.edit().sequence().is_empty());
    }

    #[test]
    fn seeding_past_the_maximum_refuses_and_stores_nothing() {
        // The toolkit's `setKeySequence` truncates silently, so a caller cannot tell a
        // sequence that fit from one that was cut down to fit.
        let mut edit = KeySequenceEdit::new().with_max_len(2);
        let mut long = KeySequence::new();
        for key in ["a", "b", "c"] {
            long.push(ctrl(key), 8).expect("the source bound is wider");
        }
        let refusal = edit.set_sequence(&long).expect_err("3 chords, max 2");
        assert_eq!(refusal.max, 2);
        assert_eq!(refusal.dropped.portable(), "Ctrl+c");
        assert!(
            edit.sequence().is_empty(),
            "a refused seed leaves no half-applied value",
        );
    }

    #[test]
    fn the_default_maximum_is_qts_four() {
        assert_eq!(KeySequenceEdit::new().max_len(), QT_MAX_SEQUENCE_LENGTH);
        assert_eq!(
            KeySequenceEdit::new().with_max_len(0).max_len(),
            1,
            "clamped"
        );
    }

    #[test]
    fn the_wire_reads_back_what_the_widget_holds() {
        let mut w = recording();
        w.record(&ctrl("k"));
        assert_eq!(
            w.query("state"),
            Some(IntrospectValue::Text("Recording".into()))
        );
        assert_eq!(w.query("recording"), Some(IntrospectValue::Bool(true)));
        assert_eq!(
            w.query("in_flight"),
            Some(IntrospectValue::Text("Ctrl+k".into()))
        );
        assert_eq!(
            w.query("sequence"),
            Some(IntrospectValue::Text(String::new()))
        );
        assert_eq!(w.query("max_len"), Some(IntrospectValue::Int(4)));
        // R1566 — an undeclared name is UnknownPath; a declared read-only one
        // is ReadOnly, and the two are told apart from the schema.
        assert!(w.query("no_such_slot").is_none());
    }

    #[test]
    fn the_record_verb_round_trips_a_chord_spelling_over_the_wire() {
        let mut w = recording();
        assert_eq!(
            w.invoke("record", IntrospectValue::Text("Ctrl+Shift+P".into())),
            Ok(IntrospectValue::Text("recorded:0".into())),
        );
        assert_eq!(
            w.query("in_flight"),
            Some(IntrospectValue::Text("Ctrl+Shift+P".into())),
        );
        // An unreadable spelling is a NAMED refusal, where the toolkit's `fromString`
        // would hand back a sequence containing `Key_unknown`.
        let err = w
            .invoke("record", IntrospectValue::Text("Ctrl+Frobnicate+P".into()))
            .expect_err("unreadable");
        let reason = err
            .reason()
            .expect("a refusal states why")
            .as_str()
            .to_owned();
        assert!(reason.contains("Frobnicate"), "{reason}");
    }
}
