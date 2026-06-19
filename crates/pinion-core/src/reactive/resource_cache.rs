//! `ResourceCache<K, V, E>` — a keyed map of async [`Resource`]s with
//! idempotent get-or-fetch, a reactive state snapshot, and optional
//! **LRU eviction** for unbounded key spaces (§5.22 R927 + R934).
//!
//! ## Why this exists (the 2nd-consumer lift)
//!
//! A virtualized view over an *out-of-memory, page-fetched* source keeps only
//! the visible slices resident: each slice is an async [`Resource`], and the
//! set of resident slices is a small map keyed by the slice's identity. Two
//! examples grew the same map independently —
//!
//! - `hello-lazy-list` (R924): an infinite-scroll list keyed by **page index**
//!   (`HashMap<usize, Rc<Resource<Vec<Row>>>>`).
//! - `hello-asset-browser` (R927): a list with **source-side sort/filter**,
//!   where a fetched page is identified by its full query
//!   (`HashMap<(SortMode, FilterKey, usize), …>`) — changing the sort or
//!   filter yields fresh keys, so pages fetched under one ordering are never
//!   reused under another.
//!
//! The map mechanism is identical in both and **correctness-critical**: a
//! key must be fetched *exactly once* (insert the `Loading` carrier *before*
//! spawning the fetch, and guard on presence) or a hand-rolled copy re-fetches
//! the same page every frame, or double-fetches under re-entrancy. That is a
//! divergence-is-a-bug duplication, so it lifts at the second consumer (the
//! [`OrderMemo`](super::super::widgets::order_memo) precedent — one correct
//! copy of a cache-invalidation dance prevents the bug). The *data model* (how
//! rows are shaped, how a page maps to source indices) stays in the consumer:
//! this owns only the keyed-async-carrier cache, not a unified `Model` trait
//! (still deliberately premature — there is no homogeneous row source).
//!
//! ## Bounded vs unbounded (R934)
//!
//! [`new`](ResourceCache::new) builds an **unbounded** cache — every fetched
//! slice is retained for the cache's lifetime. That is correct when the key
//! space is bounded: a fixed page count (`hello-lazy-list`, 10 000 rows = 100
//! pages) or a small set of sort/filter combinations (`hello-asset-browser`).
//!
//! [`with_capacity`](ResourceCache::with_capacity) builds an **LRU-bounded**
//! cache for a *truly unbounded* source — a million-row asset store
//! (`hello-million-row`, 10 000 pages). At most `capacity` slices stay
//! resident; inserting past capacity evicts the **least-recently-used** key.
//! Every access promotes a key to most-recently-used:
//! [`ensure`](ResourceCache::ensure) (re-requesting a resident key),
//! [`state`](ResourceCache::state), and [`snapshot`](ResourceCache::snapshot)
//! (the per-frame read of the visible window). So a virtualized view that
//! ensures + snapshots its visible pages every frame keeps exactly those pages
//! hot, and eviction only ever claims pages that have scrolled away.
//!
//! **Capacity invariant.** `capacity` must exceed the visible working set
//! (window pages + overscan, plus any prefetch margin). The LRU order keeps
//! the visible window resident *because* it is touched every frame; were the
//! capacity smaller than the window, a single frame's ensures could evict a
//! page that frame still needs — the classic cache-thrash. Size it like
//! [`LayoutCache`](../../../pinion_text/cache/struct.LayoutCache.html) does
//! (256 layouts ≫ on-screen runs): a handful of pages of headroom over the
//! tallest viewport.
//!
//! ## Eviction is fetch-safe
//!
//! Evicting a slice whose fetch is still in flight is safe: the spawned driver
//! future owns clones of the [`Resource`]'s state `Signal` + generation cell
//! (not the `Rc<Resource>` the cache holds — see
//! [`Resource::fetch_with`](super::resource::Resource::fetch_with)), so it runs
//! to completion and writes onto now-detached storage — no panic, no write
//! back into the map. A scroll-back simply re-[`ensure`](ResourceCache::ensure)s
//! the key (a fresh fetch); the orphaned result is discarded. Wasted work on a
//! far-away page is the intended trade for bounded memory.
//!
//! ## Contract
//!
//! - [`ensure`](ResourceCache::ensure) is **idempotent**: a key already
//!   present (Loading, Ready, or Error) is a no-op and its future factory is
//!   never invoked, so a slice is fetched once and retained. A hit promotes
//!   the key to most-recently-used.
//! - [`state`](ResourceCache::state) / [`snapshot`](ResourceCache::snapshot)
//!   read each entry's [`Resource::state`], **subscribing** the active
//!   reactive scope — a slice's resolution re-renders the view that read it —
//!   and promote the read key(s) to most-recently-used.
//! - [`contains`](ResourceCache::contains) is a pure presence query: it does
//!   **not** promote, so introspection / tests can probe LRU order without
//!   perturbing it.
//!
//! Single-thread (`RefCell`, `!Send`): matches the `!Send` reactive runtime
//! (§6.3) and the [`LocalTaskPump`](super::resource::LocalTaskPump) the fetch
//! is driven through.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::rc::Rc;

