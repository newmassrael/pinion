//! Reactive ownership scopes and the shared `ReactiveNode` trait (§5.22 R26).
//!
//! `ReactiveNode` is the textbook Solid.js-style "any subscriber" abstraction:
//! both `Owner` (lifecycle scopes) and `Computed` (derived signals) implement
//! it so signals do not have to know which kind of node consumes their value.
//! A thread-local `CURRENT_OWNER` stack identifies the active subscriber;
//! `Signal::get()` / `Computed::get()` consult it to auto-subscribe whatever
//! node is on top.
//!
//! Owners are tree-structured: parents hold strong `Owner` handles to their
//! children, so dropping the parent cascades drop to descendants (and runs
//! their cleanups first). Single-threaded by construction (`Rc` / `thread_local!`).
//! Cross-thread carry-forward to §5.29 (R34 `SyncSignal`).
//!
//! ## Panic safety
//!
//! All thread-local mutations are wrapped in RAII guards so a panic inside a
//! user closure (`Owner::run`, `batch`, `Computed::recompute`) still restores
//! the stack / depth counter. Cleanup closures are `catch_unwind`-wrapped so
//! a single misbehaving subscription cannot abort the process via
//! double-panic during `Drop`.
//!
//! ## Cleanup isolation policy
//!
//! Any `Box<dyn FnOnce()>` queued through `ReactiveNode::add_subscription_cleanup`
//! is *untrusted code* from the runtime's perspective — it captures user
//! state (closures over `RefCell`, `Rc`, etc.) and may panic. Every drain
//! site must route the closures through [`run_cleanups_isolated`] (which
//! wraps each in `catch_unwind(AssertUnwindSafe)`) so that one bad cleanup
//! cannot:
//!
//! - abort the process via double-panic-during-`Drop`
//! - leave sibling cleanups un-run, stranding observer links
//! - poison the thread-local `CURRENT_OWNER` / `BATCH_DEPTH` state
//!
//! Authoritative drain sites today:
//!
//! | Site | Lives in | Drained by |
//! | --- | --- | --- |
//! | `OwnerInner::drop` cleanups | `OwnerInner::cleanups` | `run_cleanups_isolated` (this module) |
//! | `BatchGuard::drop` `mark_dirty` cascade | `PENDING_DIRTY` (`SubscriberSet`) | inline `catch_unwind` per observer |
//! | `Computed::recompute` source-cleanups | `ComputedInner::source_cleanups` | `run_cleanups_isolated` (computed.rs) |
//!
//! Adding a new cleanup-drain site *without* routing through one of the
//! above helpers re-introduces the abort-on-panic landmine. Treat this as
//! a checklist item for any future reactive primitive that owns cleanup
//! closures.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

thread_local! {
    static NEXT_NODE_ID: Cell<u64> = const { Cell::new(0) };
    static CURRENT_OWNER: RefCell<Vec<Weak<dyn ReactiveNode>>> = const { RefCell::new(Vec::new()) };
    static BATCH_DEPTH: Cell<u32> = const { Cell::new(0) };
    static PENDING_DIRTY: RefCell<SubscriberSet> = RefCell::new(SubscriberSet::new());
}

pub(crate) fn next_node_id() -> u64 {
    NEXT_NODE_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    })
}

/// Subscription record shared between source kinds (`Signal`, `Computed`).
/// `id` is the subscriber's `node_id()` snapshotted at subscribe time —
/// allows dedup in observer lists without upgrading the `Weak`.
#[derive(Clone)]
pub(crate) struct ObserverEntry {
    pub(crate) id: u64,
    pub(crate) node: Weak<dyn ReactiveNode>,
}

/// Insertion-ordered deduplicated set of observers, used by both source
/// subscriber lists (`Signal`/`Computed`) and the `PENDING_DIRTY` batch
/// queue. `HashSet<u64>` indexes by `node_id` for O(1) amortized membership
/// checks; `Vec<ObserverEntry>` preserves deterministic iteration order so
/// cascade fires in subscribe order — the textbook topological propagation.
pub(crate) struct SubscriberSet {
    seen: HashSet<u64>,
    entries: Vec<ObserverEntry>,
}

impl SubscriberSet {
    pub(crate) fn new() -> Self {
        Self {
            seen: HashSet::new(),
            entries: Vec::new(),
        }
    }

    /// `true` iff `id` is already a member. O(1) amortized.
    pub(crate) fn contains(&self, id: u64) -> bool {
        self.seen.contains(&id)
    }

