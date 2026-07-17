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

/// Owner-cache newtype: [`Owner::cache`](super::owner::Owner::cache) stores
/// `Rc<dyn Any>`, so the `Send` trait object rides inside this holder. The outer
/// `Rc<QuitSinkHolder>` stays on the UI thread; the inner `Arc<dyn QuitSink>` is
/// the handle that crosses to the producer thread.
pub(crate) struct QuitSinkHolder(pub(crate) Arc<dyn QuitSink>);

/// Private owner-cache key for the single per-owner quit sink slot.
pub(crate) const QUIT_SINK_KEY: &str = "__pinion.reactive.quit_sink";

/// R1363 §5.55 — hook returning the active owner scope's [`QuitSink`].
///
/// Returns the sink the shell seeded at boot, or a [`NullQuitSink`] off a live
/// shell. Bindings call this inside `create_extra_externals` (alongside the
/// producer-thread spawn) and hand the returned `Arc` to the thread — the
/// [`use_repaint_sink`](super::repaint::use_repaint_sink) discipline.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (the same shape as the
/// other `use_*` hooks).
#[must_use]
pub fn use_quit_sink() -> Arc<dyn QuitSink> {
    super::owner::Owner::current()
        .expect("use_quit_sink requires an active Owner scope")
        .quit_sink()
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

    #[test]
    fn null_default_is_a_silent_no_op() {
        let owner = Owner::new();
        owner.quit_sink().request_quit();
        owner.quit_sink().request_quit();
    }

    #[test]
    fn provided_sink_receives_the_request() {
        let owner = Owner::new();
        let count = Arc::new(AtomicUsize::new(0));
        owner.provide_quit_sink(Arc::new(CountingSink(Arc::clone(&count))));
        owner.quit_sink().request_quit();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn provide_is_first_write_wins() {
        let owner = Owner::new();
        let first = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(AtomicUsize::new(0));
        owner.provide_quit_sink(Arc::new(CountingSink(Arc::clone(&first))));
        owner.provide_quit_sink(Arc::new(CountingSink(Arc::clone(&second))));
        owner.quit_sink().request_quit();
        assert_eq!(first.load(Ordering::SeqCst), 1);
        assert_eq!(second.load(Ordering::SeqCst), 0, "the late seed is dropped");
    }

    #[test]
    fn sink_handle_crosses_a_thread_boundary() {
        // The `Send + Sync` bound is the point: sprag's poll thread owns this.
        let owner = Owner::new();
        let count = Arc::new(AtomicUsize::new(0));
        owner.provide_quit_sink(Arc::new(CountingSink(Arc::clone(&count))));
        let handle = owner.quit_sink();
        std::thread::spawn(move || handle.request_quit())
            .join()
            .expect("producer thread panicked");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
