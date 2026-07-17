//! R1363 §5.55 §5.23 — `QuitSink`: the "end this application" boundary.
//!
//! # Why this is not a window control
//!
//! Quitting is an APP-lifecycle act: one per process, not one per window. Until
//! R1363 pinion had no verb for it — the app exited as a fall-through of an
//! unhandled `WindowControl::Close`, which welded two lifecycles into one word.
//! The fingerprints were everywhere, and each one is a bug this trait retires:
//!
//! * **`Escape` bypassed the binding's veto in BOTH shells, independently.** The
//!   winit shell called `event_loop.exit()` directly; the TUI shell `break`s its
//!   loop. Neither consulted `window_close_requested`, because Escape does not
//!   mean "close this window" — it means *quit* — and there was no verb to route
//!   it through. Two backends hard-coded the same bypass for the same reason.
//! * **A terminal backend could not have the seam at all.** `WindowControl` is a
//!   window vocabulary, and `pinion-tui` deps neither `pinion-overlay` nor
//!   `pinion-shell`. So the app-lifecycle half was unreachable from the backend
//!   that has no windows — even though quitting is exactly what it can do.
//!   (sprag, the consumer that forced this seam, is itself a terminal
//!   multiplexer.)
//! * **A multi-window editor closing its last document died** unless every
//!   window remembered to veto: the default meant "kill the app".
//!
//! So quitting gets its own vocabulary (no window id — there is nothing to
//! address), its own veto
//! ([`WidgetCore::app_quit_requested`](crate::WidgetCore::app_quit_requested)),
//! and its own sink. It lives in `pinion-core` — the layer BOTH backends dep —
//! which is what makes the §2 #6 GUI/TUI dual true for app lifecycle instead of
//! a GUI-only axis with a terminal-shaped hole.
//!
//! # Distinct from `RepaintSink` and `IntentSink`
//!
//! Interface segregation, the rule
//! [`RepaintSink`](super::repaint::RepaintSink)'s module doc states for
//! `IntentSink`: *"a producer that only repaints must not depend on
//! `send(Intent)`"*. A producer that only quits must depend on neither.
//!
//! # This grants no privileged exit
//!
//! [`QuitSink::request_quit`] is a REQUEST. It lands on the same arm `Escape`,
//! the OS close button (via the last-window policy), and the `app/quit` RPC
//! reach, so [`WidgetCore::app_quit_requested`](crate::WidgetCore::app_quit_requested)
//! still gets to refuse — a binding cannot bypass its own unsaved-changes gate
//! by calling this, and neither can an AI.

use std::sync::Arc;

use super::provider_slot::ProviderSlot;

/// R1363 §5.55 — the shell-supplied "end this application" edge.
///
/// `Send + Sync + 'static` so the handle clones into a background producer
/// thread — the winit shell's impl wraps an `EventLoopProxy`, the TUI shell's
/// wraps its loop-control channel, and both are `Send + Sync`. A binding obtains
/// the active scope's sink through [`use_quit_sink`] and either calls it inline
/// (a tray Quit item, a menu action) or hands a clone to its producer thread
/// (sprag's socket poll thread, closing the client when its daemon dies).
///
/// Delivery is asynchronous by construction: the quit is applied on a later
/// UI-thread turn, never inside this call. That is what lets an in-flight RPC
/// request return its `result` before the process ends.
pub trait QuitSink: Send + Sync + 'static {
    /// Ask the app to end.
    ///
    /// Non-blocking and infallible: an already-shut-down shell drops the
    /// request (there is nothing left to quit), matching the
    /// [`RepaintSink`](super::repaint::RepaintSink) error-absorption
    /// convention. Offered to
    /// [`WidgetCore::app_quit_requested`](crate::WidgetCore::app_quit_requested)
    /// first — a binding that handles it (pops "Save changes?") keeps the app
    /// alive.
    fn request_quit(&self);
}

/// R1363 §5.55 — Null Object [`QuitSink`] (the default when no shell provided a
/// real one: headless screenshot, RPC-driven tests, unit tests).
///
/// Dropping the request is correct off a live shell: there is no event loop to
/// end. Bindings therefore call [`use_quit_sink`] unconditionally, without an
/// `Option` or a panic guard.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullQuitSink;

impl QuitSink for NullQuitSink {
    fn request_quit(&self) {}
}

/// R1366.2 §5.22 §5.55 — the quit slot: its key, its Null default and its
/// inherit verdict as one expression, in the module that owns the capability.
///
/// **`Inherited`** by the mechanical predicate — the shell DRIVES this at the
/// root owner, and BOTH backends seed it before any binding factory reads it:
/// `AppShell::new`'s `new_with_seed` closure on the winit side, the
/// `ShellCoreTui::new_with_seed` seed on the terminal side. That pair is itself
/// the §2 #6 dual this slot exists to serve.
///
/// This is the slot where a per-scope verdict is loudest. Under the deferred
/// R680 atomic — each window's `view` running in `window_owner(id).run(..)` — a
/// secondary window would resolve a freshly minted [`NullQuitSink`] and its Quit
/// button would do nothing: no panic, no log, precisely the defect R1362 existed
/// to fix. `r1364_scope_tests` characterized that by hand;
/// [`provider_slot_tests!`](crate::provider_slot_tests) now EMITS the verdict
/// from this declaration, so it cannot be forgotten the way R1365 forgot five.
///
/// The payload is the `Arc<dyn QuitSink>` itself, with no newtype wrapper:
/// [`Owner::cache`](super::owner::Owner::cache) keys on `(TypeId::of::<V>(),
/// key)`, so the trait object is already its own type. R1363's `QuitSinkHolder`
/// was the `Rc<dyn Any>` storage showing through the abstraction — the same
/// wrapper R1366.1 retired for [`REPAINT_SINK`](super::repaint::REPAINT_SINK).
pub static QUIT_SINK: ProviderSlot<Arc<dyn QuitSink>> =
    ProviderSlot::inherited("__pinion.reactive.quit_sink", || Arc::new(NullQuitSink));

