//! R51.4 §5.38 — generic widget facade over [`sce_rust_runtime::Engine`].
//!
//! Tier-1 widget bindings (Button R12, Toggle R51.2, future Checkbox /
//! Radio / `MenuItem` / Tab / ...) share a thin facade over the SCE
//! runtime: construct an `Engine<P>` from a default `StatePolicy`,
//! drive transitions via `process_event`, observe the current state.
//! Before R51.4 every widget repeated this facade inline; the only
//! per-widget variation is the policy type and any value sidecar
//! (e.g. Toggle's `bool`).
//!
//! [`Widget`] is the shared facade. A widget with no extra state is a
//! type alias (`pub type Button = Widget<ButtonPolicy>`); a widget
//! with sidecar state (Toggle's `value: bool`) is a newtype that
//! holds a `Widget<P>` and adds its own fields and methods.

use sce_rust_runtime::{Engine, StatePolicy};

use crate::intent::Intent;

/// Thin facade over [`Engine<P>`] that constructs, drives, and
/// observes a generated SCXML state machine.
///
/// Generic over [`StatePolicy`] so every Tier-1 widget shares the
/// same construction / event / state surface; the SCE runtime layer
/// is already generic, this type lifts that genericness up into
/// pinion's widget API so each new widget only contributes its
/// policy + (optional) sidecar state instead of redeclaring the
/// facade methods.
pub struct Widget<P: StatePolicy> {
    engine: Engine<P>,
}

impl<P: StatePolicy> Widget<P> {
    /// Construct a widget around `policy`. The engine is
    /// `initialize`d immediately so the first observable state is
    /// the SCXML `initial` target, matching the per-widget facades
    /// that R51.4 replaces.
    pub fn with_policy(policy: P) -> Self {
        let mut engine = Engine::new(policy);
        engine.initialize();
        Self { engine }
    }

    /// Drive a `P::Event` through the engine. Pure forwarding to
    /// [`Engine::process_event`]; placed on the facade so callers
    /// never need to name the engine field.
    pub fn send(&mut self, event: P::Event) {
        self.engine.process_event(event);
    }

    /// Current SCXML state (the policy's `State` associated type).
    /// Pure forwarding to [`Engine::get_current_state`].
    pub fn state(&self) -> P::State {
        self.engine.get_current_state()
    }
}

impl<P: StatePolicy + Default> Widget<P> {
    /// Construct with `P::default()`. The codegen emits
    /// `impl Default for {Policy}` on every generated policy, so this
    /// is the path Tier-1 widgets use when they do not need a
    /// pre-configured policy instance.
    #[must_use]
    pub fn new() -> Self {
        Self::with_policy(P::default())
    }
}

impl<P: StatePolicy + Default> Default for Widget<P> {
    fn default() -> Self {
        Self::new()
    }
}

/// R51.5 §5.38 — §5.20 intent buffer helper for `*External` adapters.
///
/// Every Tier-1 widget's `*External` type pairs an inner widget with
/// a `Vec<Intent>` that holds pending §5.20 intents emitted by the
/// widget's `send` path until the framework drains them (either via
/// the runtime walk or the `scene/intents` RPC method). Before R51.5
/// each adapter declared its own `pending_intents: Vec<Intent>`
/// field plus a hand-rolled `drain_intents` / `is_dirty` pair; this
/// helper lifts those pieces into one place.
///
/// `inner` is `pub` so widget adapters can call inner methods
/// directly (`self.em.inner.state()`) without a method-forwarding
/// layer; the helper deliberately does **not** implement `Deref` —
/// `IntentEmitter` is a wrapper, not a smart pointer, and the
/// explicit `.inner` keeps the layer boundary readable.
pub struct IntentEmitter<W> {
    /// The wrapped widget. Adapters drive transitions and observe
    /// state through this field directly.
    pub inner: W,
    pending: Vec<Intent>,
}

impl<W> IntentEmitter<W> {
    /// Wrap `inner` with an empty intent buffer.
    pub const fn new(inner: W) -> Self {
        Self { inner, pending: Vec::new() }
    }

    /// Enqueue an intent. Called from the adapter's `send` after
    /// detecting an emitting transition (e.g. `Pressed -> Hover`).
    pub fn push(&mut self, intent: Intent) {
        self.pending.push(intent);
    }

    /// Forward every pending intent to `sink` and clear the buffer.
    /// Mirrors the `External::drain_intents` contract; adapters
    /// usually just delegate.
    pub fn drain(&mut self, sink: &mut dyn FnMut(Intent)) {
        for intent in self.pending.drain(..) {
            sink(intent);
        }
    }

    /// `true` if at least one intent is pending. Mirrors the
    /// `External::is_dirty` contract.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.pending.is_empty()
    }
}

impl<W: Default> Default for IntentEmitter<W> {
    fn default() -> Self {
        Self::new(W::default())
    }
}

