//! `Effect` — eager-rerun reactive side-effect primitive (§5.23 R27).
//!
//! `Effect` is the [`ReactiveNode`] sibling of [`Computed`]: it subscribes to
//! [`Signal`] / [`Computed`] reads inside its closure on first run, then re-runs
//! the closure whenever any subscribed source fires `mark_dirty`. Unlike
//! `Computed`, `Effect`:
//!
//! - is **eager** — the initial run happens inside [`Effect::new`] so the
//!   dependency set is captured before the constructor returns;
//! - has **no return value** — it exists for its side-effect (writing to a
//!   `RefCell`, queuing a `Command`, kicking off a render frame);
//! - re-runs **eagerly on `mark_dirty`** rather than lazily on `get`;
//! - is **owned by an [`Owner`]** — dropping the owner cancels the effect by
//!   draining its source subscriptions, so the closure never re-fires after
//!   that point.
//!
//! Per §5.23 R27 ratified shape:
//!
//! - Effect lazy-registers on the first `Signal::get` inside its scope;
//!   subsequent re-runs drain stale subs and re-register from the new pass so
//!   branching closures rebalance their dependencies automatically (mirrors
//!   `Computed::recompute`'s `source_cleanups` drain).
//! - Cleanup on Owner drop — `cancelled` flag flips, source subscriptions are
//!   drained, further `mark_dirty` is a no-op.
//! - Conflating reactive scope with async (React `useEffect`) is the
//!   anti-pattern the §5.23 charter explicitly rejects — `Effect` here is
//!   sync; async description belongs to `Command<Intent>` (carry to R51.138+).
//! - `view-fn` no Effect access (read-only Signal context, §5.3 caller-side
//!   discipline) — not enforced at this layer.
//! - `dry_run` skip is deferred to the `dry_run` substrate round (R51.138+);
//!   the cancelled-after-owner-drop branch is the only side-effect gate today.
//!
//! ## Structural mirror of `Computed`
//!
//! `EffectInner` deliberately mirrors `ComputedInner` (`id` + `in_run`
//! reentrancy flag + `source_cleanups` drained per pass + [`ReactiveNode`]
//! impl). The only material difference is `mark_dirty` — `Computed` lazily
//! caches and cascades, while `Effect` re-runs eagerly. Sharing the
//! [`ReactiveNode`] surface keeps `Signal::subscribe` / `Computed::subscribe`
//! agnostic to which downstream kind they feed.
//!
//! ## Self-pointer (`Weak<EffectInner>`)
//!
//! [`ReactiveNode::mark_dirty`] receives `&self`, but [`EffectInner::rerun`]
//! needs `&Rc<Self>` (the rerun publishes a fresh `Rc<dyn ReactiveNode>`
//! onto `CURRENT_OWNER` so source subscriptions see the effect as their
//! subscriber). `EffectInner` therefore stores a [`OnceCell<Weak<Self>>`] that
//! is populated immediately after `Rc::new`, and `mark_dirty` upgrades it back
//! to a strong `Rc` to dispatch `rerun`. This is the textbook Solid.js port
//! pattern; the `OnceCell` adds one word per effect and is set exactly once.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use super::owner::{
    Owner, ReactiveNode, next_node_id, run_cleanups_isolated, run_with_node,
};

/// Reactive side-effect scope.
///
/// Constructed via [`Effect::new`] with an [`Owner`] (lifetime anchor) and a
/// closure (side-effect body). The closure runs eagerly once at construction
/// — capturing its dependency set — then re-runs whenever any [`Signal`] or
/// [`Computed`] it read fires a value-changing write.
///
/// Cloning yields a handle to the same effect; both observe identical re-runs.
/// The user typically does not need to retain the handle — the effect lives
/// as long as the owner does. Dropping the [`Effect`] handle while the owner
/// is still alive does **not** stop re-runs: the owner's cleanup queue holds
/// the only authoritative strong reference once the constructor returns.
///
/// # Panics
///
/// - On reactive cycle (the closure writes a `Signal` it also reads in a way
///   that triggers a transitive re-entry into its own `rerun`). Mirrors
///   `Computed::recompute`'s cycle detector (R37.5 #3).
/// - User closure panics are propagated by the constructor's eager initial
///   run (matches `Computed::new` + first `get`). Subsequent re-runs catch
///   the panic — the `BatchGuard::drop` drain wraps each `mark_dirty` in
///   `catch_unwind`, so a panicking effect cannot poison sibling cascades.
pub struct Effect {
    #[allow(dead_code)] // handle retention only — the owner holds the live Rc
    inner: Rc<EffectInner>,
}

