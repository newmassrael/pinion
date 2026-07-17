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

use pinion_core::Owner;
use pinion_overlay::WindowControl;

/// The `Owner::cache` slot the shell's [`WindowControlSink`] lives in.
///
/// Private to this module: the key and BOTH of its writers
/// ([`provide_window_control_sink`]'s seed, [`resolve_window_control_sink`]'s
/// lazy Null default) live in exactly one file, so a future divergence cannot be
/// silently swallowed by `Owner::cache`'s first-write-wins (the `waiter.rs`
/// discipline).
const WINDOW_CONTROL_SINK_KEY: &str = "__pinion.shell.window_control_sink";

/// Owner-cache newtype: [`Owner::cache`] stores `Rc<dyn Any>`, so the `Send`
/// trait object rides inside this holder. The outer `Rc<WindowControlSinkHolder>`
/// stays on the UI thread; the inner `Arc<dyn WindowControlSink>` is the handle
/// that crosses to the producer thread. (Mirrors core's `RepaintSinkHolder`.)
struct WindowControlSinkHolder(Arc<dyn WindowControlSink>);

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

/// Seed a scope's [`WindowControlSink`] — the shell-side peer of
/// [`Owner::provide_repaint_sink`](pinion_core::Owner::provide_repaint_sink),
/// public for the same reasons and with the same caveat.
///
/// `AppShell::new` calls this from its
/// [`CoreShell::new_with_seed`](pinion_runtime::CoreShell::new_with_seed)
/// closure — i.e. after the root [`Owner`] exists but **before** the binding
/// factories (`create_external` / `create_extra_externals`) resolve any hook, so
/// the first [`use_window_control_sink`] read inside those factories gets the
/// live sink rather than the Null default.
///
/// Idempotent-by-first-write: like every [`Owner::cache`] slot the first call
/// wins and a later one is a no-op (the supplied sink is dropped, no panic
/// path). The shell seeds exactly once, before any read, so that is never
/// observed — and `new_with_seed` is what makes "before any read" structural
/// rather than a caller obligation. A BINDING calling this would therefore lose
/// to the shell's earlier seed and silently no-op: seeding is the backend's job.
///
/// It is public anyway, because the useful caller is a **test**: seed a bare
/// `Owner`, then run the binding's own factory inside it, and the factory's real
/// [`use_window_control_sink`] call resolves the recording sink. That exercises
/// the production resolution path — as `examples/hello-tray`'s
/// `r1362_quit_requests_a_close_through_the_window_control_sink` does — instead
/// of routing around it by hand-constructing the widget with an injected handle.
pub fn provide_window_control_sink(owner: &Owner, sink: Arc<dyn WindowControlSink>) {
    // `cache`'s factory is `FnOnce` and only runs when the slot is empty, so a
    // plain move closure seeds on the first call.
    owner.cache::<WindowControlSinkHolder, _>(WINDOW_CONTROL_SINK_KEY, move || {
        WindowControlSinkHolder(sink)
    });
}

