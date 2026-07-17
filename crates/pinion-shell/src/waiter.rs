//! Shared async `scene/waitFor` wiring — one home for resolving the async
//! [`WaiterRegistry`] and the single scene
//! [`SceneRevision`] token they wake off (R1270 §6.3).
//!
//! Both are seeded on the root [`Owner`] at boot and resolved through
//! [`Owner::cache_inherited`], so the shell's dispatch (which parks a
//! `scene/waitFor {since}`), the boot wake-observer install, and a binding's
//! external-data producer all resolve the *same* `Arc` — one instance by
//! construction, not by two handles happening to agree, and not merely while
//! every scope happens to BE root (R1365 §5.22). The key **and** the factory for
//! each live in exactly one function here (`resolve_*`), so a future divergence
//! cannot be silently dropped by `Owner::cache`'s first-write-wins.
//!
//! The [`WaiterRegistry`] owns no version counter:
//! it wakes off the OCC [`SceneRevision`] the whole app already shares. The
//! shell installs a wake observer on that revision at boot
//! ([`ShellCore`](crate::ShellCore) `with_core`), so **every** bump — a
//! dispatched mutation, shell input, or an external-data arrival — advances the
//! one token and wakes parked waiters. A producer thread resolves the same
//! revision via [`use_scene_revision`] and calls
//! [`SceneRevision::bump`](pinion_core::SceneRevision::bump) on arrival: the
//! wake sibling of [`RepaintSink::request_repaint`](pinion_core::RepaintSink::request_repaint).

use std::sync::Arc;

use pinion_core::{Owner, SceneRevision};
use pinion_rpc::WaiterRegistry;

/// The `Owner::cache` slot the shared [`WaiterRegistry`]
/// lives in.
pub(crate) const WAITER_REGISTRY_KEY: &str = "__pinion.rpc.waiter_registry";

/// The `Owner::cache` slot the shared scene [`SceneRevision`] lives in.
pub(crate) const SCENE_REVISION_KEY: &str = "__pinion.core.scene_revision";

/// Resolve the binding's shared async [`WaiterRegistry`], creating it on first
/// access. The **one** home for its key + factory: the shell's dispatch (park
/// side) and the boot observer install (wake side) both call this, so they land
/// on a single `Arc`. The outer `Rc` from the cache stays on the UI thread; the
/// returned inner `Arc` is the shareable handle.
///
/// R1365 §5.22 — [`Owner::cache_inherited`]. Every caller passes `root_owner()`
/// explicitly today, so no child scope can reach it and R680 cannot break it;
/// see the `cache_inherited` table for why it inherits regardless.
pub(crate) fn resolve_waiter_registry(owner: &Owner) -> Arc<WaiterRegistry> {
    Arc::clone(&owner.cache_inherited(WAITER_REGISTRY_KEY, || Arc::new(WaiterRegistry::new())))
}

/// Resolve the binding's shared scene [`SceneRevision`] — the single scene
/// version token (§5.34 OCC + §6.3 waitFor), creating it on first access. The
/// **one** home for its key + factory: [`ShellCore`](crate::ShellCore) holds it
/// for dispatch, and an external-data producer resolves the SAME `Arc` (via
/// [`use_scene_revision`]) to bump it on arrival.
///
/// R1365 §5.22 — [`Owner::cache_inherited`], the seventh slot to need it and the
/// one R1364 missed. There is exactly ONE authoritative revision per process:
/// the shell holds it for dispatch, the RPC layer hands it to every handler as
/// `ctx.revision`, and the boot observer wakes parked waiters off it. A child
/// scope minting its own would not be a per-window token, it would be a private
/// counter nobody reads — every `scene/waitFor` parked against the real one
/// would hang forever. Unlike `viewport_size` there is no honest per-window
/// reading of "the scene changed" to preserve.
pub(crate) fn resolve_scene_revision(owner: &Owner) -> Arc<SceneRevision> {
    Arc::clone(&owner.cache_inherited(SCENE_REVISION_KEY, || Arc::new(SceneRevision::default())))
}

