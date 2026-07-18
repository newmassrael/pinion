//! R1362 PR-65 §5.16 §5.23 §5.49 §2 #2 — the binding-facing "request a window
//! control from my own code" boundary.
//!
//! # Why this exists
//!
//! pinion owns the exit decision, and it is already correct: `Close` is offered
//! to the binding first ([`WidgetView::window_close_requested`](crate::WidgetView::window_close_requested))
//! and only an UNHANDLED close exits the app. But every producer that could
//! reach that arm was **external input** — a physical press on a chrome control
//! (`AppShell::try_chrome_press`), an OS `WindowEvent::CloseRequested`, or an RPC
//! `scene/click` on a control tag (R1188). When the binding is the one that
//! *knows* a window must close, it had no way to say so.
//!
//! (The Escape convention ends the APP, and is not a producer of that arm at
//! all. R1363 §5.55 — it never consults `window_close_requested` because a quit
//! is not a close; it passes
//! [`WidgetCore::app_quit_requested`](pinion_core::WidgetCore::app_quit_requested)
//! instead. This paragraph used to say Escape "bypasses the binding's veto",
//! which was true when written and false one round later. See
//! `AppShell::request_quit`'s termination map.)
//!
//! The forcing cases are two, and they disagree about threads:
//!
//! * **`hello-tray`** — the tray menu's `Quit` item ran on the UI thread, set a
//!   `Signal<bool>` nothing consumed, and the app stayed up. pinion shipped a
//!   Quit button that could not quit.
//! * **sprag** (PR-65) — a socket poll thread discovers its daemon is gone (a
//!   parked `scene/waitFor` returns `UnexpectedEof`) and must close the client,
//!   the tmux convention. It is the same off-thread producer
//!   [`RepaintSink`](pinion_core::RepaintSink) was built for (R999): the sibling
//!   that says *"my host is gone, close me"* to the one that says *"my data
//!   changed, repaint"*.
//!
//! # Why a sink, and not a `ShellCore` method
//!
//! A binding never reaches the LIVE [`ShellCore`](crate::ShellCore): every
//! `WidgetView` / `WidgetCore` / `WidgetA11y` method is an associated fn with no
//! `self`, and `AppShell`'s `core` field is private. `ShellCore` is `pub` and
//! `ShellCore::new()` is `pub`, so a `ShellCore::request_window_control` WOULD
//! compile for a binding — against a throwaway instance driving nothing. A
//! public API that silently does nothing for its apparent audience is worse than
//! one that does not exist, so the request rides a handle the binding actually
//! holds. `ShellCore::request_redraw` is no counter-example: it has zero binding
//! callers and is the shell-internal *terminus* of the
//! [`RepaintSink`](pinion_core::RepaintSink) path, not its entry point.
//!
//! # Why this is not one more `AppEvent::ExternalRepaint` rider
//!
//! Interface segregation, in this repo's own words
//! ([`RepaintSink`](pinion_core::RepaintSink)'s module doc): *"a producer that
//! only repaints must not depend on `send(Intent)`"*. The mirror holds — a
//! producer that only closes must not depend on `request_repaint`. Hence a
//! distinct trait and a distinct [`AppEvent`](crate::AppEvent) variant.
//!
//! # Why this trait lives HERE, above `pinion-core`
//!
//! No crate below pinion-shell has windows to control (`pinion-tui` deps neither
//! overlay nor shell), and `pinion-shell` already deps both the vocabulary and
//! the DI substrate ([`pinion_core::Owner`]) — the R1077 lesson: prefer the
//! crate that deps both over a type relocation.
//!
//! R1363 §5.55 — this section used to argue further that relocating
//! [`WindowControl`] itself "would re-split exactly what R1190 fused". That
//! argument was WRONG and R1363 acted against it: `WindowControl` now lives in
//! `pinion-runtime` beside `DEFAULT_WINDOW` (the addressee every consumer needs
//! next to the verb), re-exported here for the existing path. R1190 fused the
//! tag→semantic DECISION, which is still `chrome_tag_semantic`'s alone in
//! pinion-overlay; that a type's home is not a decision's home is exactly the
//! conflation the argument made. It is recorded rather than deleted because a
//! plausible wrong argument in-tree is quoted as settled by the next round.
//!
//! The app-lifecycle peer of this seam is [`QuitSink`](pinion_core::QuitSink),
//! which lives in `pinion-core` for the opposite and equally deliberate reason:
//! BOTH backends dep it, so §2 #6 holds for app lifecycle instead of leaving a
//! terminal-shaped hole.
//!
//! The seeding window is the only thing that made this awkward, and
//! [`CoreShell::new_with_seed`](pinion_runtime::CoreShell::new_with_seed) closes
//! it: `AppShell::new` seeds this slot through the same door as the core-homed
//! repaint sink, before any binding factory can resolve a hook.

