//! `Signal<T>` — single-cell reactive primitive (§5.22).
//!
//! R26 caveats locked here:
//! - `T: Clone + PartialEq` for value-change skip propagation
//! - R36 §5.31 extends bound with `Serialize + DeserializeOwned` for hot reload
//! - eager init via `new(initial)`; no lazy constructor variant
//! - sync read via `get()` cloning the stored value
//! - `set_with(|prev| -> T)` functional update; equality check at the `set` boundary
//! - thread-local v0; `SyncSignal` cross-thread variant carry-forward (§5.29 R34)
//!
//! `get()` auto-subscribes whatever `ReactiveNode` is on top of the
//! `CURRENT_OWNER` stack — typically an `Owner` (UI scope) or a `Computed`
//! (during its recompute). `set()` marks every subscribed node dirty; dirty
//! cascade through `Computed` chains happens inside their `mark_dirty` impl.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::owner::{
    ObserverEntry, ReactiveNode, SnapshotRestoreError, SnapshotableSignal, SubscriberSet,
    dispatch_dirty, next_node_id, with_current_owner,
};

/// Reactive value cell. Cloning yields a new handle to the same underlying
/// storage (`Rc` semantics) — both handles observe and produce the same writes.
pub struct Signal<T> {
    inner: Rc<SignalInner<T>>,
}

struct SignalInner<T> {
    id: u64,
    value: RefCell<T>,
    revision: Cell<u64>,
    observers: RefCell<SubscriberSet>,
}

impl<T> Signal<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    /// Construct a signal eagerly initialized to `initial`. Revision starts at 0.
    #[must_use]
    pub fn new(initial: T) -> Self {
        Self {
            inner: Rc::new(SignalInner {
                id: next_node_id(),
                value: RefCell::new(initial),
                revision: Cell::new(0),
                observers: RefCell::new(SubscriberSet::new()),
            }),
        }
    }

    /// Stable identity. Used by the snapshot registry (`Owner::snapshot`) to
    /// key restore-time downcasts back to the right signal.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// Read the current value. If called within a subscriber scope (`Owner::run`
    /// or `Computed::recompute`), that node auto-subscribes and is marked
    /// dirty on the next value-changing `set` / `set_with`.
    #[must_use]
    pub fn get(&self) -> T {
        with_current_owner(|node_opt| {
            if let Some(node) = node_opt {
                self.subscribe(node);
            }
        });
        self.inner.value.borrow().clone()
    }

    /// Write `value`. Equality-skips: if `*current == value`, neither the
    /// revision counter advances nor are subscribers notified.
    pub fn set(&self, value: T) {
        {
            let mut current = self.inner.value.borrow_mut();
            if *current == value {
                return;
            }
            *current = value;
        }
        self.inner.revision.set(self.inner.revision.get() + 1);
        self.notify_observers();
    }

    /// Functional update — `f` receives the previous value by reference and
    /// returns the new value. Equality-skips at the `set` boundary.
    pub fn set_with<F>(&self, f: F)
    where
        F: FnOnce(&T) -> T,
    {
        let new_value = {
            let current = self.inner.value.borrow();
            f(&current)
        };
        self.set(new_value);
    }

    /// Monotonic value-change counter. Increments only when `set` actually
    /// mutated the cell (post equality check).
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.inner.revision.get()
    }

    fn subscribe(&self, node: &Rc<dyn ReactiveNode>) {
        let node_id = node.node_id();
        // Fast dedup: O(1) membership check on the `SubscriberSet` index.
        if self.inner.observers.borrow().contains(node_id) {
            return;
        }
        self.inner.observers.borrow_mut().insert(ObserverEntry {
            id: node_id,
            node: Rc::downgrade(node),
        });
        // Cleanup runs when the subscriber's subscription set tears down
        // (owner drop, or computed re-evaluation): remove this node from the
        // observer list so a stale id is never notified. We capture a `Weak`
        // — not a strong `Rc` — so a long-lived subscriber observing a
        // short-lived signal does not pin the signal's storage forever
        // (R37.5 #2 leak fix).
        let signal_weak: Weak<SignalInner<T>> = Rc::downgrade(&self.inner);
        node.add_subscription_cleanup(Box::new(move || {
            if let Some(inner) = signal_weak.upgrade() {
                inner.observers.borrow_mut().remove(node_id);
            }
        }));
    }

    fn notify_observers(&self) {
        // Snapshot to release the immutable borrow before invoking subscriber
        // callbacks — those may recursively touch this signal's lists.
        let snapshot = self.inner.observers.borrow().snapshot();
        dispatch_dirty(&snapshot);
        // Prune dead weaks opportunistically (cheap walk; cleanup closures
        // already remove most entries, this catches subscribers that leaked).
        self.inner.observers.borrow_mut().prune_dead();
    }

    #[cfg(test)]
    pub(crate) fn observer_count(&self) -> usize {
        self.inner.observers.borrow().len()
    }
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> SnapshotableSignal for Signal<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    fn snapshot_id(&self) -> u64 {
        self.inner.id
    }

    fn save_snapshot(&self) -> Box<dyn Any> {
        Box::new(self.inner.value.borrow().clone())
    }

    fn restore_snapshot(&self, snap: Box<dyn Any>) -> Result<(), SnapshotRestoreError> {
        // Downcast may fail when the snapshot was minted against a different
        // signal graph that happened to share this id (cross-process / hot
        // reload). Surface as `TypeMismatch` rather than panic so the caller
        // can decide whether to log, retry, or abort the restore pass.
        let value: T = *snap
            .downcast::<T>()
            .map_err(|_| SnapshotRestoreError::TypeMismatch)?;
        self.set(value);
        Ok(())
    }
}

