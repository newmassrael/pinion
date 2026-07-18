//! Shared async `scene/waitFor` wiring — one home for the shared
//! [`WaiterRegistry`] and the single scene [`SceneRevision`] token they wake off
//! (R1270 §6.3).
//!
//! Both are [`ProviderSlot`] `inherited` statics
//! (R1366.9 §5.22): key, default and inherit-scope in one declaration, in the
//! crate that owns the slot. So the shell's dispatch (which parks a
//! `scene/waitFor {since}`), the boot wake-observer install, and a binding's
//! external-data producer all resolve the *same* `Arc` — one instance by
//! construction, not by two handles happening to agree, and not merely while
//! every scope happens to BE root (R1365 §5.22). Seeded on the root [`Owner`] at
//! boot by [`ShellCore`](crate::ShellCore) `with_core`, then resolved through the
//! slot's `Inherited` parent walk from wherever a `view` runs.
//!
//! The [`WaiterRegistry`] owns no version counter: it wakes off the OCC
//! [`SceneRevision`] the whole app already shares. The shell installs a wake
//! observer on that revision at boot, so **every** bump — a dispatched mutation,
//! shell input, or an external-data arrival — advances the one token and wakes
//! parked waiters. A producer thread resolves the same revision via
//! [`use_scene_revision`] and calls
//! [`SceneRevision::bump`] on arrival: the wake
//! sibling of [`RepaintSink::request_repaint`](pinion_core::RepaintSink::request_repaint).

use std::sync::Arc;

use pinion_core::{Owner, ProviderSlot, SceneRevision};
use pinion_rpc::WaiterRegistry;

/// The shared async `scene/waitFor` [`WaiterRegistry`] — parked replies for every
/// binding, woken off the one scene [`SceneRevision`].
///
/// R1366.9 §5.22 — [`ProviderSlot`] `inherited`. The
/// dispatch park side and the boot wake-observer install both resolve this, so
/// they land on a single `Arc`. It inherits because the deferred R680 atomic
/// (`window_owner(id).run(..)`) would otherwise hand a secondary window its own
/// empty registry, and a `scene/waitFor` parked there would never be woken by the
/// shell that drives the root's. `resolve_waiter_registry` takes `&Owner`, so its
/// type already admits a child scope; "no caller passes one today" is not a
/// reason to leave an accepted input partial.
static WAITER_REGISTRY: ProviderSlot<Arc<WaiterRegistry>> =
    ProviderSlot::inherited("__pinion.rpc.waiter_registry", || {
        Arc::new(WaiterRegistry::new())
    });

/// The shared scene [`SceneRevision`] — the single scene version token (§5.34 OCC
/// + §6.3 waitFor).
///
/// R1366.9 §5.22 — [`ProviderSlot`] `inherited`, the
/// last of the ten framework slots to move off a hand-rolled key +
/// `cache_inherited` pair. There is exactly ONE authoritative revision per
/// process: [`ShellCore`](crate::ShellCore) seeds it on the root owner at boot,
/// the RPC layer hands it to every handler as `ctx.revision`, and the boot wake
/// observer wakes parked waiters off it. A child scope minting its own would be a
/// private counter nobody reads — every `scene/waitFor` parked against the real
/// one would hang forever. Unlike `viewport_size` there is no honest per-window
/// reading of "the scene changed" to preserve, so it inherits.
static SCENE_REVISION: ProviderSlot<Arc<SceneRevision>> =
    ProviderSlot::inherited("__pinion.core.scene_revision", || {
        Arc::new(SceneRevision::default())
    });

/// Resolve the binding's shared async [`WaiterRegistry`]. The outer `Rc` from the
/// slot stays on the UI thread; the returned inner `Arc` is the shareable handle
/// the boot observer moves into its `Send` wake closure.
pub(crate) fn resolve_waiter_registry(owner: &Owner) -> Arc<WaiterRegistry> {
    Arc::clone(&WAITER_REGISTRY.resolve(owner))
}

/// Resolve the binding's shared scene [`SceneRevision`] — the single version
/// token. The outer `Rc` stays on the UI thread; the returned inner `Arc` is the
/// handle a producer thread bumps on arrival (via [`use_scene_revision`]).
pub(crate) fn resolve_scene_revision(owner: &Owner) -> Arc<SceneRevision> {
    Arc::clone(&SCENE_REVISION.resolve(owner))
}