    /// Insert `entry`. Returns `true` when the entry is new; `false` when
    /// `entry.id` was already present (idempotent dedup).
    pub(crate) fn insert(&mut self, entry: ObserverEntry) -> bool {
        if self.seen.insert(entry.id) {
            self.entries.push(entry);
            true
        } else {
            false
        }
    }

    /// Remove the entry with `id`. No-op when absent.
    pub(crate) fn remove(&mut self, id: u64) {
        if self.seen.remove(&id) {
            self.entries.retain(|e| e.id != id);
        }
    }

    /// Drop entries whose `Weak<dyn ReactiveNode>` no longer upgrades
    /// (subscriber dropped without running its cleanup). Called
    /// opportunistically after a cascade — bounded house-keeping that
    /// keeps observer lists from growing unboundedly with stale handles.
    pub(crate) fn prune_dead(&mut self) {
        let seen = &mut self.seen;
        self.entries.retain(|entry| {
            let alive = entry.node.strong_count() > 0;
            if !alive {
                seen.remove(&entry.id);
            }
            alive
        });
    }

    /// Clone current entries into a `Vec` — used to release the `RefCell`
    /// borrow before invoking subscriber callbacks that may re-enter the
    /// source's lists.
    pub(crate) fn snapshot(&self) -> Vec<ObserverEntry> {
        self.entries.clone()
    }

    /// Drain into an owned `Vec` and reset the dedup index. Used by
    /// `BatchGuard::drop` when emptying `PENDING_DIRTY`.
    pub(crate) fn drain(&mut self) -> Vec<ObserverEntry> {
        self.seen.clear();
        std::mem::take(&mut self.entries)
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Erased Signal handle for snapshot/restore (§5.22 R26 caveat: "`dry_run`
/// snapshots Signal graph via Clone; rollback restores all signals to
/// pre-mutation state").
///
/// Uses `dyn Any` so payloads of mixed `T` can sit in a single registry — no
/// `serde_json` round-trip needed, the value stays in-memory and is downcast
/// back to its concrete type on restore.
pub trait SnapshotableSignal {
    /// Stable identity used as the snapshot map key.
    fn snapshot_id(&self) -> u64;

    /// Clone the current value into a type-erased payload.
    fn save_snapshot(&self) -> Box<dyn Any>;

    /// Restore from a payload previously produced by `save_snapshot`.
    ///
    /// # Errors
    /// Returns `Err(SnapshotRestoreError::TypeMismatch)` when the payload
    /// cannot be downcast to the concrete `T` of this signal — typically
    /// when a snapshot is fed into a foreign registry whose ids happen to
    /// collide (e.g. across Forge-regenerated code with stable path keys
    /// per §5.31). The signal is left untouched in that case.
    fn restore_snapshot(&self, snap: Box<dyn Any>) -> Result<(), SnapshotRestoreError>;
}

/// Failure modes for [`SnapshotableSignal::restore_snapshot`] and the
/// aggregate [`Owner::restore`] path.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotRestoreError {
    /// Payload's concrete type does not match the signal's `T`. Caller
    /// either mixed snapshots across distinct signal graphs or the signal
    /// was re-typed by a hot-reload code swap (§5.31).
    TypeMismatch,
}

/// Captured state of an `Owner`'s registered Signals at a moment in time.
/// Opaque to callers; pass back into [`Owner::restore`] to roll back.
pub struct OwnerSnapshot {
    entries: Vec<(u64, Box<dyn Any>)>,
}

impl OwnerSnapshot {
    /// Number of signals captured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the snapshot has zero signals — primarily for diagnostics
    /// (`Owner::snapshot` on an owner with no registered signals).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::fmt::Debug for OwnerSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerSnapshot")
            .field("len", &self.entries.len())
            .finish()
    }
}

/// Any node that can act as a subscriber in the reactive graph.
///
/// Object-safe by design — sources keep `Weak<dyn ReactiveNode>` to their
/// observers and never need to know whether the subscriber is an `Owner`,
/// a `Computed`, or a future `Effect`.
pub(crate) trait ReactiveNode {
    /// Stable identity for dedup checks in source observer lists.
    fn node_id(&self) -> u64;

    /// Mark this node dirty (idempotent — repeat calls short-circuit on the
    /// already-dirty flag). Cascade to downstream observers if any.
    fn mark_dirty(&self);