/// Resolve the scope's [`WindowControlSink`] — whatever the shell seeded via
/// [`provide_window_control_sink`], or a [`NullWindowControlSink`] when none was
/// provided. The **one** home for this slot's key + lazy default.
///
/// R1364 §5.22 — [`Owner::cache_inherited`], so a child scope resolves the
/// ROOT's real sink instead of minting its own Null. This slot is why the walk
/// had to be the general answer rather than R1335's copy-the-handle-at-construction
/// trick: it is defined in `pinion-shell`, and `Owner::new_child` cannot name a
/// type from a crate above it.
fn resolve_window_control_sink(owner: &Owner) -> Arc<dyn WindowControlSink> {
    Arc::clone(
        &owner
            .cache_inherited::<WindowControlSinkHolder, _>(WINDOW_CONTROL_SINK_KEY, || {
                WindowControlSinkHolder(Arc::new(NullWindowControlSink))
            })
            .0,
    )
}

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
/// ([`Owner::cache_inherited`]), so a child scope gets the REAL sink.
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
/// R1365.1 — and "discharged for the family" is still not a thing this doc can
/// promise. `Owner::cache_inherited`'s census names a slot with no verdict, but
/// it cannot see a slot that skips the `__pinion.` prefix (nothing enforces it),
/// and it cannot tell a TRUE verdict from a false one — that is per-slot
/// behavioural coverage, which 3 of the 8 inheriting slots have. The promise
/// belongs to the `ProviderSlot<V>` declaration type (R1366), where the scope is
/// a constructor argument and the prefix is a compile error. Until then this
/// slot is fixed, tested, and the family is not certified.
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
    let owner = Owner::current().expect("use_window_control_sink requires an active Owner scope");
    resolve_window_control_sink(&owner)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn null_default_is_a_silent_no_op() {
        // No shell provided a sink: the lazy default is NullWindowControlSink,
        // and a request must not panic (bindings call it unconditionally).
        let owner = Owner::new();
        resolve_window_control_sink(&owner).request_window_control("main", WindowControl::Close);
        resolve_window_control_sink(&owner).request_window_control("main", WindowControl::Minimize);
    }

    #[test]
    fn provided_sink_receives_requests() {
        let owner = Owner::new();
        let sink = Arc::new(RecordingSink::default());
        provide_window_control_sink(&owner, sink.clone());
        resolve_window_control_sink(&owner).request_window_control("main", WindowControl::Close);
        assert_eq!(
            *sink.0.lock().expect("recording sink poisoned"),
            vec![("main".to_owned(), WindowControl::Close)],
        );
    }

    #[test]
    fn provide_is_first_write_wins() {
        // The shell seeds once before any read; a stray second provide is a
        // no-op (the supplied sink is dropped, the first stays installed).
        let owner = Owner::new();
        let first = Arc::new(RecordingSink::default());
        let second = Arc::new(RecordingSink::default());
        provide_window_control_sink(&owner, first.clone());
        provide_window_control_sink(&owner, second.clone());
        resolve_window_control_sink(&owner).request_window_control("main", WindowControl::Close);
        assert_eq!(first.0.lock().expect("poisoned").len(), 1);
        assert!(second.0.lock().expect("poisoned").is_empty());
    }

    /// The seeding landmine this slot is shaped to avoid: a read BEFORE the
    /// seed caches the Null default permanently, and the late seed is silently
    /// dropped (`Owner::cache` is first-write-wins with no failure path). This
    /// pins WHY `CoreShell::new_with_seed` exists — a shell that seeded after
    /// `ShellCore::new` returned would hand every binding a dead handle, with no
    /// panic and no log to reveal it.
    #[test]
    fn a_read_before_the_seed_permanently_wins_and_the_seed_is_dropped() {
        let owner = Owner::new();
        // A binding factory resolves the hook first (the too-late-seed order).
        let resolved = resolve_window_control_sink(&owner);
        let real = Arc::new(RecordingSink::default());
        provide_window_control_sink(&owner, real.clone());
        // The late seed lost: both the pre-seed handle and a fresh resolve are
        // the Null default, and the real sink never sees a request.
        resolved.request_window_control("main", WindowControl::Close);
        resolve_window_control_sink(&owner).request_window_control("main", WindowControl::Close);
        assert!(
            real.0.lock().expect("poisoned").is_empty(),
            "a seed after the first read must be silently dropped — the landmine \
             `CoreShell::new_with_seed` closes structurally",
        );
    }

    #[test]
    fn sink_handle_crosses_a_thread_boundary() {
        // The `Send + Sync` bound is the point: sprag's poll thread owns this.
        let owner = Owner::new();
        let sink = Arc::new(RecordingSink::default());
        provide_window_control_sink(&owner, sink.clone());
        let handle = resolve_window_control_sink(&owner);
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
        // R1365.1 §5.22 — the verdict, not the row. This slot is the one R1362
        // and R1364 existed to fix, and until now the ONLY thing asserting that
        // it inherits was a markdown row in `Owner::cache_inherited`'s rustdoc:
        // a silent revert to plain `cache` passed all four gates. An audit of
        // R1365 found 5 of the 8 `yes` slots in that state, this among them.
        //
        // `Owner::new_child` is exactly what R680 will run a secondary window's
        // view in, and the default here is `NullWindowControlSink` — so the
        // regression is a Close button that does nothing, with no panic and no
        // log. That is the bug R1362 was written to kill.
        let root = Owner::new();
        let sink = Arc::new(RecordingSink::default());
        provide_window_control_sink(&root, Arc::clone(&sink) as Arc<dyn WindowControlSink>);

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
