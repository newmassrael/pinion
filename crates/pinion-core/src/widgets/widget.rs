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
    /// `ButtonEvent`).
    type Event;
    /// A cheap, owned view of everything the detection step needs
    /// from the widget — usually the SCXML state, optionally tupled
    /// with a value sidecar (`bool`, `f32`, ...).
    type Snapshot: Copy;

    /// Capture the current widget snapshot. Called before and after
    /// each [`drive`](WidgetTransition::drive) call.
    fn snapshot(&self) -> Self::Snapshot;

    /// Drive `event` through the widget. Implementations forward to
    /// the widget's own `send` so the value-sidecar mutation logic
    /// (e.g. Toggle's `Pressed → Hover` flip) stays in one place.
    fn drive(&mut self, event: Self::Event);

    /// Decide whether the (`before`, `after`) snapshot pair signals
    /// an emitting transition; return the intent to push or `None`
    /// when the transition is silent on the §5.20 channel. Pure
    /// function of the two snapshots — does not borrow widget state.
    fn detect(before: Self::Snapshot, after: Self::Snapshot) -> Option<Intent>;
}

impl<W: WidgetTransition> IntentEmitter<W> {
    /// R51.12 §5.38 — drive `event` through the wrapped widget and
    /// queue any [`Intent`] the transition produces.
    ///
    /// Encodes the textbook snapshot → drive → detect → push pipeline
    /// once; every Tier-1 `*External::send` delegates to this method
    /// instead of re-implementing the five-line dance. The
    /// substrate-vs-application boundary stays clean: `IntentEmitter`
    /// owns the pipeline shape, the widget owns its three
    /// [`WidgetTransition`] associated items.
    pub fn dispatch(&mut self, event: W::Event) {
        let before = self.inner.snapshot();
        self.inner.drive(event);
        let after = self.inner.snapshot();
        if let Some(intent) = W::detect(before, after) {
            self.push(intent);
        }
    }
}
