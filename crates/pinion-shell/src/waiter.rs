//! `use_waiter_registry` — the binding-facing hook resolving the shared async
//! `scene/waitFor` registry from the root [`Owner`] scope (PR-50 §6.3).
//!
//! The [`WaiterRegistry`] is the wake sibling of
//! the [`RepaintSink`](pinion_core::RepaintSink): a binding's external-data
//! observer (a producer thread reading a pane / socket / timer) already calls
//! [`RepaintSink::request_repaint`](pinion_core::RepaintSink::request_repaint)
//! to wake the shell; the *same* dirty edge also calls
//! [`WaiterRegistry::notify_changed`](pinion_rpc::WaiterRegistry::notify_changed)
//! so any parked async `scene/waitFor` fires. The two consumers — that
//! producer and the shell's RPC dispatch (which parks a `scene/waitFor {since}`
//! whose baseline is current) — resolve the *same* `Arc` from one `Owner::cache`
//! slot, so a wait parked by dispatch and the change that wakes it share a
//! single registry by construction, not by two handles happening to agree.

use std::sync::Arc;

use pinion_core::Owner;
use pinion_rpc::WaiterRegistry;

/// The `Owner::cache` slot the shared [`WaiterRegistry`] lives in. Addressed by
/// both [`use_waiter_registry`] (the binding side) and the shell's dispatch
/// (the park side) so they land on one instance.
pub(crate) const WAITER_REGISTRY_KEY: &str = "__pinion.rpc.waiter_registry";

/// Resolve the root scope's shared async `scene/waitFor`
/// [`WaiterRegistry`], creating it on first access.
///
/// Call once at wiring time (a binding's `create_extra_externals`, resolved
/// *before* spawning a producer thread — the [`use_repaint_sink`](pinion_core::use_repaint_sink)
/// discipline) and hand the returned `Arc` to that thread; it calls
/// [`WaiterRegistry::notify_changed`](pinion_rpc::WaiterRegistry::notify_changed)
/// after writing new data. The outer `Rc` from `Owner::cache` stays on the UI
/// thread; the returned inner `Arc` is the `Send` handle that crosses to the
/// producer (mirroring the repaint sink's holder split).
///
/// # Panics
///
/// Panics if called with no active [`Owner`] scope — call from within a
/// `view` / `create_extra_externals` hook, both of which run inside the root
/// `Owner::run`.
#[must_use]
pub fn use_waiter_registry() -> Arc<WaiterRegistry> {
    let holder = Owner::current()
        .expect("use_waiter_registry requires an active Owner scope")
        .cache(WAITER_REGISTRY_KEY, || Arc::new(WaiterRegistry::new()));
    Arc::clone(&holder)
}