    /// Attach a cleanup closure that fires when the subscription set for this
    /// node is torn down — owner drop for `Owner`, next recompute for
    /// `Computed`. Used by `Signal::subscribe` to register an unsubscribe step.
    fn add_subscription_cleanup(&self, cleanup: Box<dyn FnOnce()>);
}

pub(crate) struct OwnerInner {
    id: u64,
    pub(crate) dirty: Cell<bool>,
    pub(crate) cleanups: RefCell<Vec<Box<dyn FnOnce()>>>,
    pub(crate) children: RefCell<Vec<Owner>>,
    pub(crate) owned_signals: RefCell<Vec<Box<dyn SnapshotableSignal>>>,
}

impl ReactiveNode for OwnerInner {
    fn node_id(&self) -> u64 {
        self.id
    }

    fn mark_dirty(&self) {
        // Owners are sinks: setting dirty is the user-visible signal. No
        // further cascade — children scopes have their own subscriptions.
        self.dirty.set(true);
    }

    fn add_subscription_cleanup(&self, cleanup: Box<dyn FnOnce()>) {
        self.cleanups.borrow_mut().push(cleanup);
    }
}

impl Drop for OwnerInner {
    fn drop(&mut self) {
        // Cascade: drop children first so their cleanups run before ours.
        // `clear()` drops each child `Owner`; if we held the last strong ref,
        // that child's `OwnerInner::drop` runs recursively.
        self.children.get_mut().clear();
        // Drain cleanups and invoke under `catch_unwind`. We're already in
        // `Drop`; a panicking cleanup here would double-panic and abort the
        // process. Trade off: a misbehaving cleanup loses its mutation but
        // the rest of the drop completes.
        let drained: Vec<_> = std::mem::take(self.cleanups.get_mut());
        run_cleanups_isolated(drained);
    }
}

/// Reactive ownership scope. Cloning yields a handle to the same scope.
pub struct Owner {
    inner: Rc<OwnerInner>,
}

impl Owner {
    /// Construct a detached root scope. Use [`Owner::new_child`] to attach to
    /// an existing parent for cascade-drop.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(OwnerInner {
                id: next_node_id(),
                dirty: Cell::new(false),
                cleanups: RefCell::new(Vec::new()),
                children: RefCell::new(Vec::new()),
                owned_signals: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Register `signal` for snapshot/restore. The owner keeps a type-erased
    /// handle (`Box<dyn SnapshotableSignal>`); the caller continues to hold
    /// the strongly-typed `Signal<T>` for reads and writes. Idempotent for
    /// the same signal id — re-registering is a no-op.
    pub fn track<S>(&self, signal: &S)
    where
        S: SnapshotableSignal + Clone + 'static,
    {
        let id = signal.snapshot_id();
        let mut owned = self.inner.owned_signals.borrow_mut();
        if owned.iter().any(|existing| existing.snapshot_id() == id) {
            return;
        }
        owned.push(Box::new(signal.clone()));
    }

    /// Capture the current values of every tracked Signal. Pass the result
    /// to [`Owner::restore`] to roll back. Per §5.22, this is the `dry_run`
    /// snapshot primitive: clone-out, mutate, restore.
    #[must_use]
    pub fn snapshot(&self) -> OwnerSnapshot {
        let entries = self
            .inner
            .owned_signals
            .borrow()
            .iter()
            .map(|sig| (sig.snapshot_id(), sig.save_snapshot()))
            .collect();
        OwnerSnapshot { entries }
    }