use lru::LruCache;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::resource::{LocalSpawner, Resource, ResourceState};

/// A keyed cache of async [`Resource`]s with idempotent get-or-fetch and
/// optional LRU eviction.
///
/// `K` identifies a resident slice (a page index, or a full
/// sort/filter/page query tuple); `V` is the fetched payload and `E` the
/// fetch error. Store it behind an [`Rc`] in an
/// [`Owner::cache`](super::owner::Owner::cache) slot so the view, the prefetch
/// `Effect`, and the cache share one instance.
pub struct ResourceCache<K, V, E>
where
    K: Hash + Eq + Clone,
    V: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    entries: RefCell<LruCache<K, Rc<Resource<V, E>>>>,
    /// The bound, or `None` for an unbounded (retention-only) cache. Tracked
    /// alongside the backing `LruCache` because its own `cap()` reports
    /// `usize::MAX` for the unbounded variant — this keeps the distinction
    /// honest for [`capacity`](ResourceCache::capacity) / introspection.
    capacity: Option<NonZeroUsize>,
}

impl<K, V, E> ResourceCache<K, V, E>
where
    K: Hash + Eq + Clone,
    V: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    /// Construct an empty **unbounded** cache — fetched slices are retained for
    /// the cache's lifetime (correct for a bounded key space). Use
    /// [`with_capacity`](Self::with_capacity) for a truly unbounded source.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: RefCell::new(LruCache::unbounded()),
            capacity: None,
        }
    }

    /// Construct an empty **LRU-bounded** cache holding at most `capacity`
    /// resident slices; inserting past it evicts the least-recently-used key.
    /// `capacity` must exceed the visible working set (see the module's
    /// *Capacity invariant*).
    #[must_use]
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        Self {
            entries: RefCell::new(LruCache::new(capacity)),
            capacity: Some(capacity),
        }
    }

    /// The cache's LRU bound, or `None` if it is unbounded (retention-only).
    #[must_use]
    pub fn capacity(&self) -> Option<NonZeroUsize> {
        self.capacity
    }

    /// Whether `key` already has a fetch in flight or resolved. A pure presence
    /// query — does **not** promote the key in the LRU order.
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.entries.borrow().contains(key)
    }

    /// Number of resident entries (fetched or in flight). Never exceeds
    /// [`capacity`](Self::capacity) for a bounded cache.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    /// Whether the cache holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// Ensure `key` has a fetch in flight (or already resolved). **Idempotent**:
    /// if the key is present, this is a no-op and `make_future` is never called
    /// — so a key is fetched exactly once and its future is not even
    /// constructed on a cache hit. A hit **promotes** the key to
    /// most-recently-used, so the prefetch `Effect` re-ensuring the visible
    /// window every scroll keeps it hot and out of the eviction path.
    ///
    /// On a miss, a `Loading` [`Resource`] is inserted **before** the fetch is
    /// spawned (so a re-entrant `ensure` for the same key during the same frame
    /// observes it as in-flight and does not double-fetch), then
    /// `make_future()` is driven to completion through `spawner` — the
    /// shell-polled [`LocalTaskPump`](super::resource::LocalTaskPump) in
    /// production, a test executor in unit tests. The insert may evict the
    /// least-recently-used key (bounded cache); the evicted carrier drops here
    /// and its in-flight fetch, if any, completes onto detached storage (see
    /// the module's *Eviction is fetch-safe* note). The insert borrow is
    /// released before `fetch_with` runs, so a completion that re-enters the
    /// cache (an `Effect` starting a follow-up fetch) does not alias the map.
    pub fn ensure<S, F, MF>(&self, key: K, spawner: &S, make_future: MF)
    where
        S: LocalSpawner,
        F: Future<Output = Result<V, E>> + 'static,
        MF: FnOnce() -> F,
    {
        // Promote-on-hit: a re-requested key is wanted now → most-recently-used.
        // `get` (not `contains`) so the visible window the prefetch Effect
        // re-ensures every scroll is never the eviction victim. No factory call.
        if self.entries.borrow_mut().get(&key).is_some() {
            return;
        }
        let resource = Rc::new(Resource::loading());
        // `put` evicts the LRU key when at capacity; the evicted `Rc<Resource>`
        // drops at the end of this statement, before `fetch_with` runs.
        self.entries.borrow_mut().put(key, Rc::clone(&resource));
        resource.fetch_with(spawner, make_future());
    }

    /// Drop the cached entry for `key` so the next [`ensure`](Self::ensure)
    /// re-fetches it from scratch — the **streaming-tail-page** primitive
    /// (§5.22 R1005). A paged source whose row count *grows at runtime* has a
    /// partial last page: when the stream appends rows onto it, that page's
    /// cached slice is stale. The producer calls `invalidate(tail_page)` then
    /// bumps the count `Signal`, and the next render's [`ensure`](Self::ensure)
    /// fetches the now-larger page. Idempotent on an absent key (an evicted or
    /// never-fetched page already re-fetches on the next `ensure`), so the
    /// producer can invalidate the tail unconditionally.
    ///
    /// Dropping the entry's `Rc<Resource>` is **fetch-safe** exactly as LRU
    /// eviction is: an in-flight fetch holds its own `Rc` and completes onto the
    /// now-detached carrier, while the re-`ensure` starts a fresh fetch on a new
    /// carrier — a stale completion can never overwrite the fresh page because
    /// they target different `Resource`s (the module's *Eviction is fetch-safe*
    /// note). Re-keying by an embedded version instead would leak a dead carrier
    /// per append; in-place invalidation re-fetches the *same* key.
    pub fn invalidate(&self, key: &K) {
        self.entries.borrow_mut().pop(key);
    }

    /// Current [`ResourceState`] of `key`, or `None` if it has never been
    /// fetched. Reading a present entry **subscribes** the active reactive
    /// scope to it (so its resolution re-renders the reader) and **promotes**
    /// it to most-recently-used.
    #[must_use]
    pub fn state(&self, key: &K) -> Option<ResourceState<V, E>> {
        self.entries.borrow_mut().get(key).map(|r| r.state())
    }

    /// Snapshot the states of `keys`, subscribing the active scope to each
    /// **present** entry (absent keys are omitted) and promoting each present
    /// key to most-recently-used. One borrow over the whole window avoids
    /// re-borrowing per lookup and collects the visible slices' states in a
    /// single pass — the shape a virtualized view reads once per frame to map
    /// each row to loaded data or a skeleton. Touching the whole visible window
    /// each frame is what keeps it resident under LRU eviction.
    #[must_use]
    pub fn snapshot<I>(&self, keys: I) -> HashMap<K, ResourceState<V, E>>
    where
        I: IntoIterator<Item = K>,
    {
        let mut entries = self.entries.borrow_mut();
        keys.into_iter()
            .filter_map(|key| entries.get(&key).map(|r| (key, r.state())))
            .collect()
    }
}

