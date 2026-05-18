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