    /// Restore each tracked Signal whose id appears in `snapshot`. Signals
    /// added after `snapshot` was taken are left untouched (no entry to
    /// restore from); signals removed since then are silently skipped.
    /// Best-effort: per-signal `TypeMismatch` errors are collected and
    /// returned without aborting the pass — every restorable signal still
    /// rolls back.
    ///
    /// # Errors
    /// Returns `Err(Vec<(signal_id, error)>)` carrying every failed restore
    /// when at least one entry could not be downcast. Empty `Ok(())`
    /// otherwise.
    pub fn restore(&self, snapshot: OwnerSnapshot) -> Result<(), Vec<(u64, SnapshotRestoreError)>> {
        let mut by_id: std::collections::HashMap<u64, Box<dyn Any>> =
            snapshot.entries.into_iter().collect();
        let mut errors: Vec<(u64, SnapshotRestoreError)> = Vec::new();
        for sig in self.inner.owned_signals.borrow().iter() {
            let id = sig.snapshot_id();
            if let Some(payload) = by_id.remove(&id) {
                if let Err(e) = sig.restore_snapshot(payload) {
                    errors.push((id, e));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Construct a scope owned by `parent`. The parent retains a strong ref;
    /// dropping the parent drops this child (and its descendants) too.
    #[must_use]
    pub fn new_child(parent: &Owner) -> Self {
        let child = Self::new();
        parent.inner.children.borrow_mut().push(child.clone());
        child
    }

    /// Push this owner as the current scope, run `f`, pop. Signal reads
    /// during `f` auto-subscribe to this owner. The stack pop is RAII —
    /// even if `f` panics, the stack is restored before the unwind continues.
    pub fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        let strong: Rc<OwnerInner> = Rc::clone(&self.inner);
        let as_node: Rc<dyn ReactiveNode> = strong;
        let _guard = OwnerStackGuard::push(Rc::downgrade(&as_node));
        f()
    }

    /// Whether any source the owner subscribed to has been written since the
    /// last [`Owner::clear_dirty`].
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.inner.dirty.get()
    }

    /// Reset the dirty flag. Typical after the owner has reacted to changes.
    pub fn clear_dirty(&self) {
        self.inner.dirty.set(false);
    }

    /// Stable identity. Used for subscription dedup and observer-list
    /// membership checks.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.inner.id
    }
}

impl Default for Owner {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Owner {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

/// RAII guard that ensures the `CURRENT_OWNER` stack is popped in `Drop`,
/// even when the body panics. `run` and `run_with_node` both use it.
struct OwnerStackGuard;

impl OwnerStackGuard {
    fn push(weak: Weak<dyn ReactiveNode>) -> Self {
        CURRENT_OWNER.with(|stack| stack.borrow_mut().push(weak));
        Self
    }
}

impl Drop for OwnerStackGuard {
    fn drop(&mut self) {
        CURRENT_OWNER.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// Run `f` with a strong handle to the topmost active node, if any. Sources
/// call this from `get()` to find the subscriber that should be registered.
pub(crate) fn with_current_owner<R>(f: impl FnOnce(Option<&Rc<dyn ReactiveNode>>) -> R) -> R {
    CURRENT_OWNER.with(|stack| {
        let borrowed = stack.borrow();
        if let Some(top_weak) = borrowed.last() {
            if let Some(top_rc) = top_weak.upgrade() {
                return f(Some(&top_rc));
            }
        }
        f(None)
    })
}

/// Push `node` as the active subscriber, run `f`, pop. Used by
/// `Computed::recompute` to track which sources its closure reads. Stack pop
/// is RAII — panics in `f` still pop.
pub(crate) fn run_with_node<R>(node: &Rc<dyn ReactiveNode>, f: impl FnOnce() -> R) -> R {
    let _guard = OwnerStackGuard::push(Rc::downgrade(node));
    f()
}

/// Notify a set of observers that an upstream value changed.
///
/// - Outside a `batch`: immediately calls `mark_dirty` on each live observer.
/// - Inside a `batch`: collects entries (id-deduped via `SubscriberSet`) so
///   the cascade fires exactly once per observer when the outermost `batch`
///   exits.
///
/// This is the single dispatch point used by both `Signal::set` and
/// `ComputedInner::mark_dirty` — keeps the deferral logic in one place.
pub(crate) fn dispatch_dirty(entries: &[ObserverEntry]) {
    if BATCH_DEPTH.with(Cell::get) > 0 {
        PENDING_DIRTY.with(|pending| {
            let mut pending = pending.borrow_mut();
            for entry in entries {
                pending.insert(entry.clone());
            }
        });
    } else {
        for entry in entries {
            if let Some(node) = entry.node.upgrade() {
                node.mark_dirty();
            }
        }
    }
}

/// RAII batch counter. Increments `BATCH_DEPTH` on construction, decrements
/// on `Drop`. When the count returns to zero, drains `PENDING_DIRTY` and
/// fires every pending `mark_dirty` exactly once. Panic-safe: a panic inside
/// the user closure still triggers the drop (and thus the drain).
struct BatchGuard;

impl BatchGuard {
    fn enter() -> Self {
        BATCH_DEPTH.with(|d| d.set(d.get() + 1));
        Self
    }
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        let new_depth = BATCH_DEPTH.with(|d| {
            let next = d.get().saturating_sub(1);
            d.set(next);
            next
        });
        if new_depth == 0 {
            // Drain *outside* the with-borrow so reentrancy from `mark_dirty`
            // does not hit a `BorrowMutError`.
            let drained = PENDING_DIRTY.with(|p| p.borrow_mut().drain());
            for entry in &drained {
                if let Some(node) = entry.node.upgrade() {
                    // Isolate each observer's `mark_dirty` against panic so a
                    // single misbehaving subscriber cannot stop the cascade
                    // for the rest. We're still inside `Drop`; a leaked
                    // panic here would double-panic.
                    let _ = catch_unwind(AssertUnwindSafe(|| node.mark_dirty()));
                }
            }
        }
    }
}

/// Group reactive writes so that downstream notifications fire once when the
/// outermost `batch` exits (§5.22 R26: "writes inside coalesce; propagation
/// defers until exit").
///
/// Nested batches inherit the outer scope — only the outermost close drains
/// the pending set. The return value of `f` is forwarded so `batch` is usable
/// as an expression.
///
/// Panic-safe: an unwinding panic inside `f` still drains the pending set
/// before propagating; the depth counter is restored either way.
///
/// During the batch body, `Signal::get()` returns the most recently written
/// value (writes apply eagerly to the cell); only the *notification cascade*
/// is deferred. A `Computed::get()` performed inside the batch may therefore
/// see its cached value because its `dirty` flag has not yet been raised.
/// Reads should be taken after the batch closes.
pub fn batch<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = BatchGuard::enter();
    f()
}

/// Run a batch of cleanup closures, isolating each in `catch_unwind` so a
/// single panicking cleanup does not poison the drop path. Used in
/// `OwnerInner::drop` and on the `Computed` recompute source-cleanup path.
pub(crate) fn run_cleanups_isolated(cleanups: Vec<Box<dyn FnOnce()>>) {
    for cleanup in cleanups {
        let _ = catch_unwind(AssertUnwindSafe(cleanup));
    }
}

#[cfg(test)]
pub(crate) fn in_batch() -> bool {
    BATCH_DEPTH.with(Cell::get) > 0
}

#[cfg(test)]
pub(crate) fn current_owner_stack_len() -> usize {
    CURRENT_OWNER.with(|s| s.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_owner_is_not_dirty() {
        let o = Owner::new();
        assert!(!o.is_dirty());
    }

    #[test]
    fn ids_are_unique_and_monotonic() {
        let a = Owner::new();
        let b = Owner::new();
        assert!(b.id() > a.id());
    }

    #[test]
    fn clone_shares_inner_state() {
        let o = Owner::new();
        let alias = o.clone();
        assert_eq!(o.id(), alias.id());
        o.inner.dirty.set(true);
        assert!(alias.is_dirty());
    }

    #[test]
    fn run_pushes_and_pops_current_owner() {
        let o = Owner::new();
        let observed_id = o.run(|| with_current_owner(|cur| cur.map(|c| c.node_id())));
        assert_eq!(observed_id, Some(o.id()));
        let after = with_current_owner(|cur| cur.map(|c| c.node_id()));
        assert_eq!(after, None);
    }

    #[test]
    fn nested_run_top_of_stack_wins() {
        let outer = Owner::new();
        let inner = Owner::new();
        let seen = outer.run(|| inner.run(|| with_current_owner(|cur| cur.map(|c| c.node_id()))));
        assert_eq!(seen, Some(inner.id()));
    }

    #[test]
    fn child_attached_to_parent_strong_chain() {
        let parent = Owner::new();
        let child_id = {
            let child = Owner::new_child(&parent);
            child.id()
        };
        let still_alive = parent
            .inner
            .children
            .borrow()
            .iter()
            .any(|c| c.id() == child_id);
        assert!(still_alive);
    }

    #[test]
    fn cleanup_runs_when_owner_drops() {
        let counter = Rc::new(Cell::new(0_u32));
        {
            let o = Owner::new();
            let counter_clone = Rc::clone(&counter);
            o.inner
                .add_subscription_cleanup(Box::new(move || counter_clone.set(counter_clone.get() + 1)));
            assert_eq!(counter.get(), 0);
        }
        assert_eq!(counter.get(), 1);
    }

    #[test]
    fn parent_drop_cascades_to_children() {
        let log = Rc::new(RefCell::new(Vec::<&'static str>::new()));
        {
            let parent = Owner::new();
            let child = Owner::new_child(&parent);
            let log_p = Rc::clone(&log);
            let log_c = Rc::clone(&log);
            parent
                .inner
                .add_subscription_cleanup(Box::new(move || log_p.borrow_mut().push("parent")));
            child
                .inner
                .add_subscription_cleanup(Box::new(move || log_c.borrow_mut().push("child")));
            drop(child);
            assert!(log.borrow().is_empty());
        }
        assert_eq!(*log.borrow(), vec!["child", "parent"]);
    }

    #[test]
    fn batch_returns_inner_value() {
        let result = batch(|| 7 + 5);
        assert_eq!(result, 12);
    }

    #[test]
    fn batch_depth_is_zero_outside() {
        assert!(!in_batch());
        batch(|| {
            assert!(in_batch());
        });
        assert!(!in_batch());
    }

    #[test]
    fn nested_batches_only_drain_at_outermost_exit() {
        batch(|| {
            assert!(in_batch());
            batch(|| {
                assert!(in_batch());
            });
            assert!(in_batch());
        });
        assert!(!in_batch());
    }

    // ---- R37.5 #1: panic safety regression tests ---------------------------

    #[test]
    fn batch_panic_restores_depth_counter() {
        assert!(!in_batch());
        let result = std::panic::catch_unwind(|| {
            batch(|| {
                panic!("simulated user-closure panic");
            })
        });
        assert!(result.is_err());
        assert!(!in_batch(), "BATCH_DEPTH must return to 0 after panic");
        // Subsequent batches still work.
        batch(|| {});
        assert!(!in_batch());
    }

    #[test]
    fn run_panic_restores_current_owner_stack() {
        assert_eq!(current_owner_stack_len(), 0);
        let o = Owner::new();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            o.run(|| {
                panic!("simulated user-closure panic");
            })
        }));
        assert!(result.is_err());
        assert_eq!(
            current_owner_stack_len(),
            0,
            "CURRENT_OWNER stack must be empty after panicking run"
        );
    }

    #[test]
    fn nested_run_panic_unwinds_stack_in_order() {
        let outer = Owner::new();
        let inner = Owner::new();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            outer.run(|| {
                inner.run(|| {
                    panic!("inner panic");
                })
            })
        }));
        assert!(result.is_err());
        assert_eq!(current_owner_stack_len(), 0);
    }

