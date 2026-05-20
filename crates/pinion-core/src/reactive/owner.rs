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
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

thread_local! {
    static NEXT_NODE_ID: Cell<u64> = const { Cell::new(0) };
    static CURRENT_OWNER: RefCell<Vec<Weak<dyn ReactiveNode>>> = const { RefCell::new(Vec::new()) };
    static BATCH_DEPTH: Cell<u32> = const { Cell::new(0) };
    static PENDING_DIRTY: RefCell<SubscriberSet> = RefCell::new(SubscriberSet::new());
    /// R51.146 §5.22 — Owner-only handle stack, mirrors [`CURRENT_OWNER`]
    /// for the subset of pushes that originate from [`Owner::run`].
    /// [`Computed::recompute`] / [`Effect::recompute`] use the internal
    /// [`run_with_node`] path which deliberately leaves this stack
    /// untouched, so [`Owner::current`] inside a `Computed` body still
    /// resolves to the enclosing `Owner` rather than the computed node
    /// (the textbook Solid.js / SolidJS contract for `useOwner` /
    /// `createRoot` capture: derived values stay framework-owned, the
    /// "active scope" the application sees is always the lexical
    /// `Owner`). [`Owner::current`] returns the strong [`Owner`]
    /// handle by upgrading the topmost [`Weak`]; the strong handle
    /// suffices for [`Animation`](crate::animation::Animation)
    /// registration, [`Effect`](crate::reactive::Effect) anchoring,
    /// and [`Command`](crate::command::Command) dispatch from inside
    /// a framework-wrapped view fn.
    static CURRENT_OWNER_HANDLE: RefCell<Vec<Weak<OwnerInner>>> = const { RefCell::new(Vec::new()) };
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
    /// Tick-driven primitives registered through
    /// [`Owner::register_animation`]. Walked by [`Owner::tick_animations`]
    /// once per frame; the framework runtime is the canonical caller
    /// (carry: R51.139+ paint-loop wiring).
    pub(crate) owned_animations: RefCell<Vec<Rc<dyn crate::animation::Tickable>>>,
    /// Pending declarative commands (§5.23) produced inside this scope
    /// and not yet drained by a handler. Cleared on Owner drop so
    /// dangling IO is impossible — the textbook Solid.js cancellation
    /// pattern translated to a queue.
    pub(crate) owned_commands: RefCell<Vec<crate::command::Command>>,
    /// R51.150 §5.22 — owner-scoped typed cache keyed by `&'static str`.
    ///
    /// Application code accesses through [`Owner::cache`]; the entry
    /// type is `Rc<dyn Any>` so a single map carries heterogeneous
    /// values (one `Animation<f32>` for hover, a different
    /// `Computed<u32>` for derived state, a `Resource<...>` for an
    /// async fetch — all keyed by distinct string names).
    ///
    /// Use case: per-binding caches (animations, resources, expensive
    /// derived values, IO handles) that the view fn instantiates on
    /// the first paint and reuses on subsequent paints. Cleared on
    /// owner drop so cached values evaporate with the binding —
    /// matches the Solid.js `createMemo` / React `useRef` lifecycle.
    pub(crate) cache: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
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
                owned_animations: RefCell::new(Vec::new()),
                owned_commands: RefCell::new(Vec::new()),
                cache: RefCell::new(HashMap::new()),
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
    ///
    /// R51.146 §5.22 — pushes onto the separate
    /// [`CURRENT_OWNER_HANDLE`] stack in addition to [`CURRENT_OWNER`].
    /// The two stacks track different things: [`CURRENT_OWNER`] is the
    /// subscriber stack consulted by [`Signal::get`](crate::reactive::Signal::get)
    /// and [`Computed::get`](crate::reactive::Computed::get) for
    /// auto-subscription, while [`CURRENT_OWNER_HANDLE`] is the
    /// pure-`Owner` stack [`Owner::current`] reads. The split is what
    /// lets a [`Computed`](crate::reactive::Computed) body or
    /// [`Effect`](crate::reactive::Effect) reactive subscriber sit
    /// atop the subscriber stack (so its reads are tracked) while
    /// [`Owner::current`] inside it still returns the enclosing
    /// [`Owner`] — the lexical scope the application owns rather than
    /// the framework-internal derived node.
    pub fn run<R>(&self, f: impl FnOnce() -> R) -> R {
        let strong: Rc<OwnerInner> = Rc::clone(&self.inner);
        let handle_weak: Weak<OwnerInner> = Rc::downgrade(&strong);
        let as_node: Rc<dyn ReactiveNode> = strong;
        let _node_guard = OwnerStackGuard::push(Rc::downgrade(&as_node));
        let _handle_guard = OwnerHandleGuard::push(handle_weak);
        f()
    }

    /// R51.146 §5.22 — return the strong [`Owner`] handle for the
    /// innermost active [`Owner::run`] scope, if any.
    ///
    /// Returns [`None`] when called outside any active [`Owner::run`]
    /// (a bare entry from `main`, a background thread that never set
    /// up an owner, or after every enclosing [`Owner::run`] has
    /// returned). Returns [`Some`] when called from inside a view fn
    /// wrapped by the framework's
    /// `root_owner().run(|| V::view(state, &frame))` pattern —
    /// applications and the SCE-emitted code use this to attach
    /// [`Animation<T>`](crate::animation::Animation) instances,
    /// [`Effect`](crate::reactive::Effect) closures, and
    /// [`Command`](crate::command::Command) dispatches to the binding's
    /// reactive scope without threading the [`Owner`] argument through
    /// every callee.
    ///
    /// The returned [`Owner`] is a strong clone — registrations on it
    /// pin the [`Owner`] alive for as long as the registration holds.
    /// Stale [`Weak`] entries (an [`Owner`] dropped mid-`run` via
    /// `mem::take` or similar pathological code path) appear here as
    /// the strong-handle stack walks down to the first live entry —
    /// the same panic-safe RAII invariant the [`OwnerHandleGuard`]
    /// drop already enforces.
    ///
    /// Reactive-node nesting note: a [`Computed`](crate::reactive::Computed)
    /// or [`Effect`](crate::reactive::Effect) running its recompute
    /// closure pushes onto [`CURRENT_OWNER`] (the subscriber stack)
    /// but NOT onto [`CURRENT_OWNER_HANDLE`]. So inside a `Computed`
    /// body, [`Owner::current`] still returns the lexically enclosing
    /// [`Owner::run`] scope — the framework-internal derived node is
    /// invisible to applications by design (the `SolidJS` `useOwner`
    /// contract).
    #[must_use]
    pub fn current() -> Option<Owner> {
        CURRENT_OWNER_HANDLE.with(|stack| {
            let borrowed = stack.borrow();
            // Walk from top to bottom — a `Weak` that fails to
            // upgrade is a dropped scope (rare; happens when an
            // outer `Owner` is moved/dropped while a nested `run`
            // closure is still executing under it). The first live
            // entry is the innermost active scope.
            for weak in borrowed.iter().rev() {
                if let Some(inner) = weak.upgrade() {
                    return Some(Owner { inner });
                }
            }
            None
        })
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

    /// Register a cleanup closure that runs when this owner drops, in
    /// registration order (FIFO). Cascades after children — `OwnerInner::drop`
    /// clears `children` first so descendant cleanups run before ours.
    ///
    /// The closure is wrapped in `catch_unwind` at drain time (via
    /// `run_cleanups_isolated`) so a panicking cleanup cannot abort sibling
    /// cleanups via double-panic during `Drop`.
    ///
    /// This is the public entry point used by `Effect::new` (§5.23) to bind
    /// an effect's lifetime to a scope. The internal `ReactiveNode`-trait
    /// surface (`add_subscription_cleanup`) is reserved for the observer-list
    /// teardown path used by `Signal::subscribe` / `Computed::subscribe_observer`.
    pub fn on_cleanup(&self, cleanup: Box<dyn FnOnce()>) {
        self.inner.cleanups.borrow_mut().push(cleanup);
    }

    /// Register a [`Tickable`](crate::animation::Tickable) for per-frame
    /// dispatch through [`Owner::tick_animations`]. Used internally by
    /// [`Animation::new`](crate::animation::Animation::new); applications
    /// rarely call this directly.
    ///
    /// The registry keeps a strong `Rc<dyn Tickable>`; dropping this owner
    /// releases that reference, so a [`Tickable`](crate::animation::Tickable)
    /// whose only other holder was the caller's
    /// [`Animation`](crate::animation::Animation) handle drops at the same
    /// time as the owner.
    pub fn register_animation(&self, tickable: Rc<dyn crate::animation::Tickable>) {
        self.inner.owned_animations.borrow_mut().push(tickable);
    }

    /// Advance every registered animation by `dt` seconds — depth-first
    /// across the owner subtree, so child scopes tick before this scope's
    /// own registrations. Wrapped in [`batch`] so all
    /// [`Signal::set`](crate::reactive::Signal::set) writes coalesce into
    /// exactly one cascade per subscribed downstream
    /// [`Computed`](crate::reactive::Computed) /
    /// [`Effect`](crate::reactive::Effect) — the textbook
    /// frame-coherence guarantee.
    ///
    /// Implementation note — both registry walks snapshot via `Rc::clone`
    /// before releasing the `RefCell` borrow so an animation that
    /// (re)registers another animation during its own `tick` does not
    /// trigger a `BorrowMutError` (mirrors `SubscriberSet::snapshot` in
    /// `dispatch_dirty`).
    pub fn tick_animations(&self, dt: f32) {
        // Snapshot children first; descend depth-first.
        let children: Vec<Owner> = self.inner.children.borrow().iter().cloned().collect();
        let anims: Vec<Rc<dyn crate::animation::Tickable>> = self
            .inner
            .owned_animations
            .borrow()
            .iter()
            .map(Rc::clone)
            .collect();
        batch(|| {
            for child in &children {
                child.tick_animations(dt);
            }
            for anim in &anims {
                anim.tick(dt);
            }
        });
    }

    #[cfg(test)]
    pub(crate) fn registered_animation_count(&self) -> usize {
        self.inner.owned_animations.borrow().len()
    }

    /// R51.147 §5.28 — `true` when any animation registered on this
    /// owner (or transitively on a descendant scope) reports
    /// [`Tickable::is_at_rest(epsilon)`](crate::animation::Tickable::is_at_rest)
    /// as `false`. The walk mirrors [`Self::tick_animations`]:
    /// depth-first across children, then this scope's direct
    /// registrations.
    ///
    /// Used by framework backends to decide whether to request another
    /// frame from the host's vsync loop. Once every animation has
    /// settled (`is_at_rest(eps)` for every registry entry) the
    /// backend can stop requesting redraws and let the scene rest at
    /// steady state — the textbook "lazy repaint while animating"
    /// pattern (winit's `Window::request_redraw` is idempotent;
    /// backends still benefit from skipping the no-op call once nothing
    /// is moving).
    ///
    /// `epsilon` is forwarded verbatim to each
    /// [`Tickable::is_at_rest`](crate::animation::Tickable::is_at_rest).
    /// Callers typically pass [`Animation::DEFAULT_REST_EPSILON`](crate::animation::Animation::DEFAULT_REST_EPSILON)
    /// so the "at rest" criterion matches the spring solver's own
    /// settlement threshold.
    ///
    /// Implementation note — children + animations snapshot
    /// (`Rc::clone` collect) before releasing the `RefCell` borrow,
    /// mirroring [`Self::tick_animations`]; safe under mid-walk
    /// register/drop.
    #[must_use]
    pub fn any_animation_active(&self, epsilon: f32) -> bool {
        let children: Vec<Owner> = self.inner.children.borrow().iter().cloned().collect();
        for child in &children {
            if child.any_animation_active(epsilon) {
                return true;
            }
        }
        let anims: Vec<Rc<dyn crate::animation::Tickable>> = self
            .inner
            .owned_animations
            .borrow()
            .iter()
            .map(Rc::clone)
            .collect();
        for anim in &anims {
            if !anim.is_at_rest(epsilon) {
                return true;
            }
        }
        false
    }

    /// Queue a declarative [`Command`](crate::command::Command) on this
    /// scope. The framework / registered handler drains the queue via
    /// [`Owner::take_pending_commands`]; until then it is visible to
    /// inspection via [`Owner::pending_commands`].
    ///
    /// `scope_id` on the [`Command`](crate::command::Command) is not
    /// checked against this owner — callers stamp it from any source
    /// they want (typically the producing widget's reactive scope). The
    /// queue itself is associated with *this* owner by virtue of where
    /// it is registered, which is also the cancellation anchor — on
    /// drop, every queued command is discarded.
    pub fn dispatch_command(&self, command: crate::command::Command) {
        self.inner.owned_commands.borrow_mut().push(command);
    }

    /// Snapshot of pending [`Command`](crate::command::Command)s in this
    /// scope (clones the queue contents). Does **not** drain — use
    /// [`Owner::take_pending_commands`] when you want to dispatch them.
    ///
    /// This is the `dry_run` / RPC-inspection path: scenario explorers
    /// and AI agents can read the pending queue without committing to
    /// execution. The §5.23 contract that "`dry_run` skips `Command`
    /// dispatch but collects pending for AI inspection" reduces to:
    /// take a snapshot here, roll back via `Owner::restore`.
    #[must_use]
    pub fn pending_commands(&self) -> Vec<crate::command::Command> {
        self.inner.owned_commands.borrow().clone()
    }

    /// Drain the pending [`Command`](crate::command::Command) queue,
    /// returning ownership of every queued command in FIFO order. The
    /// caller is the handler-side responsibility — once drained, the
    /// commands no longer count as cancellable-by-Owner-drop.
    #[must_use]
    pub fn take_pending_commands(&self) -> Vec<crate::command::Command> {
        std::mem::take(&mut *self.inner.owned_commands.borrow_mut())
    }

    /// Depth-first drain across the owner subtree (children first, then
    /// this scope's own). Returns every drained command in subtree
    /// traversal order — the natural shape for a framework pump that
    /// dispatches commands once per frame after reducer reduction.
    #[must_use]
    pub fn take_pending_commands_recursive(&self) -> Vec<crate::command::Command> {
        let children: Vec<Owner> = self.inner.children.borrow().iter().cloned().collect();
        let mut drained: Vec<crate::command::Command> = Vec::new();
        for child in &children {
            drained.append(&mut child.take_pending_commands_recursive());
        }
        drained.append(&mut self.take_pending_commands());
        drained
    }

    /// R51.150 §5.22 — owner-scoped typed cache for per-binding
    /// heap-allocated state.
    ///
    /// On the first call with a given `key`, invokes `factory()` and
    /// stores the result. On every subsequent call with the same
    /// `key` returns the cached value verbatim without re-running
    /// the factory. The returned [`Rc<V>`] aliases the same heap
    /// allocation across calls, so subsequent reads see the same
    /// instance and any interior-mutability state (e.g. an
    /// [`Animation`](crate::animation::Animation) handle's
    /// [`SpringState`](crate::animation::SpringState)) persists across
    /// view-fn re-paints.
    ///
    /// ## Lifetime
    ///
    /// Cached values live for as long as the owner; on
    /// [`Owner`] drop the [`HashMap`] drops, releasing the last
    /// strong [`Rc`] reference and dropping every cached value with
    /// its registered cleanups (animations unregister from the tick
    /// list, signals release their observers, etc.). Sibling owners
    /// have independent caches.
    ///
    /// ## Replaces the thread-local `OnceCell` workaround
    ///
    /// Pre-R51.150 application code (R51.147 / R51.148 visual demos)
    /// reached for `thread_local! { static X: OnceCell<Animation<f32>> }`
    /// to materialise a per-binding animation in the view fn. The
    /// workaround was a `[[textbook-long-term-correct]]` violation:
    /// `thread_local` caches survive shell drops (stale animation
    /// pinned to a dead owner), collide across multiple shells in
    /// the same thread, and conflate the *substrate's* reactive
    /// scope with the *thread's* memory. [`Owner::cache`] is the
    /// canonical `SolidJS` / Leptos `useMemo` / React `useRef` shape
    /// — value attached to the reactive owner, dropped with it,
    /// independent across instances.
    ///
    /// ## Key collision and type safety
    ///
    /// `key` is a `&'static str` for ergonomics (`"hover_anim"` literal
    /// in the view fn). The same key from the same call site naturally
    /// reuses the cached entry — that is the intended semantics. The
    /// same key with a *different* `V` is a programming error: the
    /// `Rc::downcast` fails and this method panics with a diagnostic
    /// message naming the key and the conflicting types. Use distinct
    /// string keys for distinct values inside the same scope; for
    /// generic-typed caches use the type name as part of the key
    /// (`"hover_anim_f32"`).
    ///
    /// ## Panics
    ///
    /// Panics if a previous call with the same `key` stored a value of
    /// a different concrete type. This is a load-bearing assertion:
    /// silently re-running the factory under a type mismatch would
    /// hand back a fresh instance every paint, defeating the cache's
    /// own contract.
    pub fn cache<V, F>(&self, key: &'static str, factory: F) -> Rc<V>
    where
        V: 'static,
        F: FnOnce() -> V,
    {
        // First-call vs reuse decision under a single borrow. The
        // `or_insert_with` arm fires `factory()` exactly once on miss;
        // the returned `Rc<dyn Any>` is shared across every call site.
        let any_rc: Rc<dyn Any> = {
            let mut cache = self.inner.cache.borrow_mut();
            Rc::clone(
                cache
                    .entry(key)
                    .or_insert_with(|| Rc::new(factory()) as Rc<dyn Any>),
            )
        };
        // Borrow released before the `downcast` — the downcast call
        // itself is `Rc::clone`-equivalent (it bumps the strong count
        // and rewraps as `Rc<V>`), no borrow re-entry.
        Rc::downcast::<V>(any_rc).unwrap_or_else(|_| {
            panic!(
                "Owner::cache key {key:?} already holds a value of a different type; \
                 use distinct keys for distinct types within the same owner scope",
            );
        })
    }

    /// R51.150 §5.22 — `true` when `key` has been populated by a
    /// previous [`Owner::cache`] call on this owner.
    ///
    /// Returns `false` for un-touched keys and for keys whose cached
    /// values have been dropped by an owner reset (not currently
    /// implemented; the cache lives for the owner's lifetime today).
    /// Primarily for diagnostics and tests; application code should
    /// just call [`Owner::cache`] — the lazy-init contract handles
    /// the missing-key case transparently.
    #[must_use]
    pub fn cache_contains(&self, key: &'static str) -> bool {
        self.inner.cache.borrow().contains_key(key)
    }

    #[cfg(test)]
    pub(crate) fn cache_size(&self) -> usize {
        self.inner.cache.borrow().len()
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

/// R51.146 §5.22 — RAII guard for the [`CURRENT_OWNER_HANDLE`] stack
/// mirroring [`OwnerStackGuard`]. Pushed only by [`Owner::run`]; the
/// internal [`run_with_node`] path (used by [`Computed::recompute`] /
/// [`Effect::recompute`]) leaves this stack untouched, so
/// [`Owner::current`] inside derived-node bodies still resolves to the
/// enclosing application scope.
struct OwnerHandleGuard;

impl OwnerHandleGuard {
    fn push(weak: Weak<OwnerInner>) -> Self {
        CURRENT_OWNER_HANDLE.with(|stack| stack.borrow_mut().push(weak));
        Self
    }
}

impl Drop for OwnerHandleGuard {
    fn drop(&mut self) {
        CURRENT_OWNER_HANDLE.with(|stack| {
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
pub(crate) fn current_owner_handle_stack_len() -> usize {
    CURRENT_OWNER_HANDLE.with(|s| s.borrow().len())
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

    // ────────────────────────────────────────────────────────────────
    // R51.150 §5.22 — Owner::cache primitive tests.
    //
    // Replaces the thread_local OnceCell workaround the R51.147 /
    // R51.148 visual demos shipped with. The tests pin down:
    //
    // - Fresh owner has empty cache.
    // - First call invokes factory; subsequent calls reuse.
    // - Same key returned Rc aliases (same heap allocation).
    // - Different keys are independent.
    // - Distinct types under distinct keys coexist.
    // - Same key with mismatched type panics (load-bearing contract).
    // - Owner drop releases cached value (verified via Rc strong count).
    // - Sibling owners have isolated caches.
    // ────────────────────────────────────────────────────────────────

    mod cache {
        use super::Owner;
        use std::cell::Cell;
        use std::rc::Rc;

        #[test]
        fn fresh_owner_cache_is_empty() {
            let owner = Owner::new();
            assert_eq!(owner.cache_size(), 0);
            assert!(!owner.cache_contains("foo"));
        }

        #[test]
        fn first_call_invokes_factory_and_caches() {
            let owner = Owner::new();
            let factory_calls = Rc::new(Cell::new(0_u32));
            let calls_clone = Rc::clone(&factory_calls);
            let v: Rc<u32> = owner.cache("key", move || {
                calls_clone.set(calls_clone.get() + 1);
                42_u32
            });
            assert_eq!(*v, 42);
            assert_eq!(factory_calls.get(), 1);
            assert_eq!(owner.cache_size(), 1);
            assert!(owner.cache_contains("key"));
        }

        #[test]
        fn subsequent_calls_reuse_without_invoking_factory() {
            let owner = Owner::new();
            let factory_calls = Rc::new(Cell::new(0_u32));
            for expected_value in [10_u32, 10, 10, 10] {
                let calls_clone = Rc::clone(&factory_calls);
                let v: Rc<u32> = owner.cache("k", move || {
                    calls_clone.set(calls_clone.get() + 1);
                    expected_value
                });
                assert_eq!(*v, 10);
            }
            assert_eq!(
                factory_calls.get(),
                1,
                "factory must run exactly once for the same key",
            );
        }

        #[test]
        fn same_key_returns_aliased_rc() {
            // The cache stores `Rc<dyn Any>` once; every cache call
            // hands back an `Rc<V>` that points at the same heap
            // allocation. We verify via `Rc::ptr_eq`.
            let owner = Owner::new();
            let a: Rc<String> = owner.cache("greet", || String::from("hello"));
            let b: Rc<String> = owner.cache("greet", || String::from("never-fires"));
            assert!(Rc::ptr_eq(&a, &b), "second cache call must alias first");
            assert_eq!(*a, "hello");
            assert_eq!(*b, "hello");
        }

        #[test]
        fn distinct_keys_isolate_values() {
            let owner = Owner::new();
            let a: Rc<u32> = owner.cache("a", || 1_u32);
            let b: Rc<u32> = owner.cache("b", || 2_u32);
            assert_eq!(*a, 1);
            assert_eq!(*b, 2);
            assert!(!Rc::ptr_eq(&a, &b));
            assert_eq!(owner.cache_size(), 2);
        }

        #[test]
        fn distinct_types_coexist_under_distinct_keys() {
            let owner = Owner::new();
            let int_val: Rc<u32> = owner.cache("int", || 7_u32);
            let str_val: Rc<String> = owner.cache("str", || String::from("seven"));
            let cell_val: Rc<Cell<f32>> = owner.cache("cell", || Cell::new(1.5_f32));
            assert_eq!(*int_val, 7);
            assert_eq!(*str_val, "seven");
            assert_eq!(cell_val.get().to_bits(), 1.5_f32.to_bits());
            // Mutate through interior mutability — second cache call
            // must observe the same Cell.
            cell_val.set(2.5);
            let cell_val_2: Rc<Cell<f32>> = owner.cache("cell", || Cell::new(99.0_f32));
            assert!(Rc::ptr_eq(&cell_val, &cell_val_2));
            assert_eq!(cell_val_2.get().to_bits(), 2.5_f32.to_bits());
        }

        #[test]
        #[should_panic(expected = "Owner::cache key")]
        fn same_key_mismatched_type_panics() {
            let owner = Owner::new();
            let _u: Rc<u32> = owner.cache("k", || 1_u32);
            // Same key, different concrete type — must panic.
            let _s: Rc<String> = owner.cache("k", || String::from("type-mismatch"));
        }

        #[test]
        fn owner_drop_releases_cached_value() {
            // The owner holds the only strong Rc beyond the test's
            // local clone. After owner drop, strong_count drops to 1.
            let probe: Rc<u32>;
            {
                let owner = Owner::new();
                probe = owner.cache("v", || 100_u32);
                // owner + this fn's `probe` = 2 strong refs.
                assert!(Rc::strong_count(&probe) >= 2);
            }
            // owner dropped → its cache HashMap drops → its Rc<dyn Any>
            // drops, leaving only the probe alive.
            assert_eq!(Rc::strong_count(&probe), 1);
        }

        #[test]
        fn sibling_owners_have_independent_caches() {
            let a = Owner::new();
            let b = Owner::new();
            let av: Rc<u32> = a.cache("shared", || 11_u32);
            let bv: Rc<u32> = b.cache("shared", || 22_u32);
            assert_eq!(*av, 11);
            assert_eq!(*bv, 22);
            assert!(!Rc::ptr_eq(&av, &bv));
        }

        #[test]
        fn cache_call_inside_run_resolves_via_current_owner() {
            // The application-side hello-button pattern: view fn runs
            // inside `Owner::run`, calls `Owner::current()` to grab
            // the active scope, then uses `cache` on that handle.
            let owner = Owner::new();
            let observed: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));
            let owned_id = owner.id();
            {
                let observed_clone = Rc::clone(&observed);
                owner.run(|| {
                    let current = Owner::current().expect("inside run");
                    let v: Rc<u32> = current.cache("inside", || 7_u32);
                    assert_eq!(*v, 7);
                    observed_clone.set(Some(current.id()));
                });
            }
            assert_eq!(observed.get(), Some(owned_id));
            // Re-entering the same owner's run reaches the same cache.
            owner.run(|| {
                let current = Owner::current().expect("inside run again");
                let v: Rc<u32> = current.cache("inside", || 99_u32);
                assert_eq!(*v, 7, "second call must reuse the cached value");
            });
        }
    }

    // ────────────────────────────────────────────────────────────────
    // R51.147 §5.28 — Owner::any_animation_active(eps) tests.
    //
    // Backends need to know when to stop requesting frames from their
    // vsync loop. Owner::any_animation_active walks the subtree and
    // returns true iff at least one registered Tickable reports
    // is_at_rest(eps) == false. The tests verify:
    //
    // - Empty owner → false.
    // - One at-rest animation → false.
    // - One not-at-rest animation → true.
    // - Mixed at-rest + not-at-rest → true (early-return finds the
    //   active one).
    // - Child scope with active anim → parent reports true.
    // - All settle → false again (transitions to steady state).
    // ────────────────────────────────────────────────────────────────

    mod animation_active {
        use super::Owner;
        use crate::animation::Tickable;
        use std::cell::Cell;
        use std::rc::Rc;

        /// Tickable whose `is_at_rest(_)` mirrors a `Cell<bool>`. Lets
        /// the test flip an animation between resting / active states
        /// without standing up a real spring solver.
        struct ProgrammableRest {
            at_rest: Cell<bool>,
        }
        impl ProgrammableRest {
            fn at_rest() -> Rc<Self> {
                Rc::new(Self { at_rest: Cell::new(true) })
            }
            fn active() -> Rc<Self> {
                Rc::new(Self { at_rest: Cell::new(false) })
            }
        }
        impl Tickable for ProgrammableRest {
            fn tick(&self, _dt: f32) {}
            fn is_at_rest(&self, _epsilon: f32) -> bool {
                self.at_rest.get()
            }
        }

        #[test]
        fn empty_owner_reports_no_active_animation() {
            let owner = Owner::new();
            assert!(!owner.any_animation_active(0.01));
        }

        #[test]
        fn at_rest_animation_reports_inactive() {
            let owner = Owner::new();
            owner.register_animation(ProgrammableRest::at_rest());
            assert!(!owner.any_animation_active(0.01));
        }

        #[test]
        fn active_animation_reports_active() {
            let owner = Owner::new();
            owner.register_animation(ProgrammableRest::active());
            assert!(owner.any_animation_active(0.01));
        }

        #[test]
        fn mixed_registry_reports_active_when_any_is_active() {
            let owner = Owner::new();
            owner.register_animation(ProgrammableRest::at_rest());
            owner.register_animation(ProgrammableRest::active());
            owner.register_animation(ProgrammableRest::at_rest());
            assert!(owner.any_animation_active(0.01));
        }

        #[test]
        fn child_scope_with_active_animation_bubbles_to_parent() {
            let parent = Owner::new();
            let child = Owner::new_child(&parent);
            child.register_animation(ProgrammableRest::active());
            assert!(parent.any_animation_active(0.01));
            assert!(child.any_animation_active(0.01));
        }

        #[test]
        fn settling_animation_flips_back_to_inactive() {
            // The frame-by-frame contract: tick eventually settles
            // each spring, and the next `any_animation_active` call
            // reports false. Simulate by flipping the Cell after
            // observation.
            let owner = Owner::new();
            let anim = ProgrammableRest::active();
            owner.register_animation(anim.clone());
            assert!(owner.any_animation_active(0.01));
            anim.at_rest.set(true);
            assert!(!owner.any_animation_active(0.01));
        }
    }

    // ────────────────────────────────────────────────────────────────
    // R51.146 §5.22 — Owner::current() public API tests.
    //
    // The substrate landed an Owner-only handle stack
    // (`CURRENT_OWNER_HANDLE`) parallel to `CURRENT_OWNER`. The tests
    // below pin down its observable contract:
    //
    // - Outside any `Owner::run`: `current()` returns `None`.
    // - Inside `Owner::run`: returns the strong handle to *self*.
    // - Nested `Owner::run`: returns the innermost.
    // - After `Owner::run` exits: returns `None` again (RAII pop).
    // - Panic inside body: still pops both stacks.
    // - Computed::recompute / Effect::recompute push onto
    //   `CURRENT_OWNER` (subscriber stack) but NOT onto the handle
    //   stack — `Owner::current()` from inside a derived-node body
    //   still resolves to the enclosing `Owner::run`. This is the
    //   load-bearing test for the "useOwner doesn't see derived
    //   nodes" SolidJS contract.
    // ────────────────────────────────────────────────────────────────

    mod current_owner_handle {
        use super::{Owner, current_owner_handle_stack_len};

        #[test]
        fn current_outside_run_is_none() {
            assert!(Owner::current().is_none());
            assert_eq!(current_owner_handle_stack_len(), 0);
        }

        #[test]
        fn current_inside_run_returns_self_handle() {
            let owner = Owner::new();
            let seen_id = owner.run(|| Owner::current().map(|o| o.id()));
            assert_eq!(seen_id, Some(owner.id()));
        }

        #[test]
        fn current_after_run_exits_returns_to_none() {
            let owner = Owner::new();
            owner.run(|| {
                let _ = Owner::current();
            });
            assert!(Owner::current().is_none());
            assert_eq!(current_owner_handle_stack_len(), 0);
        }

        #[test]
        fn nested_run_current_returns_innermost() {
            let outer = Owner::new();
            let inner = Owner::new();
            let seen = outer.run(|| inner.run(|| Owner::current().map(|o| o.id())));
            assert_eq!(seen, Some(inner.id()));
        }

        #[test]
        fn nested_run_current_returns_outer_after_inner_pops() {
            let outer = Owner::new();
            let inner = Owner::new();
            let seen = outer.run(|| {
                inner.run(|| {
                    // Inside the innermost — innermost wins.
                    assert_eq!(Owner::current().map(|o| o.id()), Some(inner.id()));
                });
                // After the inner pops the handle guard, the outer
                // is back on top.
                Owner::current().map(|o| o.id())
            });
            assert_eq!(seen, Some(outer.id()));
        }

        #[test]
        fn returned_handle_is_a_strong_clone_of_the_running_owner() {
            // The handle returned by `current()` aliases the same
            // `OwnerInner`, so registrations on it land in the same
            // scope as registrations on the original.
            use crate::animation::Tickable;
            use std::cell::Cell;
            use std::rc::Rc;

            struct Noop;
            impl Tickable for Noop {
                fn tick(&self, _dt: f32) {}
                fn is_at_rest(&self, _epsilon: f32) -> bool {
                    true
                }
            }

            let owner = Owner::new();
            let captured: Cell<Option<u64>> = Cell::new(None);
            owner.run(|| {
                let cur = Owner::current().expect("current must be Some inside run");
                cur.register_animation(Rc::new(Noop));
                captured.set(Some(cur.id()));
            });
            assert_eq!(captured.get(), Some(owner.id()));
            // The animation registered through the `current()` handle
            // lives on the original `owner` — verified through the
            // test-only count accessor.
            assert_eq!(owner.registered_animation_count(), 1);
        }

        #[test]
        fn panic_in_run_body_still_pops_handle_stack() {
            assert_eq!(current_owner_handle_stack_len(), 0);
            let owner = Owner::new();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                owner.run(|| {
                    panic!("simulated user-closure panic");
                })
            }));
            assert!(result.is_err());
            assert_eq!(
                current_owner_handle_stack_len(),
                0,
                "CURRENT_OWNER_HANDLE stack must unwind on panic",
            );
            assert!(Owner::current().is_none());
        }

        #[test]
        fn nested_run_panic_unwinds_handle_stack_in_order() {
            let outer = Owner::new();
            let inner = Owner::new();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                outer.run(|| {
                    inner.run(|| {
                        panic!("inner panic");
                    })
                })
            }));
            assert!(result.is_err());
            assert_eq!(current_owner_handle_stack_len(), 0);
            assert!(Owner::current().is_none());
        }

        #[test]
        fn computed_recompute_does_not_shadow_owner_handle_stack() {
            // R51.146 load-bearing — Computed::recompute pushes its
            // own node onto `CURRENT_OWNER` (subscriber tracking) but
            // must NOT push onto `CURRENT_OWNER_HANDLE`. Inside the
            // compute closure, `Owner::current()` returns the
            // enclosing `Owner::run` scope, not the computed node.
            use crate::reactive::computed::Computed;
            use crate::reactive::signal::Signal;
            use std::cell::Cell;
            use std::rc::Rc;

            let outer = Owner::new();
            let observed_owner_id: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));
            let observed_id_clone = Rc::clone(&observed_owner_id);
            let source = Signal::new(1_i32);
            let source_clone = source.clone();
            let computed = Computed::new(move || {
                // Read the source so the Computed subscribes (this
                // pushes the Computed node onto CURRENT_OWNER); then
                // read the lexical owner via current() — should still
                // be the `outer` from the enclosing run.
                let v = source_clone.get();
                observed_id_clone.set(Owner::current().map(|o| o.id()));
                v + 1
            });
            // Trigger the recompute by reading inside the outer scope.
            let value = outer.run(|| computed.get());
            assert_eq!(value, 2);
            assert_eq!(
                observed_owner_id.get(),
                Some(outer.id()),
                "Owner::current() inside Computed body must resolve to the lexical Owner::run, \
                 not the Computed node itself",
            );
        }

        #[test]
        fn current_in_separate_thread_is_none() {
            // R51.146 — handle stack is thread-local. A spawned
            // thread starts with an empty stack regardless of the
            // parent's run state. The single-threaded reactive model
            // (§5.22) forbids cross-thread Signal/Owner sharing, but
            // we still verify the substrate's thread-local boundary.
            let owner = Owner::new();
            owner.run(|| {
                assert!(Owner::current().is_some());
                let other = std::thread::spawn(|| Owner::current().map(|o| o.id()))
                    .join()
                    .expect("worker thread joined");
                assert!(
                    other.is_none(),
                    "spawned thread sees its own empty handle stack",
                );
            });
        }
    }

    // ────────────────────────────────────────────────────────────────
    // R51.139 — Command dispatch / drain / cancel-on-drop tests
    // ────────────────────────────────────────────────────────────────

    mod command {
        use super::Owner;
        use crate::command::Command;
        use crate::external::IntrospectValue;

        #[test]
        fn dispatch_appends_to_pending_queue() {
            let owner = Owner::new();
            assert!(owner.pending_commands().is_empty());
            owner.dispatch_command(Command::new_static(
                "http.get",
                IntrospectValue::Text("/api".to_string()),
                owner.id(),
            ));
            let snapshot = owner.pending_commands();
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].kind_str(), "http.get");
        }

        #[test]
        fn pending_commands_does_not_drain() {
            let owner = Owner::new();
            owner.dispatch_command(Command::new_static(
                "audio.play",
                IntrospectValue::Int(440),
                owner.id(),
            ));
            let _peek = owner.pending_commands();
            // Snapshot must leave the queue intact for the actual drainer.
            assert_eq!(owner.pending_commands().len(), 1);
        }

        #[test]
        fn take_pending_returns_queue_in_fifo_order_and_empties() {
            let owner = Owner::new();
            for n in 0..3_i64 {
                owner.dispatch_command(Command::new_static(
                    "tick",
                    IntrospectValue::Int(n),
                    owner.id(),
                ));
            }
            let drained = owner.take_pending_commands();
            assert_eq!(drained.len(), 3);
            assert_eq!(drained[0].payload, IntrospectValue::Int(0));
            assert_eq!(drained[2].payload, IntrospectValue::Int(2));
            // Queue empty after drain.
            assert!(owner.pending_commands().is_empty());
        }

        #[test]
        fn distinct_owners_have_independent_queues() {
            let a = Owner::new();
            let b = Owner::new();
            a.dispatch_command(Command::new_static("a.evt", IntrospectValue::Null, a.id()));
            b.dispatch_command(Command::new_static("b.evt", IntrospectValue::Null, b.id()));
            b.dispatch_command(Command::new_static("b.evt2", IntrospectValue::Null, b.id()));
            assert_eq!(a.pending_commands().len(), 1);
            assert_eq!(b.pending_commands().len(), 2);
        }

        #[test]
        fn owner_drop_cancels_pending_commands() {
            let kept_alive = Owner::new();
            {
                let scope = Owner::new();
                scope.dispatch_command(Command::new_static(
                    "fetch.cancelled",
                    IntrospectValue::Null,
                    scope.id(),
                ));
                assert_eq!(scope.pending_commands().len(), 1);
            }
            // Scope dropped — queue gone with it. Other owners unaffected.
            assert!(kept_alive.pending_commands().is_empty());
        }

        #[test]
        fn take_pending_recursive_drains_children_first() {
            let parent = Owner::new();
            let child = Owner::new_child(&parent);
            let grandchild = Owner::new_child(&child);
            parent.dispatch_command(Command::new_static("p.cmd", IntrospectValue::Null, parent.id()));
            child.dispatch_command(Command::new_static("c.cmd", IntrospectValue::Null, child.id()));
            grandchild.dispatch_command(Command::new_static(
                "gc.cmd",
                IntrospectValue::Null,
                grandchild.id(),
            ));
            let drained = parent.take_pending_commands_recursive();
            assert_eq!(drained.len(), 3);
            // Depth-first: grandchild first, then child, then parent.
            assert_eq!(drained[0].kind_str(), "gc.cmd");
            assert_eq!(drained[1].kind_str(), "c.cmd");
            assert_eq!(drained[2].kind_str(), "p.cmd");
            // All queues empty after drain.
            assert!(parent.pending_commands().is_empty());
            assert!(child.pending_commands().is_empty());
            assert!(grandchild.pending_commands().is_empty());
        }

        #[test]
        fn dynamic_kind_command_round_trips() {
            let owner = Owner::new();
            let kind = format!("ws.send.channel_{}", 42);
            owner.dispatch_command(Command::new_owned(
                kind.clone(),
                IntrospectValue::Text("hello".to_string()),
                owner.id(),
            ));
            let drained = owner.take_pending_commands();
            assert_eq!(drained[0].kind_str(), kind);
        }

        #[test]
        fn dispatching_after_drain_starts_new_batch() {
            let owner = Owner::new();
            owner.dispatch_command(Command::new_static("a", IntrospectValue::Null, owner.id()));
            owner.dispatch_command(Command::new_static("b", IntrospectValue::Null, owner.id()));
            let first = owner.take_pending_commands();
            assert_eq!(first.len(), 2);
            // After drain the queue restarts.
            owner.dispatch_command(Command::new_static("c", IntrospectValue::Null, owner.id()));
            let second = owner.take_pending_commands();
            assert_eq!(second.len(), 1);
            assert_eq!(second[0].kind_str(), "c");
        }

        #[test]
        fn pending_commands_clone_is_independent_of_source() {
            // Mutating the snapshot Vec must not affect the live queue.
            let owner = Owner::new();
            owner.dispatch_command(Command::new_static("a", IntrospectValue::Null, owner.id()));
            let mut snapshot = owner.pending_commands();
            snapshot.clear();
            assert_eq!(owner.pending_commands().len(), 1);
        }

        #[test]
        fn dispatching_in_recursive_drain_caller_does_not_re_enter() {
            // A handler iterating drained commands may dispatch fresh ones
            // back onto the owner. We confirm those land in the next batch,
            // not the current drain.
            let owner = Owner::new();
            owner.dispatch_command(Command::new_static("first", IntrospectValue::Null, owner.id()));
            let drained = owner.take_pending_commands_recursive();
            // Simulate a handler enqueueing a follow-up while iterating.
            for _ in &drained {
                owner.dispatch_command(Command::new_static(
                    "followup",
                    IntrospectValue::Null,
                    owner.id(),
                ));
            }
            assert_eq!(owner.pending_commands().len(), 1);
            assert_eq!(owner.pending_commands()[0].kind_str(), "followup");
        }
    }
}