/// R51.12 §5.38 — transition contract that lets [`IntentEmitter`]
/// automate the snapshot → drive → detect → push pattern shared by
/// every Tier-1 widget adapter.
///
/// Before R51.12 each `*External::send` re-implemented the same five
/// lines: capture the pre-state (and any value sidecar), drive the
/// event, capture the post-state, compare the two by pattern, and
/// push an [`Intent`] on the matching activate transition. The bodies
/// only diverge in three places — the snapshot tuple, the
/// pattern-match, and the intent payload — so the textbook
/// abstraction is a trait whose associated types carry exactly those
/// three pieces.
///
/// `Snapshot` must be `Copy` — the trait enforces the design rule
/// that a snapshot is a cheap, value-typed projection of the widget
/// (typically a state enum or `(State, value)` tuple). The engine
/// state enums are `#[derive(Copy)]` and value sidecars are `bool`
/// / `f32`, so every Tier-1 widget satisfies this trivially; future
/// widgets that would need an expensive snapshot have a clear design
/// signal to rework the snapshot shape rather than blow up the
/// per-event cost.
///
/// `detect` is an associated function (not a method) because the
/// detection logic is a pure projection of two snapshots — it never
/// needs to re-borrow `self` after `drive` returns, and decoupling
/// it from `&self` makes it trivially `Send` and unit-testable in
/// isolation.
pub trait WidgetTransition {
    /// The event type the widget consumes (typically the SCXML
    /// `*Event` enum the codegen emits, e.g. crate-scope
    /// `ButtonEvent`). `Copy` so [`IntentEmitter::dispatch`] can
    /// hand the same value to both `drive` and `detect` without
    /// borrowing or cloning — SCXML event enums are cheap unit
    /// variants on every Tier-1 widget today.
    type Event: Copy;
    /// An owned view of everything the detection step needs from the
    /// widget — usually the SCXML state, optionally tupled with a
    /// value sidecar (`bool`, `f32`, ...). R51.102 §5.38 — the bound
    /// is `Clone` (not `Copy`) so widgets that need an instance-sized
    /// snapshot (e.g. `ListBox`'s `Vec<bool>` selection bitmap) can
    /// fit. Copy snapshots (every other Tier-1 widget today) auto-
    /// satisfy `Clone`; the dispatch pipeline moves the snapshot into
    /// `detect` and never re-uses it, so the heap allocation cost is
    /// "one Vec per user input event" for `ListBox` and zero for
    /// every other widget.
    type Snapshot: Clone;

    /// Capture the current widget snapshot. Called before and after
    /// each [`drive`](WidgetTransition::drive) call.
    fn snapshot(&self) -> Self::Snapshot;

    /// Drive `event` through the widget. Implementations forward to
    /// the widget's own `send` so the value-sidecar mutation logic
    /// (e.g. Toggle's `Pressed → Hover` flip) stays in one place.
    fn drive(&mut self, event: Self::Event);

    /// Decide whether the (`before`, `event`, `after`) triple signals
    /// an emitting transition; return the intents to push (zero or
    /// more) on the §5.20 channel. Pure function — does not borrow
    /// widget state.
    ///
    /// R51.54 §5.39 — the `event` argument lets internal transitions
    /// (state-stable, e.g. ARIA `keyboard_activate`) emit intents
    /// without relying on a state-change signature. Without `event`
    /// the detection rule for a state-stable click would be
    /// indistinguishable from a no-op.
    ///
    /// R51.102 §5.38 — the return shape is `Vec<Intent>` (was
    /// `Option<Intent>` pre-R51.102). Tier-1 atomic widgets keep
    /// 0-or-1 semantics by returning `Vec::new()` or `vec![intent]`;
    /// composite widgets that mutate multiple value sidecars per
    /// user input (the `ListBox` multi-select toggle being the first
    /// instance, R51.98) emit one entry per changed slot. The
    /// emptier-than-one common case allocates an empty `Vec`
    /// (`(0, 0, 0)` triple on the stack, no heap) per dispatch.
    ///
    /// `#[must_use]` because dropping the result silently loses the
    /// intents — every emitting transition has a downstream listener
    /// expecting the §5.20 channel. The [`IntentEmitter::dispatch`]
    /// pipeline consumes the return; direct callers should never
    /// discard it.
    #[must_use]
    fn detect(
        before: Self::Snapshot,
        event: Self::Event,
        after: Self::Snapshot,
    ) -> Vec<Intent>;
}

impl<W: WidgetTransition> IntentEmitter<W> {
    /// R51.12 §5.38 — drive `event` through the wrapped widget and
    /// queue any [`Intent`]s the transition produces.
    ///
    /// Encodes the textbook snapshot → drive → detect → push pipeline
    /// once; every Tier-1 `*External::send` delegates to this method
    /// instead of re-implementing the five-line dance. The
    /// substrate-vs-application boundary stays clean: `IntentEmitter`
    /// owns the pipeline shape, the widget owns its three
    /// [`WidgetTransition`] associated items.
    ///
    /// R51.102 §5.38 — pushes every entry returned by `detect`.
    /// Multi-emit widgets (`ListBox` multi-select, R51.98) reach the
    /// §5.20 channel through this loop instead of the pre-R51.102
    /// External-adapter manual push (now retired).
    pub fn dispatch(&mut self, event: W::Event) {
        let before = self.inner.snapshot();
        self.inner.drive(event);
        let after = self.inner.snapshot();
        for intent in W::detect(before, event, after) {
            self.push(intent);
        }
    }
}

