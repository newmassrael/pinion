//! `Computed<T>` — derived reactive value with lazy recompute (§5.22).
//!
//! R26 caveats locked here:
//! - lazy + cached: closure runs only on `get()` after a source has changed
//! - pure-fn contract: the closure must read sources via `Signal::get` /
//!   `Computed::get` so dependency tracking is captured automatically
//! - propagate only on value change: `mark_dirty` is idempotent (already-dirty
//!   short-circuits the cascade), and a recompute that produces an equal value
//!   does not re-cascade to downstream observers
//! - `T: Clone + PartialEq + 'static` (no `Serialize` requirement — derived
//!   values are not part of the hot-reload snapshot; only `Signal` payloads
//!   are serialized per §5.31, and the Computed re-derives on reload)
//!
//! Dynamic dependency tracking: each `recompute` first drains any stale
//! source-subscription cleanups (severing observer links into previous-pass
//! sources), then pushes itself onto `CURRENT_OWNER` and runs the closure;
//! sources read during the closure re-subscribe this `Computed`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use super::owner::{
    ObserverEntry, ReactiveNode, SubscriberSet, dispatch_dirty, next_node_id,
    run_cleanups_isolated, run_with_node, with_current_owner,
};

/// Derived reactive value. Cloning yields a handle to the same memoized cell.
pub struct Computed<T> {
    inner: Rc<ComputedInner<T>>,
}

struct ComputedInner<T> {
    id: u64,
    dirty: Cell<bool>,
    /// Reentrancy flag: set while the user's compute closure is running.
    /// Catches reactive cycles (a `Computed` reading itself, directly or
    /// transitively via another `Computed::get()` chain) and surfaces them
    /// as a panic instead of stack-overflowing the program (R37.5 #3).
    in_compute: Cell<bool>,
    cached: RefCell<Option<T>>,
    compute: Box<dyn Fn() -> T>,
    source_cleanups: RefCell<Vec<Box<dyn FnOnce()>>>,
    observers: RefCell<SubscriberSet>,
}

/// RAII guard hoisted out of `recompute` so clippy's
/// `items_after_statements` does not fire on the inline definition.
struct InComputeGuard<'a>(&'a Cell<bool>);

impl Drop for InComputeGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl<T> ReactiveNode for ComputedInner<T>
where
    T: Clone + PartialEq + 'static,
{
    fn node_id(&self) -> u64 {
        self.id
    }

    fn mark_dirty(&self) {
        if self.dirty.get() {
            // Already in the dirty set — short-circuit prevents diamond
            // dependencies from cascading the same node twice. This is the
            // idempotence step of the textbook push-pull algorithm.
            return;
        }
        self.dirty.set(true);
        // Cascade dirty to downstream observers via the shared dispatch path
        // so batch deferral coalesces transitive cascades too.
        let snapshot = self.observers.borrow().snapshot();
        dispatch_dirty(&snapshot);
    }

    fn add_subscription_cleanup(&self, cleanup: Box<dyn FnOnce()>) {
        // Computed subscriptions are per-evaluation: the next `recompute`
        // drains and runs them before re-subscribing fresh sources.
        self.source_cleanups.borrow_mut().push(cleanup);
    }
}