impl<K, V, E> Default for ResourceCache<K, V, E>
where
    K: Hash + Eq + Clone,
    V: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::owner::Owner;
    use super::super::resource::{DeferredReady, LocalTaskPump};

    type Cache = ResourceCache<usize, i32, String>;

    /// Drive a pump to completion (what the shell does each frame).
    fn drain(pump: &LocalTaskPump) {
        for _ in 0..16 {
            if !pump.poll() {
                break;
            }
        }
    }

    #[test]
    fn empty_cache_has_no_entries() {
        let cache = Cache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert!(!cache.contains(&0));
        assert_eq!(cache.state(&0), None);
    }

    #[test]
    fn ensure_inserts_loading_then_resolves_through_pump() {
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        cache.ensure(7, &pump, || DeferredReady::new(2, Ok::<i32, String>(70)));
        // Inserted immediately as Loading, before the pump runs.
        assert!(cache.contains(&7));
        assert_eq!(cache.state(&7), Some(ResourceState::Loading));
        assert_eq!(cache.len(), 1);
        drain(&pump);
        assert_eq!(cache.state(&7), Some(ResourceState::Ready(70)));
    }

    #[test]
    fn ensure_is_idempotent_and_skips_the_future_factory() {
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        cache.ensure(1, &pump, || DeferredReady::new(0, Ok::<i32, String>(11)));
        drain(&pump);
        assert_eq!(cache.state(&1), Some(ResourceState::Ready(11)));
        // A second ensure for the same key must NOT refetch — the factory
        // panics if invoked, proving the cache hit short-circuits it.
        cache.ensure(1, &pump, || -> DeferredReady<Result<i32, String>> {
            panic!("future factory must not run on a cache hit")
        });
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.state(&1), Some(ResourceState::Ready(11)));
    }

    #[test]
    fn ensure_resolves_error_arm() {
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        cache.ensure(3, &pump, || {
            DeferredReady::new(0, Err::<i32, String>("boom".to_owned()))
        });
        drain(&pump);
        assert_eq!(cache.state(&3), Some(ResourceState::Error("boom".to_owned())));
    }

    #[test]
    fn distinct_keys_fetch_independently() {
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        cache.ensure(0, &pump, || DeferredReady::new(0, Ok::<i32, String>(100)));
        cache.ensure(5, &pump, || DeferredReady::new(0, Ok::<i32, String>(105)));
        drain(&pump);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.state(&0), Some(ResourceState::Ready(100)));
        assert_eq!(cache.state(&5), Some(ResourceState::Ready(105)));
        assert_eq!(cache.state(&9), None, "an unfetched key has no state");
    }

    #[test]
    fn snapshot_collects_only_present_keys() {
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        cache.ensure(0, &pump, || DeferredReady::new(0, Ok::<i32, String>(0)));
        cache.ensure(2, &pump, || DeferredReady::new(1, Ok::<i32, String>(2)));
        // One poll resolves entry 0 (latency 0) but leaves entry 2 (latency 1)
        // still Loading — a deterministic mixed snapshot.
        pump.poll();
        let snap = cache.snapshot([0usize, 1, 2]);
        // Key 1 was never fetched → omitted; 0 ready, 2 still loading.
        assert_eq!(snap.len(), 2);
        assert_eq!(snap.get(&0), Some(&ResourceState::Ready(0)));
        assert_eq!(snap.get(&2), Some(&ResourceState::Loading));
        assert!(!snap.contains_key(&1));
    }

    #[test]
    fn reading_state_subscribes_the_scope() {
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        cache.ensure(0, &pump, || DeferredReady::new(1, Ok::<i32, String>(42)));
        let owner = Owner::new();
        owner.run(|| {
            // Read inside the scope → subscribe to entry 0.
            assert_eq!(cache.state(&0), Some(ResourceState::Loading));
        });
        assert!(!owner.is_dirty());
        drain(&pump);
        // The deferred resolution dirties the subscribing owner.
        assert!(owner.is_dirty(), "entry resolution re-renders its reader");
    }

    #[test]
    fn composite_key_distinguishes_queries() {
        // The R927 shape: a fetched page is keyed by (sort, filter, page), so
        // the same page index under a different ordering is a distinct entry.
        let pump = LocalTaskPump::new();
        let cache: ResourceCache<(u8, Option<u8>, usize), i32, String> = ResourceCache::new();
        cache.ensure((0, None, 0), &pump, || DeferredReady::new(0, Ok::<i32, String>(1)));
        cache.ensure((1, None, 0), &pump, || DeferredReady::new(0, Ok::<i32, String>(2)));
        drain(&pump);
        assert_eq!(cache.len(), 2, "same page, different sort → distinct entries");
        assert_eq!(cache.state(&(0, None, 0)), Some(ResourceState::Ready(1)));
        assert_eq!(cache.state(&(1, None, 0)), Some(ResourceState::Ready(2)));
    }

    #[test]
    fn default_is_empty() {
        let cache: Cache = ResourceCache::default();
        assert!(cache.is_empty());
    }

    // ────────────────────────────────────────────────────────────────
    // R934 §5.22 — LRU eviction for unbounded key spaces
    // ────────────────────────────────────────────────────────────────

    fn cap(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).expect("test capacity is non-zero")
    }

    #[test]
    fn new_cache_is_unbounded_and_never_evicts() {
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        assert_eq!(cache.capacity(), None, "new() is unbounded");
        for k in 0..50 {
            cache.ensure(k, &pump, || DeferredReady::new(0, Ok::<i32, String>(0)));
        }
        drain(&pump);
        assert_eq!(cache.len(), 50, "unbounded: every key retained");
    }

    #[test]
    fn with_capacity_reports_its_bound_and_caps_residency() {
        let pump = LocalTaskPump::new();
        let cache = Cache::with_capacity(cap(4));
        assert_eq!(cache.capacity(), Some(cap(4)));
        for k in 0..20 {
            cache.ensure(k, &pump, || DeferredReady::new(0, Ok::<i32, String>(0)));
        }
        drain(&pump);
        assert_eq!(cache.len(), 4, "bounded cache never exceeds capacity");
    }

    #[test]
    fn capacity_evicts_the_least_recently_used_key() {
        let pump = LocalTaskPump::new();
        let cache = Cache::with_capacity(cap(2));
        cache.ensure(0, &pump, || DeferredReady::new(0, Ok::<i32, String>(0)));
        cache.ensure(1, &pump, || DeferredReady::new(0, Ok::<i32, String>(1)));
        drain(&pump);
        assert_eq!(cache.len(), 2);
        // Inserting a third key past capacity evicts the LRU (key 0).
        cache.ensure(2, &pump, || DeferredReady::new(0, Ok::<i32, String>(2)));
        drain(&pump);
        assert_eq!(cache.len(), 2, "still bounded at capacity");
        assert!(!cache.contains(&0), "LRU key 0 evicted");
        assert!(cache.contains(&1));
        assert!(cache.contains(&2));
    }

    #[test]
    fn reading_a_key_promotes_it_so_it_survives_eviction() {
        let pump = LocalTaskPump::new();
        let cache = Cache::with_capacity(cap(2));
        cache.ensure(0, &pump, || DeferredReady::new(0, Ok::<i32, String>(0)));
        cache.ensure(1, &pump, || DeferredReady::new(0, Ok::<i32, String>(1)));
        drain(&pump);
        // Touch key 0 via snapshot (the per-frame read) → key 1 becomes LRU.
        let _ = cache.snapshot([0usize]);
        cache.ensure(2, &pump, || DeferredReady::new(0, Ok::<i32, String>(2)));
        drain(&pump);
        assert!(cache.contains(&0), "the snapshot-touched key survives");
        assert!(!cache.contains(&1), "the now-LRU key 1 is evicted instead");
        assert!(cache.contains(&2));
    }

    #[test]
    fn ensure_hit_promotes_without_running_the_factory() {
        let pump = LocalTaskPump::new();
        let cache = Cache::with_capacity(cap(2));
        cache.ensure(0, &pump, || DeferredReady::new(0, Ok::<i32, String>(0)));
        cache.ensure(1, &pump, || DeferredReady::new(0, Ok::<i32, String>(1)));
        drain(&pump);
        // Re-ensure key 0 — a hit: it promotes (key 1 now LRU) and the factory
        // must NOT run (panics if invoked).
        cache.ensure(0, &pump, || -> DeferredReady<Result<i32, String>> {
            panic!("future factory must not run on a cache hit")
        });
        cache.ensure(2, &pump, || DeferredReady::new(0, Ok::<i32, String>(2)));
        drain(&pump);
        assert!(cache.contains(&0), "re-ensured key was promoted, survives");
        assert!(!cache.contains(&1), "the now-LRU key 1 is evicted");
        assert!(cache.contains(&2));
    }

    #[test]
    fn contains_does_not_promote() {
        let pump = LocalTaskPump::new();
        let cache = Cache::with_capacity(cap(2));
        cache.ensure(0, &pump, || DeferredReady::new(0, Ok::<i32, String>(0)));
        cache.ensure(1, &pump, || DeferredReady::new(0, Ok::<i32, String>(1)));
        drain(&pump);
        // `contains` is a pure probe — it must NOT make key 0 most-recent, so
        // key 0 (the LRU) is still the eviction victim after probing it.
        assert!(cache.contains(&0));
        cache.ensure(2, &pump, || DeferredReady::new(0, Ok::<i32, String>(2)));
        drain(&pump);
        assert!(!cache.contains(&0), "probing did not save the LRU key");
        assert!(cache.contains(&1));
        assert!(cache.contains(&2));
    }

    #[test]
    fn evicting_an_in_flight_fetch_is_safe() {
        // A far-away page evicted mid-flight: the orphaned fetch still completes
        // (onto detached storage) without panicking or writing back into the
        // map — the module's "eviction is fetch-safe" contract.
        let pump = LocalTaskPump::new();
        let cache = Cache::with_capacity(cap(1));
        // key 0's fetch is deferred (in flight, not yet drained)…
        cache.ensure(0, &pump, || DeferredReady::new(3, Ok::<i32, String>(700)));
        assert_eq!(cache.state(&0), Some(ResourceState::Loading));
        // …inserting key 1 at capacity 1 evicts key 0 while its fetch is queued.
        cache.ensure(1, &pump, || DeferredReady::new(0, Ok::<i32, String>(701)));
        assert!(!cache.contains(&0), "in-flight key 0 evicted");
        // Draining completes BOTH futures: key 0's orphaned completion lands on
        // detached storage (no map write, no panic); key 1 resolves in the map.
        drain(&pump);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.state(&1), Some(ResourceState::Ready(701)));
        assert!(!cache.contains(&0), "orphaned completion did not re-add key 0");
    }

    #[test]
    fn re_ensuring_an_evicted_key_refetches_it() {
        // The scroll-back path: an evicted page is fetched fresh again.
        let pump = LocalTaskPump::new();
        let cache = Cache::with_capacity(cap(1));
        cache.ensure(0, &pump, || DeferredReady::new(0, Ok::<i32, String>(0)));
        drain(&pump);
        assert_eq!(cache.state(&0), Some(ResourceState::Ready(0)));
        // Evict key 0 by inserting key 1 at capacity 1.
        cache.ensure(1, &pump, || DeferredReady::new(0, Ok::<i32, String>(1)));
        drain(&pump);
        assert!(!cache.contains(&0));
        // Re-ensure key 0: it is gone, so the factory runs a fresh fetch.
        cache.ensure(0, &pump, || DeferredReady::new(1, Ok::<i32, String>(42)));
        assert_eq!(cache.state(&0), Some(ResourceState::Loading), "re-fetch starts loading");
        drain(&pump);
        assert_eq!(cache.state(&0), Some(ResourceState::Ready(42)), "re-fetched value lands");
    }

    #[test]
    fn invalidate_forces_a_refetch_with_grown_data() {
        // The streaming-tail-page path (R1005): a partial page is re-fetched
        // when the stream appends to it, picking up the larger content.
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        // First fetch: the tail page holds value 1 (e.g. 1 line so far).
        cache.ensure(9, &pump, || DeferredReady::new(0, Ok::<i32, String>(1)));
        drain(&pump);
        assert_eq!(cache.state(&9), Some(ResourceState::Ready(1)));
        // The stream appends: invalidate the stale tail page.
        cache.invalidate(&9);
        assert!(!cache.contains(&9), "invalidated page is gone");
        // The next ensure re-fetches it with the grown content (value 2).
        cache.ensure(9, &pump, || DeferredReady::new(1, Ok::<i32, String>(2)));
        assert_eq!(cache.state(&9), Some(ResourceState::Loading), "re-fetch starts loading");
        drain(&pump);
        assert_eq!(cache.state(&9), Some(ResourceState::Ready(2)), "grown page lands");
    }

    #[test]
    fn invalidate_is_a_noop_on_an_absent_key_and_leaves_others() {
        let pump = LocalTaskPump::new();
        let cache = Cache::new();
        cache.ensure(0, &pump, || DeferredReady::new(0, Ok::<i32, String>(100)));
        drain(&pump);
        // Absent key: no panic, no effect (the producer invalidates the tail
        // unconditionally, even when it was never fetched / already evicted).
        cache.invalidate(&999);
        assert_eq!(cache.len(), 1, "absent invalidate left the cache untouched");
        // Other keys are undisturbed.
        cache.invalidate(&999);
        assert_eq!(cache.state(&0), Some(ResourceState::Ready(100)), "key 0 intact");
    }
}
