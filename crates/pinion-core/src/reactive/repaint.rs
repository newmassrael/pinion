//! R999 §5.23 — `RepaintSink`: the runtime-agnostic "wake the shell to
//! repaint" boundary.
//!
//! # Why this exists
//!
//! pinion's `view` is a pure function of reactive state, and state changes
//! flow through folded events. An **external async producer** — a PTY reader
//! thread, a network/process monitor, a profiler stream — lives on another OS
//! thread and writes a `Send` shared handle the binding's `view` reads
//! (producer-authoritative, the sprag R969 model). Such a producer needs to
//! tell the shell *"my data changed, repaint"* without owning the event loop.
//!
//! [`RepaintSink`] is that one edge. It is the wake sibling of the schedule
//! half of the async-driver substrate
//! ([`LocalTaskPump`](super::resource::LocalTaskPump) / [`use_local_task_pump`](super::resource::use_local_task_pump)):
//! the pump drives UI-thread futures one poll per frame, while a `RepaintSink`
//! lets an *off-thread* producer request a frame. It deliberately mirrors the
//! §6.3 boundary-trait pattern of
//! [`Executor`](../../../pinion_runtime/trait.Executor.html) /
//! [`IntentSink`](../../../pinion_runtime/trait.IntentSink.html): pinion-core
//! defines the abstract trait, and the backend shell supplies the concrete
//! `EventLoopProxy`-backed impl — so this layer never sees winit.
//!
//! # Distinct from `IntentSink`
//!
//! [`IntentSink`](../../../pinion_runtime/trait.IntentSink.html) re-feeds a
//! resolved [`Intent`](crate::Intent) into the SCXML `send` channel / reducer —
//! a *semantic event*. A repaint wake is **not** a reducer event (the produced
//! data is not `State`; it is read directly by `view`), so overloading
//! `IntentSink` — or `AppEvent::WindowsDirty` — for it would conflate two
//! concerns. Interface segregation: a producer that only repaints must not
//! depend on `send(Intent)`.

use std::sync::Arc;

use super::provider_slot::ProviderSlot;

/// R999 §5.23 — the shell-supplied "request a repaint from any thread" edge.
///
/// `Send + Sync + 'static` so the concrete handle can be cloned into a
/// background producer thread (the shell's impl wraps a winit
/// `EventLoopProxy`, which is `Send + Sync`). The binding obtains the active
/// scope's sink through [`use_repaint_sink`] and hands a clone to its producer
/// thread; each time the producer writes fresh data into the shared handle
/// `view` reads, it calls [`RepaintSink::request_repaint`] so a frame runs and
/// re-reads the handle. Wakes coalesce: the shell collapses multiple requests
/// into a single `Window::request_redraw` per frame.
pub trait RepaintSink: Send + Sync + 'static {
    /// Ask the shell to schedule a repaint. Idempotent and cheap — many calls
    /// between frames collapse to one paint.
    fn request_repaint(&self);
}

/// R999 §5.23 — Null Object [`RepaintSink`] (the default when no shell has
/// provided a real one: headless screenshot, RPC-driven tests, unit tests).
///
/// Dropping wakes on the floor is the correct behaviour off the live event
/// loop: there is no window to repaint, and the headless / RPC harness drives
/// paints through `scene/snapshot` polling rather than wake events. Bindings
/// therefore call [`use_repaint_sink`] unconditionally without an `Option` or a
/// panic guard.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullRepaintSink;

impl RepaintSink for NullRepaintSink {
    fn request_repaint(&self) {}
}

/// R1366.1 §5.22 §5.23 — the repaint slot: its key, its Null default and its
/// inherit verdict as one expression, in the module that owns the capability.
///
/// **`Inherited`** by the mechanical predicate — the shell DRIVES this at the
/// root owner, seeding it in `AppShell::new`'s `new_with_seed` closure before
/// any binding factory reads it. A per-scope repaint sink would leave the
/// deferred R680 atomic (each window's `view` under `window_owner(id).run(..)`)
/// handing every secondary window a fresh `NullRepaintSink`: its producer
/// thread would call [`RepaintSink::request_repaint`] forever and wake nothing.
/// [`provider_slot_tests!`](crate::provider_slot_tests) asserts that verdict
/// below rather than leaving it to a doc table, which is what R1365.1 found had
/// been asserting it.
///
/// The payload is the `Arc<dyn RepaintSink>` itself, with no newtype wrapper:
/// [`Owner::cache`](super::owner::Owner::cache) keys on
/// `(TypeId::of::<V>(), key)`, so the trait object is already its own type.
/// R999's `RepaintSinkHolder` was the `Rc<dyn Any>` storage showing through the
/// abstraction — `pinion-shell`'s `waiter.rs` has stored a bare `Arc` since
/// R1269 (`aaa92198`), proving the holder optional.
pub static REPAINT_SINK: ProviderSlot<Arc<dyn RepaintSink>> =
    ProviderSlot::inherited("__pinion.reactive.repaint_sink", || {
        Arc::new(NullRepaintSink)
    });

