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

/// Owner-cache newtype: [`Owner::cache`](super::owner::Owner::cache) stores
/// `Rc<dyn Any>`, so the `Send` trait object rides inside this holder. The
/// outer `Rc<RepaintSinkHolder>` stays on the UI thread; the inner
/// `Arc<dyn RepaintSink>` is the handle that crosses to the producer thread.
pub(crate) struct RepaintSinkHolder(pub(crate) Arc<dyn RepaintSink>);

/// Private owner-cache key for the single per-owner repaint sink slot
/// (mirrors `LocalTaskPump`'s private key convention).
pub(crate) const REPAINT_SINK_KEY: &str = "__pinion.reactive.repaint_sink";

/// R999 §5.23 — hook returning the active owner scope's [`RepaintSink`].
///
/// Returns the sink the shell seeded via
/// [`Owner::provide_repaint_sink`](super::owner::Owner::provide_repaint_sink)
/// at boot, or a [`NullRepaintSink`] off the live event loop. Bindings call
/// this inside `create_extra_externals` (alongside the producer-thread spawn)
/// and hand the returned `Arc` to the thread.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same shape as the
/// other `use_*` hooks).
#[must_use]
pub fn use_repaint_sink() -> Arc<dyn RepaintSink> {
    super::owner::Owner::current()
        .expect("use_repaint_sink requires an active Owner scope")
        .repaint_sink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::owner::Owner;
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

    #[test]
    fn null_default_is_a_silent_no_op() {
        // No shell provided a sink: the lazy default is NullRepaintSink, and
        // request_repaint must not panic (bindings call it unconditionally).
        let owner = Owner::new();
        owner.repaint_sink().request_repaint();
        owner.repaint_sink().request_repaint();
    }

    #[test]
    fn provided_sink_is_returned() {
        let owner = Owner::new();
        let count = Arc::new(AtomicUsize::new(0));
        owner.provide_repaint_sink(Arc::new(CountingSink(Arc::clone(&count))));
        owner.repaint_sink().request_repaint();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn provide_is_first_write_wins() {
        // The shell seeds once before any read; a stray second provide is a
        // no-op (the supplied sink is dropped, the first stays installed).
        let owner = Owner::new();
        let first = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(AtomicUsize::new(0));
        owner.provide_repaint_sink(Arc::new(CountingSink(Arc::clone(&first))));
        owner.provide_repaint_sink(Arc::new(CountingSink(Arc::clone(&second))));
        owner.repaint_sink().request_repaint();
        assert_eq!(first.load(Ordering::SeqCst), 1, "first provide wins");
        assert_eq!(second.load(Ordering::SeqCst), 0, "second provide is a no-op");
    }

    #[test]
    fn use_repaint_sink_resolves_inside_owner_run() {
        let owner = Owner::new();
        let count = Arc::new(AtomicUsize::new(0));
        owner.provide_repaint_sink(Arc::new(CountingSink(Arc::clone(&count))));
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
        let count = Arc::new(AtomicUsize::new(0));
        owner.provide_repaint_sink(Arc::new(CountingSink(Arc::clone(&count))));
        let sink = owner.repaint_sink();
        let handle = std::thread::spawn(move || {
            sink.request_repaint();
            sink.request_repaint();
        });
        handle.join().expect("producer thread joins");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