/// R1363 §5.55 — hook returning the active owner scope's [`QuitSink`].
///
/// Returns the sink the shell seeded into [`QUIT_SINK`] at boot, or a
/// [`NullQuitSink`] off a live shell. Bindings call this inside
/// `create_extra_externals` (alongside the producer-thread spawn) and hand the
/// returned `Arc` to the thread — the
/// [`use_repaint_sink`](super::repaint::use_repaint_sink) discipline.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (the same shape as the
/// other `use_*` hooks).
#[must_use]
pub fn use_quit_sink() -> Arc<dyn QuitSink> {
    Arc::clone(&QUIT_SINK.resolve_current())
}

#[cfg(test)]
mod tests {
    use super::super::owner::Owner;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct CountingSink(Arc<AtomicUsize>);
    impl QuitSink for CountingSink {
        fn request_quit(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// A sink and the counter it increments.
    fn counting() -> (Arc<dyn QuitSink>, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        (Arc::new(CountingSink(Arc::clone(&count))), count)
    }

    // The verdict, EMITTED from the declaration rather than remembered.
    crate::provider_slot_tests!(r1366_2_quit_sink_inherits, super::QUIT_SINK, || {
        counting().0
    });

    #[test]
    fn null_default_is_a_silent_no_op() {
        let owner = Owner::new();
        QUIT_SINK.resolve(&owner).request_quit();
        QUIT_SINK.resolve(&owner).request_quit();
    }

    #[test]
    fn provided_sink_receives_the_request() {
        let owner = Owner::new();
        let (sink, count) = counting();
        QUIT_SINK.provide(&owner, sink);
        QUIT_SINK.resolve(&owner).request_quit();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    #[should_panic(expected = "already seeded")]
    fn r1366_2_a_late_seed_panics_where_it_used_to_be_dropped() {
        // The counterfactual of R1363's `provide_is_first_write_wins`, which
        // asserted that a second seed was a SILENT no-op leaving every reader on
        // the first sink — and `Owner::provide_quit_sink`'s own doc called that
        // an idempotent-by-first-write convenience. On THIS slot that reading is
        // the least defensible of the family: a dropped quit sink is a Quit that
        // does nothing, which is the R1362 defect, and a shell that seeds twice
        // has two quit paths and cannot know which one the binding holds.
        let owner = Owner::new();
        QUIT_SINK.provide(&owner, counting().0);
        QUIT_SINK.provide(&owner, counting().0);
    }

    #[test]
    fn sink_handle_crosses_a_thread_boundary() {
        // The `Send + Sync` bound is the point: sprag's poll thread owns this.
        let owner = Owner::new();
        let (sink, count) = counting();
        QUIT_SINK.provide(&owner, sink);
        let handle = Arc::clone(&QUIT_SINK.resolve(&owner));
        std::thread::spawn(move || handle.request_quit())
            .join()
            .expect("producer thread panicked");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod r1364_scope_tests {
    //! R1364 §5.22 §5.55 — the quit sink resolves from a child scope.
    //!
    //! This is the R680 characterization, on the slot where the failure is
    //! loudest: the shell seeds `QuitSink` once on `root_owner`, and the deferred
    //! R680 atomic runs a secondary window's view under `window_owner(id)`. With
    //! the pre-R1364 root-only `Owner::cache`, `use_quit_sink()` there returned a
    //! `NullQuitSink` and the app's Quit button did nothing, with no panic and no
    //! log — the exact defect R1362 existed to fix, resurrected by a change that
    //! never mentions quitting.

    use super::super::owner::Owner;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct CountingSink(Arc<AtomicUsize>);
    impl QuitSink for CountingSink {
        fn request_quit(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn r1364_a_child_scope_resolves_the_shells_real_quit_sink() {
        let root = Owner::new();
        let count = Arc::new(AtomicUsize::new(0));
        QUIT_SINK.provide(&root, Arc::new(CountingSink(Arc::clone(&count))));

        // What `window_owner(secondary)` is, and what R680 will run the view in.
        // The generated verdict test above asserts inheritance through
        // `resolve`; this asserts the same through the BINDING's path, so a hook
        // that stopped delegating to the slot could not pass both.
        let window_scope = Owner::new_child(&root);
        window_scope.run(|| use_quit_sink().request_quit());

        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "a Quit raised from a secondary window's scope must reach the shell; \
             0 here is the silent Null — a Quit button that does not quit",
        );
    }
}