/// R999 §5.23 — hook returning the active owner scope's [`RepaintSink`].
///
/// Returns the sink the shell seeded into [`REPAINT_SINK`] at boot, or a
/// [`NullRepaintSink`] off the live event loop. Bindings call this inside
/// `create_extra_externals` (alongside the producer-thread spawn) and hand the
/// returned `Arc` to the thread.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same shape as the
/// other `use_*` hooks).
#[must_use]
pub fn use_repaint_sink() -> Arc<dyn RepaintSink> {
    Arc::clone(&REPAINT_SINK.resolve_current())
}

#[cfg(test)]
mod tests {
    use super::super::owner::Owner;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Recording sink: counts `request_repaint` calls so a test can assert the
    /// wire reached the handle (and crossed a thread boundary).
    #[derive(Debug)]
    struct CountingSink(Arc<AtomicUsize>);
    impl RepaintSink for CountingSink {
        fn request_repaint(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// A sink and the counter it increments.
    fn counting() -> (Arc<dyn RepaintSink>, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        (Arc::new(CountingSink(Arc::clone(&count))), count)
    }

    // The verdict, EMITTED from the declaration rather than remembered: a child
    // scope resolves the root's sink. R1365 wrote this slot's verdict test by
    // hand and forgot five of its siblings'; a generated one cannot be forgotten.
    crate::provider_slot_tests!(r1366_1_repaint_sink_inherits, super::REPAINT_SINK, || {
        counting().0
    });

    #[test]
    fn null_default_is_a_silent_no_op() {
        // No shell provided a sink: the lazy default is NullRepaintSink, and
        // request_repaint must not panic (bindings call it unconditionally).
        let owner = Owner::new();
        REPAINT_SINK.resolve(&owner).request_repaint();
        REPAINT_SINK.resolve(&owner).request_repaint();
    }

    #[test]
    fn provided_sink_is_returned() {
        let owner = Owner::new();
        let (sink, count) = counting();
        REPAINT_SINK.provide(&owner, sink);
        REPAINT_SINK.resolve(&owner).request_repaint();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[should_panic(expected = "already seeded")]
    fn r1366_1_a_late_seed_panics_where_it_used_to_be_dropped() {
        // The counterfactual of R999's `provide_is_first_write_wins`, which
        // asserted that a second seed was a SILENT no-op leaving every reader on
        // the first sink. `Owner::provide_repaint_sink`'s own doc called that
        // "never observed" without checking — the M5 shape, five sites deep. A
        // seed that loses a race is a wiring bug, and now says so.
        let owner = Owner::new();
        REPAINT_SINK.provide(&owner, counting().0);
        REPAINT_SINK.provide(&owner, counting().0);
    }

    #[test]
    fn use_repaint_sink_resolves_inside_owner_run() {
        let owner = Owner::new();
        let (sink, count) = counting();
        REPAINT_SINK.provide(&owner, sink);
        owner.run(|| {
            use_repaint_sink().request_repaint();
        });
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn sink_wakes_across_a_thread_boundary() {
        // The motivating case: an off-thread producer holds a clone of the
        // sink and wakes the (would-be) shell. `join` makes it deterministic —
        // no wall-clock poll.
        let owner = Owner::new();
        let (sink, count) = counting();
        REPAINT_SINK.provide(&owner, sink);
        let handle_sink = Arc::clone(&REPAINT_SINK.resolve(&owner));
        let handle = std::thread::spawn(move || {
            handle_sink.request_repaint();
            handle_sink.request_repaint();
        });
        handle.join().expect("producer thread joins");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn r1365_1_a_child_scope_resolves_the_shells_real_repaint_sink() {
        // The generated verdict test above asserts `Rc::ptr_eq` through
        // `resolve`. This one asserts the same inheritance end-to-end through
        // the BINDING's path — `use_repaint_sink()` inside a child `Owner::run`,
        // which is the scope R680 will run a secondary window's view in — so a
        // hook that stopped delegating to the slot could not pass both.
        let root = Owner::new();
        let (sink, count) = counting();
        REPAINT_SINK.provide(&root, sink);

        let window_scope = Owner::new_child(&root);
        window_scope.run(|| use_repaint_sink().request_repaint());

        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "a child scope resolved a Null instead of the shell's sink — a \
             secondary window's producer would wake nothing",
        );
    }
}
