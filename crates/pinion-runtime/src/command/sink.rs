//! [`IntentSink`] — receiving surface for [`Intent`] values produced by
//! resolved [`Command`](pinion_core::Command) futures.
//!
//! ## Where this fits in the §5.23 pipeline
//!
//! ```text
//! Owner.dispatch_command(cmd)              [R51.139, pinion-core]
//!   → Owner.take_pending_commands_recursive()
//!   → CommandExecutor::dispatch(cmd)       [R51.156, this crate]
//!     → HandlerRegistry::dispatch(cmd) → HandlerFuture
//!     → Executor::spawn(wrapped)            [R51.156]
//!       → future.await → Intent
//!       → IntentSink::send(intent)          [this module]
//!         → backend wakes UI thread (winit EventLoopProxy /
//!           tokio mpsc / crossterm poll wake), forwards to a
//!           subsequent `core.dispatch_intent(intent)` call that
//!           lands on the §5.20 intent channel.
//! ```
//!
//! ## Why a trait?
//!
//! The §6.3 boundary keeps async runtime + window-system wake plumbing
//! out of `pinion-runtime`. Backends supply their own [`IntentSink`]
//! impl that knows how to reach their event loop:
//!
//! - `pinion-shell` (winit / Vello) — wraps an
//!   `EventLoopProxy<AppEvent>` and forwards via `send_event(
//!   AppEvent::IntentArrived(intent))`. The user-event arm on the UI
//!   thread re-feeds the intent into `ShellCore::dispatch_intent`.
//! - `pinion-tui` — `mpsc::Sender<Intent>` + `crossterm` poll
//!   timeout that drains the channel on every tick.
//! - Tests — the [`VecSink`] in this module collects intents into a
//!   `Mutex<Vec<Intent>>` so a `block_on`-driven test can assert on the
//!   final intent sequence without standing up a real event loop.
//!
//! The trait is `Send + Sync + 'static` because the [`CommandExecutor`]
//! (R51.156) clones an [`Arc<dyn IntentSink>`] into every dispatched
//! future. A multi-thread executor's worker may resolve the future on
//! any thread and call `IntentSink::send` from there, so `Send + Sync`
//! is load-bearing.

use std::sync::{Arc, Mutex};

use pinion_core::Intent;

/// Receiving surface for [`Intent`] values produced by resolved
/// [`Command`](pinion_core::Command) futures.
///
/// Implementations forward each received intent to the UI thread's
/// re-dispatch path. The trait is infallible at the substrate surface
/// because the futures the
/// [`CommandExecutor`](crate::command::CommandExecutor) wraps are
/// fire-and-forget — a closed channel or dropped event loop simply
/// drops the intent on the floor, the same way a [`Command`] queued
/// on a dropped [`Owner`](pinion_core::Owner) evaporates.
///
/// `Send + Sync + 'static` — backends share an
/// `Arc<dyn IntentSink>` across the substrate (UI thread) and the
/// async executor (worker thread). The trait bound is what makes that
/// share legal.
pub trait IntentSink: Send + Sync + 'static {
    /// Deliver one [`Intent`] to the UI thread's re-dispatch path.
    ///
    /// `&self` (not `&mut self`) so the same `Arc<dyn IntentSink>` can
    /// be cloned into every future the executor spawns without an
    /// outer lock; interior mutability is the impl's concern.
    fn send(&self, intent: Intent);
}

/// Test fixture [`IntentSink`] that collects every sent [`Intent`]
/// into a shared `Mutex<Vec<Intent>>`.
///
/// `pub` rather than `cfg(test)` so downstream crates' integration
/// tests (`pinion-shell`, `pinion-tui`) can reuse the same fixture
/// instead of re-implementing it per crate. The
/// [`test-fixtures pattern`](https://docs.rs/) the workspace already
/// uses for [`pinion_core::test_fixtures::ButtonFixture`] (R51.127
/// pattern) is overkill for a 30-LOC helper.
///
/// Internally `Arc<Mutex<Vec<Intent>>>` so the sink can be cloned
/// freely while every clone observes the same drain. `clone()` is a
/// cheap `Arc` bump.
#[derive(Debug, Clone, Default)]
pub struct VecSink {
    intents: Arc<Mutex<Vec<Intent>>>,
}