use std::sync::Arc;

use pinion_core::ProviderSlot;
use pinion_overlay::WindowControl;

/// R1362 PR-65 §5.16 §5.49 §2 #2 — the shell-supplied "request a window control
/// from any thread" edge.
///
/// `Send + Sync + 'static` so the concrete handle clones into a background
/// producer thread (the shell's impl wraps a winit `EventLoopProxy`, which is
/// `Send + Sync`). A binding obtains the active scope's sink through
/// [`use_window_control_sink`] and either calls it inline (a tray menu item on
/// the UI thread) or hands a clone to its producer thread (sprag's socket poll
/// thread).
///
/// The request is a **request**, not an order: it lands on the one execution arm
/// (`AppShell::apply_window_control`) that a chrome press and an RPC
/// `scene/click` already share, so a `Close` is still offered to
/// [`WidgetView::window_close_requested`](crate::WidgetView::window_close_requested)
/// first. A binding therefore cannot bypass its own veto by calling this — it is
/// one more producer into one arm (`ControlProducer::Binding`; R1364 made that
/// roster a type, because this sentence said "a third producer" and was wrong
/// from the round that wrote it), which is precisely why it needs no new exit
/// semantics, no new state, and no second close vocabulary.
///
/// R1363 §5.55 — and no exit semantics at all now: a `Close` closes a window and
/// never ends the app. The app-lifecycle peer of this seam is
/// [`QuitSink`](pinion_core::QuitSink).
///
/// Delivery is asynchronous by construction: the control executes on a later UI
/// -thread turn, never inside this call. That is what lets a tray `invoke` return
/// its RPC result, and an in-flight `scene/click {close}` client see its
/// `result`, before the window goes away.
pub trait WindowControlSink: Send + Sync + 'static {
    /// Ask the shell to apply `control` to the window `window_id` names (the
    /// [`WindowSpec::id`](crate::WindowSpec::id) canonical id;
    /// [`pinion_runtime::DEFAULT_WINDOW`] for a single-window binding).
    ///
    /// Non-blocking and infallible: an unknown `window_id` or an
    /// already-shut-down event loop drops the request, matching the
    /// [`RepaintSink`](pinion_core::RepaintSink) / `RpcIngress` error-absorption
    /// convention (at that point there is no window left to control).
    fn request_window_control(&self, window_id: &str, control: WindowControl);
}

/// R1362 PR-65 — Null Object [`WindowControlSink`]: the default when no shell
/// has provided a real one (headless screenshot, RPC-driven tests, unit tests).
///
/// Dropping requests on the floor is the correct behaviour off the live event
/// loop: there is no winit `Window` to minimize and no `ActiveEventLoop` to
/// exit. Bindings therefore call [`use_window_control_sink`] unconditionally,
/// without an `Option` or a panic guard — exactly as they call
/// [`use_repaint_sink`](pinion_core::use_repaint_sink).
#[derive(Debug, Default, Clone, Copy)]
pub struct NullWindowControlSink;

impl WindowControlSink for NullWindowControlSink {
    fn request_window_control(&self, _window_id: &str, _control: WindowControl) {}
}

/// R1366.4 PR-65 §5.16 §5.49 — the window-control slot: its key, its Null default
/// and its inherit verdict as one expression, in the crate that owns it. The
/// shell-side peer of core's [`REPAINT_SINK`](pinion_core::REPAINT_SINK),
/// declared here because `pinion-core` cannot name [`WindowControlSink`].
///
/// **`Inherited`** by the mechanical predicate — `AppShell::new` DRIVES this at
/// the root owner from its
/// [`CoreShell::new_with_seed`](pinion_runtime::CoreShell::new_with_seed) closure,
/// after the root [`Owner`](pinion_core::Owner) exists but BEFORE any binding
/// factory (`create_external` / `create_extra_externals`) resolves a hook, so the
/// first [`use_window_control_sink`] read gets the live sink. A child scope
/// resolves that root value through
/// `Owner::cache_inherited` (crate-private to pinion-core since R1366.10).
///
/// This is the slot R1362 and R1364 existed to fix, and the one where the
/// per-scope failure is loudest: under the deferred R680 atomic
/// (`window_owner(id).run(..)`) a secondary window would resolve a freshly minted
/// [`NullWindowControlSink`] and its Close button would silently no-op — no panic,
/// no log. [`provider_slot_tests!`](pinion_core::provider_slot_tests) EMITS the
/// verdict from this declaration, so it cannot be forgotten the way R1365 forgot
/// five of the inheriting slots (this among them).
///
/// The payload is the `Arc<dyn WindowControlSink>` itself, with no newtype
/// wrapper: [`Owner::cache`](pinion_core::Owner::cache) keys on
/// `(TypeId::of::<V>(), key)`, so the trait
/// object is already its own type — R1362's `WindowControlSinkHolder` was the
/// `Rc<dyn Any>` storage showing through, the wrapper R1366.1 retired for the
/// repaint sink. `Send + Sync` so the handle clones into a producer thread (the
/// shell's impl wraps a `Send + Sync` winit `EventLoopProxy`).
///
/// Seeding is the backend's job: a BINDING seeding this would now PANIC on the
/// shell's earlier seed rather than silently losing to it
/// ([`ProviderSlot::provide`](pinion_core::ProviderSlot::provide)). It stays
/// reachable because the useful caller is a **test** — seed a bare `Owner`, run
/// the binding's own factory inside it, and its real [`use_window_control_sink`]
/// resolves the recording sink, exercising the production path instead of
/// injecting a handle by hand.
pub static WINDOW_CONTROL_SINK: ProviderSlot<Arc<dyn WindowControlSink>> =
    ProviderSlot::inherited("__pinion.shell.window_control_sink", || {
        Arc::new(NullWindowControlSink)
    });

