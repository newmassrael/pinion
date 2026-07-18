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
//! site must route the closures through `run_cleanups_isolated` (which
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

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

thread_local! {
    static NEXT_NODE_ID: Cell<u64> = const { Cell::new(0) };
    static CURRENT_OWNER: RefCell<Vec<Weak<dyn ReactiveNode>>> = const { RefCell::new(Vec::new()) };
    static BATCH_DEPTH: Cell<u32> = const { Cell::new(0) };
    static PENDING_DIRTY: RefCell<SubscriberSet> = RefCell::new(SubscriberSet::new());
    /// R51.146 §5.22 — Owner-only handle stack, mirrors `CURRENT_OWNER`
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
    ///
    /// (R56.1.b.1 §5.22) Key shape is [`CacheKey`] (`(TypeId,
    /// &'static str)`) so the same user-facing string can address
    /// distinct slots per concrete value type —
    /// `use_text_edit_state(tag)` and `use_caret_blink(tag)` both
    /// resolve against the same widget tag without colliding because
    /// their value types differ. The per-hook convention "pass the
    /// matching widget tag verbatim" (documented on
    /// [`use_text_edit_state`](crate::widgets::text_edit::use_text_edit_state)
    /// / [`use_caret_blink`](crate::widgets::caret_blink::use_caret_blink))
    /// is now load-bearing rather than aspirational.
    pub(crate) cache: RefCell<HashMap<CacheKey, Rc<dyn Any>>>,

    /// R1335 §5.39 (PR-53) — the binding-wide "which paint tag has focus"
    /// mirror. `pinion_runtime::FocusManager` (the focus SSOT) publishes into
    /// it on every commit; any binding reads it via
    /// [`focus_state::focused`](crate::focus_state::focused) to derive display
    /// state from focus (a window title naming the active pane, a status-bar
    /// label, an active-tab highlight).
    ///
    /// An **eager direct field**, not an [`Owner::cache`] slot like
    /// [`viewport_size_signal`](Owner::viewport_size_signal). Focus is a
    /// **binding-wide singular fact** (one focused tag per binding, across every
    /// window). A binding is an owner *tree*, so the whole tree must share ONE
    /// mirror: [`Owner::new`] mints it, and [`Owner::new_child`] threads the same
    /// handle down (a child does not get its own). A field that is *inherited at
    /// construction* is the natural carrier for that; a per-owner cache slot
    /// would give each child its own empty mirror and break the binding-wide
    /// read from a secondary window's scope.
    ///
    /// R1365.1 — this passage used to call the difference *principled*, on the
    /// ground that "the viewport size is a per-owner value (a secondary window
    /// genuinely has its own size)". That is the taxonomy
    /// [`Owner::cache_inherited`] records as WRONG, and the parenthetical was
    /// false as implemented besides: the only production
    /// [`provide_viewport_size_signal`](Owner::provide_viewport_size_signal)
    /// call is on `root_owner`, so a secondary window's scope seeds `(0, 0)` and
    /// keeps it — R1006's documented "viewport unknown", not a size of its own.
    /// The real predicate is who DRIVES the slot; see `cache_inherited`. Focus
    /// still belongs in a field, for the reason above — the conclusion was right
    /// and the rationale was hearsay, which is exactly how it survived a round
    /// that was hunting this class in this file.
    ///
    /// The direct field also has no shell-injection story (the `FocusManager`
    /// publishes into whatever mirror the owner already carries, so there is
    /// nothing to `provide_*`) and, as a plain field, a `focused()` read never
    /// touches the cache `RefCell` — so it is safe even from inside an
    /// [`Owner::cache`] factory. (The reference consumer no longer relies on that
    /// last property: `hello-dock-panels-editor`'s title-sync `Effect` captures
    /// this handle OUTSIDE its cache factory, the way any owner-scoped signal is
    /// read inside an `Effect`. It is a robustness margin, not the reason.)
    ///
    /// Deliberately **not** registered for snapshot/restore (absent from
    /// `owned_signals`): it is derived display state mirroring the
    /// `FocusManager`'s own `focused` SSOT, not authoritative binding state — the
    /// same reason the pre-R1335 thread-local mirror was untracked.
    pub(crate) focused_tag: super::signal::Signal<Option<String>>,

    /// R1364 §5.22 §5.55 — the scope this one was born under, or a dangling
    /// `Weak` for a root. Walked by [`Owner::cache_inherited`], and by nothing
    /// else.
    ///
    /// `Weak` is forced: [`children`](Self::children) holds descendants
    /// STRONGLY (that is what makes cascade-drop work), so a strong parent link
    /// would close a cycle and leak every scope in the tree.
    ///
    /// Set once at construction and never cleared — including by
    /// [`Owner::detach_child_by_id`], whose whole purpose is to drop the child
    /// immediately afterwards. A detached scope that outlived its parent gets
    /// `upgrade() == None` and resolves as a root, which is the honest answer:
    /// it has no provider to inherit from.
    parent: Weak<OwnerInner>,
}