impl VecSink {
    /// Construct an empty [`VecSink`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain every collected intent, leaving the sink empty.
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] is poisoned — a panicked
    /// worker thread is a test-time signal something needs fixing,
    /// not an error to absorb.
    #[must_use]
    pub fn drain(&self) -> Vec<Intent> {
        let mut guard = self.intents.lock().expect("VecSink mutex poisoned");
        std::mem::take(&mut *guard)
    }

    /// Snapshot the collected intents without draining.
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] is poisoned.
    #[must_use]
    pub fn snapshot(&self) -> Vec<Intent> {
        self.intents.lock().expect("VecSink mutex poisoned").clone()
    }

    /// Number of intents collected so far.
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] is poisoned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.intents.lock().expect("VecSink mutex poisoned").len()
    }

    /// `true` when no intents have been collected since the last
    /// [`Self::drain`].
    ///
    /// # Panics
    /// Panics if the internal [`Mutex`] is poisoned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.intents
            .lock()
            .expect("VecSink mutex poisoned")
            .is_empty()
    }
}

impl IntentSink for VecSink {
    fn send(&self, intent: Intent) {
        self.intents
            .lock()
            .expect("VecSink mutex poisoned")
            .push(intent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;

    #[test]
    fn vec_sink_collects_one_intent() {
        let sink = VecSink::new();
        sink.send(Intent::new_static("a", IntrospectValue::Null));
        let drained = sink.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].tag_str(), "a");
    }

    #[test]
    fn vec_sink_preserves_send_order() {
        let sink = VecSink::new();
        sink.send(Intent::new_static("first", IntrospectValue::Int(1)));
        sink.send(Intent::new_static("second", IntrospectValue::Int(2)));
        sink.send(Intent::new_static("third", IntrospectValue::Int(3)));
        let drained = sink.drain();
        let tags: Vec<&str> = drained.iter().map(Intent::tag_str).collect();
        assert_eq!(tags, vec!["first", "second", "third"]);
    }

    #[test]
    fn vec_sink_drain_empties_storage() {
        let sink = VecSink::new();
        sink.send(Intent::new_static("x", IntrospectValue::Null));
        assert_eq!(sink.len(), 1);
        let _ = sink.drain();
        assert!(sink.is_empty());
        assert_eq!(sink.len(), 0);
    }

    #[test]
    fn vec_sink_snapshot_does_not_drain() {
        let sink = VecSink::new();
        sink.send(Intent::new_static("x", IntrospectValue::Null));
        let _ = sink.snapshot();
        let _ = sink.snapshot();
        assert_eq!(sink.len(), 1, "snapshot must not consume the queue");
    }

    #[test]
    fn vec_sink_clones_share_storage() {
        let sink_a = VecSink::new();
        let sink_b = sink_a.clone();
        sink_a.send(Intent::new_static("via_a", IntrospectValue::Null));
        sink_b.send(Intent::new_static("via_b", IntrospectValue::Null));
        assert_eq!(sink_a.len(), 2, "cloned sinks must observe the same Vec",);
        let drained = sink_b.drain();
        assert_eq!(drained.len(), 2);
        assert!(
            sink_a.is_empty(),
            "drain through one clone empties the other"
        );
    }

    #[test]
    fn vec_sink_default_is_empty() {
        let sink = VecSink::default();
        assert!(sink.is_empty());
        assert_eq!(sink.len(), 0);
    }

    #[test]
    fn vec_sink_implements_intent_sink_via_trait_object() {
        // R51.156 — `Arc<dyn IntentSink>` is the production carrier
        // shape; verify the test fixture works through the dyn trait
        // (not just inherent methods).
        let sink: Arc<dyn IntentSink> = Arc::new(VecSink::new());
        sink.send(Intent::new_static("via_dyn", IntrospectValue::Null));
        // Need to downcast back to read; alternative is to keep a
        // typed VecSink handle alongside. The downcast verifies the
        // Arc<dyn IntentSink> can be backed by VecSink.
        let typed: Arc<VecSink> = Arc::new(VecSink::new());
        let dyn_handle: Arc<dyn IntentSink> = typed.clone();
        dyn_handle.send(Intent::new_static("typed_via_dyn", IntrospectValue::Null));
        assert_eq!(typed.len(), 1);
    }
}