impl<T> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signal")
            .field("id", &self.inner.id)
            .field("revision", &self.inner.revision.get())
            .field("observers", &self.inner.observers.borrow().len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::super::owner::Owner;
    use super::Signal;

    #[test]
    fn new_then_get_returns_initial() {
        let s = Signal::new(42_i32);
        assert_eq!(s.get(), 42);
        assert_eq!(s.revision(), 0);
    }

    #[test]
    fn set_updates_value_and_bumps_revision() {
        let s = Signal::new(1_i32);
        s.set(2);
        assert_eq!(s.get(), 2);
        assert_eq!(s.revision(), 1);
        s.set(3);
        assert_eq!(s.get(), 3);
        assert_eq!(s.revision(), 2);
    }

    #[test]
    fn set_equal_value_skips_revision_bump() {
        let s = Signal::new(7_i32);
        s.set(7);
        assert_eq!(s.revision(), 0);
        s.set(8);
        assert_eq!(s.revision(), 1);
        s.set(8);
        assert_eq!(s.revision(), 1);
    }

    #[test]
    fn set_with_receives_previous_value() {
        let s = Signal::new(10_i32);
        s.set_with(|prev| prev + 5);
        assert_eq!(s.get(), 15);
        assert_eq!(s.revision(), 1);
    }

    #[test]
    fn set_with_equal_result_skips_revision_bump() {
        let s = Signal::new(4_i32);
        s.set_with(|prev| *prev);
        assert_eq!(s.revision(), 0);
    }

    #[test]
    fn clone_is_shared_handle() {
        let a = Signal::new(0_i32);
        let b = a.clone();
        b.set(99);
        assert_eq!(a.get(), 99);
        assert_eq!(a.revision(), 1);
        assert_eq!(b.revision(), 1);
    }

    #[test]
    fn works_with_owned_string_payload() {
        let s = Signal::new(String::from("hello"));
        s.set(String::from("world"));
        assert_eq!(s.get(), "world");
        s.set_with(|prev| format!("{prev}!"));
        assert_eq!(s.get(), "world!");
        assert_eq!(s.revision(), 2);
    }

    #[test]
    fn read_outside_scope_does_not_subscribe_anyone() {
        let s = Signal::new(0_i32);
        let _ = s.get();
        assert_eq!(s.observer_count(), 0);
    }

    #[test]
    fn read_inside_scope_subscribes_owner() {
        let s = Signal::new(0_i32);
        let owner = Owner::new();
        owner.run(|| {
            let _ = s.get();
        });
        assert_eq!(s.observer_count(), 1);
    }

    #[test]
    fn repeated_reads_in_one_scope_dedup() {
        let s = Signal::new(0_i32);
        let owner = Owner::new();
        owner.run(|| {
            for _ in 0..50 {
                let _ = s.get();
            }
        });
        assert_eq!(s.observer_count(), 1);
    }

    #[test]
    fn distinct_owners_record_distinct_observers() {
        let s = Signal::new(0_i32);
        let a = Owner::new();
        let b = Owner::new();
        a.run(|| {
            let _ = s.get();
        });
        b.run(|| {
            let _ = s.get();
        });
        assert_eq!(s.observer_count(), 2);
    }

    #[test]
    fn set_marks_subscribed_owner_dirty() {
        let s = Signal::new(0_i32);
        let owner = Owner::new();
        owner.run(|| {
            let _ = s.get();
        });
        assert!(!owner.is_dirty());
        s.set(1);
        assert!(owner.is_dirty());
    }

    #[test]
    fn set_skipped_by_equality_does_not_dirty_owner() {
        let s = Signal::new(5_i32);
        let owner = Owner::new();
        owner.run(|| {
            let _ = s.get();
        });
        s.set(5);
        assert!(!owner.is_dirty());
    }

    #[test]
    fn nested_scope_subscribes_inner_only() {
        let s = Signal::new(0_i32);
        let outer = Owner::new();
        let inner = Owner::new();
        outer.run(|| {
            inner.run(|| {
                let _ = s.get();
            });
        });
        s.set(1);
        assert!(!outer.is_dirty());
        assert!(inner.is_dirty());
    }

    #[test]
    fn dropping_owner_unsubscribes_from_signal() {
        let s = Signal::new(0_i32);
        {
            let owner = Owner::new();
            owner.run(|| {
                let _ = s.get();
            });
            assert_eq!(s.observer_count(), 1);
        }
        assert_eq!(s.observer_count(), 0);
        s.set(1);
    }

    #[test]
    fn parent_drop_cascade_unsubscribes_child_from_signal() {
        let s = Signal::new(0_i32);
        let parent = Owner::new();
        {
            let child = Owner::new_child(&parent);
            child.run(|| {
                let _ = s.get();
            });
        }
        assert_eq!(s.observer_count(), 1);
        drop(parent);
        assert_eq!(s.observer_count(), 0);
    }

    #[test]
    fn clear_dirty_resets_owner_state() {
        let s = Signal::new(0_i32);
        let owner = Owner::new();
        owner.run(|| {
            let _ = s.get();
        });
        s.set(1);
        assert!(owner.is_dirty());
        owner.clear_dirty();
        assert!(!owner.is_dirty());
        s.set(2);
        assert!(owner.is_dirty());
    }

    #[test]
    fn batch_defers_owner_dirty_until_exit() {
        use super::super::owner::batch;
        let s = Signal::new(0_i32);
        let owner = Owner::new();
        owner.run(|| {
            let _ = s.get();
        });
        batch(|| {
            s.set(1);
            s.set(2);
            s.set(3);
            // Cascade deferred — owner still pristine inside batch.
            assert!(!owner.is_dirty());
            // Value updates apply eagerly though.
            assert_eq!(s.get(), 3);
            assert_eq!(s.revision(), 3);
        });
        // Outermost batch exit drains pending; owner now dirty exactly once.
        assert!(owner.is_dirty());
    }

    #[test]
    fn batch_coalesces_multi_signal_writes_to_shared_owner() {
        use super::super::owner::batch;
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let owner = Owner::new();
        owner.run(|| {
            let _ = a.get();
            let _ = b.get();
        });
        batch(|| {
            a.set(10);
            b.set(20);
            assert!(!owner.is_dirty());
        });
        assert!(owner.is_dirty());
    }

    #[test]
    fn nested_batch_drains_only_at_outer_exit() {
        use super::super::owner::batch;
        let s = Signal::new(0_i32);
        let owner = Owner::new();
        owner.run(|| {
            let _ = s.get();
        });
        batch(|| {
            s.set(1);
            batch(|| {
                s.set(2);
                assert!(!owner.is_dirty());
            });
            // Inner batch exit did not drain — we're still inside outer.
            assert!(!owner.is_dirty());
        });
        assert!(owner.is_dirty());
    }

    // ---- R37.5 #2: Weak cleanup capture (no strong-Rc leak) ---------------

    #[test]
    fn long_lived_owner_does_not_pin_short_lived_signals() {
        use std::rc::Rc;
        // Hold a Weak to the signal's inner. After the user-side `Signal`
        // handle is dropped, the Weak must fail to upgrade — confirming the
        // owner does *not* anchor the source via its cleanup closure.
        let owner = Owner::new();
        let weak = {
            let s = Signal::new(0_i32);
            let inner_weak = Rc::downgrade(&s.inner);
            owner.run(|| {
                let _ = s.get();
            });
            // `s` drops here.
            inner_weak
        };
        assert!(
            weak.upgrade().is_none(),
            "Signal inner must be freed once the user handle drops, even with owner alive"
        );
        // Owner stays alive — its cleanup closure now holds a stale Weak and
        // safely no-ops on drop.
        drop(owner);
    }

    #[test]
    fn dropping_signal_then_owner_does_not_panic() {
        let owner = Owner::new();
        {
            let s = Signal::new(0_i32);
            owner.run(|| {
                let _ = s.get();
            });
            // Drop the signal first.
        }
        // Owner drop must not panic when cleanup tries to upgrade a dead Weak.
        drop(owner);
    }
}