/// Binding-facing hook: the shared scene [`SceneRevision`], the single version
/// token the OCC preview lifecycle and async `scene/waitFor` both key off.
///
/// A producer thread (a pane / socket / timer reader) resolves this once at
/// wiring time — a `create_extra_externals` hook, before spawning the thread,
/// the [`use_repaint_sink`](pinion_core::use_repaint_sink) discipline — and
/// calls [`SceneRevision::bump`](pinion_core::SceneRevision::bump) each time it
/// changes the scene. That one call advances the OCC token (so an in-flight
/// preview's `base_revision` detects the change) **and** wakes any parked async
/// `scene/waitFor` (the shell installed a wake observer on this token at boot)
/// — the wake sibling of
/// [`RepaintSink::request_repaint`](pinion_core::RepaintSink::request_repaint).
///
/// # Any scope in the binding's tree (R1365)
///
/// The shell seeds this slot on `root_owner` at boot and resolution walks up to
/// find it, so a child scope gets the REAL token. This matters for the same
/// reason it did for the sinks R1364 fixed: the deferred R680 atomic wraps each
/// window's `view` in `window_owner(id).run(..)`, and a secondary window's
/// producer would otherwise bump a private counter while every parked
/// `scene/waitFor` slept on the shell's.
///
/// # Panics
///
/// Panics if called with no active [`Owner`] scope — call from within a `view`
/// / `create_extra_externals` hook, both of which run inside an `Owner::run`.
#[must_use]
pub fn use_scene_revision() -> Arc<SceneRevision> {
    let owner = Owner::current().expect("use_scene_revision requires an active Owner scope");
    resolve_scene_revision(&owner)
}

#[cfg(test)]
mod r1365_slot_inherits {
    //! R1365 §5.22 §6.3 — both slots resolve the ROOT's instance from a child
    //! scope. These pin the R680 question before R680, the way
    //! `r1364_cache_inherited_tests` does for the sinks: the day the view wrap
    //! becomes `window_owner(id).run(..)`, a secondary window's
    //! `use_scene_revision` must still be the token the shell's wake observer
    //! watches. The failure is silent — `bump()` succeeds, nothing wakes — so
    //! nothing about landing R680 would make its author think of `waitFor`.

    use super::{resolve_scene_revision, resolve_waiter_registry};
    use pinion_core::Owner;

    #[test]
    fn r1365_a_child_scope_bumps_the_roots_scene_revision() {
        let root = Owner::new();
        let seeded = resolve_scene_revision(&root);
        let child = Owner::new_child(&root);

        let from_child = resolve_scene_revision(&child);
        assert!(
            std::sync::Arc::ptr_eq(&seeded, &from_child),
            "a child scope minted its own SceneRevision — every parked \
             scene/waitFor would sleep on the shell's token forever",
        );

        // R1365.1 — the wake, not the counter. The shell installs its observer
        // on the token it resolved at boot (`ShellCore::with_core`); the failure
        // this test exists for is a producer bumping something that observer is
        // not watching. So install one HERE, on the root's token, and bump
        // through the CHILD's handle.
        //
        // R1365 asserted `seeded.current() == before + 1` instead, which
        // `ptr_eq` two lines up had already made unfailable — and then its
        // ledger called that "the property, not just Arc identity". It was the
        // identity, restated. An observer is the property.
        let woken = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let sink = std::sync::Arc::clone(&woken);
        assert!(
            seeded.set_observer(move |rev| sink.store(rev, std::sync::atomic::Ordering::SeqCst)),
            "the test must own the one observer slot",
        );
        from_child.bump();
        assert_eq!(
            woken.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a bump through the child scope did not reach the observer the \
             shell installed on the root's token",
        );
        // Against `seeded.current()` rather than `1`, this assertion would be
        // dead too: when the child mints its own token BOTH sides read 0 and it
        // passes. A constant is what discriminates. (Verified by running it
        // against plain `cache`.)
    }

    #[test]
    fn r1365_a_child_scope_parks_on_the_roots_waiter_registry() {
        let root = Owner::new();
        let seeded = resolve_waiter_registry(&root);
        let child = Owner::new_child(&root);
        assert!(
            std::sync::Arc::ptr_eq(&seeded, &resolve_waiter_registry(&child)),
            "a child scope minted its own WaiterRegistry",
        );
    }
}