struct EffectInner {
    id: u64,
    /// Reentrancy flag — `true` while the closure is running. `mark_dirty`
    /// short-circuits when set so an effect that writes a `Signal` it also
    /// reads does not re-enter `rerun` from within its own pass. Mirrors
    /// `ComputedInner::in_compute` (R37.5 #3).
    in_run: Cell<bool>,
    /// `true` once `Owner::drop` has run the registered cancellation closure.
    /// Subsequent `mark_dirty` is a no-op so a stale subscriber-list entry
    /// (one that has not yet been pruned via `prune_dead`) cannot re-fire the
    /// closure after the owner is gone.
    cancelled: Cell<bool>,
    /// User closure. `Box<dyn FnMut>` so the closure may carry mutable state
    /// across re-runs (mutable capture is the natural Rust shape; Solid.js
    /// `createEffect` uses the same contract). The `RefCell` borrow brackets
    /// each closure call — re-entrant `rerun` is impossible once the
    /// `in_run` guard is set on top of it.
    closure: RefCell<Box<dyn FnMut()>>,
    /// Per-pass source cleanups. Drained at the start of every `rerun` so
    /// dynamic dependency tracking reflects only the reads from the current
    /// pass. Mirrors `ComputedInner::source_cleanups`.
    source_cleanups: RefCell<Vec<Box<dyn FnOnce()>>>,
    /// Self-pointer. Set exactly once immediately after `Rc::new`. Allows
    /// `mark_dirty(&self)` to recover a strong `Rc<Self>` for `rerun` without
    /// breaking the [`ReactiveNode`] dyn-safety contract.
    weak_self: OnceCell<Weak<EffectInner>>,
}

/// RAII guard that clears the `in_run` reentrancy flag on `Drop`, even when
/// the user closure unwinds. Mirrors `computed::InComputeGuard` exactly.
struct InRunGuard<'a>(&'a Cell<bool>);

impl Drop for InRunGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl EffectInner {
    /// Execute the closure under this effect as the active subscriber.
    ///
    /// Idempotent on `cancelled`: a `rerun` after owner drop is a no-op so a
    /// race between `prune_dead` and `dispatch_dirty` cannot resurrect a dead
    /// effect.
    fn rerun(self: &Rc<Self>) {
        if self.cancelled.get() {
            return;
        }
        assert!(
            !self.in_run.get(),
            "reactive cycle detected: Effect (id={}) re-entered during its own run",
            self.id
        );

        // Drain stale source subscriptions before re-running so the next pass
        // can establish a fresh dependency set. Each cleanup is `catch_unwind`-
        // wrapped (R37.5 #4) so a panicking unsubscribe cannot abort mid-state.
        let drained: Vec<Box<dyn FnOnce()>> =
            std::mem::take(&mut *self.source_cleanups.borrow_mut());
        run_cleanups_isolated(drained);

        self.in_run.set(true);
        let _guard = InRunGuard(&self.in_run);

        let strong: Rc<EffectInner> = Rc::clone(self);
        let self_as_node: Rc<dyn ReactiveNode> = strong;
        run_with_node(&self_as_node, || {
            let mut closure = self.closure.borrow_mut();
            closure();
        });
    }
}

impl ReactiveNode for EffectInner {
    fn node_id(&self) -> u64 {
        self.id
    }