/// R1362 PR-65 §5.16 §5.23 — binding-facing hook: the active owner scope's
/// [`WindowControlSink`].
///
/// Resolve it once at wiring time — inside `create_extra_externals`, before
/// spawning the thread, the [`use_repaint_sink`](pinion_core::use_repaint_sink)
/// / [`use_scene_revision`](crate::use_scene_revision) discipline — and hand the
/// returned `Arc` to the producer. A UI-thread caller (a tray menu handler) may
/// hold it and call it inline just as well; the request is queued either way.
///
/// # Any scope in the binding's tree (R1364)
///
/// The shell seeds this slot on `root_owner`, and resolution walks up to find it
/// (`Owner::cache_inherited`), so a child
/// scope gets the REAL sink.
///
/// Until R1364 it was root-only, and R1362 documented that as "not a live hazard
/// today" because every view runs under root — while noting that the deferred
/// R680 atomic (`window_owner(window_id).run(..)`) would make a secondary
/// window's `view` resolve a [`NullWindowControlSink`] that no-ops with no panic
/// and no log. That prescription ("a parent walk for provider slots, or
/// per-window re-seeding") is discharged for this slot and for the three
/// core-homed ones that shared the shape verbatim. It was paid off before R680
/// rather than as part of it, because the failure it produces is silent and
/// names nothing: the round that finally lands R680 would have had no reason to
/// suspect quitting.
///
/// R1365 — this sentence first read "is now discharged", full stop, which was an
/// overstatement: R1364 fixed the slots it had enumerated, and the enumeration
/// was short. `scene_revision` was still root-only, with the same silent
/// post-R680 failure one seam over.
///
/// R1365.1 — "discharged for the family" was not a thing this doc could promise
/// while the census was source-text: it could not see a slot that skips the
/// `__pinion.` prefix (nothing enforced it), and it could not tell a TRUE verdict
/// from a false one — that is per-slot behavioural coverage, which only 3 of the
/// 8 inheriting slots then had. R1366 delivered the promise: the `ProviderSlot<V>`
/// declaration type makes the scope a constructor argument and the prefix a
/// compile error, every framework slot migrated (the last two, `scene_revision`
/// and `waiter_registry`, in R1366.9), and each ships the generated wiring guard
/// plus a value-based discriminator. The family is certified.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope — call it from a `view`
/// / `create_extra_externals` hook (the same shape as every other `use_*` hook).
/// R1365.1 — this said both "run inside the root `Owner::run`"; the deferred
/// R680 atomic makes that false for a secondary window's `view`, which is the
/// hazard the paragraphs above are about.
#[must_use]
pub fn use_window_control_sink() -> Arc<dyn WindowControlSink> {
    Arc::clone(&WINDOW_CONTROL_SINK.resolve_current())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use std::sync::Mutex;

    /// Recording sink: captures every request so a test can assert the wire
    /// reached the handle (and crossed a thread boundary).
    #[derive(Debug, Default)]
    struct RecordingSink(Mutex<Vec<(String, WindowControl)>>);

    impl WindowControlSink for RecordingSink {
        fn request_window_control(&self, window_id: &str, control: WindowControl) {
            self.0
                .lock()
                .expect("recording sink poisoned")
                .push((window_id.to_owned(), control));
        }
    }

    // The verdict, EMITTED from the declaration — the generated `Inherited`
    // check R1365 forgot for five of its slots, this among them.
    pinion_core::provider_slot_tests!(
        r1366_4_window_control_sink_inherits,
        super::WINDOW_CONTROL_SINK,
        || -> Arc<dyn WindowControlSink> { Arc::new(RecordingSink::default()) }
    );

    #[test]
    fn null_default_is_a_silent_no_op() {
        // No shell provided a sink: the lazy default is NullWindowControlSink,
        // and a request must not panic (bindings call it unconditionally).
        let owner = Owner::new();
        WINDOW_CONTROL_SINK
            .resolve(&owner)
            .request_window_control("main", WindowControl::Close);
        WINDOW_CONTROL_SINK
            .resolve(&owner)
            .request_window_control("main", WindowControl::Minimize);
    }

    #[test]
    fn provided_sink_receives_requests() {
        let owner = Owner::new();
        let sink = Arc::new(RecordingSink::default());
        WINDOW_CONTROL_SINK.provide(&owner, sink.clone());
        WINDOW_CONTROL_SINK
            .resolve(&owner)
            .request_window_control("main", WindowControl::Close);
        assert_eq!(
            *sink.0.lock().expect("recording sink poisoned"),
            vec![("main".to_owned(), WindowControl::Close)],
        );
    }

    #[test]
    #[should_panic(expected = "already seeded")]
    fn r1366_4_a_late_seed_panics_where_it_used_to_be_dropped() {
        // The counterfactual of R1362's `provide_is_first_write_wins`, which
        // asserted a second seed was a SILENT no-op leaving every reader on the
        // first sink. On THIS slot that is the least defensible reading: a
        // dropped window-control sink is a Close button that does nothing (the
        // R1362 defect), and a shell that seeds twice cannot know which path a
        // binding holds.
        let owner = Owner::new();
        WINDOW_CONTROL_SINK.provide(&owner, Arc::new(RecordingSink::default()));
        WINDOW_CONTROL_SINK.provide(&owner, Arc::new(RecordingSink::default()));
    }

    #[test]
    #[should_panic(expected = "already seeded")]
    fn r1366_4_a_seed_after_a_read_panics_where_it_used_to_be_dropped() {
        // The seeding landmine `CoreShell::new_with_seed` closes, now LOUD. A
        // read BEFORE the seed used to cache the Null default permanently and
        // drop the late seed in silence — a binding left holding a dead handle.
        // `ProviderSlot::provide` turns that read-then-seed order into a panic,
        // so a shell that seeded too late aborts instead of shipping a Close
        // button that silently no-ops.
        let owner = Owner::new();
        let _resolved = WINDOW_CONTROL_SINK.resolve(&owner);
        WINDOW_CONTROL_SINK.provide(&owner, Arc::new(RecordingSink::default()));
    }

    #[test]
    fn sink_handle_crosses_a_thread_boundary() {
        // The `Send + Sync` bound is the point: sprag's poll thread owns this.
        let owner = Owner::new();
        let sink = Arc::new(RecordingSink::default());
        WINDOW_CONTROL_SINK.provide(&owner, sink.clone());
        let handle = Arc::clone(&WINDOW_CONTROL_SINK.resolve(&owner));
        std::thread::spawn(move || {
            handle.request_window_control("main", WindowControl::Close);
        })
        .join()
        .expect("producer thread panicked");
        assert_eq!(
            *sink.0.lock().expect("poisoned"),
            vec![("main".to_owned(), WindowControl::Close)],
        );
    }

    #[test]
    fn r1365_1_a_child_scope_resolves_the_shells_real_window_control_sink() {
        // R1365.1 §5.22 — the verdict through the BINDING's path. The generated
        // test above asserts inheritance through `resolve`; this asserts the
        // same through `use_window_control_sink`, so a hook that stopped
        // delegating to the slot could not pass both. This slot is the one R1362
        // and R1364 existed to fix; `Owner::new_child` is what R680 will run a
        // secondary window's view in, and the default here is
        // `NullWindowControlSink` — so a regression is a Close button that does
        // nothing, with no panic and no log.
        let root = Owner::new();
        let sink = Arc::new(RecordingSink::default());
        WINDOW_CONTROL_SINK.provide(&root, Arc::clone(&sink) as Arc<dyn WindowControlSink>);

        let window_scope = Owner::new_child(&root);
        window_scope
            .run(|| use_window_control_sink().request_window_control("main", WindowControl::Close));

        assert_eq!(
            *sink.0.lock().expect("poisoned"),
            vec![("main".to_owned(), WindowControl::Close)],
            "a child scope resolved a NullWindowControlSink instead of the \
             shell's — a secondary window's Close would silently no-op",
        );
    }
}