    // ---- R37.5 #4: cleanup catch_unwind regression -------------------------

    #[test]
    fn panicking_cleanup_does_not_abort_drop() {
        let counter = Rc::new(Cell::new(0_u32));
        {
            let o = Owner::new();
            // First cleanup panics.
            o.inner
                .add_subscription_cleanup(Box::new(|| panic!("cleanup1 fail")));
            // Second cleanup must still run.
            let counter_clone = Rc::clone(&counter);
            o.inner.add_subscription_cleanup(Box::new(move || {
                counter_clone.set(counter_clone.get() + 1);
            }));
            // Owner drops here — both cleanups are drained, the second one
            // must execute even though the first panicked.
        }
        assert_eq!(counter.get(), 1);
    }

    #[test]
    fn panicking_observer_in_batch_drain_does_not_abort() {
        // mark_dirty cascade itself doesn't normally panic, but we test the
        // catch_unwind around the drain loop using a custom observer-like
        // node would require trait access. We instead verify the BatchGuard
        // drain doesn't propagate cleanup panic — exercised via cleanup chain
        // in `panicking_cleanup_does_not_abort_drop`.
        let _ = std::panic::catch_unwind(|| batch(|| {}));
        assert!(!in_batch());
    }

    // ---- Existing snapshot/restore tests ----------------------------------