#[cfg(test)]
mod tests {
    //! R51.12.1 §5.38 — substrate isolation tests for
    //! [`IntentEmitter::dispatch`] + [`WidgetTransition`].
    //!
    //! The Tier-1 widget suites (`button::tests`, `toggle::tests`,
    //! ...) cover the substrate **indirectly** through each widget's
    //! own state machine. These tests exercise the trait + dispatch
    //! pipeline **directly** with a minimal stub widget, so a
    //! regression in the snapshot → drive → detect → push pipeline
    //! shows up as a substrate failure (this file) rather than five
    //! correlated per-widget failures. Failure-mode coverage:
    //!
    //! * `drive` actually mutates inner state during dispatch
    //! * `detect` sees the pre-drive snapshot as `before`
    //! * `detect` sees the post-drive snapshot as `after`
    //! * `Some(intent)` is pushed to the buffer
    //! * `None` does not push (silent transitions)
    use super::*;
    use crate::external::IntrospectValue;

    /// Minimal `WidgetTransition` fixture with explicit state control.
    /// `Event` carries the next state directly so tests fully control
    /// the (before, after) pair without going through an engine.
    struct StubWidget {
        state: u8,
    }

    impl StubWidget {
        const fn new() -> Self {
            Self { state: 0 }
        }
    }

    impl WidgetTransition for StubWidget {
        type Event = u8;
        type Snapshot = u8;

        fn snapshot(&self) -> Self::Snapshot {
            self.state
        }

        fn drive(&mut self, event: Self::Event) {
            self.state = event;
        }

        fn detect(
            before: Self::Snapshot,
            _event: Self::Event,
            after: Self::Snapshot,
        ) -> Vec<Intent> {
            // Direction-sensitive: only the canonical 1 → 2 transition
            // emits, so reverse and unrelated transitions stay silent
            // and the tests can distinguish (before, after) ordering.
            if before == 1 && after == 2 {
                vec![Intent::new_static("stub.fire", IntrospectValue::Null)]
            } else {
                Vec::new()
            }
        }
    }

    #[test]
    fn dispatch_pushes_intent_when_detect_returns_some() {
        let mut em = IntentEmitter::new(StubWidget::new());
        em.inner.state = 1;
        em.dispatch(2);
        assert!(em.is_dirty(), "1 → 2 must arm the §5.20 buffer");
        let mut harvested = Vec::new();
        em.drain(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "stub.fire");
        assert!(!em.is_dirty(), "drain must leave the buffer empty");
    }

    #[test]
    fn dispatch_skips_push_when_detect_returns_none() {
        let mut em = IntentEmitter::new(StubWidget::new());
        // 0 → 5 does not match the 1 → 2 detect rule.
        em.dispatch(5);
        assert!(!em.is_dirty(), "non-matching transition must be silent");
    }

    #[test]
    fn dispatch_drives_engine_between_snapshots() {
        let mut em = IntentEmitter::new(StubWidget::new());
        assert_eq!(em.inner.state, 0);
        em.dispatch(42);
        assert_eq!(
            em.inner.state, 42,
            "dispatch must invoke drive — inner state must reflect the event"
        );
    }

    #[test]
    fn dispatch_passes_pre_drive_state_as_before_to_detect() {
        // Verifies that `before` is captured BEFORE drive, not after.
        // If dispatch erroneously snapshotted twice post-drive, detect
        // would see (2, 2) and emit nothing. The (1, 2) emission is
        // proof that `before` is the pre-drive state.
        let mut em = IntentEmitter::new(StubWidget::new());
        em.inner.state = 1;
        em.dispatch(2);
        assert!(em.is_dirty(), "before-snapshot must precede drive");
    }

    #[test]
    fn dispatch_passes_post_drive_state_as_after_to_detect() {
        // Verifies that `after` is captured AFTER drive, not before.
        // If dispatch erroneously snapshotted both pre-drive, detect
        // would see (1, 1) and emit nothing. The (1, 2) emission is
        // proof that `after` is the post-drive state.
        let mut em = IntentEmitter::new(StubWidget::new());
        em.inner.state = 1;
        em.dispatch(2);
        assert!(em.is_dirty(), "after-snapshot must follow drive");
    }

    #[test]
    fn detect_is_direction_sensitive_in_dispatch() {
        // The same (1, 2) state pair fires; the reverse (2, 1) must
        // not. Guards against accidental commutativity in the pipeline.
        let mut em = IntentEmitter::new(StubWidget::new());
        em.inner.state = 2;
        em.dispatch(1);
        assert!(!em.is_dirty(), "reverse transition must be silent");
    }
}
