//! `ResourceCache<K, V, E>` — a keyed map of async [`Resource`]s with
//! idempotent get-or-fetch and a reactive state snapshot (§5.22 R927).
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
//! ## Contract
//!
//! - [`ensure`](ResourceCache::ensure) is **idempotent**: a key already
//!   present (Loading, Ready, or Error) is a no-op and its future factory is
//!   never invoked, so a slice is fetched once and retained.
//! - [`state`](ResourceCache::state) / [`snapshot`](ResourceCache::snapshot)
//!   read each entry's [`Resource::state`], **subscribing** the active
//!   reactive scope — a slice's resolution re-renders the view that read it.
//! - **Retention, not eviction.** Fetched entries are held for the cache's
//!   lifetime (bounded when the key space is bounded — a fixed page count, a
//!   small set of sort/filter combinations). A truly unbounded source adds
//!   LRU eviction of far-away keys; that is a deliberate follow-up, not wired
//!   here (the same honest boundary R924 declared for its page cache).
//!
//! Single-thread (`RefCell`, `!Send`): matches the `!Send` reactive runtime
//! (§6.3) and the [`LocalTaskPump`](super::resource::LocalTaskPump) the fetch
//! is driven through.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::rc::Rc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::resource::{LocalSpawner, Resource, ResourceState};

/// A keyed cache of async [`Resource`]s with idempotent get-or-fetch.
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
    entries: RefCell<HashMap<K, Rc<Resource<V, E>>>>,
}

impl<K, V, E> ResourceCache<K, V, E>
where
    K: Hash + Eq + Clone,
    V: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    E: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    /// Construct an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: RefCell::new(HashMap::new()),
        }
    }

    /// Whether `key` already has a fetch in flight or resolved.
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.entries.borrow().contains_key(key)
    }

    /// Number of resident entries (fetched or in flight).
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
    /// constructed on a cache hit.
    ///
    /// On a miss, a `Loading` [`Resource`] is inserted **before** the fetch is
    /// spawned (so a re-entrant `ensure` for the same key during the same frame
    /// observes it as in-flight and does not double-fetch), then
    /// `make_future()` is driven to completion through `spawner` — the
    /// shell-polled [`LocalTaskPump`](super::resource::LocalTaskPump) in
    /// production, a test executor in unit tests. The insert borrow is released
    /// before `fetch_with` runs, so a completion that re-enters the cache (an
    /// `Effect` starting a follow-up fetch) does not alias the map.
    pub fn ensure<S, F, MF>(&self, key: K, spawner: &S, make_future: MF)
    where
        S: LocalSpawner,
        F: Future<Output = Result<V, E>> + 'static,
        MF: FnOnce() -> F,
    {
        if self.entries.borrow().contains_key(&key) {
            return;
        }
        let resource = Rc::new(Resource::loading());
        self.entries.borrow_mut().insert(key, Rc::clone(&resource));
        resource.fetch_with(spawner, make_future());
    }

    /// Current [`ResourceState`] of `key`, or `None` if it has never been
    /// fetched. Reading a present entry **subscribes** the active reactive
    /// scope to it, so the entry's resolution re-renders the reader.
    #[must_use]
    pub fn state(&self, key: &K) -> Option<ResourceState<V, E>> {
        self.entries.borrow().get(key).map(|r| r.state())
    }

    /// Snapshot the states of `keys`, subscribing the active scope to each
    /// **present** entry (absent keys are omitted). One borrow over the whole
    /// window avoids re-borrowing per lookup and collects the visible slices'
    /// states in a single pass — the shape a virtualized view reads once per
    /// frame to map each row to loaded data or a skeleton.
    #[must_use]
    pub fn snapshot<I>(&self, keys: I) -> HashMap<K, ResourceState<V, E>>
    where
        I: IntoIterator<Item = K>,
    {
        let entries = self.entries.borrow();
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
}
