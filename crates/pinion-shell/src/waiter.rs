//! Shared async `scene/waitFor` wiring — one home for resolving the async
//! [`WaiterRegistry`] and the single scene
//! [`SceneRevision`] token they wake off (R1270 §6.3).
//!
//! Both live in the root [`Owner`] cache so the shell's dispatch (which parks a
//! `scene/waitFor {since}`), the boot wake-observer install, and a binding's
//! external-data producer all resolve the *same* `Arc` — one instance by
//! construction, not by two handles happening to agree. The key **and** the
//! factory for each live in exactly one function here (`resolve_*`), so a
//! future divergence cannot be silently dropped by `Owner::cache`'s
//! first-write-wins.
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

/// Resolve the root scope's shared async [`WaiterRegistry`],
/// creating it on first access. The **one** home for its key + factory: the
/// shell's dispatch (park side) and the boot observer install (wake side) both
/// call this, so they land on a single `Arc`. The outer `Rc` from
/// `Owner::cache` stays on the UI thread; the returned inner `Arc` is the
/// shareable handle.
pub(crate) fn resolve_waiter_registry(owner: &Owner) -> Arc<WaiterRegistry> {
    Arc::clone(&owner.cache(WAITER_REGISTRY_KEY, || Arc::new(WaiterRegistry::new())))
}

/// Resolve the root scope's shared scene [`SceneRevision`] — the single scene
/// version token (§5.34 OCC + §6.3 waitFor), creating it on first access. The
/// **one** home for its key + factory: [`ShellCore`](crate::ShellCore) holds it
/// for dispatch, and an external-data producer resolves the SAME `Arc` (via
/// [`use_scene_revision`]) to bump it on arrival.
pub(crate) fn resolve_scene_revision(owner: &Owner) -> Arc<SceneRevision> {
    Arc::clone(&owner.cache(SCENE_REVISION_KEY, || Arc::new(SceneRevision::default())))
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
/// # Panics
///
/// Panics if called with no active [`Owner`] scope — call from within a `view`
/// / `create_extra_externals` hook, both of which run inside the root
/// `Owner::run`.
#[must_use]
pub fn use_scene_revision() -> Arc<SceneRevision> {
    let owner = Owner::current().expect("use_scene_revision requires an active Owner scope");
    resolve_scene_revision(&owner)
}