    fn mark_dirty(&self) {
        if self.cancelled.get() || self.in_run.get() {
            // Cancelled effects are dormant; re-entrancy is the cycle case
            // that `rerun`'s own assertion already covers — we just skip
            // here so a self-write inside the closure does not panic.
            return;
        }
        // Recover a strong handle so we can call `rerun(self: &Rc<Self>)`.
        // The `weak_self` cell is populated immediately after `Rc::new`, so
        // a `None` here would mean we are being dispatched during the
        // constructor's `Rc::new` itself, which the construction order
        // forbids — treat as a no-op for defence-in-depth.
        if let Some(strong) = self.weak_self.get().and_then(Weak::upgrade) {
            strong.rerun();
        }
    }

    fn add_subscription_cleanup(&self, cleanup: Box<dyn FnOnce()>) {
        // Effect subscriptions are per-evaluation, identical to `Computed`.
        self.source_cleanups.borrow_mut().push(cleanup);
    }
}

impl Effect {
    /// Construct an effect tied to `owner` and run the closure once eagerly.
    ///
    /// The eager initial run captures the dependency set: every [`Signal`] /
    /// [`Computed`] read inside the closure auto-subscribes this effect.
    /// Subsequent value-changing writes to those sources schedule a re-run
    /// (immediate outside a [`batch`], coalesced to one re-run at outermost
    /// `batch` exit inside).
    ///
    /// Dropping `owner` (or any of its ancestor owners) drains the effect's
    /// source subscriptions and flips its `cancelled` flag, so the closure
    /// never re-fires after that.
    ///
    /// # Panics
    ///
    /// Panics if the eager initial run's closure panics (the construction
    /// point is the natural unwind site, matching `Computed::new` + first
    /// `get`). Re-runs after construction are caught by `BatchGuard::drop`'s
    /// `catch_unwind` (`batch` flush) or propagate normally for
    /// non-batched writes — same contract as `Computed::recompute`.
    pub fn new<F>(owner: &Owner, f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        let inner = Rc::new(EffectInner {
            id: next_node_id(),
            in_run: Cell::new(false),
            cancelled: Cell::new(false),
            closure: RefCell::new(Box::new(f)),
            source_cleanups: RefCell::new(Vec::new()),
            weak_self: OnceCell::new(),
        });
        // Populate the self-pointer before any `mark_dirty` can fire (the
        // eager initial `rerun` below subscribes sources, which only later
        // call back into `mark_dirty`).
        let _ = inner.weak_self.set(Rc::downgrade(&inner));

        // Eager initial pass — captures dependencies, fires the side-effect.
        inner.rerun();

        // Register an owner-tied cleanup: when `owner` drops it walks its
        // cleanup queue (in registration order) and drains our subscriptions.
        // We capture a `Weak<EffectInner>` so an owner that outlives the
        // effect handle does not leak via the cleanup queue (R37.5 #2 pattern).
        let inner_weak: Weak<EffectInner> = Rc::downgrade(&inner);
        owner.on_cleanup(Box::new(move || {
            if let Some(inner) = inner_weak.upgrade() {
                inner.cancelled.set(true);
                let drained: Vec<Box<dyn FnOnce()>> =
                    std::mem::take(&mut *inner.source_cleanups.borrow_mut());
                run_cleanups_isolated(drained);
            }
        }));

        Self { inner }
    }

    /// Stable identity. Used by tests and (future) introspection.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    #[cfg(test)]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.cancelled.get()
    }

    #[cfg(test)]
    pub(crate) fn source_cleanup_count(&self) -> usize {
        self.inner.source_cleanups.borrow().len()
    }
}