impl<T> Computed<T>
where
    T: Clone + PartialEq + 'static,
{
    /// Construct a derived value. `compute` is the pure function that reads
    /// sources via their `get()` accessors. The first `get()` triggers the
    /// initial evaluation.
    #[must_use]
    pub fn new<F>(compute: F) -> Self
    where
        F: Fn() -> T + 'static,
    {
        Self {
            inner: Rc::new(ComputedInner {
                id: next_node_id(),
                dirty: Cell::new(true),
                in_compute: Cell::new(false),
                cached: RefCell::new(None),
                compute: Box::new(compute),
                source_cleanups: RefCell::new(Vec::new()),
                observers: RefCell::new(SubscriberSet::new()),
            }),
        }
    }

    /// Read the memoized value, recomputing only if a source has changed
    /// since the last read. Auto-subscribes the active `Owner` / `Computed`
    /// scope.
    ///
    /// # Panics
    /// Never in correct usage — `recompute` always populates the cache before
    /// the read path observes it. The `expect` is a structural assertion that
    /// the invariant holds.
    #[must_use]
    pub fn get(&self) -> T {
        // Order matters: recompute first, then subscribe the caller. Otherwise
        // a value-change cascade fired inside `recompute` would falsely mark
        // the caller dirty even though the caller is about to receive the
        // freshly computed value as the return of this `get()`.
        if self.inner.dirty.get() || self.inner.cached.borrow().is_none() {
            self.recompute();
        }

        with_current_owner(|node_opt| {
            if let Some(node) = node_opt {
                self.subscribe_observer(node);
            }
        });

        self.inner
            .cached
            .borrow()
            .as_ref()
            .expect("cached value must be present after recompute")
            .clone()
    }

    /// Stable identity for tests and dedup.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    fn recompute(&self) {
        // Cycle detection (R37.5 #3): if a Computed reads itself transitively,
        // `recompute` re-enters before the outer call finished. Catch it
        // explicitly — otherwise the program stack-overflows.
        assert!(
            !self.inner.in_compute.get(),
            "reactive cycle detected: Computed (id={}) read itself during compute",
            self.inner.id
        );
        // Drain stale source subscriptions before re-running so dependency
        // tracking reflects only the reads that happen in *this* pass. This
        // is what makes branching `compute` bodies correct. Each cleanup is
        // isolated so a panicking one cannot abort the recompute pass
        // mid-state (R37.5 #4).
        let drained: Vec<Box<dyn FnOnce()>> =
            std::mem::take(&mut *self.inner.source_cleanups.borrow_mut());
        run_cleanups_isolated(drained);

        // RAII guard: clear `in_compute` even if the user closure panics,
        // so the Computed is not stuck "in compute" forever.
        self.inner.in_compute.set(true);
        let _guard = InComputeGuard(&self.inner.in_compute);

        let strong: Rc<ComputedInner<T>> = Rc::clone(&self.inner);
        let self_as_node: Rc<dyn ReactiveNode> = strong;
        let new_value = run_with_node(&self_as_node, || (self.inner.compute)());

        let value_changed = match self.inner.cached.borrow().as_ref() {
            Some(old) => old != &new_value,
            None => true,
        };
        *self.inner.cached.borrow_mut() = Some(new_value);
        self.inner.dirty.set(false);

        if value_changed {
            // Observers were already marked dirty by upstream cascade; this
            // path is a no-op in the common case but keeps the invariant
            // explicit: when *our* value changed, downstream is stale.
            let snapshot = self.inner.observers.borrow().snapshot();
            dispatch_dirty(&snapshot);
        }
    }

    fn subscribe_observer(&self, node: &Rc<dyn ReactiveNode>) {
        let node_id = node.node_id();
        if self.inner.observers.borrow().contains(node_id) {
            return;
        }
        self.inner.observers.borrow_mut().insert(ObserverEntry {
            id: node_id,
            node: Rc::downgrade(node),
        });
        // `Weak` capture: a subscriber dropping after us must not keep this
        // Computed alive (R37.5 #2 leak fix). On upgrade-failure the cleanup
        // is a no-op — Computed already gone, no observer list to prune.
        let inner_weak: Weak<ComputedInner<T>> = Rc::downgrade(&self.inner);
        node.add_subscription_cleanup(Box::new(move || {
            if let Some(inner) = inner_weak.upgrade() {
                inner.observers.borrow_mut().remove(node_id);
            }
        }));
    }

    #[cfg(test)]
    pub(crate) fn is_dirty(&self) -> bool {
        self.inner.dirty.get()
    }
}