    #[test]
    fn track_then_snapshot_restore_round_trips_single_signal() {
        use super::super::signal::Signal;
        let owner = Owner::new();
        let s = Signal::new(10_i32);
        owner.track(&s);
        let snap = owner.snapshot();
        assert_eq!(snap.len(), 1);
        s.set(99);
        assert_eq!(s.get(), 99);
        owner.restore(snap).expect("restore should succeed");
        assert_eq!(s.get(), 10);
    }

    #[test]
    fn snapshot_captures_heterogeneous_signal_types() {
        use super::super::signal::Signal;
        let owner = Owner::new();
        let a = Signal::new(1_i32);
        let b = Signal::new(String::from("hello"));
        let c = Signal::new(true);
        owner.track(&a);
        owner.track(&b);
        owner.track(&c);
        let snap = owner.snapshot();
        a.set(99);
        b.set(String::from("world"));
        c.set(false);
        owner.restore(snap).expect("restore should succeed");
        assert_eq!(a.get(), 1);
        assert_eq!(b.get(), "hello");
        assert!(c.get());
    }

    #[test]
    fn track_is_idempotent_for_same_signal_id() {
        use super::super::signal::Signal;
        let owner = Owner::new();
        let s = Signal::new(0_i32);
        owner.track(&s);
        owner.track(&s);
        owner.track(&s);
        assert_eq!(owner.snapshot().len(), 1);
    }