impl Clone for Effect {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl std::fmt::Debug for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Effect")
            .field("id", &self.inner.id)
            .field("cancelled", &self.inner.cancelled.get())
            .field("sources", &self.inner.source_cleanups.borrow().len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::Effect;
    use super::super::computed::Computed;
    use super::super::owner::{Owner, batch};
    use super::super::signal::Signal;

    #[test]
    fn effect_runs_eagerly_on_construction() {
        let owner = Owner::new();
        let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let log_for_effect = Rc::clone(&log);
        let _e = Effect::new(&owner, move || {
            log_for_effect.borrow_mut().push(1);
        });
        assert_eq!(*log.borrow(), vec![1]);
    }

    #[test]
    fn effect_reruns_when_subscribed_signal_changes() {
        let owner = Owner::new();
        let signal = Signal::new(0_i32);
        let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let signal_for_effect = signal.clone();
        let log_for_effect = Rc::clone(&log);
        let _e = Effect::new(&owner, move || {
            log_for_effect.borrow_mut().push(signal_for_effect.get());
        });
        assert_eq!(*log.borrow(), vec![0]);
        signal.set(1);
        assert_eq!(*log.borrow(), vec![0, 1]);
        signal.set(2);
        assert_eq!(*log.borrow(), vec![0, 1, 2]);
    }

    #[test]
    fn equality_skipping_signal_set_does_not_rerun_effect() {
        let owner = Owner::new();
        let signal = Signal::new(5_i32);
        let count = Rc::new(Cell::new(0_u32));
        let signal_for_effect = signal.clone();
        let count_for_effect = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            count_for_effect.set(count_for_effect.get() + 1);
            let _ = signal_for_effect.get();
        });
        assert_eq!(count.get(), 1);
        signal.set(5); // no value change
        assert_eq!(count.get(), 1);
        signal.set(6);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn dropping_owner_stops_further_reruns() {
        let owner = Owner::new();
        let signal = Signal::new(0_i32);
        let count = Rc::new(Cell::new(0_u32));
        {
            let signal_for_effect = signal.clone();
            let count_for_effect = Rc::clone(&count);
            let _e = Effect::new(&owner, move || {
                count_for_effect.set(count_for_effect.get() + 1);
                let _ = signal_for_effect.get();
            });
            assert_eq!(count.get(), 1);
            signal.set(1);
            assert_eq!(count.get(), 2);
        }
        drop(owner);
        signal.set(2);
        assert_eq!(count.get(), 2, "no further rerun after owner drop");
    }

    #[test]
    fn dropping_parent_owner_cascades_to_child_effect() {
        let parent = Owner::new();
        let child = Owner::new_child(&parent);
        let signal = Signal::new(0_i32);
        let count = Rc::new(Cell::new(0_u32));
        let signal_for_effect = signal.clone();
        let count_for_effect = Rc::clone(&count);
        let _e = Effect::new(&child, move || {
            count_for_effect.set(count_for_effect.get() + 1);
            let _ = signal_for_effect.get();
        });
        assert_eq!(count.get(), 1);
        // Child stays alive only via parent's `children` strong ref.
        // Drop user-side `child` handle first to confirm parent keeps it alive.
        drop(child);
        signal.set(1);
        assert_eq!(count.get(), 2, "child kept alive via parent's strong ref");
        drop(parent);
        signal.set(2);
        assert_eq!(count.get(), 2, "parent drop cascades to child → effect cancelled");
    }

    #[test]
    fn dynamic_dependency_tracking_swaps_subscriptions_across_reruns() {
        let owner = Owner::new();
        let cond = Signal::new(true);
        let a = Signal::new(1_i32);
        let b = Signal::new(100_i32);
        let last: Rc<Cell<i32>> = Rc::new(Cell::new(0));

        let cond_for_effect = cond.clone();
        let a_for_effect = a.clone();
        let b_for_effect = b.clone();
        let last_for_effect = Rc::clone(&last);
        let _e = Effect::new(&owner, move || {
            let v = if cond_for_effect.get() {
                a_for_effect.get()
            } else {
                b_for_effect.get()
            };
            last_for_effect.set(v);
        });
        assert_eq!(last.get(), 1);
        assert_eq!(a.observer_count(), 1);
        assert_eq!(b.observer_count(), 0);

        cond.set(false);
        assert_eq!(last.get(), 100);
        assert_eq!(a.observer_count(), 0, "stale `a` subscription drained");
        assert_eq!(b.observer_count(), 1);

        // Writing the no-longer-subscribed signal must not re-fire.
        a.set(999);
        assert_eq!(last.get(), 100);
        b.set(200);
        assert_eq!(last.get(), 200);
    }