/// Binding-facing hook: the shared scene [`SceneRevision`], the single version
/// token the OCC preview lifecycle and async `scene/waitFor` both key off.
///
/// A producer thread (a pane / socket / timer reader) resolves this once at
/// wiring time — a `create_extra_externals` hook, before spawning the thread,
/// the [`use_repaint_sink`](pinion_core::use_repaint_sink) discipline — and
/// calls [`SceneRevision::bump`] each time it
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
mod r1366_9_slots {
    //! R1366.9 §5.22 §6.3 — both slots are `Inherited`, and each ships the
    //! generated wiring guard PLUS a value-based discriminator the guard cannot
    //! be. The R1366.8.1 audit named why the guard is not enough: its `ptr_eq`
    //! outcome and its assertion branch both derive from `scope()`, so flipping
    //! the verdict flips them together and it still passes — it catches a broken
    //! `cache_inherited`/`cache` or a forgotten test, not a WRONG verdict. The
    //! value tests below seed a distinct, observable state on the root and read
    //! it back through a CHILD scope, so a wrong `per_scope` verdict FAILS them.
    //!
    //! They pin the R680 question before R680: the day the view wrap becomes
    //! `window_owner(id).run(..)`, a secondary window must still reach the ONE
    //! token the shell's wake observer watches. The failure is silent — `bump()`
    //! succeeds, nothing wakes — so nothing about landing R680 would make its
    //! author think of `waitFor`.

    use super::{resolve_scene_revision, resolve_waiter_registry};
    use pinion_core::{Owner, SceneRevision};
    use pinion_rpc::{RpcReply, WaiterRegistry};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    // The wiring guards — a `cache_inherited`/`cache` regression or a forgotten
    // test fails these; a WRONG verdict does not (that is the value tests below).
    pinion_core::provider_slot_tests!(r1366_9_scene_revision_wiring, super::SCENE_REVISION, || {
        Arc::new(SceneRevision::default())
    });
    pinion_core::provider_slot_tests!(
        r1366_9_waiter_registry_wiring,
        super::WAITER_REGISTRY,
        || Arc::new(WaiterRegistry::new())
    );

    #[test]
    fn r1366_9_a_child_scope_wakes_the_roots_scene_revision() {
        // The value-based discriminator for SCENE_REVISION. The shell installs
        // its wake observer on the token it resolved at boot
        // (`ShellCore::with_core`); the failure this exists for is a producer
        // bumping something that observer is not watching. So install one on the
        // root's token and bump through the CHILD's handle — if SCENE_REVISION
        // were wrongly `per_scope`, the child would mint its own token, the
        // observer would never fire, and this FAILS against the constant `1`.
        // (Verified by running it against plain `cache`.)
        let root = Owner::new();
        let seeded = resolve_scene_revision(&root);
        let child = Owner::new_child(&root);
        let from_child = resolve_scene_revision(&child);

        let woken = Arc::new(AtomicU64::new(0));
        let sink = Arc::clone(&woken);
        assert!(
            seeded.set_observer(move |rev| sink.store(rev, Ordering::SeqCst)),
            "the test must own the one observer slot",
        );
        from_child.bump();
        assert_eq!(
            woken.load(Ordering::SeqCst),
            1,
            "a bump through the child scope did not reach the observer the shell \
             installed on the root's token — SCENE_REVISION must be Inherited",
        );
    }

    #[test]
    fn r1366_9_a_child_scope_parks_on_the_roots_waiter_registry() {
        // The value-based discriminator for WAITER_REGISTRY — not just
        // `Arc::ptr_eq`. Park a waiter on the root's registry, then observe it
        // through a CHILD scope's handle. If WAITER_REGISTRY were wrongly
        // `per_scope`, the child would mint its own empty registry,
        // `parked_count()` would read 0, and this FAILS — a secondary window's
        // `scene/waitFor` would park where the shell's wake observer never looks.
        let root = Owner::new();
        let root_registry = resolve_waiter_registry(&root);
        let rev = SceneRevision::new(); // current == 0
        // `since = 1 > 0`, so this parks rather than answering immediately. A
        // notification (id `None`) still parks; only `wake` drops it un-replied.
        root_registry.park_if_current(&rev, 1, None, RpcReply::new(|_| {}));
        assert_eq!(root_registry.parked_count(), 1);

        let child = Owner::new_child(&root);
        assert_eq!(
            resolve_waiter_registry(&child).parked_count(),
            1,
            "a child scope minted its own WaiterRegistry — the waiter parked on \
             the root's is invisible, so a secondary window's scene/waitFor would \
             never be woken by the shell that drives the root",
        );
    }
}