impl<T> Clone for Computed<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::owner::Owner;
    use super::super::signal::Signal;
    use super::Computed;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn lazy_first_get_runs_compute_once() {
        let counter = Rc::new(Cell::new(0_u32));
        let c = {
            let counter = Rc::clone(&counter);
            Computed::new(move || {
                counter.set(counter.get() + 1);
                7_i32
            })
        };
        assert_eq!(counter.get(), 0);
        assert_eq!(c.get(), 7);
        assert_eq!(counter.get(), 1);
        // Repeated reads hit the cache.
        assert_eq!(c.get(), 7);
        assert_eq!(c.get(), 7);
        assert_eq!(counter.get(), 1);
    }

    #[test]
    fn signal_write_marks_computed_dirty_and_recompute_picks_up() {
        let a = Signal::new(2_i32);
        let a2 = a.clone();
        let c = Computed::new(move || a2.get() * 10);
        assert_eq!(c.get(), 20);
        a.set(3);
        assert!(c.is_dirty());
        assert_eq!(c.get(), 30);
        assert!(!c.is_dirty());
    }

    #[test]
    fn equality_skip_at_signal_does_not_dirty_computed() {
        let a = Signal::new(5_i32);
        let a2 = a.clone();
        let c = Computed::new(move || a2.get() + 1);
        assert_eq!(c.get(), 6);
        a.set(5); // no value change
        assert!(!c.is_dirty());
    }

    #[test]
    fn computed_recompute_after_value_change_clears_dirty() {
        let a = Signal::new(0_i32);
        let a2 = a.clone();
        let c = Computed::new(move || a2.get() + 1);
        let _ = c.get();
        a.set(1);
        assert!(c.is_dirty());
        let _ = c.get();
        assert!(!c.is_dirty());
    }

    #[test]
    fn computed_subscribes_owner_that_reads_it() {
        let owner = Owner::new();
        let a = Signal::new(1_i32);
        let a2 = a.clone();
        let c = Computed::new(move || a2.get() * 2);
        owner.run(|| {
            assert_eq!(c.get(), 2);
        });
        assert!(!owner.is_dirty());
        a.set(2);
        assert!(owner.is_dirty());
    }

    #[test]
    fn chained_computed_propagates_dirty_through_levels() {
        let a = Signal::new(1_i32);
        let a2 = a.clone();
        let b = Computed::new(move || a2.get() + 1);
        let b_for_c = b.clone();
        let c = Computed::new(move || b_for_c.get() * 10);
        assert_eq!(c.get(), 20);
        assert_eq!(b.get(), 2);
        a.set(2);
        assert!(b.is_dirty());
        assert!(c.is_dirty());
        assert_eq!(c.get(), 30);
        assert!(!c.is_dirty());
        assert!(!b.is_dirty());
    }

    #[test]
    fn diamond_dependency_short_circuits_via_dirty_flag() {
        let counter = Rc::new(Cell::new(0_u32));
        let a = Signal::new(1_i32);
        let a_for_b = a.clone();
        let b = Computed::new(move || a_for_b.get() + 10);
        let a_for_c = a.clone();
        let c = Computed::new(move || a_for_c.get() + 100);
        let b_for_d = b.clone();
        let c_for_d = c.clone();
        let counter_for_d = Rc::clone(&counter);
        let d = Computed::new(move || {
            counter_for_d.set(counter_for_d.get() + 1);
            b_for_d.get() + c_for_d.get()
        });
        assert_eq!(d.get(), 112);
        assert_eq!(counter.get(), 1);
        a.set(2);
        // d should be dirty exactly once even though both b and c marked it.
        assert!(d.is_dirty());
        assert_eq!(d.get(), 114);
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn dynamic_dependency_tracking_drops_unread_sources() {
        let cond = Signal::new(true);
        let a = Signal::new(1_i32);
        let b = Signal::new(100_i32);
        let cond_c = cond.clone();
        let a_c = a.clone();
        let b_c = b.clone();
        let c = Computed::new(move || if cond_c.get() { a_c.get() } else { b_c.get() });
        // First read: cond=true, reads a only.
        assert_eq!(c.get(), 1);
        assert_eq!(a.observer_count(), 1);
        assert_eq!(b.observer_count(), 0);
        // Flip cond; recompute. New pass reads b instead of a — a's
        // subscription must be cleared, b's must be added.
        cond.set(false);
        assert_eq!(c.get(), 100);
        assert_eq!(a.observer_count(), 0);
        assert_eq!(b.observer_count(), 1);
        // Writing to a (no longer a dep) should not dirty c.
        a.set(999);
        assert!(!c.is_dirty());
    }

    #[test]
    fn computed_clone_shares_cache_and_dirty_state() {
        let a = Signal::new(1_i32);
        let a2 = a.clone();
        let c = Computed::new(move || a2.get() * 3);
        let alias = c.clone();
        assert_eq!(c.get(), 3);
        a.set(2);
        // alias sees the same dirty state and recomputes on read.
        assert!(alias.is_dirty());
        assert_eq!(alias.get(), 6);
        assert!(!c.is_dirty());
    }

    #[test]
    fn batch_defers_dirty_cascade_through_computed() {
        use super::super::owner::batch;
        let a = Signal::new(1_i32);
        let a2 = a.clone();
        let b = Computed::new(move || a2.get() * 2);
        let b_for_c = b.clone();
        let c = Computed::new(move || b_for_c.get() + 1);
        // Prime the cache and subscriptions.
        assert_eq!(c.get(), 3);
        assert!(!b.is_dirty());
        assert!(!c.is_dirty());
        batch(|| {
            a.set(2);
            a.set(3);
            // Deferred — neither Computed has its dirty bit set yet.
            assert!(!b.is_dirty());
            assert!(!c.is_dirty());
        });
        assert!(b.is_dirty());
        assert!(c.is_dirty());
        assert_eq!(c.get(), 7);
    }

    #[test]
    fn batch_recompute_runs_at_most_once_per_diamond_observer() {
        use super::super::owner::batch;
        let counter = Rc::new(Cell::new(0_u32));
        let a = Signal::new(1_i32);
        let a_for_b = a.clone();
        let b = Computed::new(move || a_for_b.get() + 10);
        let a_for_c = a.clone();
        let c = Computed::new(move || a_for_c.get() + 100);
        let b_for_d = b.clone();
        let c_for_d = c.clone();
        let counter_for_d = Rc::clone(&counter);
        let d = Computed::new(move || {
            counter_for_d.set(counter_for_d.get() + 1);
            b_for_d.get() + c_for_d.get()
        });
        assert_eq!(d.get(), 112);
        assert_eq!(counter.get(), 1);
        batch(|| {
            a.set(2);
        });
        // One recompute even though both b and c marked d dirty.
        assert_eq!(d.get(), 114);
        assert_eq!(counter.get(), 2);
    }

    // ---- R37.5 #3: cycle detection ----------------------------------------

    #[test]
    #[should_panic(expected = "reactive cycle detected")]
    fn self_referential_computed_panics_with_cycle_message() {
        use std::cell::RefCell;
        // A Computed that reads its own value through a shared handle slot.
        // Without cycle detection this stack-overflows.
        let slot: Rc<RefCell<Option<Computed<i32>>>> = Rc::new(RefCell::new(None));
        let slot_for_compute = Rc::clone(&slot);
        let c = Computed::new(move || {
            if let Some(self_ref) = slot_for_compute.borrow().as_ref() {
                self_ref.get() + 1
            } else {
                0
            }
        });
        *slot.borrow_mut() = Some(c.clone());
        // First read triggers recompute; closure attempts self-read → panic.
        let _ = c.get();
    }

    #[test]
    fn computed_recovers_after_panic_in_compute_closure() {
        use std::cell::Cell as StdCell;
        // The in_compute RAII guard must clear the flag on closure panic so a
        // later get() is not poisoned with the panicked-during-compute state.
        let trip = Rc::new(StdCell::new(true));
        let trip_for_c = Rc::clone(&trip);
        let c = Computed::new(move || {
            assert!(!trip_for_c.get(), "compute fail");
            42_i32
        });
        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.get()));
        assert!(first.is_err());
        // Flip the trip and confirm subsequent reads work.
        trip.set(false);
        // Computed remains dirty (recompute did not complete); next get retries.
        assert_eq!(c.get(), 42);
    }

    // ---- R37.5 #5: batch dedup correctness regression ---------------------

    #[test]
    fn batch_with_many_writes_to_same_signal_dedups_in_pending_set() {
        use super::super::owner::batch;
        // Stress: 200 writes to the same signal during a batch should still
        // produce exactly one observer notification at close.
        let a = Signal::new(0_i32);
        let a_for_c = a.clone();
        let counter = Rc::new(Cell::new(0_u32));
        let counter_for_c = Rc::clone(&counter);
        let c = Computed::new(move || {
            counter_for_c.set(counter_for_c.get() + 1);
            a_for_c.get()
        });
        // Prime cache.
        assert_eq!(c.get(), 0);
        assert_eq!(counter.get(), 1);
        batch(|| {
            for i in 1..=200 {
                a.set(i);
            }
        });
        // Lazy: c not recomputed until read; one read = one recompute.
        let _ = c.get();
        assert_eq!(counter.get(), 2);
    }
}