    #[test]
    fn batch_coalesces_multiple_writes_into_one_rerun() {
        let owner = Owner::new();
        let signal = Signal::new(0_i32);
        let count = Rc::new(Cell::new(0_u32));
        let signal_for_effect = signal.clone();
        let count_for_effect = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            count_for_effect.set(count_for_effect.get() + 1);
            let _ = signal_for_effect.get();
        });
        assert_eq!(count.get(), 1);
        batch(|| {
            signal.set(1);
            signal.set(2);
            signal.set(3);
            assert_eq!(count.get(), 1, "deferred — no rerun inside batch yet");
        });
        assert_eq!(count.get(), 2, "outermost batch exit → exactly one rerun");
    }

    #[test]
    fn nested_batch_drains_only_at_outermost_exit() {
        let owner = Owner::new();
        let signal = Signal::new(0_i32);
        let count = Rc::new(Cell::new(0_u32));
        let signal_for_effect = signal.clone();
        let count_for_effect = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            count_for_effect.set(count_for_effect.get() + 1);
            let _ = signal_for_effect.get();
        });
        batch(|| {
            signal.set(1);
            batch(|| {
                signal.set(2);
                assert_eq!(count.get(), 1, "inner batch deferred");
            });
            assert_eq!(count.get(), 1, "inner exit did not drain");
        });
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn effect_subscribes_to_computed_chain() {
        let owner = Owner::new();
        let a = Signal::new(1_i32);
        let a_for_computed = a.clone();
        let doubled = Computed::new(move || a_for_computed.get() * 2);
        let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let doubled_for_effect = doubled.clone();
        let log_for_effect = Rc::clone(&log);
        let _e = Effect::new(&owner, move || {
            log_for_effect.borrow_mut().push(doubled_for_effect.get());
        });
        assert_eq!(*log.borrow(), vec![2]);
        a.set(5);
        assert_eq!(*log.borrow(), vec![2, 10]);
    }

    #[test]
    #[should_panic(expected = "reactive cycle detected")]
    fn self_referential_effect_panics_with_cycle_message() {
        let owner = Owner::new();
        let signal = Signal::new(0_i32);

        // Slot to satisfy the closure's hold on `self`. The closure reads
        // signal, then writes signal — Signal::set fires `mark_dirty` on this
        // effect, which would re-enter `rerun` while `in_run` is set. The
        // assertion inside `rerun` panics on that re-entry path.
        //
        // To reach the assertion path we must bypass the `in_run` short-circuit
        // in `mark_dirty`. We synthesize that by exposing a slot the closure
        // can flip to force re-entry: the closure first records its own dep,
        // then directly invokes the inner `rerun` once on the second pass.
        //
        // Simpler: have the closure cause re-entry via a `batch` flush that
        // happens *inside* the closure. `batch` exit re-fires `mark_dirty`,
        // but the closure is still on the stack — yet `in_run` short-circuits.
        //
        // The cleanest reproducer is a closure that re-enters `rerun`
        // directly. We expose that via a back-channel: the effect handle held
        // by the closure itself.
        let effect_slot: Rc<RefCell<Option<Effect>>> = Rc::new(RefCell::new(None));
        let effect_slot_for_closure = Rc::clone(&effect_slot);
        let signal_for_effect = signal.clone();
        let e = Effect::new(&owner, move || {
            let _ = signal_for_effect.get();
            if let Some(handle) = effect_slot_for_closure.borrow().as_ref() {
                // Force re-entry into our own `rerun`. The `in_run` flag is
                // set, so this hits the cycle assertion.
                handle.inner.rerun();
            }
        });
        *effect_slot.borrow_mut() = Some(e.clone());
        // Trigger a re-run via the signal write; the second pass calls into
        // its own `rerun` and panics.
        signal.set(1);
    }

    #[test]
    fn effect_panicking_during_initial_run_propagates_then_owner_recovers() {
        let owner = Owner::new();
        let stack_before = super::super::owner::current_owner_stack_len();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = Effect::new(&owner, || {
                panic!("simulated effect panic");
            });
        }));
        assert!(result.is_err());
        let stack_after = super::super::owner::current_owner_stack_len();
        assert_eq!(
            stack_before, stack_after,
            "CURRENT_OWNER stack restored after panicking effect"
        );
        // Owner still usable for further effects.
        let count = Rc::new(Cell::new(0_u32));
        let count_for_effect = Rc::clone(&count);
        let _e2 = Effect::new(&owner, move || {
            count_for_effect.set(count_for_effect.get() + 1);
        });
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn effect_inside_self_write_skips_reentry_via_in_run_short_circuit() {
        // Closure writes the same signal it reads. The `in_run` guard inside
        // `mark_dirty` short-circuits the rerun so we do not stack-overflow.
        let owner = Owner::new();
        let signal = Signal::new(0_i32);
        let count = Rc::new(Cell::new(0_u32));
        let signal_for_effect = signal.clone();
        let count_for_effect = Rc::clone(&count);
        let _e = Effect::new(&owner, move || {
            count_for_effect.set(count_for_effect.get() + 1);
            let current = signal_for_effect.get();
            if current < 1 {
                signal_for_effect.set(current + 1);
            }
        });
        // Initial run reads 0, writes 1. mark_dirty fires but `in_run` is
        // still set on the outer pass, so the rerun is skipped.
        assert_eq!(count.get(), 1);
        assert_eq!(signal.get(), 1);
    }

    #[test]
    fn distinct_effects_under_same_owner_are_independent() {
        let owner = Owner::new();
        let signal = Signal::new(0_i32);
        let first_count = Rc::new(Cell::new(0_u32));
        let second_count = Rc::new(Cell::new(0_u32));

        let signal_for_first = signal.clone();
        let first_count_clone = Rc::clone(&first_count);
        let _first = Effect::new(&owner, move || {
            first_count_clone.set(first_count_clone.get() + 1);
            let _ = signal_for_first.get();
        });
        let signal_for_second = signal.clone();
        let second_count_clone = Rc::clone(&second_count);
        let _second = Effect::new(&owner, move || {
            second_count_clone.set(second_count_clone.get() + 1);
            let _ = signal_for_second.get();
        });
        assert_eq!(first_count.get(), 1);
        assert_eq!(second_count.get(), 1);
        signal.set(1);
        assert_eq!(first_count.get(), 2);
        assert_eq!(second_count.get(), 2);
    }

    #[test]
    fn effect_clone_shares_state() {
        let owner = Owner::new();
        let signal = Signal::new(0_i32);
        let count = Rc::new(Cell::new(0_u32));
        let signal_for_effect = signal.clone();
        let count_for_effect = Rc::clone(&count);
        let e = Effect::new(&owner, move || {
            count_for_effect.set(count_for_effect.get() + 1);
            let _ = signal_for_effect.get();
        });
        let alias = e.clone();
        assert_eq!(e.id(), alias.id());
        signal.set(1);
        // Both handles observe the same rerun count via the shared owner.
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn cancelled_effect_drains_source_subscriptions() {
        let owner = Owner::new();
        let signal = Signal::new(0_i32);
        let signal_for_effect = signal.clone();
        let e = Effect::new(&owner, move || {
            let _ = signal_for_effect.get();
        });
        assert_eq!(signal.observer_count(), 1);
        assert_eq!(e.source_cleanup_count(), 1);
        assert!(!e.is_cancelled());
        drop(owner);
        assert!(e.is_cancelled());
        assert_eq!(e.source_cleanup_count(), 0);
        assert_eq!(signal.observer_count(), 0);
    }
}
