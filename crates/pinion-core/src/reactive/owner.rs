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

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

thread_local! {
    static NEXT_NODE_ID: Cell<u64> = const { Cell::new(0) };
    static CURRENT_OWNER: RefCell<Vec<Weak<dyn ReactiveNode>>> = const { RefCell::new(Vec::new()) };
    static BATCH_DEPTH: Cell<u32> = const { Cell::new(0) };
    static PENDING_DIRTY: RefCell<Vec<ObserverEntry>> = const { RefCell::new(Vec::new()) };
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
    fn restore_snapshot(&self, snap: Box<dyn Any>);
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
        // Drain cleanups and invoke. Each closure removes this owner from a
        // source's observer list, severing the back-reference.
        let drained: Vec<_> = std::mem::take(self.cleanups.get_mut());
        for cleanup in drained {
            cleanup();
        }
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
    pub fn restore(&self, snapshot: OwnerSnapshot) {
        let mut by_id: std::collections::HashMap<u64, Box<dyn Any>> =
            snapshot.entries.into_iter().collect();
        for sig in self.inner.owned_signals.borrow().iter() {
            if let Some(payload) = by_id.remove(&sig.snapshot_id()) {
                sig.restore_snapshot(payload);
            }
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
    /// during `f` auto-subscribe to this owner.
    pub fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        let strong: Rc<OwnerInner> = Rc::clone(&self.inner);
        let as_node: Rc<dyn ReactiveNode> = strong;
        let weak_dyn: Weak<dyn ReactiveNode> = Rc::downgrade(&as_node);
        CURRENT_OWNER.with(|stack| {
            stack.borrow_mut().push(weak_dyn);
        });
        let result = f();
        CURRENT_OWNER.with(|stack| {
            stack.borrow_mut().pop();
        });
        result
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
/// `Computed::recompute` to track which sources its closure reads.
pub(crate) fn run_with_node<R>(node: &Rc<dyn ReactiveNode>, f: impl FnOnce() -> R) -> R {
    let weak: Weak<dyn ReactiveNode> = Rc::downgrade(node);
    CURRENT_OWNER.with(|stack| stack.borrow_mut().push(weak));
    let result = f();
    CURRENT_OWNER.with(|stack| {
        stack.borrow_mut().pop();
    });
    result
}

/// Notify a set of observers that an upstream value changed.
///
/// - Outside a `batch`: immediately calls `mark_dirty` on each live observer.
/// - Inside a `batch`: collects entries (id-deduped) into `PENDING_DIRTY`;
///   the cascade fires once when the outermost `batch` exits.
///
/// This is the single dispatch point used by both `Signal::set` and
/// `ComputedInner::mark_dirty` — keeps the deferral logic in one place.
pub(crate) fn dispatch_dirty(entries: &[ObserverEntry]) {
    let in_batch = BATCH_DEPTH.with(Cell::get) > 0;
    if in_batch {
        PENDING_DIRTY.with(|pending| {
            let mut pending = pending.borrow_mut();
            for entry in entries {
                if !pending.iter().any(|existing| existing.id == entry.id) {
                    pending.push(entry.clone());
                }
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

/// Group reactive writes so that downstream notifications fire once when the
/// outermost `batch` exits (§5.22 R26: "writes inside coalesce; propagation
/// defers until exit").
///
/// Nested batches inherit the outer scope — only the outermost close drains
/// the pending set. The return value of `f` is forwarded so `batch` is usable
/// as an expression.
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
    BATCH_DEPTH.with(|depth| depth.set(depth.get() + 1));
    let result = f();
    let new_depth = BATCH_DEPTH.with(|depth| {
        let next = depth.get() - 1;
        depth.set(next);
        next
    });
    if new_depth == 0 {
        let drained: Vec<ObserverEntry> =
            PENDING_DIRTY.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
        for entry in &drained {
            if let Some(node) = entry.node.upgrade() {
                node.mark_dirty();
            }
        }
    }
    result
}

#[cfg(test)]
pub(crate) fn in_batch() -> bool {
    BATCH_DEPTH.with(Cell::get) > 0
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
        // Inner batch increments and decrements; depth returns to 1 (still
        // batching). PENDING_DIRTY should remain held until outer exits.
        batch(|| {
            assert!(in_batch());
            batch(|| {
                assert!(in_batch());
            });
            assert!(in_batch());
        });
        assert!(!in_batch());
    }

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
        owner.restore(snap);
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
        owner.restore(snap);
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
        owner.restore(snap);
        // Restore wrote 5 back, which differs from 10 — observer must wake.
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
        // No intervening mutation — restore should be a no-op via equality skip.
        owner.restore(snap);
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
            owner.restore(snap);
            assert!(!observer.is_dirty());
        });
        assert!(observer.is_dirty());
        assert_eq!(s.get(), 0);
    }
}