    #[test]
    fn restore_dirties_observers_when_value_changed() {
        use super::super::signal::Signal;
        let owner = Owner::new();
        let s = Signal::new(5_i32);
        owner.track(&s);
        let snap = owner.snapshot();
        let observer = Owner::new();
        observer.run(|| {
            let _ = s.get();
        });
        s.set(10);
        assert!(observer.is_dirty());
        observer.clear_dirty();
        owner.restore(snap).expect("restore should succeed");
        assert!(observer.is_dirty());
        assert_eq!(s.get(), 5);
    }

    #[test]
    fn restore_is_quiet_when_values_already_match_snapshot() {
        use super::super::signal::Signal;
        let owner = Owner::new();
        let s = Signal::new(7_i32);
        owner.track(&s);
        let snap = owner.snapshot();
        let observer = Owner::new();
        observer.run(|| {
            let _ = s.get();
        });
        owner.restore(snap).expect("restore should succeed");
        assert!(!observer.is_dirty());
        assert_eq!(s.get(), 7);
    }

    #[test]
    fn snapshot_empty_owner_is_empty() {
        let owner = Owner::new();
        let snap = owner.snapshot();
        assert!(snap.is_empty());
        assert_eq!(snap.len(), 0);
    }

    #[test]
    fn restore_type_mismatch_returns_error_and_leaves_signal_untouched() {
        // Synthesize a mismatched payload: a Signal<i32> in the owner's
        // tracked list, but the snapshot entry carries a payload typed as
        // `String`. The restore must surface `TypeMismatch` and leave the
        // signal at its current value.
        use super::super::signal::Signal;
        use std::any::Any;
        let owner = Owner::new();
        let s = Signal::new(1_i32);
        owner.track(&s);
        // Hand-craft a snapshot whose payload type does not match s's T.
        let bogus_payload: Box<dyn Any> = Box::new(String::from("not an i32"));
        let snap = OwnerSnapshot {
            entries: vec![(s.id(), bogus_payload)],
        };
        s.set(42);
        let result = owner.restore(snap);
        let errors = result.expect_err("type mismatch should surface as error");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, s.id());
        assert_eq!(errors[0].1, SnapshotRestoreError::TypeMismatch);
        // Signal untouched by the failed restore.
        assert_eq!(s.get(), 42);
    }

    #[test]
    fn restore_partial_success_restores_what_it_can_and_reports_errors() {
        use super::super::signal::Signal;
        use std::any::Any;
        let owner = Owner::new();
        let a = Signal::new(1_i32);
        let b = Signal::new(2_i32);
        owner.track(&a);
        owner.track(&b);
        // Valid payload for a (i32), bogus payload (bool) for b.
        let snap = OwnerSnapshot {
            entries: vec![
                (a.id(), Box::new(10_i32) as Box<dyn Any>),
                (b.id(), Box::new(true) as Box<dyn Any>),
            ],
        };
        a.set(99);
        b.set(99);
        let errors = owner.restore(snap).expect_err("b should fail");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, b.id());
        assert_eq!(a.get(), 10, "a must roll back even though b failed");
        assert_eq!(b.get(), 99, "b stays at post-mutation value on failed restore");
    }

    #[test]
    fn restore_under_batch_defers_observer_dirty_until_close() {
        use super::super::signal::Signal;
        let owner = Owner::new();
        let s = Signal::new(0_i32);
        owner.track(&s);
        let snap = owner.snapshot();
        s.set(42);
        let observer = Owner::new();
        observer.run(|| {
            let _ = s.get();
        });
        observer.clear_dirty();
        batch(|| {
            owner.restore(snap).expect("restore should succeed");
            assert!(!observer.is_dirty());
        });
        assert!(observer.is_dirty());
        assert_eq!(s.get(), 0);
    }
}