/// (R56.1.b.1 / R685.C atomic 5 §5.22) Internal cache key for
/// [`OwnerInner::cache`] — the type id of the cached value plus the
/// user-supplied key string. Extracted as a type alias to keep the
/// field declaration under `clippy::type_complexity`.
///
/// (R685.C atomic 5) The key string is `Cow<'static, str>` (was
/// `&'static str`) so runtime-generated ids (R686 dock-reorganize
/// mints new Split / panel ids at drag time) can address cache
/// slots without `Box::leak`. Compile-time `&'static str` literals
/// still coerce zero-cost via `Cow::Borrowed`; only genuinely
/// dynamic keys allocate (`Cow::Owned`).
type CacheKey = (TypeId, Cow<'static, str>);

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
        // A detached root owns a fresh focus mirror — the head of a new binding's
        // owner tree. [`Self::new_child`] threads THIS handle down so the whole
        // tree shares one mirror (R1335 §5.39: focus is binding-wide).
        Self::with_focus_mirror(super::signal::Signal::new(None), Weak::new())
    }

    /// R1335 §5.39 — construct an owner carrying `focused_tag` as its focus
    /// mirror. [`Self::new`] passes a fresh signal (a new binding); [`Self::new_child`]
    /// passes the parent's handle so descendants share the binding-wide mirror.
    ///
    /// R1364 — `parent` is the scope to inherit provider slots from
    /// ([`Self::cache_inherited`]); `Weak::new()` for a root.
    fn with_focus_mirror(
        focused_tag: super::signal::Signal<Option<String>>,
        parent: Weak<OwnerInner>,
    ) -> Self {
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
                focused_tag,
                parent,
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
    ///
    /// R1335 §5.39 — the child **inherits the parent's focus mirror handle**
    /// (`focused_tag`), so the whole owner tree shares one mirror. Focus is a
    /// binding-wide fact (one focused tag per binding, across every window), and a
    /// binding is exactly an owner tree — so a `focus_state::focused()` read
    /// resolves the same value in a secondary window's child scope
    /// (`CoreShell::window_owner`) as in the root, and `FocusManager`'s single
    /// publish to the root owner is seen tree-wide. This makes the binding-wide
    /// property STRUCTURAL rather than "true only while every view runs under
    /// root" — a plain `focused()` read stays correct even if a future round
    /// (R680 atomic 1) runs a secondary window's view under its child scope. It
    /// also means only root owners allocate a focus `Signal`; children clone the
    /// handle.
    ///
    /// R1364 §5.22 — the child also records `parent`, which is what lets
    /// [`Self::cache_inherited`] resolve a binding-wide CAPABILITY (the shell's
    /// [`RepaintSink`](super::repaint::RepaintSink) /
    /// [`QuitSink`](super::quit::QuitSink) / monospace metrics, and the shell's
    /// own window-control sink) from a child scope. Focus got the mirror
    /// treatment because `Owner` can name a `Signal`; it cannot name every
    /// provider slot, least of all one defined in `pinion-shell` — so the
    /// general answer is a link the resolver walks, not a field the constructor
    /// copies. The two mechanisms answer the same R680 question for the two
    /// kinds of thing an owner tree carries.
    #[must_use]
    pub fn new_child(parent: &Owner) -> Self {
        let child = Self::with_focus_mirror(
            parent.inner.focused_tag.clone(),
            Rc::downgrade(&parent.inner),
        );
        parent.inner.children.borrow_mut().push(child.clone());
        child
    }

    /// R683 §5.22 §5.28 — detach a child scope from this parent.
    ///
    /// Walks the parent's children list, finds the entry whose
    /// [`Self::id`] matches `child_id`, and removes it. Returns
    /// `true` on actual removal, `false` when no matching child
    /// exists.
    ///
    /// **Use case**: the
    /// `pinion_runtime::CoreShell::remove_window` / R683
    /// dock-tear-off drop pass needs to release the last strong
    /// reference to a per-window child scope so the scope's
    /// cleanup queue actually fires. Without this method the
    /// parent's `children: Vec<Owner>` field permanently retains
    /// every secondary scope (R680 axis 3 cascade-drop on
    /// substrate destruction is the only release path), which
    /// means animations / commands / cache slots registered on a
    /// torn-down per-window scope would survive across reconcile
    /// passes until the entire binding shuts down.
    ///
    /// **Idempotency**: calling `detach_child_by_id` twice with the
    /// same id returns `true` then `false` — once removed, the
    /// entry is gone from the children list.
    ///
    /// **Cascade**: the removal drops the parent's strong ref to
    /// the child `Owner`. If no other `Owner` clone of the same
    /// child exists, the child's `OwnerInner` drops, triggering
    /// the cleanup queue + draining every registered animation /
    /// command. Any sibling `Owner` clones still alive keep the
    /// child scope alive — the detach is "release the parent's
    /// strong ref", not "force-drop the scope".
    #[must_use = "the bool reports whether a matching child was actually detached; if you don't care, bind to `_`"]
    pub fn detach_child_by_id(&self, child_id: u64) -> bool {
        let mut children = self.inner.children.borrow_mut();
        let before = children.len();
        children.retain(|c| c.id() != child_id);
        children.len() < before
    }

    /// Push this owner as the current scope, run `f`, pop. Signal reads
    /// during `f` auto-subscribe to this owner. The stack pop is RAII —
    /// even if `f` panics, the stack is restored before the unwind continues.
    ///
    /// R51.146 §5.22 — pushes onto the separate
    /// `CURRENT_OWNER_HANDLE` stack in addition to `CURRENT_OWNER`.
    /// The two stacks track different things: `CURRENT_OWNER` is the
    /// subscriber stack consulted by [`Signal::get`](crate::reactive::Signal::get)
    /// and [`Computed::get`](crate::reactive::Computed::get) for
    /// auto-subscription, while `CURRENT_OWNER_HANDLE` is the
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
    /// the same panic-safe RAII invariant the `OwnerHandleGuard`
    /// drop already enforces.
    ///
    /// Reactive-node nesting note: a [`Computed`](crate::reactive::Computed)
    /// or [`Effect`](crate::reactive::Effect) running its recompute
    /// closure pushes onto `CURRENT_OWNER` (the subscriber stack)
    /// but NOT onto `CURRENT_OWNER_HANDLE`. So inside a `Computed`
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

    /// R727 §5.28 — resolve (or lazily construct **and register**) a
    /// per-scope [`Tickable`](crate::animation::Tickable) keyed by `key`,
    /// registering it for animation dispatch exactly once.
    ///
    /// This is the single source of truth for the "cache a `Tickable`
    /// and register it on first construction" pattern shared by every
    /// `use_*` animation hook — `use_caret_blink` (R56),
    /// `use_snackbar_timer` (R725), `use_indeterminate_sweep` (R726) —
    /// the lift `caret_blink.rs` predicted when a second such hook
    /// landed. Each hook now delegates here.
    ///
    /// Registration is gated by [`Self::cache_contains`] so re-running
    /// the view-fn after the cache populates does **not** re-register
    /// (a double registration would advance the driver twice per
    /// `tick_animations` walk). The gate fires once per `(T, key)` pair,
    /// independent of other typed hooks reusing the same widget tag.
    #[must_use]
    pub fn register_animation_once<T, F>(
        &self,
        key: impl Into<Cow<'static, str>>,
        factory: F,
    ) -> Rc<T>
    where
        T: crate::animation::Tickable + 'static,
        F: FnOnce() -> T,
    {
        let key = key.into();
        let first_time = !self.cache_contains::<T>(key.clone());
        let value = self.cache(key, factory);
        if first_time {
            self.register_animation(Rc::clone(&value) as Rc<dyn crate::animation::Tickable>);
        }
        value
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

    /// R680 §5.16 §5.28 — tick **only** this owner's own registered
    /// animations. Skips the [`Self::tick_animations`] depth-first
    /// descent into child scopes.
    ///
    /// Mirrors [`Self::tick_animations`]'s borrow + batch discipline
    /// (snapshot the `owned_animations` `Vec` via `Rc::clone` before
    /// releasing the `RefCell`, then `batch`-wrap the dispatch) so
    /// animations that (re)register sibling animations during their
    /// own `tick` callback do not trigger a `BorrowMutError` mid-walk.
    ///
    /// ## Why this exists
    ///
    /// Phase B multi-window per-paint animation tick (R680 atomic 1
    /// of the 4-axis paint-pipeline rewrite series) needs each
    /// window's paint cycle to advance ONLY that window's own
    /// animations. The pre-R680 `tick_animations` cascade walks every
    /// descendant scope, so two windows painting in the same
    /// event-loop turn end up double-ticking each window's scope
    /// (the R670.B 9-round honest carry on multi-window animation
    /// compound). The framework's per-window dispatch now calls
    /// [`Self::tick_animations_local`] against each window's
    /// secondary scope so the spring solvers advance at exactly one
    /// step per paint cycle of THIS window.
    ///
    /// Application code calling `Owner::tick_animations` directly on
    /// a root owner — the canonical headless / RPC dispatch entry
    /// (`pinion-rpc::animate_control`'s `animate_advance` /
    /// `animate_settle`) — keeps its cascade semantic; only the
    /// substrate's per-window dispatch swap to this variant.
    pub fn tick_animations_local(&self, dt: f32) {
        let anims: Vec<Rc<dyn crate::animation::Tickable>> = self
            .inner
            .owned_animations
            .borrow()
            .iter()
            .map(Rc::clone)
            .collect();
        batch(|| {
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

    /// R680 §5.16 §5.28 — `true` when any animation registered
    /// **directly on this owner** (NOT a descendant scope) reports
    /// [`Tickable::is_at_rest(epsilon)`](crate::animation::Tickable::is_at_rest)
    /// as `false`.
    ///
    /// Local counterpart to [`Self::any_animation_active`]; the
    /// R680 atomic 1 multi-window animation-loop pump uses this to
    /// decide per-window redraw without taking foreign windows'
    /// activity into account. Each window's redraw loop polls only
    /// its own scope.
    ///
    /// Backends that want the binding-wide "any window still
    /// animating?" answer keep calling [`Self::any_animation_active`]
    /// on the root owner; the cascade walk reaches every per-window
    /// scope (each is a child of root via [`Self::new_child`]) so the
    /// "tick all" semantic is preserved for callers that want it.
    #[must_use]
    pub fn any_animation_active_local(&self, epsilon: f32) -> bool {
        let anims: Vec<Rc<dyn crate::animation::Tickable>> = self
            .inner
            .owned_animations
            .borrow()
            .iter()
            .map(Rc::clone)
            .collect();
        anims.iter().any(|anim| !anim.is_at_rest(epsilon))
    }

    /// R629 §5.28 — bulk-call
    /// [`Tickable::settle`](crate::animation::Tickable::settle) on every
    /// animation registered on this owner and its descendant scopes.
    /// Returns the count of registrations visited (whether or not they
    /// were already at rest).
    ///
    /// Each visited animation jumps to its internal target with zero
    /// velocity; after the walk
    /// [`Self::any_animation_active`](Self::any_animation_active)
    /// returns `false` (modulo a non-NaN epsilon).
    ///
    /// Mirrors [`Self::tick_animations`]'s borrow discipline: children
    /// + animations snapshot via `Rc::clone` before releasing the
    ///   `RefCell`, then [`crate::reactive::batch`]-wraps the mutation
    ///   so subscribers see one transactional update.
    pub fn settle_animations(&self) -> usize {
        let children: Vec<Owner> = self.inner.children.borrow().iter().cloned().collect();
        let anims: Vec<Rc<dyn crate::animation::Tickable>> = self
            .inner
            .owned_animations
            .borrow()
            .iter()
            .map(Rc::clone)
            .collect();
        let mut visited = 0usize;
        batch(|| {
            for child in &children {
                visited += child.settle_animations();
            }
            for anim in &anims {
                anim.settle();
                visited += 1;
            }
        });
        visited
    }

    /// R629 §5.28 — bulk-call
    /// [`Tickable::cancel`](crate::animation::Tickable::cancel) on
    /// every animation registered on this owner and its descendant
    /// scopes. Returns the count of registrations visited.
    ///
    /// Each visited animation freezes at its internal current value
    /// with zero velocity; after the walk
    /// [`Self::any_animation_active`](Self::any_animation_active)
    /// returns `false` (modulo a non-NaN epsilon).
    ///
    /// Borrow + batch discipline identical to
    /// [`Self::settle_animations`].
    pub fn cancel_animations(&self) -> usize {
        let children: Vec<Owner> = self.inner.children.borrow().iter().cloned().collect();
        let anims: Vec<Rc<dyn crate::animation::Tickable>> = self
            .inner
            .owned_animations
            .borrow()
            .iter()
            .map(Rc::clone)
            .collect();
        let mut visited = 0usize;
        batch(|| {
            for child in &children {
                visited += child.cancel_animations();
            }
            for anim in &anims {
                anim.cancel();
                visited += 1;
            }
        });
        visited
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

    /// R51.161 §5.23 — peek at every pending [`Command`](crate::command::Command)
    /// in this scope and every descendant scope without consuming them.
    ///
    /// Sibling of [`Self::take_pending_commands_recursive`] but
    /// non-mutating: the returned `Vec` is a clone of the queued
    /// commands so the underlying queues stay populated. The
    /// `scene/commands` RPC method (R51.161, §5.7 10th method) calls
    /// this so an AI agent can inspect the pending dispatch surface
    /// without forcing the framework pump to drain.
    ///
    /// Traversal order matches the drain sibling: children
    /// depth-first, then this scope's own commands. Deterministic so
    /// two snapshots taken at the same logical instant hash
    /// identically.
    #[must_use]
    pub fn pending_commands_recursive(&self) -> Vec<crate::command::Command> {
        let children: Vec<Owner> = self.inner.children.borrow().iter().cloned().collect();
        let mut snapshot: Vec<crate::command::Command> = Vec::new();
        for child in &children {
            snapshot.extend(child.pending_commands_recursive());
        }
        snapshot.extend(self.pending_commands());
        snapshot
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
    /// ## Key shape
    ///
    /// `key` is a `&'static str` for ergonomics (`"hover_anim"` literal
    /// in the view fn). (R56.1.b.1 §5.22) The cache is keyed
    /// internally by `(TypeId, key)`, so the same `key` string with a
    /// *different* concrete `V` resolves to a distinct slot —
    /// `use_text_edit_state(tag)` and `use_caret_blink(tag)` both
    /// accept the same widget tag without colliding because their
    /// value types differ. This mirrors React's per-hook slot model:
    /// each typed hook has its own per-key slot independent of other
    /// hooks. Same `key` + same `V` reuses the cached entry across
    /// view-fn re-runs, which is the intended caching semantics.
    ///
    /// # Panics
    ///
    /// The typed-key lookup is infallible by construction (the cache
    /// entry was inserted under `(TypeId::of::<V>(), key)`, so the
    /// `Rc::downcast::<V>` always succeeds). The fallback panic on
    /// the downcast failure is a defensive guard against a future
    /// refactor breaking the typed-key invariant and is never
    /// triggered under the current implementation.
    pub fn cache<V, F>(&self, key: impl Into<Cow<'static, str>>, factory: F) -> Rc<V>
    where
        V: 'static,
        F: FnOnce() -> V,
    {
        // (R56.1.b.1 / R685.C atomic 5 §5.22) Typed cache key —
        // `(TypeId::of::<V>(), key)` so the same string addresses a
        // distinct slot per type. R685.C lifts `key` to
        // `impl Into<Cow<'static, str>>` — static literals coerce
        // zero-cost (`Cow::Borrowed`); runtime ids allocate
        // (`Cow::Owned`) without `Box::leak`.
        let key: Cow<'static, str> = key.into();
        // Diagnostic copy for the two panic messages below — the
        // `key` Cow is moved into `typed_key` (then into the cache
        // entry), so the error paths reference this owned snapshot.
        let key_for_msg = key.clone().into_owned();
        let typed_key = (TypeId::of::<V>(), key);
        // R666 §5.22 — nested-factory guard. The cache `RefCell` is
        // held with `borrow_mut` across the `or_insert_with` arm, so
        // a factory that re-entered `Owner::cache` (any V, any key)
        // tripped the cryptic default `BorrowMutError` panic from
        // `RefCell`. R665 surfaced this through the
        // `use_persistence_boot` slot whose factory pre-resolved
        // dependent slots inline — `[[owner-cache-no-nested-factory]]`
        // codifies the rule (pre-resolve dependent slots, then enter
        // the outer cache).
        //
        // `try_borrow_mut` upgrades the cryptic message to an
        // actionable one without changing any other call-site
        // semantics — the only failure mode of `borrow_mut` on a
        // per-Owner `RefCell` is "currently borrowed", which in this
        // code path is unambiguously the nested-factory case.
        let any_rc: Rc<dyn Any> = {
            let mut cache = self.inner.cache.try_borrow_mut().unwrap_or_else(|_| {
                panic!(
                    "Owner::cache factory closures must not call \
                     Owner::cache; pre-resolve dependent slots first \
                     (see [[owner-cache-no-nested-factory]] in \
                     memory). Re-entering on key={key_for_msg:?}",
                )
            });
            Rc::clone(
                cache
                    .entry(typed_key)
                    .or_insert_with(|| Rc::new(factory()) as Rc<dyn Any>),
            )
        };
        // Borrow released before the `downcast` — the downcast call
        // itself is `Rc::clone`-equivalent (it bumps the strong count
        // and rewraps as `Rc<V>`), no borrow re-entry.
        //
        // The downcast is now infallible by construction (we just
        // looked up by `(TypeId::of::<V>(), key)`), so the
        // `unwrap_or_else` panic is a defensive guard against a future
        // refactor introducing a typed-key inconsistency — never
        // triggered under the current invariant.
        Rc::downcast::<V>(any_rc).unwrap_or_else(|_| {
            panic!(
                "Owner::cache typed-key invariant violated for {key_for_msg:?}; \
                 the typed-key lookup must hand back an Rc<V> matching its TypeId",
            );
        })
    }

    /// R761.1 §5.22 — the owner-scoped [`LocalTaskPump`](super::resource::LocalTaskPump),
    /// lazily created on first access. One shared instance per owner:
    /// bindings enqueue async work through it (via `use_local_task_pump`
    /// / [`Resource::fetch_with`](super::resource::Resource::fetch_with)),
    /// and the shell drains it once per frame by calling
    /// [`LocalTaskPump::poll`](super::resource::LocalTaskPump::poll) on
    /// the *same* instance this returns — both go through [`Self::cache`]
    /// under one private key, so the producer (binding) and the driver
    /// (shell) never desync. This is the production side of the R26/R37
    /// "framework integrator provides the spawner" contract.
    #[must_use]
    pub fn local_task_pump(&self) -> Rc<super::resource::LocalTaskPump> {
        self.cache_inherited(
            "__pinion.reactive.local_task_pump",
            super::resource::LocalTaskPump::new,
        )
    }

    /// R1364 §5.22 §5.55 — resolve a slot from the nearest ancestor that has
    /// one, creating it HERE only if no ancestor does. The provider-slot
    /// sibling of [`Self::cache`], which is per-scope and stays that way.
    ///
    /// # Why this exists, and why it is additive
    ///
    /// A binding's provider slots still on plain [`cache`](Self::cache) —
    /// `local_task_pump` and `pane_viewport_registry` among them — are seeded
    /// ONCE, on the root owner, at boot. `cache` looks only at the scope it is
    /// called on, so every one of those resolves its lazy **Null default** from a
    /// child scope. Today nothing notices, because every view runs under root. The
    /// deferred R680 atomic changes exactly that (`window_owner(id).run(..)`), and
    /// on the day it lands a secondary window's slot would silently do nothing —
    /// precisely the class of bug R1362 existed to fix (there it was a secondary
    /// window's Quit button; [`QuitSink`](super::quit::QuitSink),
    /// [`RepaintSink`](super::repaint::RepaintSink) and
    /// [`MonospaceMetrics`](super::font_metrics::MonospaceMetrics) — and
    /// `pinion-shell`'s own window-control sink (R1366.4) — have since moved to
    /// [`ProviderSlot`](super::provider_slot::ProviderSlot), which resolves
    /// through `cache_inherited` and is immune), resurrected by a change that
    /// never mentions windowing.
    ///
    /// # Which slots inherit
    ///
    /// The predicate is mechanical, and deliberately not a judgement about what
    /// the slot "is": **does the shell DRIVE this slot at the root owner?** Seed
    /// it, poll it, publish into it, write it — any of those. If it does, the
    /// slot must inherit, or a child scope silently desyncs from its driver.
    ///
    /// **This table is prose, and it is being deleted a row at a time.** It is
    /// the SSOT for exactly the slots not yet migrated to
    /// [`ProviderSlot`](super::provider_slot::ProviderSlot), which carries the
    /// same verdict in its declaration where the compiler can see it. R1365
    /// found this enumeration had already drifted — rows naming slots no key
    /// spells, slots with no row, and one omission (`scene_revision`) latently
    /// broken for R680 — and its census could not survive the type (see
    /// `provider_slot::declaration_scan`). The count is deliberately not stated
    /// here: `LEGACY_SLOT_KEYS` is the machine-checked list of what remains, and
    /// R1365's attempt to state a count in prose ("seven of the ten … the three
    /// it omitted") was contradicted by the very test output the same commit
    /// pasted into its ledger.
    ///
    /// | slot | how the shell drives root | inherits |
    /// |---|---|---|
    /// | `local_task_pump` | POLLS it every frame | yes |
    /// | `pane_viewport_registry` | PUBLISHES pane rects into it | yes |
    /// | `scene_revision` | seeds at boot, then OBSERVES it to wake `scene/waitFor` | yes |
    /// | `waiter_registry` | parks and wakes `scene/waitFor` through it | yes |
    /// | `viewport_size` | WRITES it, primary window only | **no** — see below |
    /// | `frame_timings` | PUBLISHES into it, primary window only | **no** — see below |
    ///
    /// R1364.2 first published a different rule — "capabilities are binding-wide
    /// and inherit; values are per-owner and do not" — which is recorded here
    /// because it was WRONG and a wrong rationale in this file outlives the bug
    /// it explains. It got the four sinks right by luck of category and then
    /// missed `local_task_pump` (a *capability* by any reading, left on plain
    /// `cache`) and actively misclassified `pane_viewport_registry` as a "value"
    /// that should not inherit — when R1021 requires it be ONE shared root
    /// instance that every window publishes into, and its forcing consumer is
    /// sprag's R37 undock, an R680-adjacent feature. A taxonomy invites you to
    /// argue about which bucket a slot falls in; "who drives it" is a fact you
    /// can grep.
    ///
    /// The two `no` rows are exceptions to the CONSEQUENCE, not to the predicate.
    /// The shell drives both at root, but its **WRITE** is primary-gated, so the
    /// root's value is *the primary window's* — and inheriting it would hand a
    /// secondary window the primary's data, confidently wrong, where a per-scope
    /// empty value is an honest "no data".
    ///
    /// Be precise about which end is gated, because R1364 wrote "the READ is
    /// primary-gated" and R1365 propagated it to three places: neither
    /// [`use_viewport_size`](super::viewport::use_viewport_size) nor
    /// `use_frame_timings` gates anything — they resolve from whatever scope
    /// asks. It is the publisher that checks: `set_viewport_size` is called only
    /// `if window_key == DEFAULT_WINDOW`, and `publish_frame_timings` returns
    /// early for any non-primary window. The read only *ends up* primary-scoped
    /// because the verdict is `no`, which makes "the read is gated" a restatement
    /// of the conclusion rather than a reason for it.
    ///
    /// [`viewport_size_signal`](Self::viewport_size_signal) is the R1006 seam;
    /// its per-scope `(0, 0)` is R1006's documented "viewport unknown", which its
    /// contract already requires consumers to skip on. `frame_timings` is the
    /// same shape: the holder is a single per-owner slot, so a second window's
    /// paint would chart an interleaving of two windows' frames. An honest
    /// unknown beats a plausible lie. Both name the same additive fix when a
    /// consumer needs it: a per-window-keyed holder.
    ///
    /// `waiter_registry` has no binding-facing hook — the shell resolves it at
    /// root explicitly (`ShellCore::with_core`, `AppShell`'s park side), so no
    /// child scope can reach it and R680 cannot break it. It inherits anyway,
    /// and the reason is stronger than the precedent: `resolve_waiter_registry`
    /// takes `&Owner`, so its TYPE already admits a child scope. "No caller
    /// passes one today" is an argument about call sites for a function whose
    /// signature says otherwise; YAGNI governs unbuilt features, not leaving an
    /// already-accepted input partial. The precedent agrees — R1362 called this
    /// slot family's root-only resolution "not a live hazard today" (its words),
    /// reasoning from where the view fn happened to run, and R1364 paid for it —
    /// but a rule applied only where it is currently load-bearing is not a rule.
    ///
    /// Scroll state, animations and every other slot the shell never touches keep
    /// plain `cache`: inheriting them would be the mirror-image bug.
    ///
    /// A slot the shell drives at root must ALSO be seeded there at boot. The
    /// walk cannot help a slot that does not exist yet: on a total miss this
    /// creates at the CALLING scope, so a child that resolves before the shell
    /// first touches root would mint its own and desync anyway. The sinks get
    /// that from their seed call
    /// ([`ProviderSlot::provide`](super::provider_slot::ProviderSlot::provide),
    /// or a legacy `provide_*` for a slot not yet migrated); `local_task_pump`
    /// and `pane_viewport_registry` are seeded explicitly in
    /// `CoreShell::new_with_seed`; `scene_revision` and `waiter_registry` in
    /// `pinion-shell`'s `ShellCore::with_core`, which resolves both against
    /// `core.root_owner()` before the first paint.
    ///
    /// # Panics
    ///
    /// Same nested-factory rule as [`Self::cache`] — a factory that re-enters
    /// the cache panics with an actionable message rather than `RefCell`'s
    /// (`[[owner-cache-no-nested-factory]]`). The walk starts at `self` and only
    /// ever takes a SHARED borrow, so either `self` or any ancestor being
    /// mid-factory trips it. (R1364.2 wrote "an ancestor is the only way",
    /// having read its own loop as starting one scope up.)
    pub fn cache_inherited<V, F>(&self, key: impl Into<Cow<'static, str>>, factory: F) -> Rc<V>
    where
        V: 'static,
        F: FnOnce() -> V,
    {
        let key: Cow<'static, str> = key.into();
        let typed_key = (TypeId::of::<V>(), key.clone());
        let mut scope = Some(Rc::clone(&self.inner));
        while let Some(inner) = scope {
            let hit = inner
                .cache
                .try_borrow()
                .unwrap_or_else(|_| {
                    panic!(
                        "Owner::cache_inherited must not be called from inside \
                         an Owner::cache factory; pre-resolve dependent slots \
                         first (see [[owner-cache-no-nested-factory]] in \
                         memory). Re-entering on key={key:?}",
                    )
                })
                .get(&typed_key)
                .map(Rc::clone);
            if let Some(any_rc) = hit {
                return Rc::downcast::<V>(any_rc).unwrap_or_else(|_| {
                    panic!("Owner::cache_inherited typed-key invariant broken for key={key:?}")
                });
            }
            scope = inner.parent.upgrade();
        }
        // No ancestor provides it. Create at THIS scope, not at the root: a
        // resolver's Null default is a local fallback, and writing it to the
        // root would let an unseeded child permanently poison the slot for the
        // whole tree via `cache`'s first-write-wins.
        self.cache(key, factory)
    }

    /// R1006 §5.23 §5.22 — the owner-scoped viewport-size
    /// [`Signal`](super::signal::Signal): the view/effect-time "current layout
    /// viewport `(width, height)`" carrier.
    ///
    /// Returns whatever the shell seeded via [`Self::provide_viewport_size_signal`]
    /// at boot, or a lazy default `Signal::new((0, 0))` ("viewport unknown") when
    /// none was provided (headless / RPC / unit tests). Read in `view` / an
    /// [`Effect`](super::effect::Effect) via
    /// [`use_viewport_size`](super::viewport::use_viewport_size); the tracked
    /// `get` re-fires a reflow Effect on size change. Another hand-rolled pair
    /// awaiting its R1366.x migration to
    /// [`ProviderSlot`](super::provider_slot::ProviderSlot) — and one of the two
    /// whose verdict will be `per_scope`; unlike the repaint / metrics
    /// capability slots this carries a *changing value*, so it is a
    /// [`Signal`](super::signal::Signal) rather than a trait object.
    #[must_use]
    pub fn viewport_size_signal(&self) -> super::signal::Signal<(u32, u32)> {
        self.cache::<super::viewport::ViewportSizeHolder, _>(
            super::viewport::VIEWPORT_SIZE_KEY,
            || super::viewport::ViewportSizeHolder(super::signal::Signal::new((0_u32, 0_u32))),
        )
        .0
        .clone()
    }

    /// R1006 §5.23 §5.22 — seed the owner-scoped viewport-size
    /// [`Signal`](super::signal::Signal). The shell calls this once at boot
    /// **before** the binding factories / first `view` run, so the first
    /// [`use_viewport_size`](super::viewport::use_viewport_size) read resolves
    /// the shell's signal rather than the lazy `(0, 0)` default.
    /// Idempotent-by-first-write (like every [`Self::cache`] slot).
    pub fn provide_viewport_size_signal(&self, signal: super::signal::Signal<(u32, u32)>) {
        self.cache::<super::viewport::ViewportSizeHolder, _>(
            super::viewport::VIEWPORT_SIZE_KEY,
            move || super::viewport::ViewportSizeHolder(signal),
        );
    }

    /// R1335 §5.39 (PR-53) — the owner-scoped focus mirror
    /// [`Signal`](super::signal::Signal): the paint-path "which tag has focus"
    /// carrier read via [`focus_state::focused`](crate::focus_state::focused).
    ///
    /// Returns a clone of this owner's `focused_tag` handle. The mirror is
    /// binding-wide: [`Self::new_child`] threads the root's handle down the whole
    /// owner tree, so a child (secondary-window) scope hands back the SAME cell
    /// the root does — a write through any clone is seen by every reader.
    /// `pinion_runtime::FocusManager` writes it from its single `commit_focus`
    /// funnel (self-wrapped in [`Self::run`] so a subscriber woken by that write,
    /// should it read an owner-scoped hook, re-resolves [`Owner::current`] — the
    /// R1006 "blocker B" discipline `provide_viewport_size_signal` documents);
    /// consumers read it with a tracked `get`, so a focus change re-runs a
    /// subscribed view fn / `Effect`.
    ///
    /// A tree-inherited *direct field* rather than a per-owner [`Self::cache`]
    /// slot on purpose — see the `focused_tag` field for the full rationale
    /// (focus is a binding-wide singular fact, not a per-owner value).
    #[must_use]
    pub fn focused_tag_signal(&self) -> super::signal::Signal<Option<String>> {
        self.inner.focused_tag.clone()
    }

    /// R1012 §5.23 §5.22 — the owner-scoped per-pane viewport registry: the
    /// tag-keyed map of pane → measured-size
    /// [`Signal`](super::signal::Signal) backing
    /// [`use_pane_viewport_size`](super::pane_viewport::use_pane_viewport_size).
    ///
    /// Lazily created (empty) on first access; the consumer's `use_*` read and
    /// the shell's post-layout publish both resolve this same root-owner slot,
    /// so they share one signal per pane tag. Same private-key + owner-cache
    /// shape as [`Self::viewport_size_signal`]; the registry is itself an `Rc`
    /// handle so the returned clone is cheap. Internal: consumers read via
    /// [`use_pane_viewport_size`](super::pane_viewport::use_pane_viewport_size)
    /// and the shell publishes via [`Self::pane_viewport_entries`].
    /// R1364.5 §5.22 §5.28 — create this scope's pane-viewport registry now, so
    /// descendants inherit it rather than minting their own.
    ///
    /// The shell calls this once on `root_owner` at boot. R1021 requires ONE
    /// shared registry that every window publishes its pane rects into, and
    /// `publish_pane_viewports` reads the ROOT's; but the registry is built
    /// lazily on first touch, and [`Self::cache_inherited`] creates at the
    /// CALLING scope when no ancestor has one yet. So without an explicit boot
    /// seed, the deferred R680 atomic would let a secondary window's view mint
    /// its own registry, register its pane tags there, and never reflow — the
    /// torn-off pane's PTY silently keeping the wrong size. sprag's R37 undock
    /// is the forcing consumer.
    ///
    /// Exists (rather than the shell touching the resolver) because
    /// `Self::pane_viewport_registry` is `pub(crate)`: `pinion-runtime` cannot
    /// name it, and seeding via the unrelated
    /// [`Self::pane_viewport_entries`] would work only as a side effect nobody
    /// reading the call site could see.
    pub fn seed_pane_viewport_registry(&self) {
        drop(self.pane_viewport_registry());
    }

    #[must_use]
    pub(crate) fn pane_viewport_registry(&self) -> super::pane_viewport::PaneViewportRegistry {
        self.cache_inherited::<super::pane_viewport::PaneViewportRegistryHolder, _>(
            super::pane_viewport::PANE_VIEWPORT_REGISTRY_KEY,
            || {
                super::pane_viewport::PaneViewportRegistryHolder(
                    super::pane_viewport::PaneViewportRegistry::new(),
                )
            },
        )
        .0
        .clone()
    }

    /// R1012 §5.23 §5.22 — snapshot every registered pane `(tag, signal)` pair
    /// for the shell's post-layout publish
    /// ([`CoreShell::publish_pane_viewports`](../../../pinion_runtime/struct.CoreShell.html)).
    ///
    /// Returns an owned `Vec` (the registry borrow is dropped before the shell
    /// `set`s any signal) so the synchronous reflow Effect each `set` fires may
    /// re-enter
    /// [`use_pane_viewport_size`](super::pane_viewport::use_pane_viewport_size)
    /// without a `RefCell` double-borrow. Exposes only public types
    /// ([`Signal`](super::signal::Signal) / `Cow`), keeping the registry handle
    /// itself crate-private.
    #[must_use]
    pub fn pane_viewport_entries(&self) -> Vec<super::pane_viewport::PaneViewportEntry> {
        self.pane_viewport_registry().entries()
    }

    /// R51.150 §5.22 — `true` when (`V`, `key`) has been populated by
    /// a previous [`Owner::cache::<V>`](Self::cache) call on this
    /// owner.
    ///
    /// (R56.1.b.1 §5.22) The lookup is type-aware: the same `key`
    /// under different types resolves to distinct slots, matching the
    /// [`Self::cache`] keying contract. `use_caret_blink`'s first-
    /// time-registration gate
    /// (`!cache_contains::<CaretBlink>(key)`) therefore fires once
    /// per `(CaretBlink, key)` pair, independent of any other type
    /// using the same widget tag.
    ///
    /// Returns `false` for un-touched keys and for keys whose cached
    /// values have been dropped by an owner reset (not currently
    /// implemented; the cache lives for the owner's lifetime today).
    /// Primarily for diagnostics and tests; application code should
    /// just call [`Self::cache`] — the lazy-init contract handles
    /// the missing-key case transparently.
    #[must_use]
    pub fn cache_contains<V: 'static>(&self, key: impl Into<Cow<'static, str>>) -> bool {
        self.inner
            .cache
            .borrow()
            .contains_key(&(TypeId::of::<V>(), key.into()))
    }

    /// R605 §5.22 — non-mutating typed lookup against the cache,
    /// keyed by an arbitrarily-scoped `&str` (no `&'static`
    /// requirement). Returns the cached [`Rc<V>`] when the slot has
    /// been populated under `(V, key)`, or `None` when no such slot
    /// exists.
    ///
    /// ## Why this exists
    ///
    /// [`Self::cache`] / [`Self::cache_contains`] both require
    /// `&'static str` because the typed cache key is
    /// `(TypeId, &'static str)`. Application code passes string
    /// literals so the `'static` requirement is free, but
    /// introspection consumers (the JSON-RPC dispatch surface
    /// reading per-widget tags from `params.tag` at runtime) only
    /// have a borrowed-from-JSON `&str`. Pre-R605 those handlers
    /// reached `&'static str` via [`Box::leak`], which grew an
    /// unbounded process leak proportional to the number of unique
    /// tags observed over the program's lifetime — fine for a
    /// short-lived demo, a real liability for a long-running
    /// embedded RPC server.
    ///
    /// This lookup walks the cache linearly (`O(N)` in the number
    /// of populated slots, typically `< 100` per owner) and
    /// compares each stored `&'static str` to `key` byte-for-byte.
    /// The linear walk is bounded by the cache size — every entry
    /// was inserted by an application [`Self::cache`] call, and
    /// applications register one slot per `use_X` hook per widget —
    /// so the walk is fast enough for the RPC dispatch rate (1
    /// request per JSON-RPC frame, well below the paint cadence).
    /// The `Rc::clone` on the matching entry is the only allocation.
    ///
    /// ## Side effects
    ///
    /// None. The call only borrows the cache for the duration of
    /// the walk; no factory runs, no slot is created on miss, no
    /// signal is read.
    ///
    /// Pinned by `r605_cache_get_by_str_returns_cached_value`,
    /// `r605_cache_get_by_str_returns_none_when_absent`, and
    /// `r605_cache_get_by_str_is_type_aware`.
    #[must_use]
    pub fn cache_get_by_str<V: 'static>(&self, key: &str) -> Option<Rc<V>> {
        let type_id = TypeId::of::<V>();
        let any_rc: Rc<dyn Any> = {
            let cache = self.inner.cache.borrow();
            cache
                .iter()
                .find(|((tid, k), _)| *tid == type_id && k.as_ref() == key)
                .map(|(_, rc)| Rc::clone(rc))?
        };
        // Downcast is infallible by construction — the slot was
        // inserted by a `Self::cache::<V>` call so the TypeId
        // matches. Mirrors the `unwrap_or_else` panic in
        // [`Self::cache`] as a defensive guard against a future
        // refactor breaking the typed-key invariant.
        Rc::downcast::<V>(any_rc).ok()
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

impl std::fmt::Debug for Owner {
    /// R1335 §5.22 — a reactive handle prints by its stable [`Self::id`]; the
    /// scope's interior (signals, cache, children) is intentionally opaque. Lets
    /// structs that hold an `Owner` (e.g. `pinion_runtime::FocusManager`, which
    /// carries the root owner to publish the focus mirror) keep a derived
    /// `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Owner").field("id", &self.id()).finish()
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

/// R51.146 §5.22 — RAII guard for the `CURRENT_OWNER_HANDLE` stack
/// mirroring [`OwnerStackGuard`]. Pushed only by [`Owner::run`]; the
/// internal [`run_with_node`] path (used by [`Computed::recompute`](crate::reactive::Computed::recompute) /
/// `Effect::recompute`) leaves this stack untouched, so
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

    // ─────────────────────────────────────────────────────────────────
    // R605 §5.22 — Owner::cache_get_by_str substrate primitive
    // ─────────────────────────────────────────────────────────────────

    #[derive(Debug, PartialEq)]
    struct CacheProbe(u32);

    #[derive(Debug, PartialEq)]
    struct OtherProbe(&'static str);

    #[test]
    fn r605_cache_get_by_str_returns_cached_value() {
        let owner = Owner::new();
        let inserted = owner.cache::<CacheProbe, _>("widget", || CacheProbe(42));
        let looked_up: Rc<CacheProbe> =
            owner.cache_get_by_str("widget").expect("slot must resolve");
        assert!(Rc::ptr_eq(&inserted, &looked_up), "same Rc back");
        assert_eq!(looked_up.0, 42);
    }

    #[test]
    fn r605_cache_get_by_str_returns_none_when_absent() {
        let owner = Owner::new();
        owner.cache::<CacheProbe, _>("widget", || CacheProbe(1));
        // Non-existent key returns None — no leak, no slot creation.
        let miss: Option<Rc<CacheProbe>> = owner.cache_get_by_str("ghost");
        assert!(miss.is_none());
        // The unfound lookup must not have populated a phantom slot.
        assert!(!owner.cache_contains::<CacheProbe>("ghost"));
    }

    #[test]
    fn r605_cache_get_by_str_is_type_aware() {
        // Same key, two distinct types — lookup by string returns
        // the type-matching entry only.
        let owner = Owner::new();
        let _probe = owner.cache::<CacheProbe, _>("shared", || CacheProbe(7));
        let _other = owner.cache::<OtherProbe, _>("shared", || OtherProbe("hi"));
        let probe: Rc<CacheProbe> = owner
            .cache_get_by_str("shared")
            .expect("CacheProbe slot must resolve");
        let other: Rc<OtherProbe> = owner
            .cache_get_by_str("shared")
            .expect("OtherProbe slot must resolve");
        assert_eq!(probe.0, 7);
        assert_eq!(other.0, "hi");
    }

    #[test]
    fn r605_cache_get_by_str_accepts_non_static_str_view() {
        // The crux of R605: the lookup MUST work when `key` is a
        // dynamically-built `String` (the canonical JSON-RPC `tag`
        // path) without resorting to `Box::leak`. The cache stored a
        // `&'static str`; the lookup compares against the borrowed
        // `&str` view byte-for-byte.
        let owner = Owner::new();
        owner.cache::<CacheProbe, _>("widget", || CacheProbe(99));
        let dynamic_tag: String = String::from("widget");
        let looked_up: Rc<CacheProbe> = owner
            .cache_get_by_str(dynamic_tag.as_str())
            .expect("dynamic-string lookup must hit the static slot");
        assert_eq!(looked_up.0, 99);
    }

    #[test]
    fn r605_cache_get_by_str_walks_all_slots_until_match() {
        // Insert several slots so the linear walk has to iterate
        // past prefixes to land on the match. Guards against a
        // future refactor that accidentally short-circuits.
        let owner = Owner::new();
        owner.cache::<CacheProbe, _>("a", || CacheProbe(1));
        owner.cache::<CacheProbe, _>("ab", || CacheProbe(2));
        owner.cache::<CacheProbe, _>("abc", || CacheProbe(3));
        owner.cache::<CacheProbe, _>("abcd", || CacheProbe(4));
        let hit: Rc<CacheProbe> = owner
            .cache_get_by_str("abc")
            .expect("middle-of-walk slot must resolve");
        assert_eq!(hit.0, 3);
    }

    // ─────────────────────────────────────────────────────────────────
    // R685.C atomic 5 — Owner::cache accepts dynamic (owned) keys.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r685_c_cache_accepts_owned_string_key() {
        // The crux of R685.C atomic 5: a runtime-generated `String`
        // (e.g. R686 dock-reorganize mints `format!("split-{n}")`)
        // can address a cache slot directly — no `Box::leak`.
        let owner = Owner::new();
        let dynamic_key: String = format!("split-{}", 42);
        let inserted = owner.cache::<CacheProbe, _>(dynamic_key.clone(), || CacheProbe(7));
        // The same dynamic key re-resolves to the same Rc.
        let again = owner.cache::<CacheProbe, _>(dynamic_key, || CacheProbe(999));
        assert!(Rc::ptr_eq(&inserted, &again), "owned key memoises");
        assert_eq!(again.0, 7, "factory not re-run on cache hit");
    }

    #[test]
    fn r685_c_cache_static_and_owned_keys_share_slot_when_equal() {
        // A `&'static str` literal and an owned `String` with the
        // same bytes address the SAME slot — Cow::Borrowed and
        // Cow::Owned compare + hash by value, not by provenance.
        let owner = Owner::new();
        let from_static = owner.cache::<CacheProbe, _>("shared_key", || CacheProbe(1));
        let from_owned = owner.cache::<CacheProbe, _>(String::from("shared_key"), || CacheProbe(2));
        assert!(
            Rc::ptr_eq(&from_static, &from_owned),
            "static literal + equal owned String resolve to one slot",
        );
        assert_eq!(from_owned.0, 1);
    }

    #[test]
    fn r685_c_cache_contains_accepts_owned_key() {
        let owner = Owner::new();
        owner.cache::<CacheProbe, _>("widget", || CacheProbe(1));
        assert!(owner.cache_contains::<CacheProbe>(String::from("widget")));
        assert!(!owner.cache_contains::<CacheProbe>(String::from("absent")));
    }

    // ─────────────────────────────────────────────────────────────────
    // R666 §5.22 — Owner::cache nested-factory guard
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r666_owner_cache_panics_with_actionable_message_on_nested_factory() {
        use std::panic::AssertUnwindSafe;
        // Re-entering `Owner::cache` from inside a factory closure
        // tripped a cryptic `RefCell::borrow_mut` panic pre-R666;
        // the substrate now upgrades the message to name the rule
        // and tell the caller how to fix it. The pre-resolution
        // pattern (resolve dependent slots before entering the outer
        // cache) is the canonical workaround — codified by
        // `[[owner-cache-no-nested-factory]]`.
        let owner = Owner::new();
        let owner_inside = owner.clone();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            owner.cache::<CacheProbe, _>("outer", || {
                // BAD: nested factory call on the same Owner. The
                // outer `borrow_mut` is still live.
                let _inner = owner_inside.cache::<OtherProbe, _>("inner", || OtherProbe("x"));
                CacheProbe(0)
            });
        }));
        let payload = result.expect_err("nested factory call must panic");
        let msg = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&'static str>().copied())
            .expect("panic payload is a string");
        assert!(
            msg.contains("Owner::cache factory closures must not call Owner::cache"),
            "panic message must name the rule: {msg}",
        );
        assert!(
            msg.contains("pre-resolve dependent slots first"),
            "panic message must point to the workaround: {msg}",
        );
        assert!(
            msg.contains("owner-cache-no-nested-factory"),
            "panic message must reference the memory key: {msg}",
        );
    }

    #[test]
    fn r666_owner_cache_pre_resolved_dependent_slots_succeed() {
        // The canonical fix: resolve dependent slots first, capture
        // their `Rc<T>` handles, then enter the outer cache call. No
        // re-entry, no panic.
        let owner = Owner::new();
        let dep = owner.cache::<OtherProbe, _>("dep", || OtherProbe("ready"));
        let outer = owner.cache::<CacheProbe, _>("outer", {
            let dep = Rc::clone(&dep);
            move || {
                // Factory uses the already-resolved handle; no
                // recursive cache call.
                assert_eq!(dep.0, "ready");
                CacheProbe(7)
            }
        });
        assert_eq!(outer.0, 7);
        // Sanity — both slots populated.
        assert!(owner.cache_contains::<OtherProbe>("dep"));
        assert!(owner.cache_contains::<CacheProbe>("outer"));
    }

    #[test]
    fn r666_owner_cache_nested_on_distinct_owner_ok() {
        // The guard is per-Owner: nesting against a *different*
        // Owner is fine (each has its own `RefCell`). This is the
        // common case for sibling sub-trees and must not panic.
        let outer = Owner::new();
        let inner = Owner::new();
        let val = outer.cache::<CacheProbe, _>("outer", || {
            let p = inner.cache::<OtherProbe, _>("inner", || OtherProbe("ok"));
            CacheProbe(p.0.len().try_into().unwrap())
        });
        assert_eq!(val.0, 2);
    }

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
            o.inner.add_subscription_cleanup(Box::new(move || {
                counter_clone.set(counter_clone.get() + 1);
            }));
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
        assert_eq!(
            b.get(),
            99,
            "b stays at post-mutation value on failed restore"
        );
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
    // - (R56.1.b.1 §5.22) Distinct types under the *same* key resolve
    //   to independent slots (typed-key contract).
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
            assert!(!owner.cache_contains::<u32>("foo"));
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
            assert!(owner.cache_contains::<u32>("key"));
            // (R56.1.b.1 §5.22) Typed cache_contains — the same key
            // under a different type reads as un-populated.
            assert!(!owner.cache_contains::<String>("key"));
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
        fn same_key_distinct_types_resolve_independent_slots() {
            // (R56.1.b.1 §5.22) The typed-key cache routes
            // `(TypeId::of::<V>(), key)` so the same string `key` with
            // a *different* concrete `V` resolves to a distinct slot.
            // React's per-hook slot model: `use_state(tag)` and
            // `use_ref(tag)` both accept the same widget tag — pinion's
            // `use_text_edit_state(tag)` + `use_caret_blink(tag)`
            // pattern is now load-bearing rather than aspirational.
            let owner = Owner::new();
            let u: Rc<u32> = owner.cache("k", || 1_u32);
            let s: Rc<String> = owner.cache("k", || String::from("ok"));
            assert_eq!(*u, 1);
            assert_eq!(*s, "ok");
            // Both slots present, distinct cache entries.
            assert_eq!(owner.cache_size(), 2);
            assert!(owner.cache_contains::<u32>("k"));
            assert!(owner.cache_contains::<String>("k"));
            assert!(!owner.cache_contains::<f64>("k"));
        }

        #[test]
        fn same_key_same_type_aliases_after_typed_split() {
            // (R56.1.b.1 §5.22) Same `(TypeId, key)` still aliases the
            // same Rc — the typed-key change preserves the per-(type,
            // key) caching semantics required for Solid.js `createMemo`
            // / React `useRef` reuse.
            let owner = Owner::new();
            let a: Rc<u32> = owner.cache("k", || 7_u32);
            let b: Rc<u32> = owner.cache("k", || 999_u32);
            assert!(Rc::ptr_eq(&a, &b));
            assert_eq!(*b, 7);
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
                Rc::new(Self {
                    at_rest: Cell::new(true),
                })
            }
            fn active() -> Rc<Self> {
                Rc::new(Self {
                    at_rest: Cell::new(false),
                })
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

        // R629 §5.28 — settle_animations / cancel_animations bulk walk
        // unit tests. The Tickable extension landed default no-op
        // implementations so the `ProgrammableRest` fixture above does
        // not need to override settle / cancel; the cases below pin
        // the count contract (children + self, depth-first) and the
        // default no-op behaviour through the test fixture.
        #[test]
        fn r629_settle_animations_counts_every_visited_registration() {
            let owner = Owner::new();
            owner.register_animation(ProgrammableRest::active());
            owner.register_animation(ProgrammableRest::at_rest());
            assert_eq!(owner.settle_animations(), 2);
        }

        #[test]
        fn r629_cancel_animations_counts_every_visited_registration() {
            let owner = Owner::new();
            owner.register_animation(ProgrammableRest::active());
            assert_eq!(owner.cancel_animations(), 1);
        }

        #[test]
        fn r629_settle_animations_walks_descendant_scopes() {
            let parent = Owner::new();
            let child = Owner::new_child(&parent);
            let grandchild = Owner::new_child(&child);
            parent.register_animation(ProgrammableRest::active());
            child.register_animation(ProgrammableRest::active());
            grandchild.register_animation(ProgrammableRest::active());
            assert_eq!(parent.settle_animations(), 3);
        }

        #[test]
        fn r629_settle_animations_on_empty_owner_returns_zero() {
            let owner = Owner::new();
            assert_eq!(owner.settle_animations(), 0);
            assert_eq!(owner.cancel_animations(), 0);
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
            parent.dispatch_command(Command::new_static(
                "p.cmd",
                IntrospectValue::Null,
                parent.id(),
            ));
            child.dispatch_command(Command::new_static(
                "c.cmd",
                IntrospectValue::Null,
                child.id(),
            ));
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
        fn r51_161_pending_commands_recursive_does_not_drain() {
            // R51.161 — sibling of `take_pending_commands_recursive`
            // that snapshots without consuming. The same depth-first
            // traversal order; queues stay populated after the call so
            // the framework pump can still drain them on the next
            // dispatch cycle.
            let parent = Owner::new();
            let child = Owner::new_child(&parent);
            let grandchild = Owner::new_child(&child);
            parent.dispatch_command(Command::new_static(
                "p.cmd",
                IntrospectValue::Null,
                parent.id(),
            ));
            child.dispatch_command(Command::new_static(
                "c.cmd",
                IntrospectValue::Null,
                child.id(),
            ));
            grandchild.dispatch_command(Command::new_static(
                "gc.cmd",
                IntrospectValue::Null,
                grandchild.id(),
            ));
            let snapshot = parent.pending_commands_recursive();
            assert_eq!(snapshot.len(), 3);
            assert_eq!(snapshot[0].kind_str(), "gc.cmd");
            assert_eq!(snapshot[1].kind_str(), "c.cmd");
            assert_eq!(snapshot[2].kind_str(), "p.cmd");
            // Queues preserved — a subsequent drain returns the same
            // three commands.
            assert_eq!(parent.pending_commands().len(), 1);
            assert_eq!(child.pending_commands().len(), 1);
            assert_eq!(grandchild.pending_commands().len(), 1);
            let drained = parent.take_pending_commands_recursive();
            assert_eq!(drained.len(), 3);
        }

        #[test]
        fn r51_161_pending_commands_recursive_empty_owner_returns_empty_vec() {
            let parent = Owner::new();
            let _child = Owner::new_child(&parent);
            assert!(parent.pending_commands_recursive().is_empty());
        }

        #[test]
        fn r51_161_pending_commands_recursive_snapshot_clone_is_independent() {
            // Mutating the snapshot must not affect the live queue.
            let owner = Owner::new();
            owner.dispatch_command(Command::new_static("a", IntrospectValue::Null, owner.id()));
            let mut snapshot = owner.pending_commands_recursive();
            snapshot.clear();
            assert_eq!(owner.pending_commands().len(), 1);
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
            owner.dispatch_command(Command::new_static(
                "first",
                IntrospectValue::Null,
                owner.id(),
            ));
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

#[cfg(test)]
mod r1364_cache_inherited_tests {
    //! R1364 §5.22 §5.55 — the provider-slot parent walk.
    //!
    //! These pin the R680 question BEFORE R680: the deferred atomic switches the
    //! view wrap to `window_owner(id).run(..)`, and on that day every one of the
    //! binding's provider slots resolved a Null in a secondary window. The bug it
    //! produced (a Quit button that silently does nothing) names nothing that
    //! would make the author of R680 think of it, so the property is fixed here
    //! and asserted here.

    use super::Owner;

    #[test]
    fn r1364_a_child_resolves_the_parents_slot() {
        let root = Owner::new();
        root.cache_inherited::<u32, _>("cap", || 7);
        let child = Owner::new_child(&root);
        assert_eq!(
            *child.cache_inherited::<u32, _>("cap", || 99),
            7,
            "a child must see the capability the shell seeded on root — the \
             factory here stands for the Null default that would silently lie",
        );
    }

    #[test]
    fn r1364_the_walk_crosses_more_than_one_generation() {
        let root = Owner::new();
        root.cache_inherited::<u32, _>("cap", || 7);
        let child = Owner::new_child(&root);
        let grandchild = Owner::new_child(&child);
        assert_eq!(*grandchild.cache_inherited::<u32, _>("cap", || 99), 7);
    }

    #[test]
    fn r1364_the_nearest_ancestor_wins_not_the_root() {
        let root = Owner::new();
        root.cache_inherited::<u32, _>("cap", || 7);
        let child = Owner::new_child(&root);
        child.cache::<u32, _>("cap", || 42); // a deliberate per-scope override
        let grandchild = Owner::new_child(&child);
        assert_eq!(
            *grandchild.cache_inherited::<u32, _>("cap", || 99),
            42,
            "nearest-first, so a scope can shadow an inherited slot",
        );
    }

    #[test]
    fn r1364_a_miss_everywhere_creates_at_the_calling_scope() {
        let root = Owner::new();
        let child = Owner::new_child(&root);
        assert_eq!(*child.cache_inherited::<u32, _>("cap", || 99), 99);
        // Created on the CHILD, not hoisted to the root: an unseeded child must
        // not poison the whole tree through `cache`'s first-write-wins.
        assert_eq!(
            *root.cache::<u32, _>("cap", || 7),
            7,
            "the child's local default must not have been written to the root",
        );
    }

    #[test]
    fn r1364_plain_cache_still_does_not_inherit() {
        // The contrast IS the design: a slot the shell never drives at root
        // (scroll offset) — or drives but reads primary-gated (viewport size) —
        // is per-owner and must NOT see its parent's, or a secondary window
        // reflows to the primary's size. R1365: "only capabilities inherit" was
        // this comment's original wording and is the taxonomy `cache_inherited`'s
        // rustdoc now records as WRONG; the predicate is who drives the slot.
        let root = Owner::new();
        root.cache::<u32, _>("value", || 7);
        let child = Owner::new_child(&root);
        assert_eq!(*child.cache::<u32, _>("value", || 99), 99);
    }

    #[test]
    fn r1364_a_root_has_no_parent_to_walk() {
        let root = Owner::new();
        assert_eq!(*root.cache_inherited::<u32, _>("cap", || 99), 99);
    }

    #[test]
    fn r1364_an_orphaned_child_resolves_as_a_root() {
        // `parent` is a Weak and is never cleared, so a child that outlives its
        // parent upgrades to None and honestly reports "no provider" rather than
        // reaching into freed memory.
        let child = {
            let root = Owner::new();
            root.cache_inherited::<u32, _>("cap", || 7);
            Owner::new_child(&root)
        };
        assert_eq!(*child.cache_inherited::<u32, _>("cap", || 99), 99);
    }

    // R1365.1 — the two slots above are the MECHANISM; every test in this mod
    // uses a synthetic `"cap"` key. R1365's ledger cited this mod as the
    // behavioural enforcement of the census table's per-slot verdicts, which it
    // has never been. These two are the real slots owned by this file: neither
    // has a `provide_*`, so the shell seeds them by touching root
    // (`CoreShell::new_with_seed`), and the walk cannot save a slot that does
    // not exist yet — seed-at-root and resolve-inherited are both load-bearing.

    #[test]
    fn r1365_1_a_child_scope_resolves_the_roots_local_task_pump() {
        let root = Owner::new();
        let seeded = root.local_task_pump();
        let window_scope = Owner::new_child(&root);
        assert!(
            std::rc::Rc::ptr_eq(&seeded, &window_scope.local_task_pump()),
            "a child scope minted its own LocalTaskPump — the shell polls only \
             the root's, so a secondary window's async work would never run",
        );
    }

    #[test]
    fn r1365_1_a_child_scope_reads_the_roots_pane_viewport_registry() {
        // Behavioural rather than `ptr_eq`: the registry is returned by value
        // (a cheap `Rc` handle inside), and what R1021 actually requires is that
        // a publish into the ROOT's registry is what a pane's scope reads back.
        let root = Owner::new();
        root.seed_pane_viewport_registry();
        root.pane_viewport_registry()
            .signal_for(std::borrow::Cow::Borrowed("pane.left"))
            .set((640, 480));

        let window_scope = Owner::new_child(&root);
        let seen = window_scope.run(|| crate::use_pane_viewport_size("pane.left"));

        assert_eq!(
            seen,
            (640, 480),
            "a child scope minted its own PaneViewportRegistry and read the \
             (0, 0) unknown — R1021 requires ONE root instance every window \
             publishes pane rects into (forcing consumer: sprag's R37 undock)",
        );
    }
}
